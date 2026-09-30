//! TIR integration: derive TCap from TIR functions.
//!
//! A `Var` reads the place of its innermost binder, resolved as
//! `tessera_tir::verify` resolves it: the parameters are bound first, each
//! `let` binds its name for the extent of its body (not its initializer), and
//! the innermost binding wins. A parameter is the `Remote` place for its
//! argument index; a `let` binder gets its own `Local` place once its
//! initializer has been derived, and that place is dropped when the body ends.
//! A `Var` with no binder in scope (TIR that fails verification) has no place
//! and adds no edge.

use super::graph::CapabilityGraph;
use super::lattice::{BorrowKind, CapabilityState, PlaceId};
use super::nodes::{PlaceNode, PlaceType, Span};
use super::transitions::{TransitionResult, TransitionSystem};
use std::collections::HashMap;
use tessera_tir::{TirExpr, TirFunction, TirType};

/// A function body being derived: the transition system, the binders in
/// scope, and the next free place id. Shared by both derivers.
struct Body {
    system: TransitionSystem,
    /// Binders in scope, innermost last: one entry per parameter, then one
    /// per enclosing `let`.
    scope: Vec<(String, PlaceId)>,
    next_place_id: u32,
}

impl Body {
    fn new() -> Self {
        Self {
            system: TransitionSystem::new(CapabilityGraph::new()),
            scope: Vec::new(),
            next_place_id: 1,
        }
    }

    fn fresh_place_id(&mut self) -> PlaceId {
        let id = PlaceId(self.next_place_id);
        self.next_place_id += 1;
        id
    }

    /// Add `place` to the graph and bring it into scope as `name`.
    fn bind(&mut self, name: &str, place: PlaceNode) -> PlaceId {
        let id = place.id();
        self.system.graph_mut().add_place(place);
        self.scope.push((name.to_owned(), id));
        id
    }

    /// Place of the innermost binder named `name`.
    fn resolve(&self, name: &str) -> Option<PlaceId> {
        self.scope
            .iter()
            .rev()
            .find(|(bound, _)| bound == name)
            .map(|&(_, place)| place)
    }

    fn derive_expr(&mut self, expr: &TirExpr) {
        match expr {
            TirExpr::Var { name, .. } => {
                if let Some(place) = self.resolve(name) {
                    let _ = self
                        .system
                        .execute_read(place, Span::dummy(), format!("read {name}"));
                }
            }
            TirExpr::Add { lhs, rhs, .. }
            | TirExpr::Eq { lhs, rhs }
            | TirExpr::And { lhs, rhs } => {
                self.derive_expr(lhs);
                self.derive_expr(rhs);
            }
            TirExpr::Not { expr } => {
                self.derive_expr(expr);
            }
            TirExpr::Let {
                name,
                ty,
                init,
                body,
            } => {
                // The binder is not in scope in its own initializer.
                self.derive_expr(init);
                let local = PlaceNode::Local {
                    id: self.fresh_place_id(),
                    name: name.clone(),
                    ty: place_type(*ty),
                };
                let place = self.bind(name, local);
                self.derive_expr(body);
                self.scope.pop();
                let _ = self
                    .system
                    .execute_drop(place, Span::dummy(), format!("drop {name}"));
            }
            TirExpr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                // Prototype: both branches are visited in sequence; per-branch
                // capability joins belong to #3/#12.
                self.derive_expr(cond);
                self.derive_expr(then_branch);
                self.derive_expr(else_branch);
            }
            TirExpr::Call { args, .. } => {
                for arg in args {
                    self.derive_expr(arg);
                }
            }
            TirExpr::Int { .. } | TirExpr::Bool { .. } => {
                // Literals don't affect capability state
            }
        }
    }
}

fn place_type(ty: TirType) -> PlaceType {
    match ty {
        TirType::I64 | TirType::Bool => PlaceType::Scalar,
    }
}

/// Context for deriving TCap from TIR.
pub struct TCapDeriver {
    body: Body,
}

impl TCapDeriver {
    #[must_use]
    pub fn new() -> Self {
        Self { body: Body::new() }
    }

