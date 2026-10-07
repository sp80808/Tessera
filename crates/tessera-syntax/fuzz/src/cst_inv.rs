//! Invariants of `tessera_syntax::cst::parse_file` (CST-1..3, DIAG-1).
//!
//! All traversals here are iterative on purpose: the checks must survive the
//! very inputs (hostile nesting) that a recursive walker would overflow on, so
//! a stack overflow can only come from the code under test.

use std::collections::HashSet;
use std::time::Instant;

use tessera_phases::{FileId, Phase, Provenance, Severity};
use tessera_syntax::cst::{self, Child, Cst, NodeKind, ParsedFile};
use tessera_syntax::lexer::{self, TokenKind};
use tessera_syntax::{MAX_EXPR_DEPTH, MAX_NESTING};

use crate::stats::{Counter, hit};
use crate::{assert_prompt, show};

const FILE: FileId = FileId(0);

/// Diagnostic codes the tolerant parser is documented to emit.
const KNOWN_CODES: [&str; 4] = [
    "E-syntax-expected",
    "E-syntax-empty-program",
    "E-syntax-trailing-input",
    "E-syntax-nesting-too-deep",
];

/// Entry point for arbitrary bytes: invalid UTF-8 is rejected by the driver
/// before the parser (compiler-phases.md, B0), so it is not parser input.
pub fn check_bytes(data: &[u8]) {
    if let Some(src) = crate::decode(data) {
        check_text(src);
    }
}

/// What the iterative walk learned about a tree.
#[derive(Debug, Default)]
pub struct Shape {
    pub nodes: usize,
    pub errors: usize,
    pub missing: usize,
    /// Max number of `BinExpr`/`ParenExpr` nodes on one root-to-leaf path.
    pub max_expr_depth: usize,
    /// Max number of `ParenExpr` nodes on one root-to-leaf path.
    pub max_paren_depth: usize,
}

struct Frame {
    node: usize,
    next_child: usize,
    /// End of the last child seen (start of the node before any child).
    cursor: usize,
    expr_depth: usize,
    paren_depth: usize,
}

