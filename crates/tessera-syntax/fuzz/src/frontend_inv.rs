//! Invariants of the TC frontend facade: `parse`, `parse_with_spans`, `fmt`,
//! `expand`, and the TC -> TIR -> TC round trip (issue #2 / #18 contracts).

use std::collections::HashSet;
use std::time::Instant;

use tessera_phases::{FileId, Span};
use tessera_syntax::ast::AstSpans;
use tessera_syntax::lexer::{self, TokenKind};
use tessera_syntax::{
    AstExpr, AstFunction, SyntaxError, expand, fmt, format_tc, lower_to_tc, parse,
    parse_with_spans, to_tir,
};
use tessera_tir::{TirModule, verify_module};

use crate::stats::{Counter, hit};
use crate::{assert_prompt, show};

const FILE: FileId = FileId(0);

/// Entry point for arbitrary bytes: invalid UTF-8 is rejected by the driver
/// before the parser (compiler-phases.md, B0), so it is not frontend input.
pub fn check_bytes(data: &[u8]) {
    if let Some(src) = crate::decode(data) {
        check_text(src);
    }
}

/// `at` is a byte offset that legitimately differs between two spellings of
/// the same program (PROV-2), so semantic comparisons ignore it.
fn erase_at(err: SyntaxError) -> SyntaxError {
    match err {
        SyntaxError::Unexpected { want, got, .. } => SyntaxError::Unexpected { at: 0, want, got },
        SyntaxError::UnknownType { name, .. } => SyntaxError::UnknownType { at: 0, name },
        SyntaxError::UnboundVar { name, .. } => SyntaxError::UnboundVar { at: 0, name },
        SyntaxError::TypeMismatch { want, got, .. } => SyntaxError::TypeMismatch { at: 0, want, got },
        SyntaxError::IntOutOfRange { .. } => SyntaxError::IntOutOfRange { at: 0 },
        SyntaxError::TrailingInput { .. } => SyntaxError::TrailingInput { at: 0 },
        SyntaxError::NestingTooDeep { .. } => SyntaxError::NestingTooDeep { at: 0 },
        SyntaxError::EmptyProgram => SyntaxError::EmptyProgram,
    }
}

fn erase<T>(result: Result<T, SyntaxError>) -> Result<T, SyntaxError> {
    result.map_err(erase_at)
}

/// Pre-order (node, lhs, rhs) traversal without recursion.
fn preorder(root: &AstExpr) -> Vec<&AstExpr> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(expr) = stack.pop() {
        out.push(expr);
        if let AstExpr::Add(lhs, rhs) = expr {
            stack.push(rhs);
            stack.push(lhs);
        }
    }
    out
}

/// Independent oracle for name resolution: every `Var` names a parameter.
fn all_vars_bound(func: &AstFunction) -> bool {
    let params: HashSet<&str> = func.params.iter().map(|(p, _)| p.name.as_str()).collect();
    preorder(&func.body)
        .into_iter()
        .all(|e| !matches!(e, AstExpr::Var(name) if !params.contains(name.as_str())))
}

/// The same tokens with different trivia: whitespace runs are replaced by
/// other whitespace and extra whitespace is inserted between tokens. Never
/// touches the line break that ends a comment, so the significant token
/// stream is unchanged.
fn respace(src: &str) -> String {
    const WS: [&str; 5] = [" ", "\n", "\t", "  ", "\r\n"];
    let tokens = lexer::lex(src);
    let mut state = 0x9E37_79B9_7F4A_7C15_u64 ^ src.len() as u64;
    let mut out = String::with_capacity(src.len() * 2);
    for (i, token) in tokens.iter().enumerate() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let pick = WS[(state >> 33) as usize % WS.len()];
        let after_comment = i > 0 && tokens[i - 1].kind == TokenKind::LineComment;
        let text = token.text(src);
        match token.kind {
            TokenKind::Whitespace if after_comment => out.push_str(text),
            TokenKind::Whitespace => out.push_str(pick),
            TokenKind::LineComment => out.push_str(text),
            _ => {
                out.push_str(text);
                let next_is_trivia = tokens.get(i + 1).is_some_and(|t| t.kind.is_trivia());
                if (state >> 40) % 3 == 0 && !next_is_trivia {
                    out.push_str(pick);
                }
            }
        }
    }
    out
}

fn check_span(src: &str, what: &str, span: Span) -> (usize, usize) {
    let (start, end) = (span.start as usize, span.end as usize);
    invariant!(
        "SPAN-well-formed",
        span.file == FILE && start <= end && end <= src.len(),
        "{what} span {span} is outside {} bytes of {}",
        src.len(),
        show(src)
    );
    invariant!(
        "SPAN-char-boundary",
        src.is_char_boundary(start) && src.is_char_boundary(end),
        "{what} span {span} splits a char in {}",
        show(src)
    );
    (start, end)
}

