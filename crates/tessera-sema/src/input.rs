//! What sema requires of its HIR input, and how it locates diagnostics.
//!
//! HIR lowering is trusted to follow the B2 contract, but sema is a total
//! function (INV-DIAG-1): a hand-built or buggy HIR must produce a diagnostic,
//! never a panic, a hang or an exponential blow-up. [`check`] therefore
//! validates the *shape* every later step relies on, once, in `resolve`:
//!
//! - the root and every operand id is in range;
//! - every operand id is greater than its parent's id (pre-order arena), so the
//!   expression graph is acyclic and typing can run in one reverse pass;
//! - no expression is an operand twice, so the graph is a forest and lowering
//!   to a tree cannot duplicate nodes;
//! - every parameter names an existing local;
//! - provenance is total over the function (PROV-1).
//!
//! A function that fails is reported once as `E-hir-malformed` and is not
//! analysed further.

use tessera_hir::{Expr, ExprId, FnItem, FnProvenance, LocalId, TypePos};
use tessera_phases::{FileId, Provenance, Span};

/// HIR ids are `u32`; arenas never exceed that, but the conversion stays total.
pub(crate) fn id32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// Stand-in used only when the HIR provenance table itself is missing an entry
/// (a PROV-1 violation upstream). It is `Synthesized` with an explicit reason so
/// it can never be mistaken for a real source location.
fn absent(file: FileId) -> Provenance {
    Provenance::Synthesized {
        origin: Span::new(file, 0, 0),
        why: "hir-provenance-missing",
    }
}

/// Provenance lookups for one function, total by construction: a missing entry
/// falls back to the item's provenance, and a missing table to [`absent`].
pub(crate) struct Site<'a> {
    file: FileId,
    table: Option<&'a FnProvenance>,
}

impl<'a> Site<'a> {
    pub(crate) fn new(file: FileId, table: Option<&'a FnProvenance>) -> Self {
        Self { file, table }
    }

    pub(crate) fn table(&self) -> Option<&'a FnProvenance> {
        self.table
    }

    pub(crate) fn item(&self) -> Provenance {
        self.table.map_or_else(|| absent(self.file), |t| t.item)
    }

    pub(crate) fn name(&self) -> Provenance {
        self.table.map_or_else(|| absent(self.file), |t| t.name)
    }

    pub(crate) fn expr(&self, id: ExprId) -> Provenance {
        self.table
            .and_then(|t| t.exprs.get(id))
            .unwrap_or_else(|| self.item())
    }

    pub(crate) fn local(&self, id: LocalId) -> Provenance {
        self.table
            .and_then(|t| t.locals.get(id))
            .unwrap_or_else(|| self.item())
    }

    pub(crate) fn ty(&self, pos: TypePos) -> Provenance {
        self.table
            .and_then(|t| t.types.get(pos))
            .unwrap_or_else(|| self.item())
    }
}

/// The first violated input requirement of `func`, described for a diagnostic,
/// or `None` if the function is well-formed HIR.
pub(crate) fn check(func: &FnItem, table: Option<&FnProvenance>) -> Option<String> {
    let Some(table) = table else {
        return Some("no provenance table for this item".to_owned());
    };
    let body = &func.body;
    let count = body.exprs.len();
    let root = body.root.0 as usize;
    if root >= count {
        return Some(format!(
            "root expression e{root} is out of range ({count} expressions)"
        ));
    }
    for (i, param) in func.params.iter().enumerate() {
        if param.local.0 as usize >= body.locals.len() {
            return Some(format!(
                "parameter {i} names local l{} but the body has {} locals",
                param.local.0,
                body.locals.len()
            ));
        }
    }
    let mut uses = vec![0_u8; count];
    for (i, expr) in body.exprs.iter().enumerate() {
        if let Expr::Binary { lhs, rhs, .. } = expr {
            for operand in [lhs, rhs] {
                let child = operand.0 as usize;
                if child >= count {
                    return Some(format!(
                        "operand e{child} of e{i} is out of range ({count} expressions)"
                    ));
                }
                if child <= i {
                    return Some(format!(
                        "operand e{child} of e{i} does not come after it (the arena must be in pre-order)"
                    ));
                }
                uses[child] = uses[child].saturating_add(1);
            }
        }
    }
    if uses[root] != 0 {
        return Some(format!("root expression e{root} is also an operand"));
    }
    if let Some(shared) = uses.iter().position(|&n| n > 1) {
        return Some(format!("expression e{shared} is an operand more than once"));
    }
    if let Some(id) = table
        .exprs
        .missing((0..count).map(|i| ExprId(id32(i))))
        .first()
    {
        return Some(format!("no provenance for expression e{}", id.0));
    }
    if let Some(id) = table
        .locals
        .missing((0..body.locals.len()).map(|i| LocalId(id32(i))))
        .first()
    {
        return Some(format!("no provenance for local l{}", id.0));
    }
    let positions = (0..func.params.len())
        .map(|i| TypePos::Param(id32(i)))
        .chain(std::iter::once(TypePos::Ret));
    if let Some(pos) = table.types.missing(positions).first() {
        return Some(format!("no provenance for type annotation {pos:?}"));
    }
    None
}