/// Well-formedness of the tree, checked without recursion. Mirrors (and
/// strengthens) `assert_well_formed` in `cst.rs`: root covers the source,
/// children are ordered, non-overlapping and inside their parent, and the
/// leaf tokens are exactly `0..tokens.len()` in order, each used once.
#[must_use]
pub fn check_shape(parsed: &ParsedFile, src: &str) -> Shape {
    let cst: &Cst = &parsed.cst;
    let tokens = &parsed.tokens;
    let root = cst.root();
    invariant!(
        "CST-root-kind",
        cst.kind(root) == NodeKind::File,
        "root is {:?} for {}",
        cst.kind(root),
        show(src)
    );
    invariant!(
        "CST-root-covers",
        cst.range(root) == (0, src.len()),
        "root range {:?} != 0..{} for {}",
        cst.range(root),
        src.len(),
        show(src)
    );

    let mut shape = Shape::default();
    let mut seen = HashSet::new();
    let mut next_token = 0usize;
    let mut stack: Vec<Frame> = Vec::new();
    let enter = |stack: &mut Vec<Frame>,
                 seen: &mut HashSet<usize>,
                 shape: &mut Shape,
                 node: usize,
                 (parent_expr, parent_paren): (usize, usize)| {
        invariant!(
            "CST-tree-not-dag",
            seen.insert(node),
            "node {node} reachable twice in {}",
            show(src)
        );
        let (start, end) = cst.range(node);
        invariant!(
            "CST-node-range",
            start <= end && end <= src.len(),
            "node {node} has range {start}..{end} (len {}) in {}",
            src.len(),
            show(src)
        );
        let kind = cst.kind(node);
        shape.nodes += 1;
        match kind {
            NodeKind::Error => shape.errors += 1,
            NodeKind::Missing => shape.missing += 1,
            _ => {}
        }
        let is_expr = matches!(kind, NodeKind::BinExpr | NodeKind::ParenExpr);
        let is_paren = kind == NodeKind::ParenExpr;
        let expr_depth = parent_expr + usize::from(is_expr);
        let paren_depth = parent_paren + usize::from(is_paren);
        shape.max_expr_depth = shape.max_expr_depth.max(expr_depth);
        shape.max_paren_depth = shape.max_paren_depth.max(paren_depth);
        check_node_shape(parsed, src, node);
        stack.push(Frame {
            node,
            next_child: 0,
            cursor: start,
            expr_depth,
            paren_depth,
        });
    };

    enter(&mut stack, &mut seen, &mut shape, root, (0, 0));
    while let Some(top) = stack.last_mut() {
        let children = cst.children(top.node);
        let Some(child) = children.get(top.next_child).copied() else {
            let frame = stack.pop().expect("non-empty");
            let end = cst.range(frame.node).1;
            if frame.node != root {
                // a node ends exactly where its last child ends (and is
                // zero-width when it has none: `cursor` starts at `start`)
                invariant!(
                    "CST-node-spans-children",
                    end == frame.cursor,
                    "node {} ({:?}) ends at {end} but its children end at {} in {}",
                    frame.node,
                    cst.kind(frame.node),
                    frame.cursor,
                    show(src)
                );
            } else {
                invariant!(
                    "CST-root-covers",
                    frame.cursor == src.len(),
                    "root children end at {} for {} bytes in {}",
                    frame.cursor,
                    src.len(),
                    show(src)
                );
            }
            continue;
        };
        top.next_child += 1;
        let (node_start, node_end) = cst.range(top.node);
        let (cs, ce) = match child {
            Child::Token(t) => {
                invariant!(
                    "CST-token-order",
                    t == next_token && t < tokens.len(),
                    "leaf token {t} where {next_token} of {} was expected in {}",
                    tokens.len(),
                    show(src)
                );
                next_token += 1;
                (tokens[t].start, tokens[t].end)
            }
            Child::Node(c) => cst.range(c),
        };
        if top.next_child == 1 && top.node != root {
            // a node starts exactly where its first child starts
            invariant!(
                "CST-node-spans-children",
                cs == node_start,
                "node {} ({:?}) starts at {node_start} but its first child starts at {cs} in {}",
                top.node,
                cst.kind(top.node),
                show(src)
            );
        }
        invariant!(
            "CST-children-ordered",
            cs >= top.cursor && cs <= ce,
            "child {child:?} at {cs}..{ce} overlaps the previous sibling ending at {} in {}",
            top.cursor,
            show(src)
        );
        invariant!(
            "CST-child-inside-parent",
            cs >= node_start && ce <= node_end,
            "child {child:?} at {cs}..{ce} outside parent {node_start}..{node_end} in {}",
            show(src)
        );
        top.cursor = ce;
        let depths = (top.expr_depth, top.paren_depth);
        if let Child::Node(c) = child {
            enter(&mut stack, &mut seen, &mut shape, c, depths);
        }
    }
    invariant!(
        "CST-1-every-token-once",
        next_token == tokens.len(),
        "{next_token} of {} tokens are in the tree for {}",
        tokens.len(),
        show(src)
    );
    shape
}