/// PROV-2 / AstSpans contract: spans are in bounds, nested, and name the text
/// they describe; `exprs` has one entry per AST node in pre-order.
fn check_spans(src: &str, func: &AstFunction, spans: &AstSpans) {
    let (func_s, func_e) = check_span(src, "func", spans.func);
    let inside = |what: &str, span: Span| {
        let (s, e) = check_span(src, what, span);
        invariant!(
            "SPAN-nested",
            func_s <= s && e <= func_e,
            "{what} span {span} outside func span {} in {}",
            spans.func,
            show(src)
        );
        (s, e)
    };
    let (ns, ne) = inside("name", spans.name);
    invariant!(
        "SPAN-text",
        src[ns..ne] == func.name,
        "name span text {:?} != {:?} in {}",
        &src[ns..ne],
        func.name,
        show(src)
    );
    invariant!(
        "SPAN-arity",
        spans.params.len() == func.params.len(),
        "{} param spans for {} params in {}",
        spans.params.len(),
        func.params.len(),
        show(src)
    );
    for ((param, ty), sp) in func.params.iter().zip(&spans.params) {
        let (s, e) = inside("param name", sp.name);
        invariant!(
            "SPAN-text",
            src[s..e] == param.name,
            "param span text {:?} != {:?} in {}",
            &src[s..e],
            param.name,
            show(src)
        );
        let (s, e) = inside("param type", sp.ty);
        invariant!(
            "SPAN-text",
            src[s..e] == *ty.as_str(),
            "param type span text {:?} != {ty} in {}",
            &src[s..e],
            show(src)
        );
    }
    let (s, e) = inside("ret", spans.ret);
    invariant!(
        "SPAN-text",
        src[s..e] == *func.ret.as_str(),
        "ret span text {:?} != {} in {}",
        &src[s..e],
        func.ret,
        show(src)
    );
    let (body_s, body_e) = inside("body", spans.body);

    // exprs: pre-order, one per node, nested in their parent and in the body
    let nodes = preorder(&func.body);
    invariant!(
        "SPAN-exprs-arity",
        spans.exprs.len() == nodes.len(),
        "{} expr spans for {} AST nodes in {}",
        spans.exprs.len(),
        nodes.len(),
        show(src)
    );
    // parent of each pre-order node, from the pre-order layout alone
    let mut parents: Vec<Option<usize>> = Vec::with_capacity(nodes.len());
    let mut open: Vec<(usize, usize)> = Vec::new(); // (children still to come, node)
    for (index, node) in nodes.iter().enumerate() {
        while open.last().is_some_and(|&(remaining, _)| remaining == 0) {
            open.pop();
        }
        parents.push(open.last().map(|&(_, parent)| parent));
        if let Some(top) = open.last_mut() {
            top.0 -= 1;
        }
        if matches!(node, AstExpr::Add(..)) {
            open.push((2, index));
        }
    }
    for (index, node) in nodes.iter().enumerate() {
        let span = spans.exprs[index];
        let (s, e) = check_span(src, "expr", span);
        invariant!(
            "SPAN-nested",
            body_s <= s && e <= body_e,
            "expr span {span} outside body span {} in {}",
            spans.body,
            show(src)
        );
        if let Some(parent) = parents[index] {
            invariant!(
                "SPAN-nested",
                spans.exprs[parent].contains(span),
                "expr span {span} outside its parent's {} in {}",
                spans.exprs[parent],
                show(src)
            );
        }
        match node {
            AstExpr::Var(name) => invariant!(
                "SPAN-text",
                src[s..e] == *name,
                "Var span text {:?} != {name:?} in {}",
                &src[s..e],
                show(src)
            ),
            AstExpr::Int(value) => invariant!(
                "SPAN-text",
                src[s..e].parse::<i64>() == Ok(*value),
                "Int span text {:?} does not denote {value} in {}",
                &src[s..e],
                show(src)
            ),
            AstExpr::Add(..) => {}
        }
    }
}

