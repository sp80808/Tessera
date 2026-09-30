//! MIR verifier (contract B6, MIR-1..3).
//!
//! Checks a [`MirModule`] from the module alone: no TIR, no parser, no target.
//! It must never panic, whatever the input: every index is bounds-checked and
//! every finding is a value.
//!
//! Rules, per function:
//!
//! * **Shape.** An entry block exists; every block target is in range; every
//!   block is reachable from `bb0`. (A block without a terminator cannot be
//!   built: [`BasicBlock::terminator`] is not optional.)
//! * **Locals.** `_0` exists, has kind `Return` and the function's return type;
//!   the parameter list is exactly `_1..=_n` with kind `Param`; all other locals
//!   are `Var` or `Temp`; every referenced local exists.
//! * **Types.** Each assignment's destination type equals its rvalue's type;
//!   `Not` takes `bool`; `Add` takes two `i64` and states an overflow mode, `Eq`
//!   takes two operands of one type and states none; a `Branch` condition is
//!   `bool`; a call names an existing function, with matching arity, argument
//!   types and (through the destination) result type.
//! * **Initialization** (MIR-1, "assigned before use on all paths"): a forward
//!   must-analysis over the CFG run to a fixed point ([`analyze_init`]). Params
//!   are initialized at entry; an assignment initializes its destination; `Move`
//!   and `Drop` deinitialize; every read (`Copy`/`Move` operand, `Branch`
//!   condition, `Drop`) needs the local definitely initialized on all paths, and
//!   `_0` must be at every `Return`.
//! * **Provenance** (MIR-2): a `Synthesized` provenance has a non-empty reason.
//!   Totality is structural: every statement, terminator and function has one.
//!
//! Findings come out in a deterministic order, per function in module order:
//! signature and locals, then each block in id order (its statements, then its
//! terminator), then unreachable blocks, then initialization findings in block
//! and statement order. Each initialization finding is reported at the first
//! offending statement of a block: after a bad read the local counts as
//! initialized for the rest of that block, so one bad statement yields one
//! finding per local rather than a cascade.

use std::collections::BTreeSet;
use std::fmt;

use tessera_phases::Provenance;

use crate::ir::{
    BinOp, BlockId, FuncId, LocalId, LocalKind, MirFunction, MirModule, Operand, OverflowMode,
    Rvalue, Stmt, StmtKind, TermKind, Terminator, TirType, UnOp,
};

/// Where in a function a finding sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirLoc {
    Function,
    Local(LocalId),
    Block(BlockId),
    Stmt { block: BlockId, index: usize },
    Terminator { block: BlockId },
}

/// What the initialization analysis knows about one local at one point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalState {
    /// Definitely initialized on every path here.
    Init,
    /// Never assigned on any path here.
    Uninit,
    /// Moved out on every path here.
    Moved,
    /// Dropped on every path here.
    Dropped,
    /// Paths disagree: not definitely initialized.
    Maybe,
}

