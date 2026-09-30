//! Normalized HIR (contract B2, issue #19).
//!
//! HIR is the first semantic-facing representation: syntax noise (trivia,
//! redundant grouping, token kinds) is gone, every node has a stable
//! *identity*, and *provenance* (where it came from in the source text) lives in
//! side tables, never inside nodes (PROV-3). Two spellings of the same program
//! therefore lower to equal [`HirModule`]s and different [`HirProvenance`]s,
//! which is what lets an incremental engine cut off after a formatting edit.
//!
//! This file fixes the *types* that lowering (`CST -> HIR`), semantic analysis
//! (`tessera-sema`) and the canonical printer share. Names are unresolved
//! strings here: resolution is B3, types are B4.
//!
//! Identity rules (contract §2.4, INV-ID-1..3): no ID is derived from a byte
//! offset, pointer, text hash or global counter. [`ItemId`] is structural;
//! [`ExprId`]/[`LocalId`] are dense pre-order indices, **body-local**, and only
//! meaningful together with the owning [`FnItem`].
//!
//! Errors are values, not exits (INV-DIAG-1): erroneous CST lowers to a
//! structurally valid HIR containing explicit [`Expr::Error`] /
//! [`Expr::Missing`] / [`TypeRef::Missing`] / `None` names, so later phases and
//! editors keep working (HIR-2). Nothing is silently dropped.

use tessera_phases::{FileId, Provenance, ProvenanceMap};

mod dump;
mod lower;
mod print;

pub use dump::dump;
pub use lower::lower;
pub use print::{PrintError, print_tc};

/// What kind of top-level item an [`ItemId`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKind {
    Fn,
}

impl ItemKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fn => "fn",
        }
    }
}

/// Structural identity of a top-level item: stable across every edit that does
/// not add, remove or rename that item or reorder same-named siblings
/// (contract §2.4). `disambiguator` counts earlier same-named items of the same
/// kind in the same file, in source order, and is 0 for the first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    pub file: FileId,
    pub kind: ItemKind,
    /// Item name; an item whose name is missing in the source uses `""`.
    pub name: String,
    pub disambiguator: u32,
}

impl ItemId {
    /// Debug/dump spelling: `fn/add`, or `fn/add#1` for a second `add`.
    #[must_use]
    pub fn path(&self) -> String {
        if self.disambiguator == 0 {
            format!("{}/{}", self.kind.as_str(), self.name)
        } else {
            format!(
                "{}/{}#{}",
                self.kind.as_str(),
                self.name,
                self.disambiguator
            )
        }
    }
}

/// Body-local expression id: index into [`Body::exprs`], assigned in pre-order
/// (node, then operands left to right) by the deterministic lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(pub u32);

/// Body-local binder id: index into [`Body::locals`]. Parameters come first, in
/// declaration order (`LocalId(0)` is the first parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);

/// A type as written; resolution to a real type is B3/B4.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeRef {
    /// A path such as `i64`. Whether it names a type is not HIR's business.
    Path(String),
    /// The required type was absent in the source.
    Missing,
    /// Unexpected tokens stood where the type was expected.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
}

impl BinOp {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
        }
    }
}

/// Expression node. Children are [`ExprId`]s into the same [`Body`]; redundant
/// parentheses do not exist here (`(e)` is `e`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Int(i64),
    /// A name occurrence, still unresolved.
    Path(String),
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    /// Unexpected tokens, or a literal that does not fit `i64`.
    Error,
    /// A required expression was absent.
    Missing,
}

/// A binder. Parameters are the first locals of the body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Local {
    /// `None` if the name was missing in the source.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Param {
    pub local: LocalId,
    pub ty: TypeRef,
}

/// Per-function arenas. `exprs` is in pre-order, `root` is `ExprId(0)` for a
/// lowering that follows the contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Body {
    pub locals: Vec<Local>,
    pub exprs: Vec<Expr>,
    pub root: ExprId,
}

impl Body {
    /// The expression with this id. Panics only on an id from another body, a
    /// caller bug; lowering never produces one.
    #[must_use]
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    #[must_use]
    pub fn local(&self, id: LocalId) -> &Local {
        &self.locals[id.0 as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnItem {
    pub id: ItemId,
    /// `None` if the name was missing in the source.
    pub name: Option<String>,
    pub params: Vec<Param>,
    pub ret: TypeRef,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Item {
    Fn(FnItem),
}

impl Item {
    #[must_use]
    pub fn id(&self) -> &ItemId {
        match self {
            Self::Fn(f) => &f.id,
        }
    }
}

/// One file's items in source order. Equal for any two spellings of the same
/// program (INV-ID-2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirModule {
    pub file: FileId,
    pub items: Vec<Item>,
}

/// Where in a function a type annotation sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypePos {
    /// The `i`-th parameter's type.
    Param(u32),
    Ret,
}

/// Provenance of one function's HIR: side tables keyed by the function's
/// body-local IDs (PROV-3). Total over the function when every table covers all
/// of its IDs (PROV-1); desugared/compiler-introduced nodes are
/// `Provenance::Synthesized`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnProvenance {
    /// The whole function item (excludes leading/trailing trivia).
    pub item: Provenance,
    /// The function name, or the place it should have been.
    pub name: Provenance,
    pub locals: ProvenanceMap<LocalId>,
    pub exprs: ProvenanceMap<ExprId>,
    pub types: ProvenanceMap<TypePos>,
}

/// Provenance for a whole module, parallel to [`HirModule::items`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HirProvenance {
    pub items: Vec<FnProvenance>,
}

/// What `CST -> HIR` lowering returns as its phase value (wrapped in
/// `PhaseOutput`, so diagnostics travel alongside).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirOutput {
    pub module: HirModule,
    pub provenance: HirProvenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_paths_disambiguate_same_named_siblings() {
        let id = |d| ItemId {
            file: FileId(0),
            kind: ItemKind::Fn,
            name: "add".to_owned(),
            disambiguator: d,
        };
        assert_eq!(id(0).path(), "fn/add");
        assert_eq!(id(1).path(), "fn/add#1");
        assert!(id(0) < id(1));
    }
}
