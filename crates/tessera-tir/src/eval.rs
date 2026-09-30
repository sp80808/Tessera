//! Reference evaluator: the executable meaning of TIR.
//!
//! TIR is the contract MIR lowers from (TIR-5), so "what you read is what
//! runs" needs a definition of what a TIR program *does* that does not go
//! through MIR. This module is that definition, kept deliberately plain; the
//! MIR interpreter (`tessera_mir::interp`) must agree with it on every
//! verified module, which is what the differential tests check.
//!
//! # Semantics
//!
//! - Values are `i64` (two's-complement, 64-bit) and `bool`.
//! - Evaluation is strict and left to right: operands and call arguments are
//!   evaluated left to right, before the operation or call.
//! - `and` short-circuits: the right operand is evaluated only when the left
//!   one is `true` (this matches the MIR lowering's `and-short-circuit`).
//! - `if` evaluates its condition, then exactly one branch.
//! - `let` evaluates its initializer, binds it for the extent of its body, and
//!   the innermost binding of a name wins.
//! - `call` evaluates the arguments, then the callee's body in a fresh
//!   environment holding only the callee's parameters (no closures).
//! - `add` overflow is **not decided** at the language level (open question
//!   O1). The evaluator takes the behavior as an explicit [`Overflow`]
//!   parameter, with no default, exactly like the MIR lowering's `LowerOptions`.
//!
//! # Totality
//!
//! Any input yields a value or a [`Halt`]; nothing panics. The machine keeps
//! its own work stack, so neither expression depth nor call depth consumes
//! host stack. Unverified input that breaks a verifier rule halts with
//! [`Halt::Malformed`]; resource limits halt with [`Halt::OutOfFuel`] or
//! [`Halt::CallDepthExceeded`], which are *not* part of a program's meaning.

use std::fmt;

use crate::{TirExpr, TirFunction, TirModule, TirType};

/// A runtime value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl Value {
    #[must_use]
    pub const fn ty(self) -> TirType {
        match self {
            Self::Int(_) => TirType::I64,
            Self::Bool(_) => TirType::Bool,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
        }
    }
}

/// What `add` does when the mathematical sum does not fit in `i64`.
///
/// Stands for the undecided language question O1; every run states the
/// behavior it assumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overflow {
    /// The result wraps around (two's complement).
    Wrapping,
    /// Evaluation halts with [`Trap::IntegerOverflow`].
    Trapping,
}

/// Resource bounds. They only decide *whether* a run finishes; a run that
/// finishes has the same result under any larger limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limits {
    /// Maximum number of evaluation steps.
    pub fuel: u64,
    /// Maximum number of active calls, counting the entry function as 1.
    pub max_call_depth: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 10_000_000,
            max_call_depth: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EvalOptions {
    pub overflow: Overflow,
    pub limits: Limits,
}

/// A trap is part of a program's meaning: every correct executor of the same
/// program with the same arguments traps the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trap {
    /// A trapping `add` overflowed `i64`.
    IntegerOverflow,
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntegerOverflow => f.write_str("integer overflow"),
        }
    }
}

/// Why a run ended without a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Halt {
    /// The program trapped.
    Trap(Trap),
    /// The step budget ([`Limits::fuel`]) ran out.
    OutOfFuel,
    /// A call would exceed [`Limits::max_call_depth`].
    CallDepthExceeded,
    /// The module or the entry call breaks a precondition: an unverified
    /// module, an unknown entry function, or wrong entry arguments. Never
    /// happens for a verified module called with arguments of the right types.
    Malformed(String),
}

impl fmt::Display for Halt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trap(trap) => write!(f, "trap: {trap}"),
            Self::OutOfFuel => f.write_str("out of fuel (step limit reached)"),
            Self::CallDepthExceeded => f.write_str("call depth limit exceeded"),
            Self::Malformed(why) => write!(f, "malformed input: {why}"),
        }
    }
}

impl std::error::Error for Halt {}

/// Check `args` against `params` (shared by entry calls and nested calls).
fn check_args(func: &TirFunction, args: &[Value]) -> Result<(), Halt> {
    if args.len() != func.params.len() {
        return Err(Halt::Malformed(format!(
            "`{}` takes {} arguments, got {}",
            func.name,
            func.params.len(),
            args.len()
        )));
    }
    for (param, arg) in func.params.iter().zip(args) {
        if param.ty != arg.ty() {
            return Err(Halt::Malformed(format!(
                "`{}` parameter `{}` is {}, got {}",
                func.name,
                param.name,
                param.ty,
                arg.ty()
            )));
        }
    }
    Ok(())
}

