//! Capability state transition system.

use super::graph::{CapabilityGraph, Edge, EdgeKind, NodeId};
use super::lattice::{BorrowId, BorrowKind, BorrowRef, Capability, CapabilityState, PlaceId};
use super::nodes::{BorrowExtent, BorrowNode, PlaceNode, Span};
use thiserror::Error;

/// Errors during capability transitions.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TransitionError {
    #[error("cannot move from {place}: capability is {cap}, need Exclusive")]
    CannotMove { place: PlaceId, cap: Capability },
    #[error("cannot share {place}: capability is {cap}, need Exclusive or Read")]
    CannotShare { place: PlaceId, cap: Capability },
    #[error("cannot loan_mut {place}: capability is {cap}, need Exclusive")]
    CannotLoanMut { place: PlaceId, cap: Capability },
    #[error("cannot reborrow {borrow}: kind is {kind}, need Mutable")]
    CannotReborrow { borrow: BorrowId, kind: BorrowKind },
    #[error("cannot restore {borrow}: not found or already restored")]
    CannotRestore { borrow: BorrowId },
    #[error("cannot split {place}: not a composite place")]
    CannotSplit { place: PlaceId },
    #[error("cannot join {place}: fields not all initialized")]
    CannotJoin { place: PlaceId },
    #[error("cannot write {place}: capability is {cap}, need Exclusive or Write")]
    CannotWrite { place: PlaceId, cap: Capability },
    #[error("cannot read {place}: capability is {cap}, need Read, Write, or Exclusive")]
    CannotRead { place: PlaceId, cap: Capability },
    #[error("borrow {borrow} still active at point {point}")]
    BorrowActive { borrow: BorrowId, point: u32 },
    #[error("place {place} has outstanding borrows: {borrows:?}")]
    OutstandingBorrows {
        place: PlaceId,
        borrows: Vec<BorrowRef>,
    },
    #[error("place {place} is partially moved")]
    PartiallyMoved { place: PlaceId },
}

/// Result of a transition attempt.
pub type TransitionResult<T> = Result<T, TransitionError>;

/// The transition system for capability state changes.
pub struct TransitionSystem {
    graph: CapabilityGraph,
}

impl TransitionSystem {
    #[must_use]
    pub fn new(graph: CapabilityGraph) -> Self {
        Self { graph }
    }

    #[must_use]
    pub fn graph(&self) -> &CapabilityGraph {
        &self.graph
    }

    #[must_use]
    pub fn graph_mut(&mut self) -> &mut CapabilityGraph {
        &mut self.graph
    }

