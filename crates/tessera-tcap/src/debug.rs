//! Debug projection for `tsr explain --ownership` trace format.

use super::graph::{CapabilityGraph, Edge, EdgeKind, Node};
use super::lattice::{BorrowId, BorrowKind, Capability, CapabilityState, PlaceId};
use super::nodes::{BorrowExtent, PlaceNode, PlaceType, Span};
use std::fmt;

/// Debug projection for ownership traces.
pub struct DebugProjection<'a> {
    graph: &'a CapabilityGraph,
    verbose: bool,
}

impl<'a> DebugProjection<'a> {
    #[must_use]
    pub fn new(graph: &'a CapabilityGraph) -> Self {
        Self { graph, verbose: false }
    }

    #[must_use]
    pub fn verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Generate a compact trace for `tsr explain --ownership`.
    #[must_use]
    pub fn trace(&self) -> String {
        let mut output = String::new();
        
        // Initial state: all places at function entry
        for place in self.graph.places() {
            let name = match place {
                PlaceNode::Local { name, .. } => name.clone(),
                PlaceNode::Remote { arg_index, .. } => format!("arg#{arg_index}"),
                _ => continue,
            };
            let state = self.graph.state(place.id()).cloned().unwrap_or_default();
            output.push_str(&format!("{name}:{}", state.capability));
            output.push('\n');
        }

        // Trace edges in order
        for edge in self.graph.edges() {
            output.push_str(&self.format_edge(edge));
            output.push('\n');
        }

        output.trim_end().to_string()
    }

    /// Generate verbose explanation with full details.
    #[must_use]
    pub fn explain(&self) -> String {
        let mut output = String::new();
        
        output.push_str("=== TCap Ownership Trace ===\n\n");
        
        // Places
        output.push_str("Places:\n");
        for place in self.graph.places() {
            let state = self.graph.state(place.id()).cloned().unwrap_or_default();
            output.push_str(&format!("  {} @{} = {} ({:?})\n", 
                self.place_short_name(place), place.id(), state.capability, place));
        }
        output.push('\n');

        // Borrows
        output.push_str("Borrows:\n");
        for borrow in self.graph.borrows() {
            output.push_str(&format!("  {} @{} {:?} place={} extent={:?}\n",
                borrow.id, borrow.kind, borrow.id, borrow.place, borrow.extent));
        }
        output.push('\n');

        // Transitions
        output.push_str("Transitions:\n");
        for edge in self.graph.edges() {
            output.push_str(&format!("  {}: {}\n", edge.span, self.format_edge_verbose(edge)));
        }

        output
    }

    fn format_edge(&self, edge: &Edge) -> String {
        let from_name = self.node_short_name(edge.from);
        let to_name = self.node_short_name(edge.to);
        
        match edge.kind {
            EdgeKind::Move => format!("{} move {}->{} => {}:{}, {}:{}", 
                edge.span, from_name, to_name,
                from_name, edge.before_state.capability,
                to_name, edge.after_state.capability),
            EdgeKind::Share => format!("{} share {}->{} => {}:{}, {}:{}", 
                edge.span, from_name, to_name,
                from_name, edge.before_state.capability,
                to_name, edge.after_state.capability),
            EdgeKind::LoanMut => format!("{} loan_mut {}->{} => {}:{}, {}:{}", 
                edge.span, from_name, to_name,
                from_name, edge.before_state.capability,
                to_name, edge.after_state.capability),
            EdgeKind::Reborrow => format!("{} reborrow {}->{}", edge.span, from_name, to_name),
            EdgeKind::Restore => format!("{} restore {}->{} => {}:{}", 
                edge.span, from_name, to_name,
                to_name, edge.after_state.capability),
            EdgeKind::Split => format!("{} split {}->{}", edge.span, from_name, to_name),
            EdgeKind::Join => format!("{} join {}->{} => {}:{}", 
                edge.span, from_name, to_name,
                to_name, edge.after_state.capability),
            EdgeKind::Write => format!("{} write {} => {}:{}", 
                edge.span, from_name,
                from_name, edge.after_state.capability),
            EdgeKind::Read => format!("{} read {}", edge.span, from_name),
            EdgeKind::Drop => format!("{} drop {} => {}:{}", 
                edge.span, from_name,
                from_name, edge.after_state.capability),
        }
    }

