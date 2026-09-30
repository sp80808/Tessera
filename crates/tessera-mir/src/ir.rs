//! The MIR data model (contract B6): functions as CFGs of basic blocks over
//! typed locals.
//!
//! Everything here is a plain value (`Clone + Eq + Hash`), numbered densely and
//! deterministically, and free of target detail. Types are TIR's scalar types
//! ([`TirType`]) re-exported unchanged: MIR adds no type system of its own yet.
//!
//! Provenance is a field on every statement, terminator and function (total by
//! construction: there is no `Option`), not a side table, because MIR ids are
//! not stable across edits anyway (contract §2.4).

use std::fmt;

use tessera_phases::Provenance;
pub use tessera_tir::TirType;

/// Index of a function in [`MirModule::funcs`]. Calls refer to this, never to
/// a name; [`MirFunction::name`] is a debug label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuncId(pub u32);

/// Index of a local in [`MirFunction::locals`]. `_0` is the return place,
/// `_1..=_n` are the parameters, the rest are binders and temporaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);

impl LocalId {
    /// The return place `_0`.
    pub const RETURN: LocalId = LocalId(0);
}

/// Index of a block in [`MirFunction::blocks`]; `bb0` is the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

impl BlockId {
    /// The entry block `bb0`.
    pub const ENTRY: BlockId = BlockId(0);
}

impl fmt::Display for FuncId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl fmt::Display for LocalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

/// What a local is for. Kinds are checked by the verifier against the
/// position of the local (`_0`, parameter slots, everything else).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalKind {
    /// The return place `_0`.
    Return,
    /// A parameter, initialized on entry.
    Param,
    /// A user-declared binder (`let`).
    Var,
    /// A compiler-introduced temporary.
    Temp,
}

impl LocalKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Return => "return",
            Self::Param => "param",
            Self::Var => "var",
            Self::Temp => "temp",
        }
    }
}

/// Declaration of one local. `name` is a debug label only: nothing in MIR
/// resolves through it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalDecl {
    pub ty: TirType,
    pub name: Option<String>,
    pub kind: LocalKind,
}

/// A literal value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Const {
    Int(i64),
    Bool(bool),
}

impl Const {
    #[must_use]
    pub const fn ty(self) -> TirType {
        match self {
            Self::Int(_) => TirType::I64,
            Self::Bool(_) => TirType::Bool,
        }
    }
}

/// A value read by a statement or terminator. `Move` consumes the local (it is
/// uninitialized afterwards); `Copy` leaves it initialized. Lowering from
/// today's TIR only produces `Copy` because every TIR type is `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operand {
    Copy(LocalId),
    Move(LocalId),
    Const(Const),
}

impl Operand {
    /// The local read by this operand, if it reads one.
    #[must_use]
    pub const fn local(&self) -> Option<LocalId> {
        match self {
            Self::Copy(l) | Self::Move(l) => Some(*l),
            Self::Const(_) => None,
        }
    }

    /// Type of the operand in `func`, or `None` if it names a missing local.
    #[must_use]
    pub fn ty(&self, func: &MirFunction) -> Option<TirType> {
        match self {
            Self::Copy(l) | Self::Move(l) => func.local(*l).map(|d| d.ty),
            Self::Const(c) => Some(c.ty()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    /// Boolean negation: `bool -> bool`.
    Not,
}

impl UnOp {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Not => "Not",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// `i64 + i64 -> i64`; carries an [`OverflowMode`].
    Add,
    /// Equality of two operands of the same type: `T, T -> bool`.
    Eq,
}

impl BinOp {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Eq => "Eq",
        }
    }
}

/// What integer overflow does, stated per operation (contract B6: "no
/// default"). The language-level choice (open question O1) is undecided;
/// `Trapping` means execution aborts on overflow, and the exact runtime
/// behavior is a backend-contract matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverflowMode {
    Wrapping,
    Trapping,
}

impl OverflowMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wrapping => "wrapping",
            Self::Trapping => "trapping",
        }
    }
}

/// The right-hand side of an assignment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Rvalue {
    Use(Operand),
    Unary {
        op: UnOp,
        operand: Operand,
    },
    /// `overflow` is `Some` exactly for [`BinOp::Add`] and `None` for
    /// [`BinOp::Eq`]; the verifier enforces both directions.
    Binary {
        op: BinOp,
        overflow: Option<OverflowMode>,
        lhs: Operand,
        rhs: Operand,
    },
    Call {
        callee: FuncId,
        args: Vec<Operand>,
    },
}

impl Rvalue {
    /// Every operand read, in evaluation order.
    #[must_use]
    pub fn operands(&self) -> Vec<&Operand> {
        match self {
            Self::Use(op) | Self::Unary { operand: op, .. } => vec![op],
            Self::Binary { lhs, rhs, .. } => vec![lhs, rhs],
            Self::Call { args, .. } => args.iter().collect(),
        }
    }

    /// Result type in `func` of module `module`, or `None` when it cannot be
    /// determined (an operand names a missing local, or the callee is missing).
    #[must_use]
    pub fn ty(&self, module: &MirModule, func: &MirFunction) -> Option<TirType> {
        match self {
            Self::Use(op) => op.ty(func),
            Self::Unary { op: UnOp::Not, .. } => Some(TirType::Bool),
            Self::Binary { op, .. } => Some(match op {
                BinOp::Add => TirType::I64,
                BinOp::Eq => TirType::Bool,
            }),
            Self::Call { callee, .. } => module.func(*callee).map(|f| f.ret),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StmtKind {
    /// `dest = rvalue`. Assigning an initialized local is allowed (loops
    /// reassign); with today's all-`Copy` types nothing is leaked by it.
    Assign(LocalId, Rvalue),
    /// Explicit end of a local's extent: it is uninitialized afterwards.
    Drop(LocalId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stmt {
    pub kind: StmtKind,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TermKind {
    Goto(BlockId),
    Branch {
        cond: Operand,
        then_bb: BlockId,
        else_bb: BlockId,
    },
    Return,
}

impl TermKind {
    /// Successor blocks in a fixed order: `then` before `else`.
    #[must_use]
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Self::Goto(b) => vec![*b],
            Self::Branch {
                then_bb, else_bb, ..
            } => vec![*then_bb, *else_bb],
            Self::Return => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Terminator {
    pub kind: TermKind,
    pub provenance: Provenance,
}

/// Straight-line statements followed by exactly one terminator. The terminator
/// is mandatory by construction: an open block cannot be represented.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BasicBlock {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirFunction {
    /// Debug label; calls use [`FuncId`].
    pub name: String,
    /// Parameter locals; the verifier requires exactly `_1..=_n`.
    pub params: Vec<LocalId>,
    pub ret: TirType,
    pub locals: Vec<LocalDecl>,
    /// `blocks[0]` is the entry.
    pub blocks: Vec<BasicBlock>,
    pub provenance: Provenance,
}

impl MirFunction {
    #[must_use]
    pub fn local(&self, id: LocalId) -> Option<&LocalDecl> {
        self.locals.get(usize::try_from(id.0).ok()?)
    }

    #[must_use]
    pub fn block(&self, id: BlockId) -> Option<&BasicBlock> {
        self.blocks.get(usize::try_from(id.0).ok()?)
    }
}

/// A set of functions; [`FuncId`] indexes [`Self::funcs`], whose order is the
/// TIR module order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct MirModule {
    pub funcs: Vec<MirFunction>,
}

impl MirModule {
    #[must_use]
    pub fn func(&self, id: FuncId) -> Option<&MirFunction> {
        self.funcs.get(usize::try_from(id.0).ok()?)
    }
}
