//! Canonical HIR text dump (contract §2.6): S-expression, IDs printed as paths,
//! no spans inline, spans in a separate `provenance:` section. See
//! `docs/architecture/hir.md` for the format.
//!
//! The dump is a debug/snapshot artifact, not a public format. It is total: it
//! accepts any [`HirModule`]/[`HirProvenance`] pair, including hand-built ones
//! with dangling ids or provenance that does not line up with the module, and
//! never panics or loops. It does not walk the expression tree (that would
//! need recursion, and a cycle in a hand-built body would never terminate);
//! it lists the arena in id order and names children by id.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use tessera_phases::{FileId, Provenance, ProvenanceMap, Span};

use crate::lower::disambiguators;
use crate::{
    Body, Expr, ExprId, FnItem, FnProvenance, HirModule, HirProvenance, Item, LocalId, TypePos,
    TypeRef,
};

/// Deterministic snapshot text for goldens and humans.
#[must_use]
pub fn dump(module: &HirModule, provenance: &HirProvenance) -> String {
    let mut out = String::new();
    let _ = write!(out, "(module file/{}", module.file.0);
    for item in &module.items {
        let Item::Fn(func) = item;
        dump_fn(func, &mut out);
    }
    out.push_str(")\nprovenance:\n");
    for (i, item) in module.items.iter().enumerate() {
        let Item::Fn(func) = item;
        let _ = writeln!(out, "  {}", func.id.path());
        match provenance.items.get(i) {
            Some(prov) => dump_fn_provenance(module.file, func, prov, &mut out),
            None => out.push_str("    (no provenance table)\n"),
        }
    }
    let extra = provenance.items.len().saturating_sub(module.items.len());
    if extra > 0 {
        let _ = writeln!(out, "  (+{extra} provenance tables without an item)");
    }
    out
}

fn quote(text: &str) -> String {
    format!("{text:?}")
}

fn type_text(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Path(name) => format!("(path {})", quote(name)),
        TypeRef::Missing => "(missing)".to_owned(),
        TypeRef::Error => "(error)".to_owned(),
    }
}

fn expr_text(expr: &Expr) -> String {
    match expr {
        Expr::Int(value) => format!("(int {value})"),
        Expr::Path(name) => format!("(path {})", quote(name)),
        Expr::Binary { op, lhs, rhs } => {
            format!("(binary {} e{} e{})", op.name(), lhs.0, rhs.0)
        }
        Expr::Error => "(error)".to_owned(),
        Expr::Missing => "(missing)".to_owned(),
    }
}

/// `local/a`, with `#k` for the k-th later local of the same name, and `?` for a
/// local without a name: the same rule as `ItemId::path`.
fn local_paths(body: &Body) -> Vec<String> {
    let keys: Vec<&str> = body
        .locals
        .iter()
        .map(|local| local.name.as_deref().unwrap_or("?"))
        .collect();
    let counts = disambiguators(keys.iter().copied());
    keys.iter()
        .zip(counts)
        .map(|(key, count)| {
            if count == 0 {
                format!("local/{key}")
            } else {
                format!("local/{key}#{count}")
            }
        })
        .collect()
}

fn local_label(paths: &[String], id: LocalId) -> String {
    paths
        .get(id.0 as usize)
        .cloned()
        .unwrap_or_else(|| format!("local/!{}", id.0))
}

fn dump_fn(func: &FnItem, out: &mut String) {
    let paths = local_paths(&func.body);
    let _ = write!(out, "\n  (fn {}", func.id.path());
    match &func.name {
        Some(name) => {
            let _ = write!(out, "\n    (name {})", quote(name));
        }
        None => out.push_str("\n    (name none)"),
    }
    for param in &func.params {
        let _ = write!(
            out,
            "\n    (param {} {})",
            local_label(&paths, param.local),
            type_text(&param.ty)
        );
    }
    // Locals no parameter binds (none exist until the grammar has `let`).
    let bound: BTreeSet<u32> = func.params.iter().map(|p| p.local.0).collect();
    for (i, path) in paths.iter().enumerate() {
        if !u32::try_from(i).is_ok_and(|i| bound.contains(&i)) {
            let _ = write!(out, "\n    (local {path})");
        }
    }
    let _ = write!(out, "\n    (ret {})", type_text(&func.ret));
    let _ = write!(out, "\n    (root e{})", func.body.root.0);
    out.push_str("\n    (exprs");
    for (i, expr) in func.body.exprs.iter().enumerate() {
        let _ = write!(out, "\n      (e{i} {})", expr_text(expr));
    }
    out.push_str("))");
}

fn span_text(home: FileId, span: Span) -> String {
    if span.file == home {
        format!("[{}..{})", span.start, span.end)
    } else {
        format!("[file/{}:{}..{})", span.file.0, span.start, span.end)
    }
}

fn provenance_text(home: FileId, provenance: Option<Provenance>) -> String {
    match provenance {
        Some(Provenance::Source(span)) => format!("{} source", span_text(home, span)),
        Some(Provenance::Synthesized { origin, why }) => {
            format!("{} synthesized {why}", span_text(home, origin))
        }
        None => "MISSING".to_owned(),
    }
}