impl LocalState {
    /// Least upper bound in the flat lattice `{Init, Uninit, Moved, Dropped} < Maybe`.
    #[must_use]
    pub fn join(self, other: LocalState) -> LocalState {
        if self == other { self } else { Self::Maybe }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirErrorKind {
    /// The function has no blocks, so no entry block.
    NoBlocks,
    /// The function has no locals, so no return place `_0`.
    MissingReturnPlace,
    /// `_0` does not have the function's declared return type.
    ReturnTypeMismatch { ret: TirType, place: TirType },
    /// A local's kind does not fit its position.
    LocalKindMismatch {
        local: LocalId,
        expected: &'static str,
        got: LocalKind,
    },
    /// The `index`-th parameter is not `_{index + 1}`.
    ParamNotSequential { index: usize, got: LocalId },
    /// A local id past the end of the function's locals.
    LocalOutOfRange { local: LocalId },
    /// A jump target past the end of the function's blocks.
    BlockOutOfRange { target: BlockId },
    UnreachableBlock,
    AssignTypeMismatch {
        dest: LocalId,
        dest_ty: TirType,
        value_ty: TirType,
    },
    OperandTypeMismatch {
        what: &'static str,
        want: TirType,
        got: TirType,
    },
    BranchCondNotBool { got: TirType },
    /// `Add` without an overflow mode.
    MissingOverflowMode,
    /// A non-`Add` operation that states an overflow mode.
    UnexpectedOverflowMode { mode: OverflowMode },
    UnknownCallee { callee: FuncId },
    ArityMismatch {
        callee: FuncId,
        want: usize,
        got: usize,
    },
    ArgTypeMismatch {
        index: usize,
        want: TirType,
        got: TirType,
    },
    /// `Synthesized` provenance with an empty reason.
    EmptySynthReason,
    /// A read of a local that is not definitely initialized.
    UseNotInitialized { local: LocalId, state: LocalState },
    /// A `Drop` of a local that is not definitely initialized
    /// (`state == Dropped` is a double drop).
    DropNotInitialized { local: LocalId, state: LocalState },
    /// `return` while `_0` is not definitely initialized.
    ReturnPlaceNotInitialized { state: LocalState },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirError {
    pub func: FuncId,
    pub func_name: String,
    pub at: MirLoc,
    pub kind: MirErrorKind,
}

impl fmt::Display for LocalState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Init => "initialized",
            Self::Uninit => "never initialized",
            Self::Moved => "moved out",
            Self::Dropped => "dropped",
            Self::Maybe => "not initialized on every path",
        })
    }
}

impl fmt::Display for MirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn `{}`", self.func_name)?;
        match self.at {
            MirLoc::Function => {}
            MirLoc::Local(l) => write!(f, " local {l}")?,
            MirLoc::Block(b) => write!(f, " {b}")?,
            MirLoc::Stmt { block, index } => write!(f, " {block} stmt {index}")?,
            MirLoc::Terminator { block } => write!(f, " {block} terminator")?,
        }
        write!(f, ": ")?;
        match &self.kind {
            MirErrorKind::NoBlocks => write!(f, "function has no entry block"),
            MirErrorKind::MissingReturnPlace => write!(f, "function has no return place `_0`"),
            MirErrorKind::ReturnTypeMismatch { ret, place } => write!(
                f,
                "return place `_0` has type {place} but the function returns {ret}"
            ),
            MirErrorKind::LocalKindMismatch {
                local,
                expected,
                got,
            } => write!(
                f,
                "local {local} has kind `{}` but its position requires {expected}",
                got.as_str()
            ),
            MirErrorKind::ParamNotSequential { index, got } => write!(
                f,
                "parameter {index} is {got}, expected _{}",
                index.saturating_add(1)
            ),
            MirErrorKind::LocalOutOfRange { local } => {
                write!(f, "local {local} does not exist")
            }
            MirErrorKind::BlockOutOfRange { target } => {
                write!(f, "jump to {target}, which does not exist")
            }
            MirErrorKind::UnreachableBlock => write!(f, "block is unreachable from the entry"),
            MirErrorKind::AssignTypeMismatch {
                dest,
                dest_ty,
                value_ty,
            } => write!(
                f,
                "assignment to {dest}: destination has type {dest_ty}, value has type {value_ty}"
            ),
            MirErrorKind::OperandTypeMismatch { what, want, got } => {
                write!(f, "{what}: expected {want}, found {got}")
            }
            MirErrorKind::BranchCondNotBool { got } => {
                write!(f, "branch condition: expected bool, found {got}")
            }
            MirErrorKind::MissingOverflowMode => {
                write!(f, "`Add` must state an overflow mode")
            }
            MirErrorKind::UnexpectedOverflowMode { mode } => write!(
                f,
                "only `Add` carries an overflow mode, found `{}`",
                mode.as_str()
            ),
            MirErrorKind::UnknownCallee { callee } => {
                write!(f, "call of function {callee}, which does not exist")
            }
            MirErrorKind::ArityMismatch { callee, want, got } => {
                write!(f, "call of {callee} with {got} arguments, expected {want}")
            }
            MirErrorKind::ArgTypeMismatch { index, want, got } => {
                write!(f, "call argument {index}: expected {want}, found {got}")
            }
            MirErrorKind::EmptySynthReason => {
                write!(f, "synthesized provenance has an empty reason")
            }
            MirErrorKind::UseNotInitialized { local, state } => match state {
                LocalState::Uninit => write!(f, "use of {local} before it is initialized"),
                LocalState::Moved => write!(f, "use of {local} after it was moved"),
                LocalState::Dropped => write!(f, "use of {local} after it was dropped"),
                _ => write!(f, "use of {local}, which is {state}"),
            },
            MirErrorKind::DropNotInitialized { local, state } => match state {
                LocalState::Uninit => write!(f, "drop of {local} before it is initialized"),
                LocalState::Moved => write!(f, "drop of {local} after it was moved"),
                LocalState::Dropped => write!(f, "double drop of {local}"),
                _ => write!(f, "drop of {local}, which is {state}"),
            },
            MirErrorKind::ReturnPlaceNotInitialized { state } => {
                write!(f, "`return` while `_0` is {state}")
            }
        }
    }
}