/// Full check of one valid-UTF-8 input.
pub fn check_text(src: &str) {
    let started = Instant::now();
    hit(Counter::FeRuns);

    // None of the four entry points may panic, overflow the stack or hang.
    let parsed = parse(src);
    let with_spans = parse_with_spans(src);
    let formatted = fmt(src);
    let expanded = expand(src);

    // -- the entry points agree with each other, and are deterministic
    invariant!(
        "FE-parse-facade",
        parsed == with_spans.clone().map(|(func, _)| func),
        "parse and parse_with_spans disagree for {}",
        show(src)
    );
    invariant!(
        "FE-determinism",
        with_spans == parse_with_spans(src) && expanded == expand(src),
        "repeated frontend calls differ for {}",
        show(src)
    );
    invariant!(
        "FE-fmt-facade",
        formatted == parsed.clone().map(|func| format_tc(&func)),
        "fmt is not format_tc(parse) for {}",
        show(src)
    );

    let func = match parsed {
        Err(err) => {
            hit(Counter::FeParseErr);
            invariant!(
                "FE-errors-propagate",
                formatted == Err(err.clone()) && expanded == Err(err.clone()),
                "parse error {err:?} is not what fmt/expand return ({formatted:?}/{expanded:?}) for {}",
                show(src)
            );
            check_respace(src, &formatted, &expanded);
            assert_prompt("frontend", started, src);
            return;
        }
        Ok(func) => func,
    };
    hit(Counter::FeParseOk);
    let canonical = formatted.expect("fmt succeeds when parse does");

    // -- canonical formatting (issue #2: one spelling, idempotent, harmless)
    invariant!(
        "FMT-canonical-shape",
        canonical.starts_with("f ")
            && canonical.chars().filter(|c| c.is_whitespace()).count() == 1
            && canonical.is_ascii(),
        "canonical text {canonical:?} is not a single-space ASCII spelling for {}",
        show(src)
    );
    invariant!(
        "FMT-idempotent",
        fmt(&canonical).as_deref() == Ok(canonical.as_str()),
        "fmt(fmt(x)) != fmt(x) for {}: first {canonical:?}, second {:?}",
        show(src),
        fmt(&canonical)
    );
    invariant!(
        "FMT-preserves-ast",
        parse(&canonical).as_ref() == Ok(&func),
        "fmt changed the tree of {}: canonical {canonical:?} parses to {:?}",
        show(src),
        parse(&canonical)
    );
    invariant!(
        "FMT-preserves-expand",
        erase(expand(&canonical)) == erase(expanded.clone()),
        "expand(fmt(x)) != expand(x) for {}: canonical {canonical:?}: {:?} vs {:?}",
        show(src),
        expand(&canonical),
        expanded
    );
    hit(Counter::FeFmtIdempotenceChecked);

    // -- spans (PROV-2)
    let (_, spans) = with_spans.expect("parse_with_spans succeeds when parse does");
    check_spans(src, &func, &spans);
    hit(Counter::FeSpansChecked);

    // -- name resolution happens on expand: exactly the unbound-variable case
    let bound = all_vars_bound(&func);
    invariant!(
        "FE-expand-iff-bound",
        expanded.is_ok() == bound,
        "expand is {:?} but all variables bound = {bound} for {}",
        expanded.as_ref().map(|_| ()),
        show(src)
    );

    match &expanded {
        Err(SyntaxError::UnboundVar { at, name }) => {
            hit(Counter::FeExpandErrUnbound);
            invariant!(
                "PROV-2-unbound-offset",
                src.get(*at..).is_some_and(|rest| rest.starts_with(name.as_str())),
                "UnboundVar at {at} does not point at {name:?} in {}",
                show(src)
            );
        }
        Err(other) => crate::fail(
            "FE-expand-error-kind",
            &format!("expand failed with {other:?} on a parsed program: {}", show(src)),
        ),
        Ok(tir_text) => check_round_trip(src, &func, &canonical, tir_text),
    }

    check_respace(src, &Ok(canonical), &expanded);
    assert_prompt("frontend", started, src);
}

/// TC -> TIR -> TC (issue #2 round-trip contract) for a program that expanded.
fn check_round_trip(src: &str, func: &AstFunction, canonical: &str, tir_text: &str) {
    hit(Counter::FeExpandOk);
    let module = match TirModule::parse(tir_text) {
        Ok(module) => module,
        Err(err) => {
            crate::fail(
                "TIR-parse-accepts-expand-output",
                &format!("TirModule::parse rejects expand output {tir_text:?}: {err} for {}", show(src)),
            );
            return;
        }
    };
    invariant!(
        "TIR-one-function",
        module.funcs.len() == 1,
        "expand produced {} functions for {}",
        module.funcs.len(),
        show(src)
    );
    let errors = verify_module(&module);
    invariant!(
        "TIR-verify-accepts-expand-output",
        errors.is_empty(),
        "verify_module rejects expand output {tir_text:?}: {errors:?} for {}",
        show(src)
    );
    invariant!(
        "TIR-text-fixed-point",
        module.to_text() == tir_text,
        "TIR text is not a print/parse fixed point for {}: {tir_text:?} -> {:?}",
        show(src),
        module.to_text()
    );
    let (_, spans) = parse_with_spans(src).expect("parses");
    invariant!(
        "TIR-parse-is-to-tir",
        to_tir(func, &spans).as_ref().ok() == module.funcs.first(),
        "parsed TIR differs from to_tir for {}",
        show(src)
    );
    invariant!(
        "ROUND-TRIP-tc-tir-tc",
        lower_to_tc(&module.funcs[0]).as_deref() == Ok(canonical),
        "lower_to_tc(TIR of x) != fmt(x) for {}: {:?} vs {canonical:?}",
        show(src),
        lower_to_tc(&module.funcs[0])
    );
    hit(Counter::FeRoundTripChecked);
}

/// Metamorphic: different trivia, same significant tokens => same result,
/// modulo source offsets.
fn check_respace(
    src: &str,
    formatted: &Result<String, SyntaxError>,
    expanded: &Result<String, SyntaxError>,
) {
    let variant = respace(src);
    invariant!(
        "FE-trivia-insensitive",
        erase(fmt(&variant)) == erase(formatted.clone())
            && erase(expand(&variant)) == erase(expanded.clone()),
        "changing only whitespace changed the result: {} vs {}",
        show(src),
        show(&variant)
    );
    hit(Counter::FeRespaceChecked);
}
