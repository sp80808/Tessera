//! Tessera Intent IR (TIR): explicit, lossless semantic expansion of TC.
//!
//! Design rule: every fact the TC surface leaves implicit (operand types,
//! literal types, result types) is written out on every node. Nothing here is
//! canonical source; TIR is always derived from TC. Where TC can spell a TIR
//! program, lowering back yields the canonical TC spelling (see
//! `tessera_sema::lower_to_tc`); where it cannot, lowering says so.
//!
//! Textual form is a boring S-expression (`.tir` files), one `(func ...)` per
//! function, e.g.
//! `(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))`.
//!
//! TIR is **explanatory, never an optimization IR** (ADR 0002 decision 5): it
//! stays a structured expression tree with no basic blocks. `Let`, `If` and
//! `Call` exist so MIR (#22) can be built and tested from TIR alone (TIR-5)
//! without any TC syntax for them yet.
//!
//! The crate is self-contained on purpose (contract TIR-2): [`TirModule::parse`]
//! reads the text form and [`verify_module`] checks it without consulting
//! the parser, HIR or any other phase, so a hand-written `.tir` file can be
//! verified and lowered.
//!
//! Provenance is a side table ([`FunctionProvenance`]) keyed by [`TirNodeId`],
//! never a field on a node (PROV-3), so TIR values compare equal across
//! reformatting of the source.

use std::fmt;

use tessera_phases::{FileId, Provenance, ProvenanceMap};

pub mod eval;
mod parse;
mod verify;

pub use parse::{MAX_TIR_DEPTH, TirParseError};
pub use verify::{TirError, TirErrorKind, verify_function, verify_module};

/// Scalar type in the v0 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TirType {
    I64,
    Bool,
}

impl TirType {
    /// Canonical spelling used in both TC and TIR text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::Bool => "bool",
        }
    }

    /// All known scalar types, in canonical declaration order.
    #[must_use]
    pub const fn all() -> &'static [TirType] {
        &[TirType::I64, TirType::Bool]
    }

    /// Parse a type from its canonical spelling, or `None` if unknown.
    #[must_use]
    pub fn parse(name: &str) -> Option<TirType> {
        match name {
            "i64" => Some(TirType::I64),
            "bool" => Some(TirType::Bool),
            _ => None,
        }
    }
}

/// Identity of an expression node within one function body: its index in
/// pre-order (node, then children left to right). Body-local; only meaningful
/// together with the owning function (contract §2.4, `ExprId`-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TirNodeId(pub u32);