    #[must_use]
    pub fn derive(mut self, func: &TirFunction) -> CapabilityGraph {
        // Create parameter places (remotes)
        for (i, param) in func.params.iter().enumerate() {
            let place = PlaceNode::Remote {
                id: self.body.fresh_place_id(),
                arg_index: i,
                ty: place_type(param.ty),
            };
            let place = self.body.bind(&param.name, place);

            // Parameters start as Exclusive (owned by caller, borrowed by callee)
            self.body
                .system
                .graph_mut()
                .set_state(place, CapabilityState::exclusive());
        }

        // Process function body; each `let` drops its local when its scope ends
        self.body.derive_expr(&func.body);

        self.body.system.graph().clone()
    }
}

/// Convenience function to derive TCap from a TIR function.
#[must_use]
pub fn derive_tcap(func: &TirFunction) -> CapabilityGraph {
    TCapDeriver::new().derive(func)
}

/// Derive TCap from TIR function with borrow tracking.
pub struct BorrowingDeriver {
    body: Body,
    active_borrows: HashMap<super::lattice::BorrowId, (PlaceId, BorrowKind)>,
}

impl BorrowingDeriver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            body: Body::new(),
            active_borrows: HashMap::new(),
        }
    }

    #[must_use]
    pub fn derive(mut self, func: &TirFunction) -> CapabilityGraph {
        // Create parameter places
        for (i, param) in func.params.iter().enumerate() {
            let is_ref = false; // Would need type info
            let place = PlaceNode::Remote {
                id: self.body.fresh_place_id(),
                arg_index: i,
                ty: if is_ref {
                    PlaceType::Reference { mutable: false }
                } else {
                    place_type(param.ty)
                },
            };
            self.body.bind(&param.name, place);
        }

        // Derive body with borrow tracking
        self.body.derive_expr(&func.body);

        self.body.system.graph().clone()
    }

    /// Create an immutable borrow of a place.
    pub fn borrow_shared(
        &mut self,
        place: super::lattice::PlaceId,
        span: Span,
    ) -> TransitionResult<super::lattice::BorrowId> {
        let (_edge, borrow_id) =
            self.body
                .system
                .execute_share(place, span, "share".to_string())?;
        self.active_borrows
            .insert(borrow_id, (place, BorrowKind::Shared));
        Ok(borrow_id)
    }

    /// Create a mutable borrow of a place.
    pub fn borrow_mut(
        &mut self,
        place: super::lattice::PlaceId,
        span: Span,
    ) -> TransitionResult<super::lattice::BorrowId> {
        let (_edge, borrow_id) =
            self.body
                .system
                .execute_loan_mut(place, span, "loan_mut".to_string())?;
        self.active_borrows
            .insert(borrow_id, (place, BorrowKind::Mutable));
        Ok(borrow_id)
    }

    /// End a borrow and restore the place.
    pub fn end_borrow(
        &mut self,
        borrow: super::lattice::BorrowId,
        span: Span,
    ) -> TransitionResult<()> {
        self.body
            .system
            .execute_restore(borrow, span, "restore".to_string())?;
        self.active_borrows.remove(&borrow);
        Ok(())
    }

    /// Reborrow mutably from an existing mutable borrow.
    pub fn reborrow_mut(
        &mut self,
        from: super::lattice::BorrowId,
        span: Span,
    ) -> TransitionResult<super::lattice::BorrowId> {
        let (_edge, to_borrow) =
            self.body
                .system
                .execute_reborrow(from, span, "reborrow".to_string())?;
        if let Some((place, _)) = self.active_borrows.get(&from) {
            self.active_borrows
                .insert(to_borrow, (*place, BorrowKind::Mutable));
        }
        Ok(to_borrow)
    }

    /// Split a composite place into fields.
    pub fn split(
        &mut self,
        place: super::lattice::PlaceId,
        fields: Vec<String>,
        span: Span,
    ) -> TransitionResult<()> {
        self.body
            .system
            .execute_split(place, fields, span, "split".to_string())?;
        Ok(())
    }

    /// Join fields back into a composite place.
    pub fn join(
        &mut self,
        place: super::lattice::PlaceId,
        fields: Vec<String>,
        span: Span,
    ) -> TransitionResult<()> {
        self.body
            .system
            .execute_join(place, fields, span, "join".to_string())?;
        Ok(())
    }
}

impl Default for TCapDeriver {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for BorrowingDeriver {
    fn default() -> Self {
        Self::new()
    }
}