/// Kind-specific structure that follows directly from the parser's rules.
fn check_node_shape(parsed: &ParsedFile, src: &str, node: usize) {
    let cst = &parsed.cst;
    let kind = cst.kind(node);
    let children = cst.children(node);
    let (start, end) = cst.range(node);
    let significant: Vec<Child> = children
        .iter()
        .copied()
        .filter(|c| match c {
            Child::Token(t) => !parsed.tokens[*t].kind.is_trivia(),
            Child::Node(_) => true,
        })
        .collect();
    let token_kind = |c: &Child| match c {
        Child::Token(t) => Some(parsed.tokens[*t].kind),
        Child::Node(_) => None,
    };
    match kind {
        NodeKind::Missing => invariant!(
            "CST-missing-zero-width",
            children.is_empty() && start == end,
            "Missing node {node} is not an empty zero-width leaf ({start}..{end}, {} children) in {}",
            children.len(),
            show(src)
        ),
        NodeKind::Error => invariant!(
            "CST-error-nonempty",
            end > start && significant.iter().any(|c| token_kind(c).is_some()),
            "Error node {node} ({start}..{end}) consumed no token in {}",
            show(src)
        ),
        NodeKind::LiteralExpr => invariant!(
            "CST-literal-shape",
            children.len() == 1 && token_kind(&children[0]) == Some(TokenKind::Int),
            "LiteralExpr {node} is not exactly one Int token in {}",
            show(src)
        ),
        NodeKind::PathExpr => invariant!(
            "CST-path-shape",
            children.len() == 1 && token_kind(&children[0]) == Some(TokenKind::Ident),
            "PathExpr {node} is not exactly one Ident token in {}",
            show(src)
        ),
        NodeKind::ParenExpr => invariant!(
            "CST-paren-shape",
            significant.first().and_then(token_kind) == Some(TokenKind::LParen),
            "ParenExpr {node} does not start with `(` in {}",
            show(src)
        ),
        NodeKind::BinExpr => invariant!(
            "CST-binexpr-shape",
            significant.len() == 3
                && matches!(significant[0], Child::Node(_))
                && token_kind(&significant[1]) == Some(TokenKind::Plus)
                && matches!(significant[2], Child::Node(_)),
            "BinExpr {node} is not [node, `+`, node] in {}",
            show(src)
        ),
        NodeKind::File => {
            let kinds: Vec<NodeKind> = children
                .iter()
                .filter_map(|c| match c {
                    Child::Node(n) => Some(cst.kind(*n)),
                    Child::Token(_) => None,
                })
                .collect();
            let ok = matches!(
                kinds.as_slice(),
                [] | [NodeKind::Fn] | [NodeKind::Fn, NodeKind::Error]
            );
            invariant!(
                "CST-file-shape",
                ok,
                "File children are {kinds:?}, expected [], [Fn] or [Fn, Error] in {}",
                show(src)
            );
        }
        NodeKind::Fn | NodeKind::ParamList | NodeKind::Param | NodeKind::TypeRef => {}
    }
}

/// Full check of one valid-UTF-8 input.
pub fn check_text(src: &str) {
    let started = Instant::now();
    hit(Counter::CstRuns);
    let out = cst::parse_file(FILE, src);
    let parsed = &out.value;

    // parse is a pure function of its input
    invariant!(
        "PARSE-determinism",
        out == cst::parse_file(FILE, src),
        "two parses of {} differ",
        show(src)
    );
    invariant!(
        "CST-tokens-are-the-lexer-tokens",
        parsed.tokens == lexer::lex(src) && parsed.file == FILE,
        "ParsedFile tokens/file differ from lexer::lex for {}",
        show(src)
    );

    let shape = check_shape(parsed, src);

    // CST-3 (compiler-phases.md B1): nesting and total expression depth are
    // bounded. Checked before any recursive consumer runs, so an unbounded
    // tree is reported as this invariant rather than as a stack overflow.
    invariant!(
        "CST-3-paren-depth",
        shape.max_paren_depth <= MAX_NESTING,
        "parenthesis nesting {} exceeds MAX_NESTING={MAX_NESTING} for {}",
        shape.max_paren_depth,
        show(src)
    );
    invariant!(
        "CST-3-expr-depth",
        shape.max_expr_depth <= MAX_EXPR_DEPTH,
        "expression tree depth {} (BinExpr+ParenExpr on one path) exceeds MAX_EXPR_DEPTH={MAX_EXPR_DEPTH} for {}",
        shape.max_expr_depth,
        show(src)
    );

    check_diagnostics(&out, src, &shape);

    // Recursive library consumers, safe now that depth is bounded.
    invariant!(
        "CST-1-lossless",
        parsed.reconstruct(src) == src,
        "reconstruct() differs from the source {}",
        show(src)
    );
    invariant!(
        "CST-kinds-count",
        parsed.cst.kinds().len() == shape.nodes,
        "kinds() lists {} nodes, walk found {} in {}",
        parsed.cst.kinds().len(),
        shape.nodes,
        show(src)
    );
    let dump = parsed.dump(src);
    invariant!(
        "CST-dump",
        dump.lines().count() == shape.nodes + parsed.tokens.len(),
        "dump has {} lines for {} nodes + {} tokens in {}",
        dump.lines().count(),
        shape.nodes,
        parsed.tokens.len(),
        show(src)
    );
    assert_prompt("parse_cst", started, src);
}