impl std::error::Error for MirError {}

fn block_id(index: usize) -> BlockId {
    BlockId(u32::try_from(index).unwrap_or(u32::MAX))
}

fn local_id(index: usize) -> LocalId {
    LocalId(u32::try_from(index).unwrap_or(u32::MAX))
}

/// Verify every function of the module. Empty result means well-formed.
#[must_use]
pub fn verify_module(m: &MirModule) -> Vec<MirError> {
    let mut errors = Vec::new();
    for (i, func) in m.funcs.iter().enumerate() {
        let id = FuncId(u32::try_from(i).unwrap_or(u32::MAX));
        Checker {
            module: m,
            id,
            func,
            errors: &mut errors,
        }
        .run();
    }
    errors
}

struct Checker<'a> {
    module: &'a MirModule,
    id: FuncId,
    func: &'a MirFunction,
    errors: &'a mut Vec<MirError>,
}

impl Checker<'_> {
    fn report(&mut self, at: MirLoc, kind: MirErrorKind) {
        self.errors.push(MirError {
            func: self.id,
            func_name: self.func.name.clone(),
            at,
            kind,
        });
    }

    fn run(&mut self) {
        self.signature();
        let f = self.func;
        self.provenance(f.provenance, MirLoc::Function);
        for (i, block) in f.blocks.iter().enumerate() {
            let b = block_id(i);
            for (index, stmt) in block.stmts.iter().enumerate() {
                self.stmt(MirLoc::Stmt { block: b, index }, stmt);
            }
            self.terminator(MirLoc::Terminator { block: b }, &block.terminator);
        }
        self.reachability();
        self.initialization();
    }

    fn signature(&mut self) {
        let f = self.func;
        if f.blocks.is_empty() {
            self.report(MirLoc::Function, MirErrorKind::NoBlocks);
        }
        match f.locals.first() {
            None => self.report(MirLoc::Function, MirErrorKind::MissingReturnPlace),
            Some(place) if place.ty != f.ret => self.report(
                MirLoc::Function,
                MirErrorKind::ReturnTypeMismatch {
                    ret: f.ret,
                    place: place.ty,
                },
            ),
            Some(_) => {}
        }
        for (i, param) in f.params.iter().enumerate() {
            if *param != local_id(i + 1) {
                self.report(
                    MirLoc::Function,
                    MirErrorKind::ParamNotSequential {
                        index: i,
                        got: *param,
                    },
                );
            }
            if f.local(*param).is_none() {
                self.report(
                    MirLoc::Function,
                    MirErrorKind::LocalOutOfRange { local: *param },
                );
            }
        }
        let n = f.params.len();
        for (i, decl) in f.locals.iter().enumerate() {
            let (ok, expected) = if i == 0 {
                (decl.kind == LocalKind::Return, "`return`")
            } else if i <= n {
                (decl.kind == LocalKind::Param, "`param`")
            } else {
                (
                    matches!(decl.kind, LocalKind::Var | LocalKind::Temp),
                    "`var` or `temp`",
                )
            };
            if !ok {
                self.report(
                    MirLoc::Local(local_id(i)),
                    MirErrorKind::LocalKindMismatch {
                        local: local_id(i),
                        expected,
                        got: decl.kind,
                    },
                );
            }
        }
    }

    fn provenance(&mut self, provenance: Provenance, at: MirLoc) {
        if let Provenance::Synthesized { why, .. } = provenance {
            if why.is_empty() {
                self.report(at, MirErrorKind::EmptySynthReason);
            }
        }
    }

    fn local_ty(&mut self, at: MirLoc, local: LocalId) -> Option<TirType> {
        let ty = self.func.local(local).map(|d| d.ty);
        if ty.is_none() {
            self.report(at, MirErrorKind::LocalOutOfRange { local });
        }
        ty
    }

    fn operand(&mut self, at: MirLoc, op: &Operand) -> Option<TirType> {
        match op {
            Operand::Const(c) => Some(c.ty()),
            Operand::Copy(l) | Operand::Move(l) => self.local_ty(at, *l),
        }
    }

    fn expect(&mut self, at: MirLoc, what: &'static str, want: TirType, got: Option<TirType>) {
        if let Some(got) = got {
            if got != want {
                self.report(at, MirErrorKind::OperandTypeMismatch { what, want, got });
            }
        }
    }

    fn stmt(&mut self, at: MirLoc, stmt: &Stmt) {
        self.provenance(stmt.provenance, at);
        match &stmt.kind {
            StmtKind::Assign(dest, rvalue) => {
                let dest_ty = self.local_ty(at, *dest);
                let value_ty = self.rvalue(at, rvalue);
                if let (Some(dest_ty), Some(value_ty)) = (dest_ty, value_ty) {
                    if dest_ty != value_ty {
                        self.report(
                            at,
                            MirErrorKind::AssignTypeMismatch {
                                dest: *dest,
                                dest_ty,
                                value_ty,
                            },
                        );
                    }
                }
            }
            StmtKind::Drop(local) => {
                self.local_ty(at, *local);
            }
        }
    }

    /// Check an rvalue; return its type if it has one.
    fn rvalue(&mut self, at: MirLoc, rvalue: &Rvalue) -> Option<TirType> {
        match rvalue {
            Rvalue::Use(op) => self.operand(at, op),
            Rvalue::Unary {
                op: UnOp::Not,
                operand,
            } => {
                let got = self.operand(at, operand);
                self.expect(at, "operand of `Not`", TirType::Bool, got);
                Some(TirType::Bool)
            }
            Rvalue::Binary {
                op,
                overflow,
                lhs,
                rhs,
            } => {
                let l = self.operand(at, lhs);
                let r = self.operand(at, rhs);
                match op {
                    BinOp::Add => {
                        self.expect(at, "left operand of `Add`", TirType::I64, l);
                        self.expect(at, "right operand of `Add`", TirType::I64, r);
                        if overflow.is_none() {
                            self.report(at, MirErrorKind::MissingOverflowMode);
                        }
                        Some(TirType::I64)
                    }
                    BinOp::Eq => {
                        if let Some(l) = l {
                            self.expect(at, "operands of `Eq`", l, r);
                        }
                        if let Some(mode) = overflow {
                            self.report(at, MirErrorKind::UnexpectedOverflowMode { mode: *mode });
                        }
                        Some(TirType::Bool)
                    }
                }
            }
            Rvalue::Call { callee, args } => {
                let got: Vec<Option<TirType>> = args.iter().map(|a| self.operand(at, a)).collect();
                let Some(target) = self.module.func(*callee) else {
                    self.report(at, MirErrorKind::UnknownCallee { callee: *callee });
                    return None;
                };
                if target.params.len() != args.len() {
                    self.report(
                        at,
                        MirErrorKind::ArityMismatch {
                            callee: *callee,
                            want: target.params.len(),
                            got: args.len(),
                        },
                    );
                }
                for (index, param) in target.params.iter().enumerate() {
                    let want = target.local(*param).map(|d| d.ty);
                    if let (Some(want), Some(Some(got))) = (want, got.get(index)) {
                        if want != *got {
                            self.report(
                                at,
                                MirErrorKind::ArgTypeMismatch {
                                    index,
                                    want,
                                    got: *got,
                                },
                            );
                        }
                    }
                }
                Some(target.ret)
            }
        }
    }

    fn target(&mut self, at: MirLoc, target: BlockId) {
        if self.func.block(target).is_none() {
            self.report(at, MirErrorKind::BlockOutOfRange { target });
        }
    }

    fn terminator(&mut self, at: MirLoc, term: &Terminator) {
        self.provenance(term.provenance, at);
        match &term.kind {
            TermKind::Goto(target) => self.target(at, *target),
            TermKind::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                if let Some(got) = self.operand(at, cond) {
                    if got != TirType::Bool {
                        self.report(at, MirErrorKind::BranchCondNotBool { got });
                    }
                }
                self.target(at, *then_bb);
                self.target(at, *else_bb);
            }
            TermKind::Return => {}
        }
    }

    fn reachability(&mut self) {
        let f = self.func;
        let n = f.blocks.len();
        if n == 0 {
            return;
        }
        let mut seen = vec![false; n];
        seen[0] = true;
        let mut stack = vec![0_usize];
        while let Some(b) = stack.pop() {
            for succ in f.blocks[b].terminator.kind.successors() {
                let s = succ.0 as usize;
                if s < n && !seen[s] {
                    seen[s] = true;
                    stack.push(s);
                }
            }
        }
        for (i, reached) in seen.iter().enumerate() {
            if !reached {
                self.report(MirLoc::Block(block_id(i)), MirErrorKind::UnreachableBlock);
            }
        }
    }

    fn initialization(&mut self) {
        let f = self.func;
        let Some(analysis) = analyze_init(f) else {
            return;
        };
        for (i, block) in f.blocks.iter().enumerate() {
            let Some(mut state) = analysis.entry[i].clone() else {
                continue;
            };
            let b = block_id(i);
            let mut found: Vec<(MirLoc, MirErrorKind)> = Vec::new();
            for (index, stmt) in block.stmts.iter().enumerate() {
                let at = MirLoc::Stmt { block: b, index };
                transfer_stmt(&mut state, stmt, true, &mut |bad| found.push((at, bad.into())));
            }
            let at = MirLoc::Terminator { block: b };
            transfer_term(&mut state, &block.terminator, true, &mut |bad| {
                found.push((at, bad.into()));
            });
            for (at, kind) in found {
                self.report(at, kind);
            }
        }
    }
}