/// Explicitly typed expression. The `ty` on every node is the inferred fact
/// TC omits; `Int` literals default to `I64` in the v0 subset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TirExpr {
    Int {
        value: i64,
        ty: TirType,
    },
    Bool {
        value: bool,
    },
    Var {
        name: String,
        ty: TirType,
    },
    Add {
        lhs: Box<Self>,
        rhs: Box<Self>,
        ty: TirType,
    },
    Eq {
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    And {
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    Not {
        expr: Box<Self>,
    },
    /// `name` is bound to `init` (declared type `ty`) for the extent of
    /// `body`; the expression's type is the body's type. Shadowing is allowed:
    /// a `Var` refers to the innermost binder.
    Let {
        name: String,
        ty: TirType,
        init: Box<Self>,
        body: Box<Self>,
    },
    /// Both branches are expressions of type `ty`; `cond` is `Bool`.
    If {
        cond: Box<Self>,
        then_branch: Box<Self>,
        else_branch: Box<Self>,
        ty: TirType,
    },
    /// Call of a function in the same module; `ty` is the callee's return type.
    Call {
        callee: String,
        args: Vec<Self>,
        ty: TirType,
    },
}

impl TirExpr {
    #[must_use]
    pub fn ty(&self) -> TirType {
        match self {
            Self::Int { ty, .. }
            | Self::Var { ty, .. }
            | Self::Add { ty, .. }
            | Self::If { ty, .. }
            | Self::Call { ty, .. } => *ty,
            Self::Bool { .. } | Self::Eq { .. } | Self::And { .. } | Self::Not { .. } => {
                TirType::Bool
            }
            Self::Let { body, .. } => body.ty(),
        }
    }

    /// Direct sub-expressions in the fixed order used for [`TirNodeId`]s.
    #[must_use]
    pub fn children(&self) -> Vec<&TirExpr> {
        match self {
            Self::Int { .. } | Self::Bool { .. } | Self::Var { .. } => Vec::new(),
            Self::Add { lhs, rhs, .. } | Self::Eq { lhs, rhs } | Self::And { lhs, rhs } => {
                vec![lhs, rhs]
            }
            Self::Not { expr } => vec![expr],
            Self::Let { init, body, .. } => vec![init, body],
            Self::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => vec![cond, then_branch, else_branch],
            Self::Call { args, .. } => args.iter().collect(),
        }
    }

    /// Operation name as written in TIR text (`add`, `let`, ...).
    #[must_use]
    pub const fn op_name(&self) -> &'static str {
        match self {
            Self::Int { .. } => "int",
            Self::Bool { .. } => "bool",
            Self::Var { .. } => "var",
            Self::Add { .. } => "add",
            Self::Eq { .. } => "eq",
            Self::And { .. } => "and",
            Self::Not { .. } => "not",
            Self::Let { .. } => "let",
            Self::If { .. } => "if",
            Self::Call { .. } => "call",
        }
    }

    /// Boring explicit textual form; every node carries its type.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write_text(&mut out);
        out
    }

    /// Append the text form to `out`. One shared buffer and no formatting
    /// machinery in the recursive frame: the stack cost per nesting level stays
    /// small, which is what lets bounded hostile input print safely.
    pub fn write_text(&self, out: &mut String) {
        match self {
            Self::Int { .. } | Self::Bool { .. } | Self::Var { .. } => self.write_leaf(out),
            Self::Add { lhs, rhs, ty } => {
                out.push_str("(add ");
                out.push_str(ty.as_str());
                out.push(' ');
                lhs.write_text(out);
                out.push(' ');
                rhs.write_text(out);
                out.push(')');
            }
            Self::Eq { lhs, rhs } | Self::And { lhs, rhs } => {
                out.push('(');
                out.push_str(self.op_name());
                out.push(' ');
                lhs.write_text(out);
                out.push(' ');
                rhs.write_text(out);
                out.push(')');
            }
            Self::Not { expr } => {
                out.push_str("(not ");
                expr.write_text(out);
                out.push(')');
            }
            Self::Let {
                name,
                ty,
                init,
                body,
            } => {
                out.push_str("(let ");
                out.push_str(name);
                out.push(' ');
                out.push_str(ty.as_str());
                out.push(' ');
                init.write_text(out);
                out.push(' ');
                body.write_text(out);
                out.push(')');
            }
            Self::If {
                cond,
                then_branch,
                else_branch,
                ty,
            } => {
                out.push_str("(if ");
                out.push_str(ty.as_str());
                out.push(' ');
                cond.write_text(out);
                out.push(' ');
                then_branch.write_text(out);
                out.push(' ');
                else_branch.write_text(out);
                out.push(')');
            }
            Self::Call { callee, args, ty } => {
                out.push_str("(call ");
                out.push_str(callee);
                out.push(' ');
                out.push_str(ty.as_str());
                for arg in args {
                    out.push(' ');
                    arg.write_text(out);
                }
                out.push(')');
            }
        }
    }

    /// Leaves format numbers; kept out of the recursive frame on purpose.
    #[inline(never)]
    fn write_leaf(&self, out: &mut String) {
        match self {
            Self::Int { value, ty } => {
                out.push_str("(int ");
                out.push_str(&value.to_string());
                out.push(' ');
                out.push_str(ty.as_str());
                out.push(')');
            }
            Self::Bool { value } => {
                out.push_str(if *value {
                    "(bool true)"
                } else {
                    "(bool false)"
                });
            }
            Self::Var { name, ty } => {
                out.push_str("(var ");
                out.push_str(name);
                out.push(' ');
                out.push_str(ty.as_str());
                out.push(')');
            }
            _ => {}
        }
    }

    /// Visit every node in pre-order with its [`TirNodeId`], numbering from
    /// `first` (a function body starts at 0).
    pub fn walk_preorder(&self, first: u32, visit: &mut impl FnMut(TirNodeId, &TirExpr)) -> u32 {
        visit(TirNodeId(first), self);
        let mut next = first + 1;
        for child in self.children() {
            next = child.walk_preorder(next, visit);
        }
        next
    }
}

/// A single function parameter with its explicit type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TirParam {
    pub name: String,
    pub ty: TirType,
}

/// Explicit function definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TirFunction {
    pub name: String,
    pub params: Vec<TirParam>,
    pub ret: TirType,
    pub body: TirExpr,
}

