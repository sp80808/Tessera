//! Backend-neutral MIR/CFG (contract B6, issue #22).
//!
//! - The data model ([`MirModule`], [`MirFunction`], ...): functions as CFGs
//!   of basic blocks over typed locals, with provenance on every statement,
//!   terminator and function.
//! - [`lower_module`]: TIR -> MIR, from TIR alone (TIR-5). Integer overflow is
//!   stated per `Add` from [`LowerOptions`], which has no default (O1).
//! - [`verify_module`]: the standalone verifier (MIR-1..3), including the
//!   definite-initialization dataflow ([`analyze_init`]).
//! - [`dump()`]: the deterministic textual CFG used for snapshots and `tsr mir`.
//! - [`interp`]: a reference interpreter. It must agree with the TIR
//!   evaluator (`tessera_tir::eval`) on every verified module; the
//!   differential tests check exactly that.
//!
//! Every entry point is total: malformed input yields diagnostics, findings or
//! a [`tessera_tir::eval::Halt`], never a panic, and nothing recurses on input
//! depth.

mod dump;
pub mod interp;
mod ir;
mod lower;
mod verify;

pub use dump::{dump, dump_function};
pub use ir::{
    BasicBlock, BinOp, BlockId, Const, FuncId, LocalDecl, LocalId, LocalKind, MirFunction,
    MirModule, Operand, OverflowMode, Rvalue, Stmt, StmtKind, TermKind, Terminator, TirType, UnOp,
};
pub use lower::{LowerOptions, lower_module, why};
pub use verify::{
    InitAnalysis, LocalState, MirError, MirErrorKind, MirLoc, analyze_init, verify_module,
};
