//! TIR integration: derive TCap from TIR functions.

use super::graph::CapabilityGraph;
use super::lattice::{BorrowKind, CapabilityState};
use super::nodes::{PlaceNode, PlaceType, Span};
use super::transitions::{TransitionResult, TransitionSystem};
use std::collections::HashMap;
use tessera_tir::{TirExpr, TirFunction, TirType};

/// Context for deriving TCap from TIR.
pub struct TCapDeriver {
    system: TransitionSystem,
    param_places: Vec<PlaceNode>,
    local_places: HashMap<String, PlaceNode>,
    next_place_id: u32,
}

impl TCapDeriver {
    #[must_use]
    pub fn new() -> Self {
        let graph = CapabilityGraph::new();
        Self {
            system: TransitionSystem::new(graph),
            param_places: Vec::new(),
            local_places: HashMap::new(),
            next_place_id: 1,
        }
    }

    #[must_use]
    pub fn derive(mut self, func: &TirFunction) -> CapabilityGraph {
        // Create parameter places (remotes)
        for (i, param) in func.params.iter().enumerate() {
            let place = PlaceNode::Remote {
                id: super::lattice::PlaceId(self.next_place_id),
                arg_index: i,
                ty: self.tir_type_to_place_type(param.ty),
            };
            self.next_place_id += 1;
            let _node_id = self.system.graph_mut().add_place(place.clone());
            self.param_places.push(place);

            // Parameters start as Exclusive (owned by caller, borrowed by callee)
            self.system.graph_mut().set_state(
                super::lattice::PlaceId(self.next_place_id - 1),
                CapabilityState::exclusive(),
            );
        }

        // Process function body
        self.derive_expr(&func.body);

        // Drop all locals at end of function
        self.drop_all_locals();

        self.system.graph().clone()
    }

    fn derive_expr(&mut self, expr: &TirExpr) {
        match expr {
            TirExpr::Var { name, ty } => {
                self.derive_var_read(name, *ty);
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
            TirExpr::Int { .. } | TirExpr::Bool { .. } => {
                // Literals don't affect capability state
            }
        }
    }

    fn derive_var_read(&mut self, name: &str, ty: TirType) {
        // Find or create local place
        let place_id = if let Some(place) = self.local_places.get(name) {
            place.id()
        } else if let Some(idx) = self.param_places.iter().position(|p| match p {
            PlaceNode::Remote { arg_index, .. } => {
                *arg_index
                    == self
                        .param_places
                        .iter()
                        .position(|p| match p {
                            PlaceNode::Remote { arg_index, .. } if *arg_index == 0 => true, // simplified
                            _ => false,
                        })
                        .unwrap_or(0)
            } // This is wrong, need better logic
            _ => false,
        }) {
            // It's a parameter
            self.param_places[idx].id()
        } else {
            // Create new local
            let place = PlaceNode::Local {
                id: super::lattice::PlaceId(self.next_place_id),
                name: name.to_string(),
                ty: self.tir_type_to_place_type(ty),
            };
            self.next_place_id += 1;
            let pid = place.id();
            self.system.graph_mut().add_place(place.clone());
            self.local_places.insert(name.to_string(), place);
            pid
        };

        // Execute read
        let _ = self
            .system
            .execute_read(place_id, Span::dummy(), format!("read {}", name));
    }

    fn tir_type_to_place_type(&self, ty: TirType) -> PlaceType {
        match ty {
            TirType::I64 | TirType::Bool => PlaceType::Scalar,
        }
    }

    fn drop_all_locals(&mut self) {
        let places: Vec<_> = self.local_places.values().map(|p| p.id()).collect();
        for place in places {
            let _ = self
                .system
                .execute_drop(place, Span::dummy(), "drop".to_string());
        }
    }
}

/// Convenience function to derive TCap from a TIR function.
#[must_use]
pub fn derive_tcap(func: &TirFunction) -> CapabilityGraph {
    TCapDeriver::new().derive(func)
}

/// Derive TCap from TIR function with borrow tracking.
pub struct BorrowingDeriver {
    system: TransitionSystem,
    param_places: Vec<PlaceNode>,
    local_places: HashMap<String, PlaceNode>,
    active_borrows: HashMap<super::lattice::BorrowId, (super::lattice::PlaceId, BorrowKind)>,
    next_place_id: u32,
}

impl BorrowingDeriver {
    #[must_use]
    pub fn new() -> Self {
        let graph = CapabilityGraph::new();
        Self {
            system: TransitionSystem::new(graph),
            param_places: Vec::new(),
            local_places: HashMap::new(),
            active_borrows: HashMap::new(),
            next_place_id: 1,
        }
    }

    #[must_use]
    pub fn derive(mut self, func: &TirFunction) -> CapabilityGraph {
        // Create parameter places
        for (i, param) in func.params.iter().enumerate() {
            let is_ref = false; // Would need type info
            let place = PlaceNode::Remote {
                id: super::lattice::PlaceId(self.next_place_id),
                arg_index: i,
                ty: if is_ref {
                    PlaceType::Reference { mutable: false }
                } else {
                    self.tir_type_to_place_type(param.ty)
                },
            };
            self.next_place_id += 1;
            self.system.graph_mut().add_place(place.clone());
            self.param_places.push(place);
        }

        // Derive body with borrow tracking
        self.derive_expr_borrow(&func.body);

        self.system.graph().clone()
    }

    fn derive_expr_borrow(&mut self, expr: &TirExpr) {
        match expr {
            TirExpr::Var { name, ty } => {
                self.derive_var_read_borrow(name, *ty);
            }
            TirExpr::Add { lhs, rhs, .. }
            | TirExpr::Eq { lhs, rhs }
            | TirExpr::And { lhs, rhs } => {
                self.derive_expr_borrow(lhs);
                self.derive_expr_borrow(rhs);
            }
            TirExpr::Not { expr } => {
                self.derive_expr_borrow(expr);
            }
            TirExpr::Int { .. } | TirExpr::Bool { .. } => {}
        }
    }

    fn derive_var_read_borrow(&mut self, name: &str, _ty: TirType) {
        // Simplified: just read
        if let Some(idx) = self.param_places.iter().position(|p| match p {
            PlaceNode::Remote { arg_index, .. } => *arg_index == self.find_param_index(name),
            _ => false,
        }) {
            let place_id = self.param_places[idx].id();
            let _ = self
                .system
                .execute_read(place_id, Span::dummy(), format!("read {}", name));
        } else if let Some(place) = self.local_places.get(name) {
            let _ = self
                .system
                .execute_read(place.id(), Span::dummy(), format!("read {}", name));
        }
    }

    fn find_param_index(&self, _name: &str) -> usize {
        // Simplified: would need proper parameter mapping
        0
    }

    fn tir_type_to_place_type(&self, ty: TirType) -> PlaceType {
        match ty {
            TirType::I64 | TirType::Bool => PlaceType::Scalar,
        }
    }

    /// Create an immutable borrow of a place.
    pub fn borrow_shared(
        &mut self,
        place: super::lattice::PlaceId,
        span: Span,
    ) -> TransitionResult<super::lattice::BorrowId> {
        let (_edge, borrow_id) = self
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
            self.system
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
        self.system
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
            self.system
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
        self.system
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
        self.system
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