/// A violation seen by the initialization transfer functions.
enum Bad {
    Use(LocalId, LocalState),
    Drop(LocalId, LocalState),
    Return(LocalState),
}

impl From<Bad> for MirErrorKind {
    fn from(bad: Bad) -> Self {
        match bad {
            Bad::Use(local, state) => Self::UseNotInitialized { local, state },
            Bad::Drop(local, state) => Self::DropNotInitialized { local, state },
            Bad::Return(state) => Self::ReturnPlaceNotInitialized { state },
        }
    }
}

/// Read `op`. `recover` marks a bad-read local initialized afterwards (used
/// only when reporting, so one bad statement yields one finding per local).
fn read_operand(
    state: &mut [LocalState],
    op: &Operand,
    recover: bool,
    bad: &mut dyn FnMut(Bad),
) {
    let Some(local) = op.local() else { return };
    let Some(slot) = state.get_mut(local.0 as usize) else {
        return;
    };
    if *slot != LocalState::Init {
        bad(Bad::Use(local, *slot));
        if recover {
            *slot = LocalState::Init;
        }
    }
    if matches!(op, Operand::Move(_)) {
        *slot = LocalState::Moved;
    }
}

/// The single transfer function of the initialization analysis. Operands are
/// read left to right before the destination is written.
fn transfer_stmt(state: &mut [LocalState], stmt: &Stmt, recover: bool, bad: &mut dyn FnMut(Bad)) {
    match &stmt.kind {
        StmtKind::Assign(dest, rvalue) => {
            for op in rvalue.operands() {
                read_operand(state, op, recover, bad);
            }
            if let Some(slot) = state.get_mut(dest.0 as usize) {
                *slot = LocalState::Init;
            }
        }
        StmtKind::Drop(local) => {
            if let Some(slot) = state.get_mut(local.0 as usize) {
                if *slot != LocalState::Init {
                    bad(Bad::Drop(*local, *slot));
                }
                *slot = LocalState::Dropped;
            }
        }
    }
}

