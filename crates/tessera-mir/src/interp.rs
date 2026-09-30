//! Reference MIR interpreter.
//!
//! Executes a [`MirModule`] statement by statement. It exists to check the
//! TIR -> MIR lowering against the TIR evaluator (`tessera_tir::eval`), which
//! defines what TIR means, and to run programs before a native backend exists
//! (`tsr run`). It shares that evaluator's [`Value`], [`Halt`], [`Trap`] and
//! [`Limits`], and its call-depth rule (the entry call counts as 1, a call is
//! refused *before* its frame would exceed the limit), so results of the two
//! compare directly.
//!
//! Integer overflow is not a parameter here: each MIR `Add` states its own
//! [`crate::OverflowMode`].
//!
//! The interpreter is also a dynamic verifier: a read of an uninitialized or
//! moved local, a type mismatch, a dangling id or an `Add` without an overflow
//! mode halts with [`Halt::Malformed`] instead of guessing. Call frames live on
//! a `Vec`, so call depth never consumes host stack.

pub use tessera_tir::eval::{Halt, Limits, Trap, Value};

use crate::ir::{
    BinOp, Const, FuncId, LocalId, MirFunction, MirModule, Operand, OverflowMode, Rvalue, StmtKind,
    TermKind, UnOp,
};

fn malformed<T>(why: impl Into<String>) -> Result<T, Halt> {
    Err(Halt::Malformed(why.into()))
}

struct Frame<'a> {
    func: &'a MirFunction,
    locals: Vec<Option<Value>>,
    block: usize,
    /// Index of the next statement of `block`; `stmts.len()` means the
    /// terminator is next.
    next: usize,
    /// The caller's local that receives this call's result (`None` for the
    /// entry call).
    ret_to: Option<LocalId>,
}

impl<'a> Frame<'a> {
    /// A frame for `func` with its parameters bound to `args`.
    fn new(func: &'a MirFunction, args: Vec<Value>, ret_to: Option<LocalId>) -> Result<Self, Halt> {
        if args.len() != func.params.len() {
            return malformed(format!(
                "`{}` takes {} arguments, got {}",
                func.name,
                func.params.len(),
                args.len()
            ));
        }
        let mut frame = Frame {
            func,
            locals: vec![None; func.locals.len()],
            block: 0,
            next: 0,
            ret_to,
        };
        for (param, arg) in func.params.iter().zip(args) {
            frame.write(*param, arg)?;
        }
        Ok(frame)
    }

    fn write(&mut self, local: LocalId, value: Value) -> Result<(), Halt> {
        let name = &self.func.name;
        let Some(decl) = self.func.local(local) else {
            return malformed(format!("`{name}`: write to missing local {local}"));
        };
        if decl.ty != value.ty() {
            return malformed(format!(
                "`{name}`: {local} has type {} but is assigned {}",
                decl.ty,
                value.ty()
            ));
        }
        match self.locals.get_mut(local.0 as usize) {
            Some(slot) => {
                *slot = Some(value);
                Ok(())
            }
            None => malformed(format!("`{name}`: write to missing local {local}")),
        }
    }

    fn read(&mut self, op: &Operand) -> Result<Value, Halt> {
        let (local, take) = match op {
            Operand::Const(Const::Int(v)) => return Ok(Value::Int(*v)),
            Operand::Const(Const::Bool(v)) => return Ok(Value::Bool(*v)),
            Operand::Copy(l) => (*l, false),
            Operand::Move(l) => (*l, true),
        };
        let name = &self.func.name;
        let Some(slot) = self.locals.get_mut(local.0 as usize) else {
            return malformed(format!("`{name}`: read of missing local {local}"));
        };
        let value = if take { slot.take() } else { *slot };
        value.map_or_else(
            || malformed(format!("`{name}`: read of uninitialized {local}")),
            Ok,
        )
    }

    fn read_int(&mut self, op: &Operand, what: &str) -> Result<i64, Halt> {
        match self.read(op)? {
            Value::Int(v) => Ok(v),
            Value::Bool(_) => malformed(format!("{what}: expected i64, found bool")),
        }
    }

    fn read_bool(&mut self, op: &Operand, what: &str) -> Result<bool, Halt> {
        match self.read(op)? {
            Value::Bool(v) => Ok(v),
            Value::Int(_) => malformed(format!("{what}: expected bool, found i64")),
        }
    }

