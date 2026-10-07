//! Resolution, type checking and HIR -> TIR lowering (contract B3-B5, issues
//! #20/#21).
//!
//! Three pure phases over one file's HIR, each returning a value **and**
//! diagnostics (INV-DIAG-1), each keyed by HIR identities (`ItemId`,
//! `ExprId`, `LocalId`) and free of spans (PROV-3):
//!
//! 1. [`resolve`] (B3): every name occurrence gets a [`Res`], every type
//!    annotation a [`Ty`]. Unknown names stay [`Res::Unresolved`]; there is no
//!    fallback to a similar name.
//! 2. [`typeck`] (B4): every expression gets a [`Ty`]. [`Ty::Error`] marks
//!    what cannot be typed and absorbs further errors, so one mistake is
//!    reported once and independent mistakes are all reported.
//! 3. [`to_tir`] (B5): functions whose facts are complete become TIR with a
//!    total provenance table; the result must pass the standalone TIR
//!    verifier (checked, reported as an internal error otherwise).
//!
//! [`analyze`] runs all three. [`dump`] prints the HIR annotated with both
//! phases' facts.
//!
//! # Rules (v0)
//!
//! - **Types.** The primitive type table is an explicit input:
//!   [`V0_PRIMS`] (`i64` only, matching the documented TC subset) unless a
//!   caller passes another to [`resolve_with`]. Adding a primitive is a
//!   language change and belongs in an RFC.
//! - **Names.** An expression path resolves to a parameter of the same
//!   function; there are no other value binders in the current grammar. With
//!   duplicate parameter names the **first** one wins (and the duplicate is an
//!   error). Items (functions) are not values.
//! - **Typing.** Integer literals are `i64`; `+` takes and returns `i64`; a
//!   function body must have the declared return type.
//! - **Effects** (#21) are not computed yet: whether `+` can trap depends on
//!   the undecided overflow semantics (O1).

mod dump;
mod index;
mod input;
mod resolve;
mod tir;
mod typeck;

use tessera_hir::HirOutput;
use tessera_phases::PhaseOutput;

pub use dump::dump;
pub use index::{FileSummary, FnSummary, ModuleIndex, Signature, Symbol};
pub use resolve::{Res, ResolvedFn, ResolvedModule, V0_PRIMS, resolve, resolve_with};
pub use tir::{TirOutput, to_tir};
pub use typeck::{TypedFn, TypedModule, typeck};

/// A semantic type. `Error` stands for "could not be determined"; it never
/// reaches TIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ty {
    I64,
    Bool,
    Error,
}

impl Ty {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::Bool => "bool",
            Self::Error => "{error}",
        }
    }

    /// The TIR spelling, or `None` for [`Ty::Error`].
    #[must_use]
    pub const fn to_tir(self) -> Option<tessera_tir::TirType> {
        match self {
            Self::I64 => Some(tessera_tir::TirType::I64),
            Self::Bool => Some(tessera_tir::TirType::Bool),
            Self::Error => None,
        }
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything the three phases produce for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub resolved: ResolvedModule,
    pub typed: TypedModule,
    pub tir: TirOutput,
}

/// Run resolution, type checking and TIR lowering with [`V0_PRIMS`];
/// diagnostics of all three, in canonical order.
#[must_use]
pub fn analyze(hir: &HirOutput) -> PhaseOutput<Analysis> {
    let resolved = resolve(hir);
    let typed = typeck(hir, &resolved.value);
    let tir = to_tir(hir, &resolved.value, &typed.value);
    let mut diagnostics = resolved.diagnostics;
    diagnostics.extend(typed.diagnostics);
    diagnostics.extend(tir.diagnostics);
    PhaseOutput::with(
        Analysis {
            resolved: resolved.value,
            typed: typed.value,
            tir: tir.value,
        },
        diagnostics,
    )
}

#[cfg(test)]
mod tests;
