//! Sema on hand-built HIR: rules the TC grammar cannot reach yet (a `bool`
//! primitive, duplicate items), malformed input, hostile depth, and the
//! identity/provenance split.

use tessera_hir::{
    BinOp, Body, Expr, ExprId, FnItem, FnProvenance, HirModule, HirOutput, HirProvenance, Item,
    ItemId, ItemKind, Local, LocalId, Param, TypePos, TypeRef,
};
use tessera_phases::{FileId, Provenance, ProvenanceMap, Span};
use tessera_tir::MAX_TIR_DEPTH;

use crate::{Res, Ty, V0_PRIMS, analyze, dump, resolve, resolve_with, to_tir, typeck};

const F: FileId = FileId(0);

/// Tiny expression tree, flattened to a pre-order arena by [`func`].
enum E {
    Int(i64),
    P(&'static str),
    Add(Box<E>, Box<E>),
    Missing,
}

fn add(l: E, r: E) -> E {
    E::Add(Box::new(l), Box::new(r))
}

fn ty(name: &str) -> TypeRef {
    TypeRef::Path(name.to_owned())
}

fn src(start: u32, end: u32) -> Provenance {
    Provenance::Source(Span::new(F, start, end))
}

/// One function with distinct, recognizable spans: item `0..1000`, name
/// `1..2`, local `i` at `10+i`, type pos `i` at `20+i`, ret `30`, expr `i` at
/// `100+i`. `shift` moves every span (a "reformatting").
fn func(
    name: &str,
    disambiguator: u32,
    params: &[(&str, TypeRef)],
    ret: TypeRef,
    body: E,
    shift: u32,
) -> (FnItem, FnProvenance) {
    let s = |a: u32| src(a + shift, a + shift + 1);
    let mut exprs = Vec::new();
    let mut stack = vec![(body, None::<(usize, bool)>)];
    while let Some((e, parent)) = stack.pop() {
        let id = exprs.len();
        if let Some((p, is_lhs)) = parent {
            if let Expr::Binary { lhs, rhs, .. } = &mut exprs[p] {
                *(if is_lhs { lhs } else { rhs }) = ExprId(id as u32);
            }
        }
        match e {
            E::Int(v) => exprs.push(Expr::Int(v)),
            E::P(n) => exprs.push(Expr::Path(n.to_owned())),
            E::Missing => exprs.push(Expr::Missing),
            E::Add(l, r) => {
                exprs.push(Expr::Binary {
                    op: BinOp::Add,
                    lhs: ExprId(0),
                    rhs: ExprId(0),
                });
                stack.push((*r, Some((id, false))));
                stack.push((*l, Some((id, true))));
            }
        }
    }
    let mut prov = FnProvenance {
        item: src(shift, shift + 1000),
        name: s(1),
        locals: ProvenanceMap::new(),
        exprs: ProvenanceMap::new(),
        types: ProvenanceMap::new(),
    };
    for i in 0..exprs.len() {
        prov.exprs.insert(ExprId(i as u32), s(100 + i as u32));
    }
    let mut locals = Vec::new();
    let mut ps = Vec::new();
    for (i, (pname, pty)) in params.iter().enumerate() {
        locals.push(Local {
            name: Some((*pname).to_owned()),
        });
        ps.push(Param {
            local: LocalId(i as u32),
            ty: pty.clone(),
        });
        prov.locals.insert(LocalId(i as u32), s(10 + i as u32));
        prov.types
            .insert(TypePos::Param(i as u32), s(20 + i as u32));
    }
    prov.types.insert(TypePos::Ret, s(30));
    let item = FnItem {
        id: ItemId {
            file: F,
            kind: ItemKind::Fn,
            name: name.to_owned(),
            disambiguator,
        },
        name: Some(name.to_owned()),
        params: ps,
        ret,
        body: Body {
            locals,
            exprs,
            root: ExprId(0),
        },
    };
    (item, prov)
}

fn module(funcs: Vec<(FnItem, FnProvenance)>) -> HirOutput {
    let (items, provs): (Vec<_>, Vec<_>) = funcs.into_iter().map(|(f, p)| (Item::Fn(f), p)).unzip();
    HirOutput {
        module: HirModule { file: F, items },
        provenance: HirProvenance { items: provs },
    }
}

fn codes(d: &tessera_phases::DiagnosticSet) -> Vec<&'static str> {
    d.iter().map(|d| d.code).collect()
}

fn bootstrap(shift: u32) -> HirOutput {
    module(vec![func(
        "add",
        0,
        &[("a", ty("i64")), ("b", ty("i64"))],
        ty("i64"),
        add(E::P("a"), E::P("b")),
        shift,
    )])
}

