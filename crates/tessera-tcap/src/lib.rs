//! Tessera Capability Graph (TCap): ownership/borrow semantic view derived from TIR.
//!
//! This crate implements the capability graph representation as specified in
//! `docs/spec/ownership-capability-graph.md`, providing:
//! - Capability lattice (E, R, W, 0)
//! - Place nodes (local, field, index, deref, remote)
//! - Borrow/lifetime projection nodes
//! - Edges (move, share, loan_mut, reborrow, restore, split, join)
//! - Capability state transitions
//! - TIR integration for deriving TCap from TIR functions
//! - Debug projection for `tsr explain --ownership` trace format
//!
//! V0 semantic implementation order (from spec):
//! 1. owned affine values + deterministic drop
//! 2. immutable borrow
//! 3. lexical borrow extent
//! 4. reborrow
//! 5. mutable borrow
//! 6. field-sensitive split/join
//! 7. non-lexical shortening

use std::collections::{HashMap, HashSet};
use std::fmt;
use tessera_tir::{TirExpr, TirFunction, TirParam, TirType};

pub mod debug;
pub mod derive;
pub mod graph;
pub mod lattice;
pub mod nodes;
pub mod transitions;

pub use crate::debug::DebugProjection;
pub use crate::derive::derive_tcap;
pub use crate::graph::{CapabilityGraph, Edge, EdgeKind, Node, NodeId};
pub use crate::lattice::{Capability, CapabilityState};
pub use crate::nodes::{BorrowNode, PlaceNode, ProjectionNode};
pub use crate::transitions::{Transition, TransitionError, TransitionSystem};

/// Entry point for the TCap crate.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}