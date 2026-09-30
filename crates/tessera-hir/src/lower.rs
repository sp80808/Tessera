//! `CST -> HIR` lowering (contract B1 -> B2). See `docs/architecture/hir.md`
//! for the normalization laws, identity and provenance rules this implements.
//!
//! Design constraints, all enforced by tests:
//!
//! - **Tolerant** (HIR-2, INV-DIAG-1): never panics, on any CST. Every lookup
//!   into a node is shape-robust (`get`/`first`/kind search, never `[0]`), so a
//!   node that has more, fewer or differently ordered children than the current
//!   parser emits still lowers to a structurally valid value.
//! - **Small stack**: expression lowering is an explicit-stack loop and
//!   parenthesis stripping is a loop, so lowering uses O(1) native stack for
//!   any expression depth. (The parser bounds depth, but nothing here relies
//!   on that.)
//! - **Identity from structure only** (INV-ID-1): `ExprId`s are dense pre-order
//!   indices, `ItemId`s come from the function name plus an order-based
//!   disambiguator. Byte offsets appear only in the provenance tables.
//! - **Total provenance** (PROV-1): every local, expression and type position
//!   gets exactly one entry as it is created.

use std::collections::BTreeMap;

use tessera_phases::{
    Diagnostic, DiagnosticSet, Phase, PhaseOutput, Provenance, ProvenanceMap, Span,
};
use tessera_syntax::cst::{Child, NodeKind, ParsedFile};
use tessera_syntax::lexer::{Token, TokenKind};

use crate::{
    BinOp, Body, Expr, ExprId, FnItem, FnProvenance, HirModule, HirOutput, HirProvenance, Item,
    ItemId, ItemKind, Local, LocalId, Param, TypePos, TypeRef,
};

// Stable `why` strings for `Provenance::Synthesized` placeholders. These are
// part of the HIR debug/dump surface; renaming one is a format change.
const WHY_MISSING_NAME: &str = "missing-name";
const WHY_ERRONEOUS_NAME: &str = "erroneous-name";
const WHY_MISSING_TYPE: &str = "missing-type";
const WHY_ERRONEOUS_TYPE: &str = "erroneous-type";
const WHY_MISSING_EXPR: &str = "missing-expression";
const WHY_ERRONEOUS_EXPR: &str = "erroneous-expression";
const WHY_INT_OUT_OF_RANGE: &str = "int-out-of-range";

/// Diagnostic code for an integer literal that does not fit `i64`.
const CODE_INT_OUT_OF_RANGE: &str = "E-hir-int-out-of-range";

/// Lower a parsed file to normalized HIR. Tolerant: erroneous CST yields
/// explicit error/missing nodes plus diagnostics, never a panic. Pure and
/// deterministic: same CST and source text give equal output (HIR-4).
///
/// `src` must be the text `parsed` was produced from. A mismatching `src` is a
/// caller bug, but it degrades to error values, not a panic.
#[must_use]
pub fn lower(parsed: &ParsedFile, src: &str) -> PhaseOutput<HirOutput> {
    let cx = Cx { parsed, src };
    let root = parsed.cst.root();
    let fn_nodes: Vec<usize> = parsed
        .cst
        .children(root)
        .iter()
        .filter_map(|child| match *child {
            Child::Node(n) if parsed.cst.kind(n) == NodeKind::Fn => Some(n),
            Child::Node(_) | Child::Token(_) => None,
        })
        .collect();
    lower_items(&cx, &fn_nodes)
}

/// Lower the given `Fn` nodes, in order, as one module. Split from [`lower`] so
/// the N-function path (which today's parser cannot produce) is unit-testable.
fn lower_items(cx: &Cx<'_>, fn_nodes: &[usize]) -> PhaseOutput<HirOutput> {
    let mut diagnostics = DiagnosticSet::new();
    let lowered: Vec<LoweredFn> = fn_nodes
        .iter()
        .map(|&node| lower_fn(cx, node, &mut diagnostics))
        .collect();
    let discriminators = disambiguators(lowered.iter().map(|f| f.name.as_deref().unwrap_or("")));

    let file = cx.parsed.file;
    let mut items = Vec::with_capacity(lowered.len());
    let mut provenance = Vec::with_capacity(lowered.len());
    for (func, disambiguator) in lowered.into_iter().zip(discriminators) {
        items.push(Item::Fn(FnItem {
            id: ItemId {
                file,
                kind: ItemKind::Fn,
                name: func.name.clone().unwrap_or_default(),
                disambiguator,
            },
            name: func.name,
            params: func.params,
            ret: func.ret,
            body: func.body,
        }));
        provenance.push(func.provenance);
    }
    PhaseOutput::with(
        HirOutput {
            module: HirModule { file, items },
            provenance: HirProvenance { items: provenance },
        },
        diagnostics,
    )
}

