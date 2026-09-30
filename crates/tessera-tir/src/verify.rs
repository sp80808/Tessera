//! Standalone TIR verifier (contract TIR-2).
//!
//! Checks a [`TirModule`] using nothing but the module itself: no parser, no
//! HIR, no resolution results. Anything a phase upstream of TIR produces must
//! pass this, and a hand-written `.tir` file can be checked the same way.
//!
//! Rules (all types are explicit on every node, so checking is local):
//! - function names are unique, valid, and parameter names are unique;
//! - `Int` is `i64`; `Add` is `i64 + i64 -> i64`; `Eq` compares two operands of
//!   the same type; `And`/`Not` take `Bool`; `If` has a `Bool` condition and two
//!   branches of the node's type; `Let` initializes a binder of its declared
//!   type; `Var` names an in-scope binder of exactly the annotated type
//!   (innermost binding wins; shadowing is allowed);
//! - `Call` names a function of the module, with matching arity, argument types
//!   and result type (recursion is allowed);
//! - a function body has the function's declared return type.
//!
//! Errors come out in a deterministic order: functions in module order, then
//! function-level checks, then body nodes in pre-order ([`TirNodeId`] order).

use std::collections::BTreeSet;
use std::fmt;

use crate::parse::is_valid_name;
use crate::{TirExpr, TirFunction, TirModule, TirNodeId, TirType};

/// One verifier finding: which function, which body node (if node-specific),
/// and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TirError {
    pub func: String,
    /// `None` for function-level findings (names, parameters, return type).
    pub node: Option<TirNodeId>,
    pub kind: TirErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TirErrorKind {
    DuplicateFunction,
    InvalidName(String),
    DuplicateParam(String),
    UnboundVar(String),
    VarTypeMismatch {
        name: String,
        declared: TirType,
        annotated: TirType,
    },
    /// `Int` literal annotated with a type other than `i64`.
    IntNotI64(TirType),
    /// An operand/branch/argument/result did not have the required type.
    TypeMismatch {
        what: &'static str,
        want: TirType,
        got: TirType,
    },
    UnknownCallee(String),
    ArityMismatch {
        callee: String,
        want: usize,
        got: usize,
    },
}

impl fmt::Display for TirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "func `{}`", self.func)?;
        if let Some(node) = self.node {
            write!(f, " node {}", node.0)?;
        }
        write!(f, ": ")?;
        match &self.kind {
            TirErrorKind::DuplicateFunction => write!(f, "duplicate function name"),
            TirErrorKind::InvalidName(n) => write!(f, "invalid name `{n}`"),
            TirErrorKind::DuplicateParam(n) => write!(f, "duplicate parameter `{n}`"),
            TirErrorKind::UnboundVar(n) => write!(f, "unbound variable `{n}`"),
            TirErrorKind::VarTypeMismatch {
                name,
                declared,
                annotated,
            } => write!(
                f,
                "variable `{name}` is declared {declared} but annotated {annotated}"
            ),
            TirErrorKind::IntNotI64(ty) => write!(f, "integer literal annotated {ty}, want i64"),
            TirErrorKind::TypeMismatch { what, want, got } => {
                write!(f, "{what}: expected {want}, found {got}")
            }
            TirErrorKind::UnknownCallee(n) => write!(f, "call of unknown function `{n}`"),
            TirErrorKind::ArityMismatch { callee, want, got } => write!(
                f,
                "call of `{callee}` with {got} arguments, expected {want}"
            ),
        }
    }
}

impl std::error::Error for TirError {}

struct Checker<'a> {
    module: &'a TirModule,
    func: &'a TirFunction,
    scope: Vec<(&'a str, TirType)>,
    next: u32,
    errors: Vec<TirError>,
}

impl<'a> Checker<'a> {
    fn report(&mut self, node: Option<TirNodeId>, kind: TirErrorKind) {
        self.errors.push(TirError {
            func: self.func.name.clone(),
            node,
            kind,
        });
    }

    fn expect(&mut self, node: TirNodeId, what: &'static str, want: TirType, got: TirType) {
        if want != got {
            self.report(Some(node), TirErrorKind::TypeMismatch { what, want, got });
        }
    }