/// Evaluate `func(args)` in `module`.
pub fn eval(
    module: &TirModule,
    func: &str,
    args: &[Value],
    opts: &EvalOptions,
) -> Result<Value, Halt> {
    let entry = module
        .function(func)
        .ok_or_else(|| Halt::Malformed(format!("no function `{func}`")))?;
    check_args(entry, args)?;
    let mut machine = Machine {
        module,
        overflow: opts.overflow,
        max_call_depth: opts.limits.max_call_depth,
        fuel: opts.limits.fuel,
        work: Vec::new(),
        vals: Vec::new(),
        env: Vec::new(),
        frames: Vec::new(),
    };
    machine.enter(entry, args.to_vec())?;
    machine.run()?;
    match machine.vals.as_slice() {
        [value] => Ok(*value),
        other => Err(Halt::Malformed(format!(
            "internal: {} values left after evaluation",
            other.len()
        ))),
    }
}

/// One pending step. Children are scheduled so that the leftmost one runs
/// first (it is pushed last).
enum Work<'a> {
    Eval(&'a TirExpr),
    /// Operands are on the value stack.
    Add,
    Eq,
    Not,
    /// The left operand is on the value stack; evaluate this if it is `true`.
    AndRhs(&'a TirExpr),
    /// The condition is on the value stack; the taken branch must produce
    /// the `if`'s type.
    Branch(&'a TirExpr, &'a TirExpr, TirType),
    /// The value on top of the stack must have this type (only unverified
    /// input can fail this).
    Expect(TirType, &'static str),
    /// The initializer is on the value stack.
    Bind(&'a str, TirType),
    Unbind,
    /// The arguments are on the value stack.
    Call(&'a TirFunction),
    /// Leave the innermost call; its result is on the value stack.
    Return(&'a TirFunction),
}

struct Machine<'a> {
    module: &'a TirModule,
    overflow: Overflow,
    max_call_depth: u32,
    fuel: u64,
    work: Vec<Work<'a>>,
    vals: Vec<Value>,
    /// Bindings of all active calls; each call sees only `env[base..]`.
    env: Vec<(&'a str, Value)>,
    /// `env` base of each active call, innermost last.
    frames: Vec<usize>,
}

fn malformed<T>(why: impl Into<String>) -> Result<T, Halt> {
    Err(Halt::Malformed(why.into()))
}

impl<'a> Machine<'a> {
    fn pop(&mut self) -> Result<Value, Halt> {
        self.vals
            .pop()
            .map_or_else(|| malformed("internal: value stack underflow"), Ok)
    }

    fn pop_int(&mut self, what: &str) -> Result<i64, Halt> {
        match self.pop()? {
            Value::Int(v) => Ok(v),
            Value::Bool(_) => malformed(format!("{what}: expected i64, found bool")),
        }
    }

    fn pop_bool(&mut self, what: &str) -> Result<bool, Halt> {
        match self.pop()? {
            Value::Bool(v) => Ok(v),
            Value::Int(_) => malformed(format!("{what}: expected bool, found i64")),
        }
    }

    /// Start a call: bind the parameters in a fresh environment and schedule
    /// the body.
    fn enter(&mut self, func: &'a TirFunction, args: Vec<Value>) -> Result<(), Halt> {
        if self.frames.len() >= self.max_call_depth as usize {
            return Err(Halt::CallDepthExceeded);
        }
        check_args(func, &args)?;
        self.frames.push(self.env.len());
        for (param, arg) in func.params.iter().zip(args) {
            self.env.push((param.name.as_str(), arg));
        }
        self.work.push(Work::Return(func));
        self.work.push(Work::Eval(&func.body));
        Ok(())
    }

    fn lookup(&self, name: &str, ty: TirType) -> Result<Value, Halt> {
        let base = self.frames.last().copied().unwrap_or(0);
        let found = self
            .env
            .get(base..)
            .and_then(|scope| scope.iter().rev().find(|(n, _)| *n == name));
        match found {
            Some((_, value)) if value.ty() == ty => Ok(*value),
            Some((_, value)) => malformed(format!(
                "variable `{name}` holds {} but is annotated {ty}",
                value.ty()
            )),
            None => malformed(format!("unbound variable `{name}`")),
        }
    }

    fn run(&mut self) -> Result<(), Halt> {
        while let Some(work) = self.work.pop() {
            if self.fuel == 0 {
                return Err(Halt::OutOfFuel);
            }
            self.fuel -= 1;
            self.step(work)?;
        }
        Ok(())
    }

    fn step(&mut self, work: Work<'a>) -> Result<(), Halt> {
        match work {
            Work::Eval(expr) => self.schedule(expr)?,
            Work::Add => {
                let rhs = self.pop_int("right operand of `add`")?;
                let lhs = self.pop_int("left operand of `add`")?;
                let sum = match self.overflow {
                    Overflow::Wrapping => lhs.wrapping_add(rhs),
                    Overflow::Trapping => lhs
                        .checked_add(rhs)
                        .ok_or(Halt::Trap(Trap::IntegerOverflow))?,
                };
                self.vals.push(Value::Int(sum));
            }
            Work::Eq => {
                let rhs = self.pop()?;
                let lhs = self.pop()?;
                if lhs.ty() != rhs.ty() {
                    return malformed(format!("`eq` compares {} with {}", lhs.ty(), rhs.ty()));
                }
                self.vals.push(Value::Bool(lhs == rhs));
            }
            Work::Not => {
                let v = self.pop_bool("operand of `not`")?;
                self.vals.push(Value::Bool(!v));
            }
            Work::AndRhs(rhs) => {
                if self.pop_bool("left operand of `and`")? {
                    self.work
                        .push(Work::Expect(TirType::Bool, "right operand of `and`"));
                    self.work.push(Work::Eval(rhs));
                } else {
                    self.vals.push(Value::Bool(false));
                }
            }
            Work::Branch(then_branch, else_branch, ty) => {
                let taken = if self.pop_bool("condition of `if`")? {
                    then_branch
                } else {
                    else_branch
                };
                self.work.push(Work::Expect(ty, "branch of `if`"));
                self.work.push(Work::Eval(taken));
            }
            Work::Expect(ty, what) => match self.vals.last() {
                Some(v) if v.ty() == ty => {}
                Some(v) => return malformed(format!("{what}: expected {ty}, found {}", v.ty())),
                None => return malformed("internal: value stack underflow"),
            },
            Work::Bind(name, ty) => {
                let value = self.pop()?;
                if value.ty() != ty {
                    return malformed(format!(
                        "`let {name}` is declared {ty} but initialized with {}",
                        value.ty()
                    ));
                }
                self.env.push((name, value));
            }
            Work::Unbind => {
                if self.env.pop().is_none() {
                    return malformed("internal: scope underflow");
                }
            }
            Work::Call(func) => {
                let split = self.vals.len().saturating_sub(func.params.len());
                let args = self.vals.split_off(split);
                self.enter(func, args)?;
            }
            Work::Return(func) => {
                let base = self
                    .frames
                    .pop()
                    .map_or_else(|| malformed("internal: frame underflow"), Ok)?;
                self.env.truncate(base);
                match self.vals.last() {
                    Some(v) if v.ty() == func.ret => {}
                    Some(v) => {
                        return malformed(format!(
                            "`{}` returns {} but its body produced {}",
                            func.name,
                            func.ret,
                            v.ty()
                        ));
                    }
                    None => return malformed("internal: no return value"),
                }
            }
        }
        Ok(())
    }

    /// Schedule the evaluation of `expr`: its children left to right, then
    /// the operation itself.
    fn schedule(&mut self, expr: &'a TirExpr) -> Result<(), Halt> {
        match expr {
            TirExpr::Int { value, ty } => {
                if *ty != TirType::I64 {
                    return malformed(format!("integer literal annotated {ty}"));
                }
                self.vals.push(Value::Int(*value));
            }
            TirExpr::Bool { value } => self.vals.push(Value::Bool(*value)),
            TirExpr::Var { name, ty } => {
                let value = self.lookup(name, *ty)?;
                self.vals.push(value);
            }
            TirExpr::Add { lhs, rhs, ty } => {
                if *ty != TirType::I64 {
                    return malformed(format!("`add` annotated {ty}"));
                }
                self.work.push(Work::Add);
                self.work.push(Work::Eval(rhs));
                self.work.push(Work::Eval(lhs));
            }
            TirExpr::Eq { lhs, rhs } => {
                self.work.push(Work::Eq);
                self.work.push(Work::Eval(rhs));
                self.work.push(Work::Eval(lhs));
            }
            TirExpr::And { lhs, rhs } => {
                self.work.push(Work::AndRhs(rhs));
                self.work.push(Work::Eval(lhs));
            }
            TirExpr::Not { expr } => {
                self.work.push(Work::Not);
                self.work.push(Work::Eval(expr));
            }
            TirExpr::Let {
                name,
                ty,
                init,
                body,
            } => {
                self.work.push(Work::Unbind);
                self.work.push(Work::Eval(body));
                self.work.push(Work::Bind(name, *ty));
                self.work.push(Work::Eval(init));
            }
            TirExpr::If {
                cond,
                then_branch,
                else_branch,
                ty,
            } => {
                self.work.push(Work::Branch(then_branch, else_branch, *ty));
                self.work.push(Work::Eval(cond));
            }
            TirExpr::Call { callee, args, ty } => {
                let Some(func) = self.module.function(callee) else {
                    return malformed(format!("call of unknown function `{callee}`"));
                };
                if func.ret != *ty {
                    return malformed(format!(
                        "call of `{callee}` annotated {ty}, but it returns {}",
                        func.ret
                    ));
                }
                if func.params.len() != args.len() {
                    return malformed(format!(
                        "call of `{callee}` with {} arguments, expected {}",
                        args.len(),
                        func.params.len()
                    ));
                }
                self.work.push(Work::Call(func));
                for arg in args.iter().rev() {
                    self.work.push(Work::Eval(arg));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: i64 = i64::MAX;

    fn module(text: &str) -> TirModule {
        let m = TirModule::parse(text).expect("parses");
        assert_eq!(crate::verify_module(&m), Vec::new(), "fixture must verify");
        m
    }

    fn opts(overflow: Overflow) -> EvalOptions {
        EvalOptions {
            overflow,
            limits: Limits::default(),
        }
    }

    fn run(m: &TirModule, f: &str, args: &[Value], overflow: Overflow) -> Result<Value, Halt> {
        eval(m, f, args, &opts(overflow))
    }

    const ADD: &str = "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))";

    #[test]
    fn bootstrap_adds_and_overflow_follows_the_stated_mode() {
        let m = module(ADD);
        let args = [Value::Int(2), Value::Int(3)];
        for mode in [Overflow::Wrapping, Overflow::Trapping] {
            assert_eq!(run(&m, "add", &args, mode), Ok(Value::Int(5)));
        }
        let edge = [Value::Int(MAX), Value::Int(1)];
        assert_eq!(
            run(&m, "add", &edge, Overflow::Wrapping),
            Ok(Value::Int(i64::MIN))
        );
        assert_eq!(
            run(&m, "add", &edge, Overflow::Trapping),
            Err(Halt::Trap(Trap::IntegerOverflow))
        );
    }

    /// `and` evaluates its right operand only when the left is `true`, and
    /// `if` evaluates only the taken branch: an overflow elsewhere is inert.
    #[test]
    fn and_short_circuits_and_if_evaluates_one_branch() {
        let boom = format!("(eq (add i64 (int {MAX} i64) (int 1 i64)) (int 0 i64))");
        let m = module(&format!(
            "(func f (param c bool) (return bool) (body (and (var c bool) {boom})))\n\
             (func g (param c bool) (return i64) (body \
               (if i64 (var c bool) (int 7 i64) (add i64 (int {MAX} i64) (int 1 i64)))))"
        ));
        let t = Overflow::Trapping;
        assert_eq!(
            run(&m, "f", &[Value::Bool(false)], t),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            run(&m, "f", &[Value::Bool(true)], t),
            Err(Halt::Trap(Trap::IntegerOverflow))
        );
        assert_eq!(run(&m, "g", &[Value::Bool(true)], t), Ok(Value::Int(7)));
        assert_eq!(
            run(&m, "g", &[Value::Bool(false)], t),
            Err(Halt::Trap(Trap::IntegerOverflow))
        );
    }

    #[test]
    fn innermost_binding_wins_and_scopes_end() {
        // (let x 1 in (let x 10 in x) + x) = 11
        let m = module(
            "(func f (return i64) (body (let x i64 (int 1 i64) \
               (add i64 (let x i64 (int 10 i64) (var x i64)) (var x i64)))))",
        );
        assert_eq!(run(&m, "f", &[], Overflow::Trapping), Ok(Value::Int(11)));
        // A `let` may shadow a parameter of a different type.
        let m = module(
            "(func f (param x i64) (return bool) (body (let x bool (bool true) (var x bool))))",
        );
        assert_eq!(
            run(&m, "f", &[Value::Int(3)], Overflow::Trapping),
            Ok(Value::Bool(true))
        );
    }

    const SUM: &str = "(func sum (param n i64) (return i64) (body \
        (if i64 (eq (var n i64) (int 0 i64)) (int 0 i64) \
          (add i64 (var n i64) (call sum i64 (add i64 (var n i64) (int -1 i64)))))))";

    /// Calls run on the machine's own stack: recursion 9 000 deep needs no
    /// host stack, and the depth limit is exact (entry counts as 1).
    #[test]
    fn recursion_is_iterative_and_the_depth_limit_is_exact() {
        let m = module(SUM);
        let t = Overflow::Trapping;
        assert_eq!(run(&m, "sum", &[Value::Int(100)], t), Ok(Value::Int(5050)));
        assert_eq!(
            run(&m, "sum", &[Value::Int(9_000)], t),
            Ok(Value::Int(40_504_500))
        );
        // sum(n) makes n + 1 active calls.
        let limits = |max_call_depth| EvalOptions {
            overflow: t,
            limits: Limits {
                fuel: u64::MAX,
                max_call_depth,
            },
        };
        assert_eq!(
            eval(&m, "sum", &[Value::Int(9)], &limits(10)),
            Ok(Value::Int(45))
        );
        assert_eq!(
            eval(&m, "sum", &[Value::Int(10)], &limits(10)),
            Err(Halt::CallDepthExceeded)
        );
        assert_eq!(
            eval(&m, "sum", &[Value::Int(0)], &limits(0)),
            Err(Halt::CallDepthExceeded)
        );
    }

    #[test]
    fn fuel_bounds_non_terminating_programs() {
        let m = module("(func f (param x i64) (return i64) (body (call f i64 (var x i64))))");
        let o = EvalOptions {
            overflow: Overflow::Wrapping,
            limits: Limits {
                fuel: 5_000,
                max_call_depth: u32::MAX,
            },
        };
        assert_eq!(eval(&m, "f", &[Value::Int(1)], &o), Err(Halt::OutOfFuel));
    }

    /// A callee sees only its own parameters, never the caller's bindings.
    #[test]
    fn calls_do_not_capture_the_callers_environment() {
        let m = TirModule::parse(
            "(func f (param x i64) (return i64) (body (call g i64)))\n\
             (func g (return i64) (body (var x i64)))",
        )
        .expect("parses");
        assert!(
            !crate::verify_module(&m).is_empty(),
            "the verifier rejects it"
        );
        assert!(matches!(
            run(&m, "f", &[Value::Int(1)], Overflow::Trapping),
            Err(Halt::Malformed(why)) if why.contains("unbound variable `x`")
        ));
    }

    #[test]
    fn bad_entry_calls_and_unverified_input_are_malformed_not_panics() {
        let m = module(ADD);
        for (f, args) in [
            ("nope", vec![]),
            ("add", vec![Value::Int(1)]),
            ("add", vec![Value::Int(1), Value::Bool(true)]),
        ] {
            assert!(
                matches!(
                    run(&m, f, &args, Overflow::Trapping),
                    Err(Halt::Malformed(_))
                ),
                "{f} {args:?}"
            );
        }
        for bad in [
            "(func f (return i64) (body (add i64 (int 1 i64) (bool true))))",
            "(func f (return i64) (body (and (bool true) (int 1 i64))))",
            "(func f (return bool) (body (int 1 i64)))",
            "(func f (return i64) (body (if i64 (bool true) (bool false) (int 1 i64))))",
            "(func f (return i64) (body (let x i64 (bool true) (int 1 i64))))",
            "(func f (return i64) (body (var x bool)))",
            "(func f (return i64) (body (call f bool)))",
            "(func f (return i64) (body (call f i64 (int 1 i64))))",
            "(func f (return i64) (body (call g i64)))",
            "(func f (return i64) (body (int 1 bool)))",
        ] {
            let m = TirModule::parse(bad).expect("parses");
            assert!(!crate::verify_module(&m).is_empty(), "{bad}");
            assert!(
                matches!(
                    run(&m, "f", &[], Overflow::Trapping),
                    Err(Halt::Malformed(_))
                ),
                "{bad}"
            );
        }
    }

    /// The deepest tree the TIR reader accepts evaluates on a default 2 MiB
    /// test-thread stack (the machine is iterative).
    #[test]
    fn deepest_accepted_tree_evaluates() {
        let depth = crate::MAX_TIR_DEPTH;
        let mut text = "(int 1 i64)".to_owned();
        for _ in 1..depth {
            text = format!("(add i64 {text} (int 1 i64))");
        }
        let m = module(&format!("(func f (return i64) (body {text}))"));
        let want = i64::try_from(depth).expect("fits");
        assert_eq!(run(&m, "f", &[], Overflow::Trapping), Ok(Value::Int(want)));
    }
}
