//! Resolved/typed HIR dump (contract §2.6: "B2 dump with `Res` annotations
//! ... with `: Ty` annotations").
//!
//! One S-expression per function, the arena in id order (like the HIR dump,
//! it never walks the tree), each expression with its type and, for paths,
//! what it resolved to. No spans: those live in the HIR provenance dump.
//! Internal format for snapshots and debugging; total on any input.
//!
//! ```text
//! (fn fn/add (sig (i64 i64) i64)
//!   (e0 (binary add e1 e2) : i64)
//!   (e1 (path "a") -> local/a : i64)
//!   (e2 (path "b") -> local/b : i64))
//! ```

use std::fmt::Write as _;

use tessera_hir::{Expr, ExprId, HirOutput, Item};

use crate::input::id32;
use crate::resolve::{Res, ResolvedModule};
use crate::typeck::TypedModule;

/// Deterministic snapshot text for `hir` annotated with `res` and `typed`.
#[must_use]
pub fn dump(hir: &HirOutput, res: &ResolvedModule, typed: &TypedModule) -> String {
    let mut out = String::new();
    for (i, item) in hir.module.items.iter().enumerate() {
        let Item::Fn(func) = item;
        let path = func.id.path();
        let (Some(rfn), Some(tfn)) = (res.funcs.get(i), typed.funcs.get(i)) else {
            let _ = writeln!(out, "(fn {path} (no facts))");
            continue;
        };
        if !rfn.well_formed {
            let _ = writeln!(out, "(fn {path} malformed)");
            continue;
        }
        let params: Vec<&str> = rfn.params.iter().map(|t| t.as_str()).collect();
        let _ = write!(out, "(fn {path} (sig ({}) {})", params.join(" "), rfn.ret);
        for (e, expr) in func.body.exprs.iter().enumerate() {
            let id = ExprId(id32(e));
            let _ = write!(out, "\n  (e{e} ");
            match expr {
                Expr::Int(v) => {
                    let _ = write!(out, "(int {v})");
                }
                Expr::Path(name) => {
                    let _ = write!(out, "(path {name:?}) -> ");
                    match rfn.names.get(&id) {
                        Some(Res::Local(local)) => {
                            let name = func
                                .body
                                .locals
                                .get(local.0 as usize)
                                .and_then(|l| l.name.as_deref())
                                .unwrap_or("?");
                            let _ = write!(out, "local/{name}");
                        }
                        Some(Res::Unresolved) | None => out.push_str("unresolved"),
                    }
                }
                Expr::Binary { op, lhs, rhs } => {
                    let _ = write!(out, "(binary {} e{} e{})", op.name(), lhs.0, rhs.0);
                }
                Expr::Error => out.push_str("(error)"),
                Expr::Missing => out.push_str("(missing)"),
            }
            let _ = write!(out, " : {})", tfn.expr(id));
        }
        out.push_str(")\n");
    }
    out
}