fn transfer_term(
    state: &mut [LocalState],
    term: &Terminator,
    recover: bool,
    bad: &mut dyn FnMut(Bad),
) {
    match &term.kind {
        TermKind::Branch { cond, .. } => read_operand(state, cond, recover, bad),
        TermKind::Return => {
            if let Some(slot) = state.first() {
                if *slot != LocalState::Init {
                    bad(Bad::Return(*slot));
                }
            }
        }
        TermKind::Goto(_) => {}
    }
}

/// Result of the forward initialization analysis: the state of every local at
/// entry to and exit from every block reachable from the entry (`None` for
/// unreachable blocks), and how many times a block was (re)processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitAnalysis {
    pub entry: Vec<Option<Vec<LocalState>>>,
    pub exit: Vec<Option<Vec<LocalState>>>,
    /// Number of block visits until the fixed point. A CFG without back edges
    /// visited in reverse post-order needs exactly one visit per block; loops
    /// force more.
    pub block_visits: usize,
}

/// Can the dataflow index blocks and locals without bounds checks failing?
fn dataflow_sound(f: &MirFunction) -> bool {
    let n = f.blocks.len();
    let nl = f.locals.len();
    let local_ok = |l: LocalId| (l.0 as usize) < nl;
    let op_ok = |op: &Operand| op.local().is_none_or(local_ok);
    if n == 0 || nl == 0 || !f.params.iter().all(|p| local_ok(*p)) {
        return false;
    }
    f.blocks.iter().all(|block| {
        let stmts_ok = block.stmts.iter().all(|s| match &s.kind {
            StmtKind::Assign(dest, rv) => local_ok(*dest) && rv.operands().into_iter().all(op_ok),
            StmtKind::Drop(l) => local_ok(*l),
        });
        let term_ok = match &block.terminator.kind {
            TermKind::Goto(t) => (t.0 as usize) < n,
            TermKind::Branch {
                cond,
                then_bb,
                else_bb,
            } => op_ok(cond) && (then_bb.0 as usize) < n && (else_bb.0 as usize) < n,
            TermKind::Return => true,
        };
        stmts_ok && term_ok
    })
}