    /// Execute a move: `move A -> B`
    /// Consumes the source place's exclusive capability.
    pub fn execute_move(
        &mut self,
        from: PlaceId,
        to: PlaceId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Edge> {
        let from_state = self.graph.state(from).cloned().unwrap_or_default();

        if !from_state.capability.can_move() {
            return Err(TransitionError::CannotMove {
                place: from,
                cap: from_state.capability,
            });
        }
        if !from_state.borrows.is_empty() {
            return Err(TransitionError::OutstandingBorrows {
                place: from,
                borrows: from_state.borrows.clone(),
            });
        }
        if from_state.is_partial {
            return Err(TransitionError::PartiallyMoved { place: from });
        }

        let mut after_from = from_state.clone();
        after_from.capability = Capability::None;

        let mut after_to = self
            .graph
            .state(to)
            .cloned()
            .unwrap_or_else(CapabilityState::exclusive);
        after_to.capability = Capability::Exclusive;

        self.graph.set_state(from, after_from.clone());
        self.graph.set_state(to, after_to.clone());

        let from_node = self.find_place_node(from);
        let to_node = self.find_place_node(to);

        Ok(self.graph.add_edge(
            EdgeKind::Move,
            from_node,
            to_node,
            span,
            tir_op,
            from_state,
            after_to,
        ))
    }

    /// Execute a share (immutable borrow): `share A -> r`
    pub fn execute_share(
        &mut self,
        place: PlaceId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<(Edge, BorrowId)> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !place_state.capability.can_share() {
            return Err(TransitionError::CannotShare {
                place,
                cap: place_state.capability,
            });
        }

        let borrow_id = self.graph.next_borrow_id();

        let borrow = BorrowNode {
            id: borrow_id,
            kind: BorrowKind::Shared,
            place,
            extent: BorrowExtent::Lexical {
                start: self.graph.current_point(),
                end: self.graph.current_point() + 1,
            },
            origin_span: span,
        };
        let borrow_node_id = self.graph.add_borrow(borrow);

        let mut after_place = place_state.clone();
        after_place.capability = Capability::Read;
        after_place.borrows.push(BorrowRef {
            id: borrow_id,
            kind: BorrowKind::Shared,
            place,
        });
        self.graph.set_state(place, after_place.clone());

        let place_node = self.find_place_node(place);

        let edge = self.graph.add_edge(
            EdgeKind::Share,
            place_node,
            borrow_node_id,
            span,
            tir_op,
            place_state,
            after_place,
        );

        Ok((edge, borrow_id))
    }

    /// Execute a mutable loan: `loan_mut A -> r`
    pub fn execute_loan_mut(
        &mut self,
        place: PlaceId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<(Edge, BorrowId)> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !place_state.capability.can_loan_mut() {
            return Err(TransitionError::CannotLoanMut {
                place,
                cap: place_state.capability,
            });
        }
        if !place_state.borrows.is_empty() {
            return Err(TransitionError::OutstandingBorrows {
                place,
                borrows: place_state.borrows.clone(),
            });
        }

        let borrow_id = self.graph.next_borrow_id();

        let borrow = BorrowNode {
            id: borrow_id,
            kind: BorrowKind::Mutable,
            place,
            extent: BorrowExtent::Lexical {
                start: self.graph.current_point(),
                end: self.graph.current_point() + 1,
            },
            origin_span: span,
        };
        let borrow_node_id = self.graph.add_borrow(borrow);

        let mut after_place = place_state.clone();
        after_place.capability = Capability::None;
        after_place.borrows.push(BorrowRef {
            id: borrow_id,
            kind: BorrowKind::Mutable,
            place,
        });
        self.graph.set_state(place, after_place.clone());

        let place_node = self.find_place_node(place);

        let edge = self.graph.add_edge(
            EdgeKind::LoanMut,
            place_node,
            borrow_node_id,
            span,
            tir_op,
            place_state,
            after_place,
        );

        Ok((edge, borrow_id))
    }

    /// Execute a reborrow: `reborrow r1 -> r2`
    pub fn execute_reborrow(
        &mut self,
        from_borrow: BorrowId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<(Edge, BorrowId)> {
        let from_borrow_node = self
            .graph
            .get_borrow(from_borrow)
            .ok_or(TransitionError::CannotReborrow {
                borrow: from_borrow,
                kind: BorrowKind::Shared,
            })?
            .clone();

        if from_borrow_node.kind != BorrowKind::Mutable {
            return Err(TransitionError::CannotReborrow {
                borrow: from_borrow,
                kind: from_borrow_node.kind,
            });
        }

        let to_borrow_id = self.graph.next_borrow_id();

        let to_borrow = BorrowNode {
            id: to_borrow_id,
            kind: BorrowKind::Mutable,
            place: from_borrow_node.place,
            extent: from_borrow_node.extent,
            origin_span: span,
        };
        let to_borrow_node_id = self.graph.add_borrow(to_borrow);

        let from_borrow_node_id = self.find_borrow_node(from_borrow);

        let edge = self.graph.add_edge(
            EdgeKind::Reborrow,
            from_borrow_node_id,
            to_borrow_node_id,
            span,
            tir_op,
            CapabilityState::exclusive(),
            CapabilityState::exclusive(),
        );

        Ok((edge, to_borrow_id))
    }

    /// Execute a restore: `restore r -> A`
    pub fn execute_restore(
        &mut self,
        borrow: BorrowId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Edge> {
        let borrow_node = self
            .graph
            .get_borrow(borrow)
            .ok_or(TransitionError::CannotRestore { borrow })?
            .clone();

        let place = borrow_node.place;
        let mut place_state = self.graph.state(place).cloned().unwrap_or_default();

        // Remove this borrow from the place's borrows
        place_state.borrows.retain(|b| b.id != borrow);

        // Restore capability based on remaining borrows
        place_state.capability = if place_state.borrows.is_empty() {
            Capability::Exclusive
        } else if place_state
            .borrows
            .iter()
            .any(|b| b.kind == BorrowKind::Mutable)
        {
            Capability::None
        } else {
            Capability::Read
        };

        self.graph.set_state(place, place_state.clone());

        let place_node = self.find_place_node(place);
        let borrow_node_id = self.find_borrow_node(borrow);

        let edge = self.graph.add_edge(
            EdgeKind::Restore,
            borrow_node_id,
            place_node,
            span,
            tir_op,
            CapabilityState::none(),
            place_state,
        );

        Ok(edge)
    }

    /// Execute a split: `split A -> A.field...`
    pub fn execute_split(
        &mut self,
        place: PlaceId,
        fields: Vec<String>,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Vec<Edge>> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !matches!(
            place_state.capability,
            Capability::Exclusive | Capability::Write
        ) {
            return Err(TransitionError::CannotSplit { place });
        }
        if !place_state.borrows.is_empty() {
            return Err(TransitionError::OutstandingBorrows {
                place,
                borrows: place_state.borrows.clone(),
            });
        }

        let mut edges = Vec::new();
        let place_node = self.find_place_node(place);

        for field in fields {
            let field_place_id = self.graph.next_place_id();

            let field_node = PlaceNode::Field {
                id: field_place_id,
                base: place,
                field: field.clone(),
                ty: super::nodes::PlaceType::Scalar, // Simplified
            };
            let field_node_id = self.graph.add_place(field_node);

            self.graph
                .set_state(field_place_id, CapabilityState::exclusive());

            let edge = self.graph.add_edge(
                EdgeKind::Split,
                place_node,
                field_node_id,
                span,
                tir_op.clone(),
                place_state.clone(),
                CapabilityState::exclusive(),
            );
            edges.push(edge);
        }

        // Mark base as partial
        let mut after_base = place_state;
        after_base.is_partial = true;
        self.graph.set_state(place, after_base);

        Ok(edges)
    }

    /// Execute a join: `join fields -> A`
    pub fn execute_join(
        &mut self,
        place: PlaceId,
        field_names: Vec<String>,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Edge> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !place_state.is_partial {
            return Err(TransitionError::CannotJoin { place });
        }

        // Check all fields are initialized (Exclusive)
        for field_name in &field_names {
            let field_place = self.find_field_place(place, field_name);
            if let Some(field_id) = field_place {
                let field_state = self.graph.state(field_id).cloned().unwrap_or_default();
                if !field_state.capability.can_move() {
                    return Err(TransitionError::CannotJoin { place });
                }
            }
        }

        let mut after_place = place_state.clone();
        after_place.is_partial = false;
        after_place.capability = Capability::Exclusive;
        after_place.field_states.clear();
        self.graph.set_state(place, after_place.clone());

        let place_node = self.find_place_node(place);
        let field_node = self
            .find_field_place(place, &field_names[0])
            .map(|id| self.find_place_node(id))
            .unwrap_or(place_node);

        Ok(self.graph.add_edge(
            EdgeKind::Join,
            field_node,
            place_node,
            span,
            tir_op,
            place_state,
            after_place,
        ))
    }

    /// Execute a write: `write A <- value`
    pub fn execute_write(
        &mut self,
        place: PlaceId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Edge> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !place_state.capability.can_write() {
            return Err(TransitionError::CannotWrite {
                place,
                cap: place_state.capability,
            });
        }
        if place_state
            .borrows
            .iter()
            .any(|b| b.kind == BorrowKind::Mutable)
        {
            return Err(TransitionError::OutstandingBorrows {
                place,
                borrows: place_state.borrows.clone(),
            });
        }

        let mut after_state = place_state.clone();
        after_state.capability = Capability::Exclusive;
        self.graph.set_state(place, after_state.clone());

        let place_node = self.find_place_node(place);

        Ok(self.graph.add_edge(
            EdgeKind::Write,
            place_node,
            place_node,
            span,
            tir_op,
            place_state,
            after_state,
        ))
    }

    /// Execute a read: `read A`
    pub fn execute_read(
        &mut self,
        place: PlaceId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Edge> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !place_state.capability.can_read() {
            return Err(TransitionError::CannotRead {
                place,
                cap: place_state.capability,
            });
        }

        let place_node = self.find_place_node(place);

        Ok(self.graph.add_edge(
            EdgeKind::Read,
            place_node,
            place_node,
            span,
            tir_op,
            place_state.clone(),
            place_state,
        ))
    }

    /// Execute a drop: `drop A`
    pub fn execute_drop(
        &mut self,
        place: PlaceId,
        span: Span,
        tir_op: String,
    ) -> TransitionResult<Edge> {
        let place_state = self.graph.state(place).cloned().unwrap_or_default();

        if !place_state.borrows.is_empty() {
            return Err(TransitionError::OutstandingBorrows {
                place,
                borrows: place_state.borrows.clone(),
            });
        }

        let mut after_state = place_state.clone();
        after_state.capability = Capability::None;
        self.graph.set_state(place, after_state.clone());

        let place_node = self.find_place_node(place);

        Ok(self.graph.add_edge(
            EdgeKind::Drop,
            place_node,
            place_node,
            span,
            tir_op,
            place_state,
            after_state,
        ))
    }

    fn find_place_node(&mut self, place: PlaceId) -> NodeId {
        self.graph
            .nodes()
            .iter()
            .find_map(|(id, node)| match node {
                crate::graph::Node::Place(p) if p.id() == place => Some(*id),
                _ => None,
            })
            .unwrap_or_else(|| {
                // Create a dummy node if not found

                self.graph.next_node_id()
            })
    }

    fn find_borrow_node(&mut self, borrow: BorrowId) -> NodeId {
        self.graph
            .nodes()
            .iter()
            .find_map(|(id, node)| match node {
                crate::graph::Node::Borrow(b) if b.id == borrow => Some(*id),
                _ => None,
            })
            .unwrap_or_else(|| self.graph.next_node_id())
    }

    fn find_field_place(&self, base: PlaceId, field: &str) -> Option<PlaceId> {
        self.graph.places().find_map(|p| match p {
            PlaceNode::Field {
                id,
                base: b,
                field: f,
                ..
            } if *b == base && f == field => Some(*id),
            _ => None,
        })
    }
}

impl Default for CapabilityState {
    fn default() -> Self {
        Self::none()
    }
}