    fn check(&mut self, expr: &'a TirExpr) {
        let id = TirNodeId(self.next);
        self.next += 1;
        match expr {
            TirExpr::Int { ty, .. } => {
                if *ty != TirType::I64 {
                    self.report(Some(id), TirErrorKind::IntNotI64(*ty));
                }
            }
            TirExpr::Bool { .. } => {}
            TirExpr::Var { name, ty } => {
                let bound = self.scope.iter().rev().find(|(n, _)| n == name);
                match bound {
                    None => self.report(Some(id), TirErrorKind::UnboundVar(name.clone())),
                    Some(&(_, declared)) if declared != *ty => self.report(
                        Some(id),
                        TirErrorKind::VarTypeMismatch {
                            name: name.clone(),
                            declared,
                            annotated: *ty,
                        },
                    ),
                    Some(_) => {}
                }
            }
            TirExpr::Add { lhs, rhs, ty } => {
                self.expect(id, "result of `add`", TirType::I64, *ty);
                self.operand(id, "left operand of `add`", TirType::I64, lhs);
                self.operand(id, "right operand of `add`", TirType::I64, rhs);
            }
            TirExpr::Eq { lhs, rhs } => {
                self.expect(id, "operands of `eq`", lhs.ty(), rhs.ty());
                self.check(lhs);
                self.check(rhs);
            }
            TirExpr::And { lhs, rhs } => {
                self.operand(id, "left operand of `and`", TirType::Bool, lhs);
                self.operand(id, "right operand of `and`", TirType::Bool, rhs);
            }
            TirExpr::Not { expr } => self.operand(id, "operand of `not`", TirType::Bool, expr),
            TirExpr::Let {
                name,
                ty,
                init,
                body,
            } => {
                if !is_valid_name(name) {
                    self.report(Some(id), TirErrorKind::InvalidName(name.clone()));
                }
                self.operand(id, "initializer of `let`", *ty, init);
                self.scope.push((name, *ty));
                self.check(body);
                self.scope.pop();
            }
            TirExpr::If {
                cond,
                then_branch,
                else_branch,
                ty,
            } => {
                self.operand(id, "condition of `if`", TirType::Bool, cond);
                self.operand(id, "`then` branch of `if`", *ty, then_branch);
                self.operand(id, "`else` branch of `if`", *ty, else_branch);
            }
            TirExpr::Call { callee, args, ty } => self.call(id, callee, args, *ty),
        }
    }

    /// Check `expr` as a child of `parent`, requiring type `want`. The type
    /// finding is attributed to `parent` (the construct imposing the rule).
    fn operand(&mut self, parent: TirNodeId, what: &'static str, want: TirType, expr: &'a TirExpr) {
        self.expect(parent, what, want, expr.ty());
        self.check(expr);
    }

    fn call(&mut self, id: TirNodeId, callee: &'a str, args: &'a [TirExpr], ty: TirType) {
        match self.module.function(callee) {
            None => self.report(Some(id), TirErrorKind::UnknownCallee(callee.to_owned())),
            Some(target) => {
                if target.params.len() != args.len() {
                    self.report(
                        Some(id),
                        TirErrorKind::ArityMismatch {
                            callee: callee.to_owned(),
                            want: target.params.len(),
                            got: args.len(),
                        },
                    );
                }
                self.expect(id, "result of call", target.ret, ty);
                for (i, arg) in args.iter().enumerate() {
                    if let Some(param) = target.params.get(i) {
                        self.expect(id, "call argument", param.ty, arg.ty());
                    }
                }
            }
        }
        for arg in args {
            self.check(arg);
        }
    }
}

/// Verify one function in the context of its module (calls resolve there).
/// Function-name uniqueness is a module property and is checked by
/// [`verify_module`].
#[must_use]
pub fn verify_function(module: &TirModule, func: &TirFunction) -> Vec<TirError> {
    let mut checker = Checker {
        module,
        func,
        scope: Vec::new(),
        next: 0,
        errors: Vec::new(),
    };
    if !is_valid_name(&func.name) {
        checker.report(None, TirErrorKind::InvalidName(func.name.clone()));
    }
    let mut seen = BTreeSet::new();
    for param in &func.params {
        if !is_valid_name(&param.name) {
            checker.report(None, TirErrorKind::InvalidName(param.name.clone()));
        }
        if !seen.insert(param.name.as_str()) {
            checker.report(None, TirErrorKind::DuplicateParam(param.name.clone()));
        }
        checker.scope.push((&param.name, param.ty));
    }
    if func.body.ty() != func.ret {
        checker.report(
            None,
            TirErrorKind::TypeMismatch {
                what: "function body",
                want: func.ret,
                got: func.body.ty(),
            },
        );
    }
    checker.check(&func.body);
    checker.errors
}

