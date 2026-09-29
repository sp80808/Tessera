//! Capability graph structure with nodes and edges.

use std::collections::HashMap;
use super::lattice::{BorrowId, Capability, CapabilityState, PlaceId};
use super::nodes::{BorrowNode, PlaceNode, ProjectionNode, Span};

/// Unique identifier for graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

impl NodeId {
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Edge kinds in the capability graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// Move ownership: `move A -> B`
    Move,
    /// Share for immutable borrow: `share A -> r`
    Share,
    /// Loan for mutable borrow: `loan_mut A -> r`
    LoanMut,
    /// Reborrow: `reborrow r1 -> r2`
    Reborrow,
    /// Restore capability after borrow ends: `restore r -> A`
    Restore,
    /// Split composite: `split A -> A.field...`
    Split,
    /// Join fields back: `join fields -> A`
    Join,
    /// Drop/cleanup: `drop A`
    Drop,
    /// Assignment/write: `write A <- value`
    Write,
    /// Read access: `read A`
    Read,
}

impl EdgeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Share => "share",
            Self::LoanMut => "loan_mut",
            Self::Reborrow => "reborrow",
            Self::Restore => "restore",
            Self::Split => "split",
            Self::Join => "join",
            Self::Drop => "drop",
            Self::Write => "write",
            Self::Read => "read",
        }
    }
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An edge in the capability graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub from: NodeId,
    pub to: NodeId,
    pub span: Span,
    pub tir_op: String,
    pub before_state: CapabilityState,
    pub after_state: CapabilityState,
    pub constraint_id: Option<ConstraintId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub u32);

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstraintId(pub u32);

impl fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C{}", self.0)
    }
}

/// A node in the capability graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Place(PlaceNode),
    Borrow(BorrowNode),
}

impl Node {
    #[must_use]
    pub fn place_id(&self) -> Option<PlaceId> {
        match self {
            Self::Place(p) => Some(p.id()),
            Self::Borrow(b) => Some(b.place),
        }
    }

    #[must_use]
    pub fn borrow_id(&self) -> Option<BorrowId> {
        match self {
            Self::Borrow(b) => Some(b.id),
            Self::Place(_) => None,
        }
    }
}

/// The full capability graph for a function.
#[derive(Debug, Clone, Default)]
pub struct CapabilityGraph {
    nodes: HashMap<NodeId, Node>,
    edges: Vec<Edge>,
    place_states: HashMap<PlaceId, CapabilityState>,
    borrow_nodes: HashMap<BorrowId, BorrowNode>,
    next_node_id: u32,
    next_edge_id: u32,
    next_borrow_id: u32,
    next_place_id: u32,
    next_constraint_id: u32,
    /// Program point counter for lexical extent tracking
    current_point: u32,
}

impl CapabilityGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a place node to the graph.
    pub fn add_place(&mut self, place: PlaceNode) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        let place_id = place.id();
        self.nodes.insert(id, Node::Place(place));
        self.place_states.insert(place_id, CapabilityState::exclusive());
        id
    }

    /// Add a borrow node to the graph.
    pub fn add_borrow(&mut self, mut borrow: BorrowNode) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        if borrow.id.0 == 0 {
            borrow.id = BorrowId(self.next_borrow_id);
            self.next_borrow_id += 1;
        }
        self.borrow_nodes.insert(borrow.id, borrow.clone());
        self.nodes.insert(id, Node::Borrow(borrow));
        id
    }

    /// Add an edge to the graph.
    pub fn add_edge(
        &mut self,
        kind: EdgeKind,
        from: NodeId,
        to: NodeId,
        span: Span,
        tir_op: String,
        before_state: CapabilityState,
        after_state: CapabilityState,
    ) -> EdgeId {
        let id = EdgeId(self.next_edge_id);
        self.next_edge_id += 1;
        let constraint_id = if self.next_constraint_id > 0 {
            Some(ConstraintId(self.next_constraint_id))
        } else {
            None
        };
        let edge = Edge {
            id,
            kind,
            from,
            to,
            span,
            tir_op,
            before_state,
            after_state,
            constraint_id,
        };
        self.edges.push(edge);
        id
    }

    /// Get a place node by ID.
    #[must_use]
    pub fn get_place(&self, id: PlaceId) -> Option<&PlaceNode> {
        self.nodes.values().find_map(|n| match n {
            Node::Place(p) if p.id() == id => Some(p),
            _ => None,
        })
    }

    /// Get a borrow node by ID.
    #[must_use]
    pub fn get_borrow(&self, id: BorrowId) -> Option<&BorrowNode> {
        self.borrow_nodes.get(&id)
    }

    /// Get all edges.
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Get all nodes.
    #[must_use]
    pub fn nodes(&self) -> &HashMap<NodeId, Node> {
        &self.nodes
    }

    /// Get current capability state for a place.
    #[must_use]
    pub fn state(&self, place: PlaceId) -> Option<&CapabilityState> {
        self.place_states.get(&place)
    }

    /// Update capability state for a place.
    pub fn set_state(&mut self, place: PlaceId, state: CapabilityState) {
        self.place_states.insert(place, state);
    }

    /// Advance to the next program point.
    pub fn next_point(&mut self) -> u32 {
        let point = self.current_point;
        self.current_point += 1;
        point
    }

    /// Current program point.
    #[must_use]
    pub fn current_point(&self) -> u32 {
        self.current_point
    }

    /// Get all places.
    #[must_use]
    pub fn places(&self) -> impl Iterator<Item = &PlaceNode> {
        self.nodes.values().filter_map(|n| match n {
            Node::Place(p) => Some(p),
            _ => None,
        })
    }

    /// Get all borrows.
    #[must_use]
    pub fn borrows(&self) -> impl Iterator<Item = &BorrowNode> {
        self.borrow_nodes.values()
    }

    /// Find place by name (for locals/remotes).
    #[must_use]
    pub fn find_place(&self, name: &str) -> Option<PlaceId> {
        self.places().find(|p| match p {
            PlaceNode::Local { name: n, .. } => n == name,
            PlaceNode::Remote { arg_index, .. } => format!("arg#{arg_index}") == name,
            _ => false,
        }).map(|p| p.id())
    }

    /// Add a constraint ID for the next edge.
    pub fn next_constraint(&mut self) -> ConstraintId {
        let id = ConstraintId(self.next_constraint_id);
        self.next_constraint_id += 1;
        id
    }
}