#[test]
fn bootstrap_resolves_types_and_lowers() {
    let out = analyze(&bootstrap(0));
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    let rfn = &out.value.resolved.funcs[0];
    assert_eq!(rfn.names.get(&ExprId(1)), Some(&Res::Local(LocalId(0))));
    assert_eq!(rfn.names.get(&ExprId(2)), Some(&Res::Local(LocalId(1))));
    assert_eq!(out.value.typed.funcs[0].exprs, [Ty::I64; 3]);
    let tir = &out.value.tir;
    assert_eq!(
        tir.module.to_text(),
        "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))"
    );
    let fp = &tir.provenance.funcs[0];
    assert!(fp.missing(&tir.module.funcs[0]).is_empty());
    // A parameter's provenance covers its name and its type annotation.
    assert_eq!(fp.params, [src(10, 21), src(11, 22)]);
    assert_eq!(fp.func, src(0, 1000));
}

#[test]
fn dump_shows_resolution_and_types() {
    let hir = bootstrap(0);
    let res = resolve(&hir).value;
    let typed = typeck(&hir, &res).value;
    assert_eq!(
        dump(&hir, &res, &typed),
        "\
(fn fn/add (sig (i64 i64) i64)
  (e0 (binary add e1 e2) : i64)
  (e1 (path \"a\") -> local/a : i64)
  (e2 (path \"b\") -> local/b : i64))
"
    );
}

/// INV-ID-2 / early cutoff: moving every span changes only provenance, never
/// a semantic value.
#[test]
fn reformatting_changes_provenance_but_no_semantic_value() {
    let (a, b) = (analyze(&bootstrap(0)).value, analyze(&bootstrap(7)).value);
    assert_eq!(a.resolved, b.resolved);
    assert_eq!(a.typed, b.typed);
    assert_eq!(a.tir.module, b.tir.module);
    assert_ne!(a.tir.provenance, b.tir.provenance);
}

#[test]
fn analysis_is_deterministic() {
    let hir = module(vec![
        func(
            "f",
            0,
            &[("a", ty("bool"))],
            ty("i64"),
            add(E::P("a"), E::P("q")),
            0,
        ),
        func("g", 0, &[], ty("i64"), E::Int(1), 0),
    ]);
    assert_eq!(analyze(&hir), analyze(&hir));
}

/// Each independent mistake once; `Ty::Error` swallows the consequences.
#[test]
fn independent_mistakes_are_each_reported_once() {
    let hir = module(vec![func(
        "f",
        0,
        &[("a", ty("bool")), ("b", ty("i64"))],
        ty("i32"),
        add(add(E::P("a"), E::P("q")), E::P("b")),
        0,
    )]);
    let out = analyze(&hir);
    assert_eq!(
        codes(&out.diagnostics),
        [
            "E-resolve-unknown-type",
            "E-resolve-unknown-type",
            "E-resolve-unbound-name"
        ]
    );
    // Pointing at the annotation and the occurrence, not the item.
    let starts: Vec<u32> = out
        .diagnostics
        .iter()
        .map(|d| d.at.primary_span().start)
        .collect();
    assert_eq!(starts, [20, 30, 103]);
    assert!(out.value.tir.module.funcs.is_empty());
    assert_eq!(out.value.typed.funcs[0].exprs[0], Ty::Error);
}

/// The typing rules, exercised through a primitive table that knows `bool`
/// (the v0 table does not, so TC cannot reach these yet).
#[test]
fn typing_rules_with_a_bool_primitive() {
    let prims = &[("i64", Ty::I64), ("bool", Ty::Bool)];
    let hir = module(vec![
        func(
            "mix",
            0,
            &[("a", ty("bool"))],
            ty("i64"),
            add(E::P("a"), E::Int(1)),
            0,
        ),
        func("ret", 0, &[("a", ty("bool"))], ty("i64"), E::P("a"), 0),
        func("id", 0, &[("a", ty("bool"))], ty("bool"), E::P("a"), 0),
    ]);
    let res = resolve_with(&hir, prims);
    assert!(res.diagnostics.is_empty());
    let typed = typeck(&hir, &res.value);
    assert_eq!(
        codes(&typed.diagnostics),
        ["E-type-mismatch", "E-type-return-mismatch"]
    );
    let tir = to_tir(&hir, &res.value, &typed.value);
    assert!(tir.diagnostics.is_empty());
    assert_eq!(
        tir.value.module.to_text(),
        "(func id (param a bool) (return bool) (body (var a bool)))"
    );
}

#[test]
fn duplicate_parameters_first_wins_and_block_tir() {
    let hir = module(vec![func(
        "f",
        0,
        &[("a", ty("i64")), ("a", ty("i64"))],
        ty("i64"),
        E::P("a"),
        0,
    )]);
    let out = analyze(&hir);
    assert_eq!(codes(&out.diagnostics), ["E-resolve-duplicate-param"]);
    assert_eq!(
        out.diagnostics
            .iter()
            .next()
            .map(|d| d.at.primary_span().start),
        Some(11),
        "points at the second parameter"
    );
    assert_eq!(
        out.value.resolved.funcs[0].names.get(&ExprId(0)),
        Some(&Res::Local(LocalId(0)))
    );
    assert!(out.value.tir.module.funcs.is_empty());
}

#[test]
fn duplicate_items_are_reported_and_only_the_first_lowers() {
    let hir = module(vec![
        func("add", 0, &[], ty("i64"), E::Int(1), 0),
        func("add", 1, &[], ty("i64"), E::Int(2), 0),
    ]);
    let out = analyze(&hir);
    assert_eq!(codes(&out.diagnostics), ["E-resolve-duplicate-item"]);
    assert_eq!(
        out.value.tir.module.to_text(),
        "(func add (return i64) (body (int 1 i64)))"
    );
}

/// Error/missing nodes were reported by the parser: sema stays silent.
#[test]
fn syntax_error_nodes_do_not_cascade() {
    let hir = module(vec![func(
        "f",
        0,
        &[("a", ty("i64"))],
        ty("i64"),
        add(E::P("a"), E::Missing),
        0,
    )]);
    let out = analyze(&hir);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        out.value.typed.funcs[0].exprs,
        [Ty::Error, Ty::I64, Ty::Error]
    );
    assert!(out.value.tir.module.funcs.is_empty());
}

#[test]
fn malformed_hir_is_reported_once_and_skipped() {
    // An operand that points backwards (not pre-order).
    let (mut f, p) = func("f", 0, &[], ty("i64"), add(E::Int(1), E::Int(2)), 0);
    f.body.exprs[0] = Expr::Binary {
        op: BinOp::Add,
        lhs: ExprId(0),
        rhs: ExprId(2),
    };
    // A function without a provenance table.
    let (g, _) = func("g", 0, &[], ty("i64"), E::Int(1), 0);
    let hir = HirOutput {
        module: HirModule {
            file: F,
            items: vec![Item::Fn(f), Item::Fn(g)],
        },
        provenance: HirProvenance { items: vec![p] },
    };
    let out = analyze(&hir);
    assert_eq!(
        codes(&out.diagnostics),
        ["E-hir-malformed", "E-hir-malformed"]
    );
    assert!(out.value.resolved.funcs.iter().all(|r| !r.well_formed));
    assert!(out.value.tir.module.funcs.is_empty());
    let text = dump(&hir, &out.value.resolved, &out.value.typed);
    assert_eq!(text, "(fn fn/f malformed)\n(fn fn/g malformed)\n");
}

/// A left-leaning chain of `n` additions, built straight into the arena (no
/// recursion): `Add_k` is `ek`, the innermost leaf `en`, `Add_k`'s right
/// operand `e(2n-k)`.
fn chain(n: u32) -> HirOutput {
    let (mut f, mut p) = func("deep", 0, &[], ty("i64"), E::Int(0), 0);
    let mut exprs = Vec::new();
    for k in 0..n {
        exprs.push(Expr::Binary {
            op: BinOp::Add,
            lhs: ExprId(k + 1),
            rhs: ExprId(2 * n - k),
        });
    }
    exprs.push(Expr::Int(1));
    for _ in 0..n {
        exprs.push(Expr::Int(1));
    }
    for i in 0..exprs.len() {
        p.exprs.insert(ExprId(i as u32), src(100, 101));
    }
    f.body.exprs = exprs;
    module(vec![(f, p)])
}

/// Hand-built HIR can be deeper than TIR allows; that is a diagnostic, never
/// a stack overflow in sema or in a TIR consumer.
#[test]
fn hir_deeper_than_tir_allows_is_an_error_not_a_crash() {
    let max = MAX_TIR_DEPTH as u32;
    let ok = analyze(&chain(max - 1));
    assert!(ok.diagnostics.is_empty(), "{:?}", ok.diagnostics);
    assert_eq!(ok.value.tir.module.funcs.len(), 1);

    let deep = analyze(&chain(100_000));
    assert_eq!(codes(&deep.diagnostics), ["E-tir-too-deep"]);
    assert!(deep.value.tir.module.funcs.is_empty());
    assert!(
        deep.value.typed.funcs[0]
            .exprs
            .iter()
            .all(|t| *t == Ty::I64)
    );
}

#[test]
fn v0_primitives_are_exactly_i64() {
    assert_eq!(V0_PRIMS, &[("i64", Ty::I64)]);
}