    fn format_edge_verbose(&self, edge: &Edge) -> String {
        let from_name = self.node_short_name(edge.from);
        let to_name = self.node_short_name(edge.to);
        
        let mut s = format!("{} {}: {} -> {} [{}]", 
            edge.span, edge.kind, from_name, to_name, edge.tir_op);
        
        if edge.before_state != edge.after_state {
            s.push_str(&format!(" ({:?} -> {:?})", edge.before_state.capability, edge.after_state.capability));
        }
        
        if let Some(cid) = edge.constraint_id {
            s.push_str(&format!(" [{}]", cid));
        }
        
        s
    }

    fn node_short_name(&self, node_id: super::graph::NodeId) -> String {
        self.graph.nodes().get(&node_id).map(|n| match n {
            Node::Place(p) => self.place_short_name(p),
            Node::Borrow(b) => format!("r{}", b.id),
        }).unwrap_or_else(|| format!("n{}", node_id.0))
    }

    fn place_short_name(&self, place: &PlaceNode) -> String {
        match place {
            PlaceNode::Local { name, .. } => name.clone(),
            PlaceNode::Remote { arg_index, .. } => format!("arg#{arg_index}"),
            PlaceNode::Field { base, field, .. } => format!("{base}.{field}"),
            PlaceNode::Index { base, index, .. } => format!("{base}[{index}]"),
            PlaceNode::Deref { base, .. } => format!("*{base}"),
        }
    }
}

/// Compact trace formatter matching the spec example.
pub fn format_trace(graph: &CapabilityGraph) -> String {
    DebugProjection::new(graph).trace()
}

/// Verbose explanation formatter.
pub fn format_explain(graph: &CapabilityGraph) -> String {
    DebugProjection::new(graph).verbose(true).explain()
}

/// Generate a diagnostic message for a transition error.
#[must_use]
pub fn format_error(place: PlaceId, cap: Capability, operation: &str) -> String {
    format!("{operation} {place} blocked: capability is {cap}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::CapabilityGraph;
    use crate::nodes::{PlaceNode, PlaceType, Span};
    use crate::lattice::{PlaceId, CapabilityState, Capability};

    #[test]
    fn trace_format_basic() {
        let mut graph = CapabilityGraph::new();
        
        let x = PlaceNode::Local {
            id: PlaceId(1),
            name: "x".to_string(),
            ty: PlaceType::Scalar,
        };
        graph.add_place(x.clone());
        graph.set_state(PlaceId(1), CapabilityState::exclusive());

        let trace = format_trace(&graph);
        assert!(trace.contains("x:E"));
    }

    #[test]
    fn trace_format_share() {
        let mut graph = CapabilityGraph::new();
        
        let x = PlaceNode::Local {
            id: PlaceId(1),
            name: "x".to_string(),
            ty: PlaceType::Scalar,
        };
        let x_id = graph.add_place(x);
        
        // Simulate share edge
        let r = crate::nodes::BorrowNode {
            id: crate::lattice::BorrowId(1),
            kind: BorrowKind::Shared,
            place: PlaceId(1),
            extent: BorrowExtent::Lexical { start: 1, end: 5 },
            origin_span: Span::dummy(),
        };
        let r_id = graph.add_borrow(r);
        
        graph.add_edge(
            EdgeKind::Share,
            x_id,
            r_id,
            Span::new(1, 12, 18),
            "share x".to_string(),
            CapabilityState::exclusive(),
            CapabilityState::read(),
        );

        let trace = format_trace(&graph);
        assert!(trace.contains("x:E"));
        assert!(trace.contains("share"));
    }
}