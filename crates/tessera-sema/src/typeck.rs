//! Type checking (contract B4).
//!
//! Assigns a [`Ty`] to every expression of every well-formed function. The
//! HIR arena puts operands after their parent (checked on input), so one
//! reverse pass over the ids types children before parents: no recursion, no
//! inference variables (the v0 rules are all local).
//!
//! [`Ty::Error`] is contagious and silent: an operand that already failed
//! (syntax error, unresolved name, unknown type) makes its parent `Error`
//! without a second diagnostic, so each mistake is reported exactly once
//! while independent mistakes are all reported.

use tessera_hir::{BinOp, Expr, ExprId, FnItem, HirOutput, Item, ItemId};
use tessera_phases::{Diagnostic, DiagnosticSet, Phase, PhaseOutput};

use crate::Ty;
use crate::input::{Site, id32};
use crate::resolve::{Res, ResolvedFn, ResolvedModule};

/// Type facts for one function.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypedFn {
    pub item: ItemId,
    /// The type of each expression, indexed by `ExprId`. Empty for a function
    /// that failed sema's input check.
    pub exprs: Vec<Ty>,
}

impl TypedFn {
    #[must_use]
    pub fn expr(&self, id: ExprId) -> Ty {
        self.exprs.get(id.0 as usize).copied().unwrap_or(Ty::Error)
    }
}

/// Type facts for one file, parallel to `HirModule::items`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypedModule {
    pub funcs: Vec<TypedFn>,
}

/// Type every function of `hir` using the resolution facts `res`.
#[must_use]
pub fn typeck(hir: &HirOutput, res: &ResolvedModule) -> PhaseOutput<TypedModule> {
    let mut diagnostics = DiagnosticSet::new();
    let funcs = hir
        .module
        .items
        .iter()
        .zip(&res.funcs)
        .enumerate()
        .map(|(i, (item, rfn))| {
            let Item::Fn(func) = item;
            let site = Site::new(hir.module.file, hir.provenance.items.get(i));
            typeck_fn(func, rfn, &site, &mut diagnostics)
        })
        .collect();
    PhaseOutput::with(TypedModule { funcs }, diagnostics)
}

fn typeck_fn(
    func: &FnItem,
    rfn: &ResolvedFn,
    site: &Site<'_>,
    diagnostics: &mut DiagnosticSet,
) -> TypedFn {
    if !rfn.well_formed {
        return TypedFn {
            item: func.id.clone(),
            exprs: Vec::new(),
        };
    }
    let exprs = &func.body.exprs;
    let mut tys = vec![Ty::Error; exprs.len()];
    for i in (0..exprs.len()).rev() {
        let at = |e: ExprId| tys.get(e.0 as usize).copied().unwrap_or(Ty::Error);
        tys[i] = match &exprs[i] {
            Expr::Int(_) => Ty::I64,
            Expr::Path(_) => match rfn.names.get(&ExprId(id32(i))) {
                Some(Res::Local(local)) => rfn.local_ty(func, *local),
                Some(Res::Unresolved) | None => Ty::Error,
            },
            Expr::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => match (at(*lhs), at(*rhs)) {
                (Ty::I64, Ty::I64) => Ty::I64,
                (Ty::Error, _) | (_, Ty::Error) => Ty::Error,
                (l, r) => {
                    diagnostics.push(Diagnostic::error(
                        Phase::Typeck,
                        "E-type-mismatch",
                        format!("`+` takes two i64 operands, found {l} and {r}"),
                        site.expr(ExprId(id32(i))),
                    ));
                    Ty::Error
                }
            },
            // The parser or HIR lowering already reported these.
            Expr::Error | Expr::Missing => Ty::Error,
        };
    }
    let root = func.body.root;
    let body = tys.get(root.0 as usize).copied().unwrap_or(Ty::Error);
    if body != Ty::Error && rfn.ret != Ty::Error && body != rfn.ret {
        diagnostics.push(Diagnostic::error(
            Phase::Typeck,
            "E-type-return-mismatch",
            format!(
                "`{}` returns {} but its body has type {body}",
                func.id.name, rfn.ret
            ),
            site.expr(root),
        ));
    }
    TypedFn {
        item: func.id.clone(),
        exprs: tys,
    }
}