    /// Evaluate a non-call rvalue.
    fn rvalue(&mut self, rvalue: &Rvalue) -> Result<Value, Halt> {
        match rvalue {
            Rvalue::Use(op) => self.read(op),
            Rvalue::Unary {
                op: UnOp::Not,
                operand,
            } => Ok(Value::Bool(!self.read_bool(operand, "operand of `Not`")?)),
            Rvalue::Binary {
                op: BinOp::Add,
                overflow,
                lhs,
                rhs,
            } => {
                let l = self.read_int(lhs, "left operand of `Add`")?;
                let r = self.read_int(rhs, "right operand of `Add`")?;
                match overflow {
                    Some(OverflowMode::Wrapping) => Ok(Value::Int(l.wrapping_add(r))),
                    Some(OverflowMode::Trapping) => l
                        .checked_add(r)
                        .map(Value::Int)
                        .ok_or(Halt::Trap(Trap::IntegerOverflow)),
                    None => malformed("`Add` without an overflow mode"),
                }
            }
            Rvalue::Binary {
                op: BinOp::Eq,
                overflow,
                lhs,
                rhs,
            } => {
                if overflow.is_some() {
                    return malformed("`Eq` with an overflow mode");
                }
                let l = self.read(lhs)?;
                let r = self.read(rhs)?;
                if l.ty() != r.ty() {
                    return malformed(format!("`Eq` compares {} with {}", l.ty(), r.ty()));
                }
                Ok(Value::Bool(l == r))
            }
            Rvalue::Call { .. } => malformed("internal: call evaluated as a plain rvalue"),
        }
    }
}

/// Run `func(args)` in `module`.
pub fn run(
    module: &MirModule,
    func: FuncId,
    args: &[Value],
    limits: &Limits,
) -> Result<Value, Halt> {
    let Some(entry) = module.func(func) else {
        return malformed(format!("no function {func}"));
    };
    // Same order as the TIR evaluator: bad entry arguments before the limit.
    let frame = Frame::new(entry, args.to_vec(), None)?;
    if limits.max_call_depth == 0 {
        return Err(Halt::CallDepthExceeded);
    }
    let mut stack = vec![frame];
    let mut fuel = limits.fuel;
    loop {
        if fuel == 0 {
            return Err(Halt::OutOfFuel);
        }
        fuel -= 1;
        let depth = stack.len();
        let Some(frame) = stack.last_mut() else {
            return malformed("internal: empty call stack");
        };
        let f = frame.func;
        let Some(block) = f.blocks.get(frame.block) else {
            return malformed(format!("`{}`: jump to missing bb{}", f.name, frame.block));
        };
        if let Some(stmt) = block.stmts.get(frame.next) {
            frame.next += 1;
            match &stmt.kind {
                StmtKind::Assign(dest, Rvalue::Call { callee, args }) => {
                    let args = args
                        .iter()
                        .map(|a| frame.read(a))
                        .collect::<Result<Vec<_>, _>>()?;
                    let Some(target) = module.func(*callee) else {
                        return malformed(format!("`{}`: call of missing {callee}", f.name));
                    };
                    if depth >= limits.max_call_depth as usize {
                        return Err(Halt::CallDepthExceeded);
                    }
                    let callee_frame = Frame::new(target, args, Some(*dest))?;
                    stack.push(callee_frame);
                }
                StmtKind::Assign(dest, rvalue) => {
                    let value = frame.rvalue(rvalue)?;
                    frame.write(*dest, value)?;
                }
                StmtKind::Drop(local) => {
                    frame.read(&Operand::Move(*local))?;
                }
            }
            continue;
        }
        match &block.terminator.kind {
            TermKind::Goto(target) => {
                frame.block = target.0 as usize;
                frame.next = 0;
            }
            TermKind::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                let taken = if frame.read_bool(cond, "branch condition")? {
                    then_bb
                } else {
                    else_bb
                };
                frame.block = taken.0 as usize;
                frame.next = 0;
            }
            TermKind::Return => {
                let value = frame.read(&Operand::Copy(LocalId::RETURN))?;
                if value.ty() != f.ret {
                    return malformed(format!(
                        "`{}` returns {} but `_0` holds {}",
                        f.name,
                        f.ret,
                        value.ty()
                    ));
                }
                let ret_to = frame.ret_to;
                stack.pop();
                match (stack.last_mut(), ret_to) {
                    (None, _) => return Ok(value),
                    (Some(caller), Some(dest)) => caller.write(dest, value)?,
                    (Some(_), None) => return malformed("internal: callee without a result slot"),
                }
            }
        }
    }
}