impl TirFunction {
    /// Boring explicit textual form (`.tir` golden files use this).
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write_text(&mut out);
        out
    }

    /// Append the text form to `out`.
    pub fn write_text(&self, out: &mut String) {
        out.push_str("(func ");
        out.push_str(&self.name);
        for param in &self.params {
            out.push_str(" (param ");
            out.push_str(&param.name);
            out.push(' ');
            out.push_str(param.ty.as_str());
            out.push(')');
        }
        out.push_str(" (return ");
        out.push_str(self.ret.as_str());
        out.push_str(") (body ");
        self.body.write_text(out);
        out.push_str("))");
    }

    /// Every body node in pre-order with its id.
    #[must_use]
    pub fn nodes(&self) -> Vec<(TirNodeId, &TirExpr)> {
        fn go<'a>(expr: &'a TirExpr, out: &mut Vec<(TirNodeId, &'a TirExpr)>) {
            out.push((
                TirNodeId(u32::try_from(out.len()).unwrap_or(u32::MAX)),
                expr,
            ));
            for child in expr.children() {
                go(child, out);
            }
        }
        let mut out = Vec::new();
        go(&self.body, &mut out);
        out
    }

    /// The node with this id, if it exists.
    #[must_use]
    pub fn node(&self, id: TirNodeId) -> Option<&TirExpr> {
        self.nodes()
            .into_iter()
            .find_map(|(n, e)| (n == id).then_some(e))
    }
}

/// A set of functions; calls resolve within it. Order is source order and is
/// part of the value (it fixes text output and MIR function numbering).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TirModule {
    pub funcs: Vec<TirFunction>,
}

impl TirModule {
    #[must_use]
    pub fn function(&self, name: &str) -> Option<&TirFunction> {
        self.funcs.iter().find(|f| f.name == name)
    }

    /// One `(func ...)` per line, no trailing newline. A single-function
    /// module prints exactly [`TirFunction::to_text`].
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, func) in self.funcs.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            func.write_text(&mut out);
        }
        out
    }

    /// Parse the text form. Total: any input yields a module or an error with
    /// a byte offset; it never panics and nesting is bounded by
    /// [`MAX_TIR_DEPTH`]. Well-formedness (types, scopes, calls) is *not*
    /// checked here; use [`verify_module`].
    pub fn parse(text: &str) -> Result<TirModule, TirParseError> {
        parse::parse_module(FileId(0), text).map(|(module, _)| module)
    }

    /// [`Self::parse`], also returning where each function, parameter and body
    /// node sits in `text` (file `file`): every span covers its whole
    /// parenthesized form. The table is total (PROV-1), so a hand-written
    /// `.tir` file can be lowered with diagnostics that point into it.
    pub fn parse_with_provenance(
        file: FileId,
        text: &str,
    ) -> Result<(TirModule, ModuleProvenance), TirParseError> {
        parse::parse_module(file, text)
    }
}

/// Provenance of one function's TIR nodes: a side table, not node fields
/// (PROV-3). Total over the function when [`Self::missing`] is empty (PROV-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionProvenance {
    /// The `func` item as a whole.
    pub func: Provenance,
    /// One entry per parameter, in order.
    pub params: Vec<Provenance>,
    pub nodes: ProvenanceMap<TirNodeId>,
}

impl FunctionProvenance {
    /// Body node ids of `func` that have no provenance (PROV-1 violations).
    #[must_use]
    pub fn missing(&self, func: &TirFunction) -> Vec<TirNodeId> {
        self.nodes
            .missing(func.nodes().into_iter().map(|(id, _)| id))
    }
}

/// Provenance for a whole module, parallel to [`TirModule::funcs`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleProvenance {
    pub funcs: Vec<FunctionProvenance>,
}