/// Verify every function of the module. Empty result means well-formed.
#[must_use]
pub fn verify_module(module: &TirModule) -> Vec<TirError> {
    let mut errors = Vec::new();
    let mut names = BTreeSet::new();
    for func in &module.funcs {
        if !names.insert(func.name.as_str()) {
            errors.push(TirError {
                func: func.name.clone(),
                node: None,
                kind: TirErrorKind::DuplicateFunction,
            });
        }
        errors.extend(verify_function(module, func));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TirParam;

    fn module(text: &str) -> TirModule {
        TirModule::parse(text).expect("fixture parses")
    }

    fn kinds(text: &str) -> Vec<(Option<u32>, TirErrorKind)> {
        verify_module(&module(text))
            .into_iter()
            .map(|e| (e.node.map(|n| n.0), e.kind))
            .collect()
    }

    #[test]
    fn bootstrap_and_control_flow_fixtures_verify() {
        for text in [
            "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))",
            "(func abs (param x i64) (return i64) (body (if i64 (eq (var x i64) (int 0 i64)) (int 0 i64) (var x i64))))",
            "(func f (param a i64) (return i64) (body (let x i64 (add i64 (var a i64) (int 1 i64)) (let x i64 (var x i64) (var x i64)))))",
            "(func fact (param n i64) (return i64) (body (if i64 (eq (var n i64) (int 0 i64)) (int 1 i64) (call fact i64 (var n i64)))))",
            "(func t (return bool) (body (and (bool true) (not (bool false)))))",
        ] {
            let m = module(text);
            assert_eq!(verify_module(&m), vec![], "{text}");
            assert_eq!(m.to_text(), text);
        }
    }

    #[test]
    fn unbound_and_mistyped_variables_are_rejected() {
        assert_eq!(
            kinds("(func f (return i64) (body (var a i64)))"),
            [(Some(0), TirErrorKind::UnboundVar("a".into()))]
        );
        assert_eq!(
            kinds("(func f (param a i64) (return bool) (body (var a bool)))"),
            [(
                Some(0),
                TirErrorKind::VarTypeMismatch {
                    name: "a".into(),
                    declared: TirType::I64,
                    annotated: TirType::Bool
                }
            )]
        );
        // a `let` binder is not in scope in its own initializer
        assert_eq!(
            kinds("(func f (return i64) (body (let x i64 (var x i64) (var x i64))))"),
            [(Some(1), TirErrorKind::UnboundVar("x".into()))]
        );
    }

    #[test]
    fn operator_type_rules_are_enforced_and_attributed_to_the_construct() {
        // `add` of bools: both operand findings point at the `add` node (0)
        let errs = kinds("(func f (return i64) (body (add i64 (bool true) (bool false))))");
        assert_eq!(errs.len(), 2);
        assert!(errs.iter().all(|(n, _)| *n == Some(0)));
        assert_eq!(
            kinds(
                "(func f (param a i64) (param b bool) (return bool) (body (eq (var a i64) (var b bool))))"
            ),
            [(
                Some(0),
                TirErrorKind::TypeMismatch {
                    what: "operands of `eq`",
                    want: TirType::I64,
                    got: TirType::Bool
                }
            )]
        );
        assert!(!kinds("(func f (return bool) (body (not (int 1 i64))))").is_empty());
        assert!(!kinds("(func f (return i64) (body (int 1 bool)))").is_empty());
        assert!(
            !kinds("(func f (return i64) (body (if i64 (int 1 i64) (int 1 i64) (int 2 i64))))")
                .is_empty()
        );
        assert!(
            !kinds("(func f (return i64) (body (if i64 (bool true) (int 1 i64) (bool false))))")
                .is_empty()
        );
        // body type vs declared return type is a function-level finding
        assert_eq!(
            kinds("(func f (return bool) (body (int 1 i64)))"),
            [(
                None,
                TirErrorKind::TypeMismatch {
                    what: "function body",
                    want: TirType::Bool,
                    got: TirType::I64
                }
            )]
        );
    }

    #[test]
    fn calls_are_checked_against_the_callee_signature() {
        let base = "(func g (param a i64) (return i64) (body (var a i64)))";
        assert_eq!(
            kinds(&format!(
                "{base}\n(func f (return i64) (body (call g i64 (int 1 i64))))"
            )),
            []
        );
        assert_eq!(
            kinds(&format!(
                "{base}\n(func f (return i64) (body (call h i64)))"
            )),
            [(Some(0), TirErrorKind::UnknownCallee("h".into()))]
        );
        let arity = kinds(&format!(
            "{base}\n(func f (return i64) (body (call g i64)))"
        ));
        assert_eq!(
            arity,
            [(
                Some(0),
                TirErrorKind::ArityMismatch {
                    callee: "g".into(),
                    want: 1,
                    got: 0
                }
            )]
        );
        let arg = kinds(&format!(
            "{base}\n(func f (return i64) (body (call g i64 (bool true))))"
        ));
        assert_eq!(arg.len(), 1);
        assert_eq!(arg[0].0, Some(0));
        let ret = kinds(&format!(
            "{base}\n(func f (return bool) (body (call g bool (int 1 i64))))"
        ));
        assert_eq!(ret.len(), 1);
    }

    #[test]
    fn duplicate_names_and_invalid_names_are_rejected() {
        let dup =
            "(func f (return i64) (body (int 1 i64)))\n(func f (return i64) (body (int 2 i64)))";
        assert_eq!(kinds(dup), [(None, TirErrorKind::DuplicateFunction)]);
        assert_eq!(
            kinds("(func f (param a i64) (param a i64) (return i64) (body (var a i64)))"),
            [(None, TirErrorKind::DuplicateParam("a".into()))]
        );
        // names built by hand (bypassing the parser) are validated too
        let m = TirModule {
            funcs: vec![TirFunction {
                name: "bad name".into(),
                params: vec![TirParam {
                    name: "9x".into(),
                    ty: TirType::I64,
                }],
                ret: TirType::I64,
                body: TirExpr::Int {
                    value: 0,
                    ty: TirType::I64,
                },
            }],
        };
        let errs = verify_module(&m);
        assert_eq!(errs.len(), 2);
        assert!(
            errs.iter()
                .all(|e| matches!(e.kind, TirErrorKind::InvalidName(_)))
        );
    }

    #[test]
    fn errors_are_ordered_and_display_names_the_node() {
        let errs = verify_module(&module(
            "(func f (return i64) (body (add i64 (var a i64) (var b i64))))",
        ));
        let nodes: Vec<_> = errs.iter().map(|e| e.node.map(|n| n.0)).collect();
        assert_eq!(nodes, [Some(1), Some(2)]);
        assert_eq!(errs[0].to_string(), "func `f` node 1: unbound variable `a`");
    }

    /// Node ids the verifier reports must be the same pre-order ids as
    /// `TirFunction::nodes`, otherwise provenance lookups would be wrong.
    #[test]
    fn verifier_node_ids_match_function_node_ids() {
        let m = module(
            "(func f (param a i64) (return i64) (body (let x i64 (var a i64) (if i64 (eq (var x i64) (var q i64)) (var x i64) (int 0 i64)))))",
        );
        let errs = verify_module(&m);
        assert_eq!(errs.len(), 1);
        let id = errs[0].node.expect("node-level finding");
        let node = m.funcs[0].node(id).expect("id exists");
        assert!(matches!(node, TirExpr::Var { name, .. } if name == "q"));
    }

    // ---- property: random well-typed programs verify and round-trip ----

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Generate an expression of type `want` valid in `scope`, calling only
    /// `callees` (name, param types, ret) which are known-good signatures.
    fn generate(
        rng: &mut Rng,
        want: TirType,
        scope: &mut Vec<(String, TirType)>,
        callees: &[(String, Vec<TirType>, TirType)],
        fuel: u32,
    ) -> TirExpr {
        let leaf = fuel == 0 || rng.below(4) == 0;
        // a binder shadowed by a later one of the same name is not referable
        let vars: Vec<_> = scope
            .iter()
            .enumerate()
            .filter(|(i, (name, ty))| {
                *ty == want && !scope[i + 1..].iter().any(|(later, _)| later == name)
            })
            .map(|(_, entry)| entry.clone())
            .collect();
        if leaf {
            if !vars.is_empty() && rng.below(2) == 0 {
                let (name, ty) = vars[rng.below(vars.len() as u64) as usize].clone();
                return TirExpr::Var { name, ty };
            }
            return match want {
                TirType::I64 => TirExpr::Int {
                    value: rng.below(200) as i64 - 100,
                    ty: TirType::I64,
                },
                TirType::Bool => TirExpr::Bool {
                    value: rng.below(2) == 0,
                },
            };
        }
        let sub = |rng: &mut Rng, ty, scope: &mut Vec<(String, TirType)>| {
            Box::new(generate(rng, ty, scope, callees, fuel - 1))
        };
        match (want, rng.below(5)) {
            (TirType::I64, 0) => TirExpr::Add {
                lhs: sub(rng, TirType::I64, scope),
                rhs: sub(rng, TirType::I64, scope),
                ty: TirType::I64,
            },
            (TirType::Bool, 0) => {
                let operand = if rng.below(2) == 0 {
                    TirType::I64
                } else {
                    TirType::Bool
                };
                TirExpr::Eq {
                    lhs: sub(rng, operand, scope),
                    rhs: sub(rng, operand, scope),
                }
            }
            (TirType::Bool, 1) => TirExpr::And {
                lhs: sub(rng, TirType::Bool, scope),
                rhs: sub(rng, TirType::Bool, scope),
            },
            (TirType::Bool, 2) => TirExpr::Not {
                expr: sub(rng, TirType::Bool, scope),
            },
            (_, 3) => TirExpr::If {
                cond: sub(rng, TirType::Bool, scope),
                then_branch: sub(rng, want, scope),
                else_branch: sub(rng, want, scope),
                ty: want,
            },
            (_, 4) => {
                let ty = if rng.below(2) == 0 {
                    TirType::I64
                } else {
                    TirType::Bool
                };
                let name = ["x", "y", "a"][rng.below(3) as usize].to_owned();
                let init = sub(rng, ty, scope);
                scope.push((name.clone(), ty));
                let body = sub(rng, want, scope);
                scope.pop();
                TirExpr::Let {
                    name,
                    ty,
                    init,
                    body,
                }
            }
            _ => {
                let fits: Vec<_> = callees.iter().filter(|(_, _, r)| *r == want).collect();
                if fits.is_empty() {
                    return generate(rng, want, scope, callees, 0);
                }
                let (name, params, ret) = fits[rng.below(fits.len() as u64) as usize];
                let args = params
                    .iter()
                    .map(|p| generate(rng, *p, scope, callees, fuel - 1))
                    .collect();
                TirExpr::Call {
                    callee: name.clone(),
                    args,
                    ty: *ret,
                }
            }
        }
    }

    #[test]
    fn random_well_typed_programs_verify_and_round_trip_through_text() {
        let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
        for _ in 0..2_000 {
            let mut funcs: Vec<TirFunction> = Vec::new();
            let mut sigs: Vec<(String, Vec<TirType>, TirType)> = Vec::new();
            for i in 0..3 {
                let params: Vec<TirParam> = (0..rng.below(3))
                    .map(|p| TirParam {
                        name: format!("p{p}"),
                        ty: if rng.below(2) == 0 {
                            TirType::I64
                        } else {
                            TirType::Bool
                        },
                    })
                    .collect();
                let ret = if rng.below(2) == 0 {
                    TirType::I64
                } else {
                    TirType::Bool
                };
                let mut scope: Vec<_> = params.iter().map(|p| (p.name.clone(), p.ty)).collect();
                // only earlier functions are callable: no recursion needed here
                let body = generate(&mut rng, ret, &mut scope, &sigs, 5);
                sigs.push((format!("f{i}"), params.iter().map(|p| p.ty).collect(), ret));
                funcs.push(TirFunction {
                    name: format!("f{i}"),
                    params,
                    ret,
                    body,
                });
            }
            let module = TirModule { funcs };
            assert_eq!(verify_module(&module), vec![], "{}", module.to_text());
            let text = module.to_text();
            let reparsed = TirModule::parse(&text).expect("printed text parses");
            assert_eq!(reparsed, module, "{text}");
            assert_eq!(reparsed.to_text(), text);
        }
    }
}