/// Forward must-analysis of which locals are definitely initialized, run to a
/// fixed point (works with loops). Returns `None` if the function is too
/// malformed to analyze (no blocks, or a local/target out of range); the
/// verifier reports those problems separately.
#[must_use]
pub fn analyze_init(f: &MirFunction) -> Option<InitAnalysis> {
    if !dataflow_sound(f) {
        return None;
    }
    let n = f.blocks.len();
    let mut entry: Vec<Option<Vec<LocalState>>> = vec![None; n];
    let mut exit: Vec<Option<Vec<LocalState>>> = vec![None; n];
    let mut start = vec![LocalState::Uninit; f.locals.len()];
    for p in &f.params {
        start[p.0 as usize] = LocalState::Init;
    }
    entry[0] = Some(start);
    // Smallest block first: deterministic, and RPO-numbered CFGs converge in one sweep.
    let mut work: BTreeSet<usize> = BTreeSet::from([0]);
    let mut block_visits = 0;
    while let Some(b) = work.pop_first() {
        let Some(mut state) = entry[b].clone() else {
            continue;
        };
        block_visits += 1;
        let block = &f.blocks[b];
        for stmt in &block.stmts {
            transfer_stmt(&mut state, stmt, false, &mut |_| {});
        }
        transfer_term(&mut state, &block.terminator, false, &mut |_| {});
        for succ in block.terminator.kind.successors() {
            let s = succ.0 as usize;
            match &mut entry[s] {
                slot @ None => {
                    *slot = Some(state.clone());
                    work.insert(s);
                }
                Some(current) => {
                    let mut changed = false;
                    for (cur, new) in current.iter_mut().zip(&state) {
                        let joined = cur.join(*new);
                        if joined != *cur {
                            *cur = joined;
                            changed = true;
                        }
                    }
                    if changed {
                        work.insert(s);
                    }
                }
            }
        }
        exit[b] = Some(state);
    }
    Some(InitAnalysis {
        entry,
        exit,
        block_visits,
    })
}
