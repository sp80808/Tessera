//! Each TIR `Var` read lands on the place of its innermost binder: the
//! parameter's `Remote` place by name, or the `Local` place of the enclosing
//! `let` that binds it.

use tessera_tcap::derive::BorrowingDeriver;
use tessera_tcap::{CapabilityGraph, Node, NodeId, PlaceNode, derive_tcap};
use tessera_tir::{TirFunction, TirModule, verify_module};

fn func(text: &str) -> TirFunction {
    let module = TirModule::parse(text).expect("parses");
    assert_eq!(verify_module(&module), Vec::new(), "verifies");
    module.funcs.into_iter().next().expect("one function")
}

/// `arg#i` for a parameter, `name@id` for a `let` binder.
fn place(graph: &CapabilityGraph, node: NodeId) -> String {
    match graph.nodes().get(&node) {
        Some(Node::Place(PlaceNode::Remote { arg_index, .. })) => format!("arg#{arg_index}"),
        Some(Node::Place(PlaceNode::Local { id, name, .. })) => format!("{name}{id}"),
        other => panic!("edge targets {other:?}"),
    }
}

/// Every edge as `kind place`, in order.
fn edges(graph: &CapabilityGraph) -> Vec<String> {
    graph
        .edges()
        .iter()
        .map(|e| {
            assert_eq!(e.from, e.to, "{} is a self edge", e.kind);
            format!("{} {}", e.kind, place(graph, e.to))
        })
        .collect()
}

/// Both derivers resolve names the same way.
fn derived_edges(text: &str) -> Vec<String> {
    let f = func(text);
    let plain = edges(&derive_tcap(&f));
    assert_eq!(plain, edges(&BorrowingDeriver::new().derive(&f)));
    plain
}

#[test]
fn each_parameter_read_targets_its_own_remote_place() {
    let text = "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))";
    assert_eq!(derived_edges(text), ["read arg#0", "read arg#1"]);

    let graph = derive_tcap(&func(text));
    let mut remotes: Vec<_> = graph
        .places()
        .map(|p| match p {
            PlaceNode::Remote { arg_index, .. } => *arg_index,
            other => panic!("parameters only, got {other}"),
        })
        .collect();
    remotes.sort_unstable();
    assert_eq!(remotes, [0, 1]);
}

#[test]
fn parameters_resolve_by_name_not_position() {
    let text = "(func pick (param x i64) (param y i64) (param z i64) (return i64) \
                (body (add i64 (var z i64) (add i64 (var y i64) (var x i64)))))";
    assert_eq!(
        derived_edges(text),
        ["read arg#2", "read arg#1", "read arg#0"]
    );
}

#[test]
fn if_and_call_operands_are_read() {
    let text = "(func f (param a i64) (param b bool) (return i64) \
                (body (if i64 (var b bool) (call f i64 (var a i64) (not (var b bool))) (var a i64))))";
    assert_eq!(
        derived_edges(text),
        ["read arg#1", "read arg#0", "read arg#1", "read arg#0"]
    );
}

#[test]
fn let_binders_shadow_and_are_dropped_when_their_scope_ends() {
    // Places @1 and @2 are the parameters; each `let a` gets a fresh local.
    let text = "(func f (param a i64) (param b i64) (return i64) (body \
                  (add i64 \
                    (let a i64 (var b i64) \
                      (let a i64 (add i64 (var a i64) (int 1 i64)) \
                        (var a i64))) \
                    (var a i64))))";
    assert_eq!(
        derived_edges(text),
        [
            // outer initializer: `a` is not bound yet, `b` is the parameter
            "read arg#1",
            // inner initializer sees the outer `let a`, not itself
            "read a@3",
            // innermost binding wins
            "read a@4",
            "drop a@4",
            "drop a@3",
            // both scopes have ended: `a` is the parameter again
            "read arg#0",
        ]
    );
}