/// The ids a table should cover, plus any extra ids it holds, in id order. A
/// table that is not total therefore shows `MISSING` rows instead of hiding
/// the gap; an entry for an id the function does not have shows up too.
fn rows<Id: Ord + Copy>(
    expected: impl IntoIterator<Item = Id>,
    table: &ProvenanceMap<Id>,
) -> Vec<(Id, Option<Provenance>)> {
    let mut ids: BTreeSet<Id> = expected.into_iter().collect();
    ids.extend(table.iter().map(|(id, _)| id));
    ids.into_iter().map(|id| (id, table.get(id))).collect()
}

fn dump_fn_provenance(home: FileId, func: &FnItem, prov: &FnProvenance, out: &mut String) {
    let line = |out: &mut String, label: &str, p: Option<Provenance>| {
        let _ = writeln!(out, "    {label} {}", provenance_text(home, p));
    };
    line(out, "item", Some(prov.item));
    line(out, "name", Some(prov.name));

    let paths = local_paths(&func.body);
    let locals = (0..func.body.locals.len()).map(|i| LocalId(u32::try_from(i).unwrap_or(u32::MAX)));
    for (id, p) in rows(locals, &prov.locals) {
        line(out, &local_label(&paths, id), p);
    }

    let types = (0..func.params.len())
        .map(|i| TypePos::Param(u32::try_from(i).unwrap_or(u32::MAX)))
        .chain(std::iter::once(TypePos::Ret));
    for (pos, p) in rows(types, &prov.types) {
        let label = match pos {
            TypePos::Param(i) => format!("type/param{i}"),
            TypePos::Ret => "type/ret".to_owned(),
        };
        line(out, &label, p);
    }

    let exprs = (0..func.body.exprs.len()).map(|i| ExprId(u32::try_from(i).unwrap_or(u32::MAX)));
    for (id, p) in rows(exprs, &prov.exprs) {
        line(out, &format!("e{}", id.0), p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BinOp, ItemId, ItemKind, Local, Param};

    fn item(name: Option<&str>, locals: &[Option<&str>], exprs: Vec<Expr>) -> Item {
        Item::Fn(FnItem {
            id: ItemId {
                file: FileId(0),
                kind: ItemKind::Fn,
                name: name.unwrap_or("").to_owned(),
                disambiguator: 0,
            },
            name: name.map(str::to_owned),
            params: (0..locals.len())
                .map(|i| Param {
                    local: LocalId(u32::try_from(i).unwrap()),
                    ty: TypeRef::Path("i64".to_owned()),
                })
                .collect(),
            ret: TypeRef::Missing,
            body: Body {
                locals: locals
                    .iter()
                    .map(|n| Local {
                        name: n.map(str::to_owned),
                    })
                    .collect(),
                exprs,
                root: ExprId(0),
            },
        })
    }

    #[test]
    fn duplicate_and_missing_local_names_get_distinct_paths() {
        let module = HirModule {
            file: FileId(0),
            items: vec![item(
                Some("f"),
                &[Some("a"), None, Some("a"), None],
                vec![Expr::Int(1)],
            )],
        };
        let text = dump(&module, &HirProvenance::default());
        for want in [
            "(param local/a (path \"i64\"))",
            "(param local/? (path \"i64\"))",
            "(param local/a#1 (path \"i64\"))",
            "(param local/?#1 (path \"i64\"))",
        ] {
            assert!(text.contains(want), "{want} in\n{text}");
        }
    }

    #[test]
    fn dump_is_total_over_hand_built_nonsense() {
        // dangling child ids, a self-referential node, unbound locals, and a
        // provenance table that is empty / longer than the module: never a
        // panic, never a hang.
        let mut module = HirModule {
            file: FileId(0),
            items: vec![item(
                None,
                &[Some("x")],
                vec![Expr::Binary {
                    op: BinOp::Add,
                    lhs: ExprId(0),
                    rhs: ExprId(99),
                }],
            )],
        };
        let Item::Fn(func) = &mut module.items[0];
        func.body.locals.push(Local { name: None });
        func.params[0].local = LocalId(42);
        func.body.root = ExprId(7);
        let text = dump(&module, &HirProvenance::default());
        assert!(text.contains("(binary add e0 e99)"), "{text}");
        assert!(text.contains("local/!42"), "{text}");
        assert!(text.contains("(local local/?)"), "{text}");
        assert!(text.contains("(no provenance table)"), "{text}");
        assert_eq!(text, dump(&module, &HirProvenance::default()));

        let empty = HirProvenance {
            items: vec![
                crate::FnProvenance {
                    item: Provenance::Source(Span::new(FileId(1), 0, 3)),
                    name: Provenance::Synthesized {
                        origin: Span::new(FileId(0), 1, 1),
                        why: "missing-name",
                    },
                    locals: ProvenanceMap::new(),
                    exprs: ProvenanceMap::new(),
                    types: ProvenanceMap::new(),
                };
                2
            ],
        };
        let text = dump(&module, &empty);
        assert!(text.contains("item [file/1:0..3) source"), "{text}");
        assert!(
            text.contains("name [1..1) synthesized missing-name"),
            "{text}"
        );
        assert!(text.contains("e0 MISSING"), "{text}");
        assert!(
            text.contains("(+1 provenance tables without an item)"),
            "{text}"
        );
    }

    #[test]
    fn empty_module_dump() {
        let module = HirModule {
            file: FileId(3),
            items: Vec::new(),
        };
        assert_eq!(
            dump(&module, &HirProvenance::default()),
            "(module file/3)\nprovenance:\n"
        );
    }
}