fn check_diagnostics(out: &tessera_phases::PhaseOutput<ParsedFile>, src: &str, shape: &Shape) {
    let parsed = &out.value;
    let diags = &out.diagnostics;
    let (mut empty, mut trailing, mut too_deep) = (0usize, 0usize, 0usize);
    let mut previous = None;
    for d in diags.iter() {
        invariant!(
            "DIAG-phase-severity",
            d.phase == Phase::Syntax && d.severity == Severity::Error,
            "diagnostic {d:?} is not a syntax error for {}",
            show(src)
        );
        invariant!(
            "DIAG-known-code",
            KNOWN_CODES.contains(&d.code),
            "unknown diagnostic code {:?} for {}",
            d.code,
            show(src)
        );
        let Provenance::Source(span) = d.at else {
            crate::violation(
                "DIAG-provenance",
                &format!("{d:?} is synthesized for {}", show(src)),
            );
        };
        let (start, end) = (span.start as usize, span.end as usize);
        invariant!(
            "DIAG-span-file",
            span.file == FILE,
            "diagnostic {d:?} names file {:?} for {}",
            span.file,
            show(src)
        );
        invariant!(
            "DIAG-span-in-bounds",
            start <= end && end <= src.len(),
            "diagnostic {d:?} span {start}..{end} is outside {} bytes of {}",
            src.len(),
            show(src)
        );
        invariant!(
            "DIAG-span-char-boundary",
            src.is_char_boundary(start) && src.is_char_boundary(end),
            "diagnostic {d:?} span {start}..{end} splits a char in {}",
            show(src)
        );
        invariant!(
            "DIAG-order",
            previous.is_none_or(|p| p <= span),
            "diagnostics are not in span order ({previous:?} then {span:?}) for {}",
            show(src)
        );
        previous = Some(span);
        match d.code {
            "E-syntax-empty-program" => empty += 1,
            "E-syntax-trailing-input" => trailing += 1,
            "E-syntax-nesting-too-deep" => too_deep += 1,
            _ => {}
        }
    }
    let error_nodes = shape.errors + shape.missing;
    let has_significant = parsed.tokens.iter().any(|t| !t.kind.is_trivia());

    // Derived from cst.rs: every diagnostic except `empty-program` is emitted
    // together with an `Error`/`Missing` node, and every such node is emitted
    // with a diagnostic (or after the one-shot nesting diagnostic poisoned
    // the parser). Hence `has_errors` holds exactly when the tree has such a
    // node or the program is empty.
    invariant!(
        "DIAG-has-errors-iff-error-nodes",
        diags.has_errors() == (error_nodes > 0 || empty > 0),
        "has_errors={} but tree has {} Error and {} Missing nodes and {} empty-program diagnostics for {}",
        diags.has_errors(),
        shape.errors,
        shape.missing,
        empty,
        show(src)
    );
    invariant!(
        "DIAG-empty-program-iff-no-significant-token",
        (empty > 0) == !has_significant && empty <= 1,
        "empty-program diagnostics: {empty}, significant tokens present: {has_significant} for {}",
        show(src)
    );
    invariant!(
        "DIAG-empty-program-is-alone",
        empty == 0 || (diags.len() == 1 && error_nodes == 0),
        "empty program has {} diagnostics and {error_nodes} error nodes for {}",
        diags.len(),
        show(src)
    );
    invariant!(
        "DIAG-one-shot",
        trailing <= 1 && too_deep <= 1,
        "trailing-input x{trailing}, nesting-too-deep x{too_deep} for {}",
        show(src)
    );
    invariant!(
        "DIAG-trailing-and-deep-have-error-node",
        (trailing == 0 && too_deep == 0) || shape.errors > 0,
        "trailing/too-deep diagnostic without an Error node for {}",
        show(src)
    );
    invariant!(
        "DIAG-count-bounded-by-nodes",
        diags.len() <= error_nodes + usize::from(empty > 0),
        "{} diagnostics but only {error_nodes} error nodes for {}",
        diags.len(),
        show(src)
    );
    invariant!(
        "DIAG-is-clean",
        out.is_clean() == !diags.has_errors() && diags.is_empty() == !diags.has_errors(),
        "is_clean/has_errors/is_empty disagree for {}",
        show(src)
    );

    if diags.has_errors() {
        hit(Counter::CstWithErrors);
    } else {
        hit(Counter::CstClean);
    }
    if empty > 0 {
        hit(Counter::CstEmptyProgram);
    }
    if too_deep > 0 {
        hit(Counter::CstNestingTooDeep);
    }
    if trailing > 0 {
        hit(Counter::CstTrailingInput);
    }
    if shape.missing > 0 {
        hit(Counter::CstMissingNode);
    }
}