/// For each key, in order, how many earlier keys were equal to it. This is the
/// contract's `ItemId::disambiguator` rule (§2.4): a pure function of the
/// ordered names, independent of spans. The dump reuses it to spell duplicate
/// local names.
pub(crate) fn disambiguators<'a>(keys: impl IntoIterator<Item = &'a str>) -> Vec<u32> {
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    keys.into_iter()
        .map(|key| {
            let count = seen.entry(key).or_insert(0);
            let earlier = *count;
            *count = count.saturating_add(1);
            earlier
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CST access
// ---------------------------------------------------------------------------

/// A non-trivia child of a CST node.
#[derive(Clone, Copy)]
enum Sig<'a> {
    Node(usize),
    Token(&'a Token),
}

struct Cx<'a> {
    parsed: &'a ParsedFile,
    src: &'a str,
}

impl<'a> Cx<'a> {
    fn kind(&self, node: usize) -> NodeKind {
        self.parsed.cst.kind(node)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Span::new(self.parsed.file, clamp(start), clamp(end))
    }

    /// Byte range of the node: first to last child, without the trivia that
    /// surrounds it (the CST hands leading/trailing trivia to the parent).
    fn node_span(&self, node: usize) -> Span {
        let (start, end) = self.parsed.cst.range(node);
        self.span(start, end)
    }

    fn token_span(&self, token: &Token) -> Span {
        self.span(token.start, token.end)
    }

    /// Zero-width span at the end of `span`: "where something should have been".
    fn end_of(&self, span: Span) -> Span {
        Span::new(span.file, span.end, span.end)
    }

    /// Source text of a token; `None` only if `src` does not match the tokens.
    fn text(&self, token: &Token) -> Option<&'a str> {
        self.src.get(token.start..token.end)
    }

    /// The non-trivia children of `node`, in order.
    fn sig(&self, node: usize) -> Vec<Sig<'a>> {
        let parsed: &'a ParsedFile = self.parsed;
        parsed
            .cst
            .children(node)
            .iter()
            .filter_map(|child| match *child {
                Child::Node(n) => Some(Sig::Node(n)),
                Child::Token(t) => parsed
                    .tokens
                    .get(t)
                    .filter(|token| !token.kind.is_trivia())
                    .map(Sig::Token),
            })
            .collect()
    }

    fn is_node(&self, sig: Sig<'_>, kind: NodeKind) -> bool {
        matches!(sig, Sig::Node(n) if self.kind(n) == kind)
    }

    fn first_node(&self, sigs: &[Sig<'_>], kind: NodeKind) -> Option<usize> {
        sigs.iter().find_map(|&s| match s {
            Sig::Node(n) if self.kind(n) == kind => Some(n),
            Sig::Node(_) | Sig::Token(_) => None,
        })
    }

    fn first_token(&self, sigs: &[Sig<'a>], kind: TokenKind) -> Option<&'a Token> {
        sigs.iter().find_map(|&s| match s {
            Sig::Token(t) if t.kind == kind => Some(t),
            Sig::Node(_) | Sig::Token(_) => None,
        })
    }

    fn has_token(sigs: &[Sig<'_>], kind: TokenKind) -> bool {
        sigs.iter()
            .any(|s| matches!(s, Sig::Token(t) if t.kind == kind))
    }

    fn nodes(sigs: &[Sig<'_>]) -> Vec<usize> {
        sigs.iter()
            .filter_map(|s| match *s {
                Sig::Node(n) => Some(n),
                Sig::Token(_) => None,
            })
            .collect()
    }

    // ----- names and types -----

    /// A name that should sit in `slot`: an identifier token gives `Some`, a
    /// `Missing`/`Error` node or an absent slot gives `None` with synthesized
    /// provenance located where the name should have been.
    fn name_in(&self, slot: Option<Sig<'a>>, fallback: Span) -> (Option<String>, Provenance) {
        let synthesized = |origin, why| (None, Provenance::Synthesized { origin, why });
        match slot {
            Some(Sig::Token(token)) if token.kind == TokenKind::Ident => {
                let at = self.token_span(token);
                match self.text(token) {
                    Some(text) => (Some(text.to_owned()), Provenance::Source(at)),
                    None => synthesized(at, WHY_ERRONEOUS_NAME),
                }
            }
            Some(Sig::Token(token)) => synthesized(self.token_span(token), WHY_ERRONEOUS_NAME),
            Some(Sig::Node(n)) if self.kind(n) == NodeKind::Missing => {
                synthesized(self.node_span(n), WHY_MISSING_NAME)
            }
            Some(Sig::Node(n)) => synthesized(self.node_span(n), WHY_ERRONEOUS_NAME),
            None => synthesized(fallback, WHY_MISSING_NAME),
        }
    }

    /// `node` is a `TypeRef` node, if the CST has one.
    fn type_in(&self, node: Option<usize>, fallback: Span) -> (TypeRef, Provenance) {
        let synthesized = |origin, why, ty| (ty, Provenance::Synthesized { origin, why });
        let Some(node) = node else {
            return synthesized(fallback, WHY_MISSING_TYPE, TypeRef::Missing);
        };
        let at = self.node_span(node);
        let sigs = self.sig(node);
        if let Some(name) = self
            .first_token(&sigs, TokenKind::Ident)
            .and_then(|token| self.text(token))
        {
            return (TypeRef::Path(name.to_owned()), Provenance::Source(at));
        }
        let erroneous = sigs.iter().any(|&s| match s {
            Sig::Token(_) => true,
            Sig::Node(n) => self.kind(n) != NodeKind::Missing,
        });
        if erroneous {
            synthesized(at, WHY_ERRONEOUS_TYPE, TypeRef::Error)
        } else {
            synthesized(at, WHY_MISSING_TYPE, TypeRef::Missing)
        }
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

struct LoweredFn {
    name: Option<String>,
    params: Vec<Param>,
    ret: TypeRef,
    body: Body,
    provenance: FnProvenance,
}

fn lower_fn(cx: &Cx<'_>, fn_node: usize, diagnostics: &mut DiagnosticSet) -> LoweredFn {
    let item_span = cx.node_span(fn_node);
    let sigs = cx.sig(fn_node);

    // Current shape: [kw] [name] ParamList [`>`] TypeRef [`=`] expr, where each
    // bracketed slot is a token or a Missing/Error node. Split on the
    // ParamList so the two leading slots are the header.
    let param_list_at = sigs
        .iter()
        .position(|&s| cx.is_node(s, NodeKind::ParamList));
    let (header, rest): (&[Sig<'_>], &[Sig<'_>]) = match param_list_at {
        Some(i) => (&sigs[..i], &sigs[i + 1..]),
        None => (&[], &sigs[..]),
    };

    let (name, name_provenance) = cx.name_in(
        header.get(1).copied(),
        Span::new(item_span.file, item_span.start, item_span.start),
    );

    let mut locals = Vec::new();
    let mut params = Vec::new();
    let mut local_provenance = ProvenanceMap::new();
    let mut type_provenance = ProvenanceMap::new();
    let param_list = cx.first_node(&sigs, NodeKind::ParamList);
    let param_nodes = param_list.map_or_else(Vec::new, |list| {
        Cx::nodes(&cx.sig(list))
            .into_iter()
            .filter(|&n| cx.kind(n) == NodeKind::Param)
            .collect()
    });
    for (i, param_node) in param_nodes.into_iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        let param_span = cx.node_span(param_node);
        let param_sigs = cx.sig(param_node);
        // [name-slot] [`:`-slot] TypeRef: the name slot is the first thing
        // before the TypeRef.
        let before_type = param_sigs
            .iter()
            .position(|&s| cx.is_node(s, NodeKind::TypeRef))
            .unwrap_or(param_sigs.len());
        let (local_name, local_prov) = cx.name_in(
            param_sigs[..before_type].first().copied(),
            Span::new(param_span.file, param_span.start, param_span.start),
        );
        let (ty, ty_prov) = cx.type_in(
            cx.first_node(&param_sigs, NodeKind::TypeRef),
            cx.end_of(param_span),
        );
        let local = LocalId(index);
        locals.push(Local { name: local_name });
        local_provenance.insert(local, local_prov);
        params.push(Param { local, ty });
        type_provenance.insert(TypePos::Param(index), ty_prov);
    }

    let ret_at = rest.iter().position(|&s| cx.is_node(s, NodeKind::TypeRef));
    let (ret, ret_prov) = cx.type_in(
        ret_at.and_then(|i| match rest[i] {
            Sig::Node(n) => Some(n),
            Sig::Token(_) => None,
        }),
        cx.end_of(item_span),
    );
    type_provenance.insert(TypePos::Ret, ret_prov);

    // After the return type: [`=`-slot] expr. With an `=` token every node here
    // belongs to the body; without one the first node is the Missing/Error that
    // stands for the absent `=`.
    let after_type = ret_at.map_or(rest, |i| &rest[i + 1..]);
    let mut body_nodes = Cx::nodes(after_type);
    if !Cx::has_token(after_type, TokenKind::Eq) && body_nodes.len() >= 2 {
        body_nodes.remove(0);
    }
    let root = cx.slot_operand(&body_nodes, cx.end_of(item_span));
    let (exprs, expr_provenance) = cx.lower_exprs(root, diagnostics);

    let mut expr_map = ProvenanceMap::new();
    for (i, provenance) in expr_provenance.into_iter().enumerate() {
        expr_map.insert(ExprId(u32::try_from(i).unwrap_or(u32::MAX)), provenance);
    }

    LoweredFn {
        name,
        params,
        ret,
        body: Body {
            locals,
            exprs,
            root: ExprId(0),
        },
        provenance: FnProvenance {
            item: Provenance::Source(item_span),
            name: name_provenance,
            locals: local_provenance,
            exprs: expr_map,
            types: type_provenance,
        },
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// What stands in an expression slot.
#[derive(Clone, Copy)]
enum Operand {
    /// Exactly one CST expression node.
    Node(usize),
    /// Nothing at all (only from a CST the parser would not emit).
    Absent(Span),
    /// More than one node where one expression belongs: the parser's depth
    /// limit recovery emits the swallowed tail as a sibling `Error` node next
    /// to the expression. The slot becomes a single `Expr::Error` covering all
    /// of it, so no swallowed text is silently repaired away.
    Tangled(Span),
}

/// One expression with its operands still unlowered.
enum Shape {
    Leaf(Expr, Provenance),
    Binary {
        lhs: Operand,
        rhs: Operand,
        at: Provenance,
    },
}

/// Which operand of its parent an expression is.
#[derive(Clone, Copy)]
enum Side {
    Lhs,
    Rhs,
}

impl<'a> Cx<'a> {
    fn slot_operand(&self, nodes: &[usize], absent_at: Span) -> Operand {
        match nodes {
            [] => Operand::Absent(absent_at),
            [one] => Operand::Node(*one),
            [first, .., last] => {
                let (a, b) = (self.node_span(*first), self.node_span(*last));
                Operand::Tangled(a.cover(b).unwrap_or(a))
            }
        }
    }

    /// The operand inside a `ParenExpr`: `[ ( ] expr [ ) ]`, where a missing or
    /// unexpected `)` is a trailing Missing/Error node (not part of the
    /// expression) that is dropped from the slot.
    fn paren_inner(&self, paren: usize) -> Operand {
        let sigs = self.sig(paren);
        let mut nodes = Self::nodes(&sigs);
        if !Self::has_token(&sigs, TokenKind::RParen) && nodes.len() >= 2 {
            nodes.pop();
        }
        self.slot_operand(&nodes, self.end_of(self.node_span(paren)))
    }

    /// Law `(e)` = `e`, at any depth: iterative, so redundant parentheses cost
    /// no stack.
    fn strip_parens(&self, mut operand: Operand) -> Operand {
        while let Operand::Node(node) = operand {
            if self.kind(node) != NodeKind::ParenExpr {
                break;
            }
            operand = self.paren_inner(node);
        }
        operand
    }

    fn shape(&self, operand: Operand, diagnostics: &mut DiagnosticSet) -> Shape {
        let synthesized = |origin, why| Provenance::Synthesized { origin, why };
        let node = match self.strip_parens(operand) {
            Operand::Node(node) => node,
            Operand::Absent(at) => {
                return Shape::Leaf(Expr::Missing, synthesized(at, WHY_MISSING_EXPR));
            }
            Operand::Tangled(at) => {
                return Shape::Leaf(Expr::Error, synthesized(at, WHY_ERRONEOUS_EXPR));
            }
        };
        let at = self.node_span(node);
        let sigs = self.sig(node);
        match self.kind(node) {
            NodeKind::BinExpr => {
                let nodes = Self::nodes(&sigs);
                let operand_at = |n: Option<&usize>, absent| {
                    n.map_or(Operand::Absent(absent), |&n| Operand::Node(n))
                };
                Shape::Binary {
                    lhs: operand_at(nodes.first(), Span::new(at.file, at.start, at.start)),
                    rhs: operand_at(nodes.get(1), self.end_of(at)),
                    at: Provenance::Source(at),
                }
            }
            NodeKind::LiteralExpr => {
                let literal = self
                    .first_token(&sigs, TokenKind::Int)
                    .and_then(|token| self.text(token).map(|text| (token, text)));
                match literal {
                    Some((token, text)) => {
                        let at = self.token_span(token);
                        match text.parse::<i64>() {
                            Ok(value) => Shape::Leaf(Expr::Int(value), Provenance::Source(at)),
                            Err(_) => {
                                diagnostics.push(Diagnostic::error(
                                    Phase::Hir,
                                    CODE_INT_OUT_OF_RANGE,
                                    format!(
                                        "integer literal `{}` does not fit in i64",
                                        abbreviate(text)
                                    ),
                                    Provenance::Source(at),
                                ));
                                Shape::Leaf(Expr::Error, synthesized(at, WHY_INT_OUT_OF_RANGE))
                            }
                        }
                    }
                    None => Shape::Leaf(Expr::Error, synthesized(at, WHY_ERRONEOUS_EXPR)),
                }
            }
            NodeKind::PathExpr => {
                match self
                    .first_token(&sigs, TokenKind::Ident)
                    .and_then(|token| self.text(token).map(|text| (token, text)))
                {
                    Some((token, text)) => Shape::Leaf(
                        Expr::Path(text.to_owned()),
                        Provenance::Source(self.token_span(token)),
                    ),
                    None => Shape::Leaf(Expr::Error, synthesized(at, WHY_ERRONEOUS_EXPR)),
                }
            }
            NodeKind::Missing => Shape::Leaf(Expr::Missing, synthesized(at, WHY_MISSING_EXPR)),
            // `Error`, and any node kind that cannot stand for an expression.
            NodeKind::Error
            | NodeKind::File
            | NodeKind::Fn
            | NodeKind::ParamList
            | NodeKind::Param
            | NodeKind::TypeRef
            | NodeKind::ParenExpr => Shape::Leaf(Expr::Error, synthesized(at, WHY_ERRONEOUS_EXPR)),
        }
    }

    /// Lower an expression tree to the body arena in pre-order (node, lhs
    /// subtree, rhs subtree) with an explicit work stack. Returns the
    /// expressions and, parallel to them, their provenance.
    fn lower_exprs(
        &self,
        root: Operand,
        diagnostics: &mut DiagnosticSet,
    ) -> (Vec<Expr>, Vec<Provenance>) {
        let mut exprs: Vec<Expr> = Vec::new();
        let mut provenance: Vec<Provenance> = Vec::new();
        let mut work: Vec<(Operand, Option<(ExprId, Side)>)> = vec![(root, None)];
        while let Some((operand, parent)) = work.pop() {
            let id = ExprId(u32::try_from(exprs.len()).unwrap_or(u32::MAX));
            if let Some((parent_id, side)) = parent {
                if let Some(Expr::Binary { lhs, rhs, .. }) = exprs.get_mut(parent_id.0 as usize) {
                    match side {
                        Side::Lhs => *lhs = id,
                        Side::Rhs => *rhs = id,
                    }
                }
            }
            match self.shape(operand, diagnostics) {
                Shape::Leaf(expr, at) => {
                    exprs.push(expr);
                    provenance.push(at);
                }
                Shape::Binary { lhs, rhs, at } => {
                    // Operand ids are patched in when the operands are popped.
                    exprs.push(Expr::Binary {
                        op: BinOp::Add,
                        lhs: id,
                        rhs: id,
                    });
                    provenance.push(at);
                    // LIFO: lhs is popped (and its whole subtree lowered) first.
                    work.push((rhs, Some((id, Side::Rhs))));
                    work.push((lhs, Some((id, Side::Lhs))));
                }
            }
        }
        (exprs, provenance)
    }
}

/// A literal shown in a diagnostic: long digit runs are cut so a hostile
/// megabyte literal does not become a megabyte message.
fn abbreviate(text: &str) -> String {
    const KEEP: usize = 24;
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(KEEP).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_phases::FileId;
    use tessera_syntax::cst::parse_file;

    fn parse(src: &str) -> ParsedFile {
        parse_file(FileId(0), src).value
    }

    #[test]
    fn disambiguators_count_earlier_equal_names_only() {
        let d = |names: &[&str]| disambiguators(names.iter().copied());
        assert_eq!(d(&[]), Vec::<u32>::new());
        assert_eq!(d(&["add"]), vec![0]);
        assert_eq!(d(&["add", "add", "add"]), vec![0, 1, 2]);
        // independent counters per name, in source order
        assert_eq!(d(&["a", "b", "a", "c", "b", "a"]), vec![0, 0, 1, 0, 1, 2]);
        // nameless items (`""`) are disambiguated among themselves
        assert_eq!(d(&["", "x", ""]), vec![0, 0, 1]);
    }

    #[test]
    fn disambiguators_depend_on_names_and_order_only() {
        // INV-ID-1: same names in the same order give the same ids no matter
        // what else about the items differs (this is a function of names alone).
        let first = disambiguators(["f", "g", "f"]);
        let second = disambiguators(["f", "g", "f"]);
        assert_eq!(first, second);
        // Reordering same-named siblings only permutes among equal names.
        assert_eq!(disambiguators(["g", "f", "f"]), vec![0, 0, 1]);
    }

    #[test]
    fn lowering_n_functions_disambiguates_same_named_items() {
        // Today's parser emits one `Fn` per file, so drive the N-function path
        // by lowering the same `Fn` node three times plus a differently named one.
        let src = "f add(a:i64)>i64=a";
        let parsed = parse(src);
        let cx = Cx {
            parsed: &parsed,
            src,
        };
        let fn_node = parsed
            .cst
            .children(parsed.cst.root())
            .iter()
            .find_map(|c| match *c {
                Child::Node(n) if parsed.cst.kind(n) == NodeKind::Fn => Some(n),
                _ => None,
            })
            .expect("one Fn");
        let out = lower_items(&cx, &[fn_node, fn_node, fn_node]);
        let paths: Vec<_> = out
            .value
            .module
            .items
            .iter()
            .map(|item| item.id().path())
            .collect();
        assert_eq!(paths, ["fn/add", "fn/add#1", "fn/add#2"]);
        // one provenance record per item, parallel to `items`
        assert_eq!(out.value.provenance.items.len(), 3);
        assert!(out.is_clean());
    }

    #[test]
    fn lowering_no_functions_gives_an_empty_module() {
        let parsed = parse("");
        let out = lower(&parsed, "");
        assert!(out.value.module.items.is_empty());
        assert!(out.value.provenance.items.is_empty());
        assert_eq!(out.value.module.file, FileId(0));
    }

    #[test]
    fn a_source_that_does_not_match_the_tokens_degrades_instead_of_panicking() {
        let parsed = parse("f add(a:i64,b:i64)>i64=a+b");
        for wrong in ["", "f", "f add(", "é", "f add(a:i64,b:i64)>i64=a+"] {
            let out = lower(&parsed, wrong);
            assert_eq!(
                out.value.provenance.items.len(),
                out.value.module.items.len()
            );
        }
    }

    #[test]
    fn abbreviate_cuts_long_literals_on_char_boundaries() {
        assert_eq!(abbreviate("123"), "123");
        assert_eq!(abbreviate(&"9".repeat(24)), "9".repeat(24));
        assert_eq!(
            abbreviate(&"9".repeat(25)),
            format!("{}...", "9".repeat(24))
        );
        assert_eq!(
            abbreviate(&"é".repeat(30)),
            format!("{}...", "é".repeat(24))
        );
    }
}
