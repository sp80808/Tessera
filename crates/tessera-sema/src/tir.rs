//! Typed HIR -> TIR (contract B4 -> B5).
//!
//! Only functions whose facts are complete are lowered: well-formed, every
//! declaration named and unique, no [`Ty::Error`] in the signature or on any
//! expression, and a body of the declared return type (TYP-3: B5 is built
//! from clean typed HIR only). Incomplete
//! functions are left out silently: the reason was already reported by the
//! phase that found it.
//!
//! TIR node `k` is the `k`-th HIR expression of a pre-order walk from the root,
//! so the provenance table maps `TirNodeId(k)` to that expression's HIR
//! provenance (total, PROV-1). The walk and the build are loops, not
//! recursion; trees deeper than `MAX_TIR_DEPTH` (only possible for hand-built
//! HIR) are an error rather than a stack overflow downstream.
//!
//! The output must pass `tessera_tir::verify_module`; a finding would be a sema
//! bug and is reported as `E-tir-internal`.

use tessera_hir::{BinOp, Expr, ExprId, FnItem, HirOutput, Item, TypePos};
use tessera_phases::{Diagnostic, DiagnosticSet, Phase, PhaseOutput, Provenance, ProvenanceMap};
use tessera_tir::{
    FunctionProvenance, MAX_TIR_DEPTH, ModuleProvenance, TirExpr, TirFunction, TirModule,
    TirNodeId, TirParam, TirType, verify_module,
};

use crate::Ty;
use crate::input::{Site, id32};
use crate::resolve::{Res, ResolvedFn, ResolvedModule};
use crate::typeck::{TypedFn, TypedModule};

/// TIR for the complete functions of one file, with provenance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TirOutput {
    pub module: TirModule,
    pub provenance: ModuleProvenance,
}

/// Lower every complete function of `hir` to TIR.
#[must_use]
pub fn to_tir(
    hir: &HirOutput,
    res: &ResolvedModule,
    typed: &TypedModule,
) -> PhaseOutput<TirOutput> {
    let mut diagnostics = DiagnosticSet::new();
    let mut out = TirOutput::default();
    let facts = res.funcs.iter().zip(&typed.funcs);
    for (i, (item, (rfn, tfn))) in hir.module.items.iter().zip(facts).enumerate() {
        let Item::Fn(func) = item;
        let site = Site::new(hir.module.file, hir.provenance.items.get(i));
        if !complete(func, rfn, tfn) {
            continue;
        }
        match lower_fn(func, rfn, tfn, &site) {
            Ok((tir, prov)) => {
                out.module.funcs.push(tir);
                out.provenance.funcs.push(prov);
            }
            Err((code, message)) => {
                diagnostics.push(Diagnostic::error(Phase::Tir, code, message, site.item()));
            }
        }
    }
    for finding in verify_module(&out.module) {
        let at = out
            .module
            .funcs
            .iter()
            .position(|f| f.name == finding.func)
            .and_then(|i| out.provenance.funcs.get(i))
            .map_or_else(|| Site::new(hir.module.file, None).item(), |p| p.func);
        diagnostics.push(Diagnostic::error(
            Phase::Tir,
            "E-tir-internal",
            format!("internal error: sema produced TIR the verifier rejects: {finding}"),
            at,
        ));
    }
    PhaseOutput::with(out, diagnostics)
}

fn complete(func: &FnItem, rfn: &ResolvedFn, tfn: &TypedFn) -> bool {
    rfn.well_formed
        && rfn.declarations_ok
        && func.name.is_some()
        && rfn.ret != Ty::Error
        && rfn.params.iter().all(|t| *t != Ty::Error)
        && tfn.exprs.len() == func.body.exprs.len()
        && tfn.exprs.iter().all(|t| *t != Ty::Error)
        // A return mismatch is the one type error that leaves no `Ty::Error`.
        && tfn.expr(func.body.root) == rfn.ret
}