/// Compact display for diagnostics and logs.
impl fmt::Display for TirType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn add_func() -> TirFunction {
        TirFunction {
            name: "add".to_owned(),
            params: vec![
                TirParam {
                    name: "a".to_owned(),
                    ty: TirType::I64,
                },
                TirParam {
                    name: "b".to_owned(),
                    ty: TirType::I64,
                },
            ],
            ret: TirType::I64,
            body: TirExpr::Add {
                lhs: Box::new(TirExpr::Var {
                    name: "a".to_owned(),
                    ty: TirType::I64,
                }),
                rhs: Box::new(TirExpr::Var {
                    name: "b".to_owned(),
                    ty: TirType::I64,
                }),
                ty: TirType::I64,
            },
        }
    }

    #[test]
    fn tir_text_makes_every_type_explicit() {
        assert_eq!(
            add_func().to_text(),
            "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))"
        );
    }

    #[test]
    fn bool_literal_round_trips() {
        let expr = TirExpr::Bool { value: true };
        assert_eq!(expr.ty(), TirType::Bool);
        assert_eq!(expr.to_text(), "(bool true)");
    }

    #[test]
    fn type_parse_round_trip() {
        for ty in TirType::all() {
            assert_eq!(TirType::parse(ty.as_str()), Some(*ty));
        }
        assert_eq!(TirType::parse("i32"), None);
    }

    #[test]
    fn new_nodes_have_explicit_text_and_types() {
        let e = TirExpr::Let {
            name: "x".to_owned(),
            ty: TirType::I64,
            init: Box::new(TirExpr::Int {
                value: 1,
                ty: TirType::I64,
            }),
            body: Box::new(TirExpr::If {
                cond: Box::new(TirExpr::Eq {
                    lhs: Box::new(TirExpr::Var {
                        name: "x".to_owned(),
                        ty: TirType::I64,
                    }),
                    rhs: Box::new(TirExpr::Int {
                        value: 1,
                        ty: TirType::I64,
                    }),
                }),
                then_branch: Box::new(TirExpr::Call {
                    callee: "g".to_owned(),
                    args: vec![TirExpr::Var {
                        name: "x".to_owned(),
                        ty: TirType::I64,
                    }],
                    ty: TirType::I64,
                }),
                else_branch: Box::new(TirExpr::Int {
                    value: 0,
                    ty: TirType::I64,
                }),
                ty: TirType::I64,
            }),
        };
        assert_eq!(e.ty(), TirType::I64);
        assert_eq!(
            e.to_text(),
            "(let x i64 (int 1 i64) (if i64 (eq (var x i64) (int 1 i64)) (call g i64 (var x i64)) (int 0 i64)))"
        );
    }

    #[test]
    fn node_ids_are_preorder_and_dense() {
        let f = add_func();
        let ids: Vec<_> = f
            .nodes()
            .iter()
            .map(|(id, e)| (id.0, e.op_name()))
            .collect();
        assert_eq!(ids, [(0, "add"), (1, "var"), (2, "var")]);
        assert_eq!(f.node(TirNodeId(2)).map(TirExpr::op_name), Some("var"));
        assert!(f.node(TirNodeId(3)).is_none());
        let mut seen = Vec::new();
        let next = f.body.walk_preorder(0, &mut |id, _| seen.push(id.0));
        assert_eq!((seen, next), (vec![0, 1, 2], 3));
    }

    #[test]
    fn provenance_table_reports_missing_nodes() {
        use tessera_phases::{FileId, Span};
        let f = add_func();
        let s = |a, b| Provenance::Source(Span::new(FileId(0), a, b));
        let mut nodes = ProvenanceMap::new();
        nodes.insert(TirNodeId(0), s(23, 26));
        nodes.insert(TirNodeId(1), s(23, 24));
        let prov = FunctionProvenance {
            func: s(0, 26),
            params: vec![s(6, 11), s(12, 17)],
            nodes,
        };
        assert_eq!(prov.missing(&f), vec![TirNodeId(2)]);
    }

    /// `.tir` provenance is total and each span is the node's whole form.
    #[test]
    fn parsed_provenance_is_total_and_points_at_each_form() {
        let text = "; header\n(func add (param a i64) (param b i64) (return i64)\n  (body (add i64 (var a i64) (var b i64))))";
        let (m, prov) =
            TirModule::parse_with_provenance(tessera_phases::FileId(3), text).expect("parses");
        assert_eq!(m, TirModule::parse(text).expect("same module"));
        let fp = &prov.funcs[0];
        assert!(fp.missing(&m.funcs[0]).is_empty());
        let at = |p: Provenance| {
            let s = p.primary_span();
            assert_eq!(s.file, tessera_phases::FileId(3));
            &text[s.start as usize..s.end as usize]
        };
        assert!(at(fp.func).starts_with("(func add") && at(fp.func).ends_with("))))"));
        assert_eq!(
            fp.params.iter().map(|p| at(*p)).collect::<Vec<_>>(),
            ["(param a i64)", "(param b i64)"]
        );
        let nodes: Vec<_> = fp.nodes.iter().map(|(_, p)| at(p)).collect();
        assert_eq!(
            nodes,
            [
                "(add i64 (var a i64) (var b i64))",
                "(var a i64)",
                "(var b i64)"
            ]
        );
    }

    #[test]
    fn module_text_is_one_function_per_line_and_single_matches_function() {
        let m = TirModule {
            funcs: vec![add_func()],
        };
        assert_eq!(m.to_text(), add_func().to_text());
        let two = TirModule {
            funcs: vec![add_func(), add_func()],
        };
        assert_eq!(two.to_text().lines().count(), 2);
    }
}