/// `Ty` of a complete function is never `Error`; `I64` keeps this total.
fn tir_ty(ty: Ty) -> TirType {
    ty.to_tir().unwrap_or(TirType::I64)
}

/// Cover `a` and `b` when both are source spans in one file, else `a`.
fn cover(a: Provenance, b: Provenance) -> Provenance {
    match (a, b) {
        (Provenance::Source(x), Provenance::Source(y)) => x.cover(y).map_or(a, Provenance::Source),
        _ => a,
    }
}

fn lower_fn(
    func: &FnItem,
    rfn: &ResolvedFn,
    tfn: &TypedFn,
    site: &Site<'_>,
) -> Result<(TirFunction, FunctionProvenance), (&'static str, String)> {
    let internal = |what: String| ("E-tir-internal", format!("internal error: {what}"));
    let body = &func.body;
    let name_of = |local| body.local(local).name.clone().unwrap_or_default();

    // Pre-order walk from the root: `order[k]` becomes `TirNodeId(k)`.
    let mut order: Vec<ExprId> = Vec::new();
    let mut stack = vec![(body.root, 1_usize)];
    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_TIR_DEPTH {
            return Err((
                "E-tir-too-deep",
                format!(
                    "`{}` is nested deeper than TIR allows ({MAX_TIR_DEPTH})",
                    func.id.name
                ),
            ));
        }
        order.push(id);
        if let Expr::Binary { lhs, rhs, .. } = body.expr(id) {
            stack.push((*rhs, depth + 1));
            stack.push((*lhs, depth + 1));
        }
    }

    // Build bottom-up: every child is built before its parent takes it.
    let mut slots: Vec<Option<TirExpr>> = vec![None; body.exprs.len()];
    for &id in order.iter().rev() {
        let ty = tir_ty(tfn.expr(id));
        let node = match body.expr(id) {
            Expr::Int(value) => TirExpr::Int { value: *value, ty },
            Expr::Path(_) => {
                let Some(Res::Local(local)) = rfn.names.get(&id) else {
                    return Err(internal(format!(
                        "unresolved path e{} in a complete function",
                        id.0
                    )));
                };
                TirExpr::Var {
                    name: name_of(*local),
                    ty,
                }
            }
            Expr::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                let take = |slots: &mut Vec<Option<TirExpr>>, e: ExprId| {
                    slots.get_mut(e.0 as usize).and_then(Option::take)
                };
                match (take(&mut slots, *lhs), take(&mut slots, *rhs)) {
                    (Some(l), Some(r)) => TirExpr::Add {
                        lhs: Box::new(l),
                        rhs: Box::new(r),
                        ty,
                    },
                    _ => return Err(internal(format!("operand of e{} missing", id.0))),
                }
            }
            Expr::Error | Expr::Missing => {
                return Err(internal(format!(
                    "error node e{} in a complete function",
                    id.0
                )));
            }
        };
        slots[id.0 as usize] = Some(node);
    }
    let body_expr = slots
        .get_mut(body.root.0 as usize)
        .and_then(Option::take)
        .ok_or_else(|| internal("root expression missing".to_owned()))?;

    let mut nodes = ProvenanceMap::new();
    for (k, id) in order.iter().enumerate() {
        nodes.insert(TirNodeId(id32(k)), site.expr(*id));
    }
    let params = func
        .params
        .iter()
        .zip(&rfn.params)
        .map(|(p, ty)| TirParam {
            name: name_of(p.local),
            ty: tir_ty(*ty),
        })
        .collect();
    let param_prov = func
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| cover(site.local(p.local), site.ty(TypePos::Param(id32(i)))))
        .collect();
    Ok((
        TirFunction {
            name: func.name.clone().unwrap_or_default(),
            params,
            ret: tir_ty(rfn.ret),
            body: body_expr,
        },
        FunctionProvenance {
            func: site.item(),
            params: param_prov,
            nodes,
        },
    ))
}
