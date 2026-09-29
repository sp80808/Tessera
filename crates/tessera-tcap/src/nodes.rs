//! Place and borrow nodes for the TCap graph.

use std::fmt;
use super::lattice::{BorrowId, BorrowKind, PlaceId};

/// A place in memory that can hold capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlaceNode {
    /// Local variable: `let x = ...`
    Local {
        id: PlaceId,
        name: String,
        ty: PlaceType,
    },
    /// Field access: `x.field`
    Field {
        id: PlaceId,
        base: PlaceId,
        field: String,
        ty: PlaceType,
    },
    /// Index access: `x[i]`
    Index {
        id: PlaceId,
        base: PlaceId,
        index: PlaceIndex,
        ty: PlaceType,
    },
    /// Dereference: `*p`
    Deref {
        id: PlaceId,
        base: PlaceId,
        ty: PlaceType,
    },
    /// Function argument: `arg#0`
    Remote {
        id: PlaceId,
        arg_index: usize,
        ty: PlaceType,
    },
}

impl PlaceNode {
    #[must_use]
    pub fn id(&self) -> PlaceId {
        match self {
            Self::Local { id, .. } => *id,
            Self::Field { id, .. } => *id,
            Self::Index { id, .. } => *id,
            Self::Deref { id, .. } => *id,
            Self::Remote { id, .. } => *id,
        }
    }

    #[must_use]
    pub fn ty(&self) -> PlaceType {
        match self {
            Self::Local { ty, .. } => *ty,
            Self::Field { ty, .. } => *ty,
            Self::Index { ty, .. } => *ty,
            Self::Deref { ty, .. } => *ty,
            Self::Remote { ty, .. } => *ty,
        }
    }

    #[must_use]
    pub fn base(&self) -> Option<PlaceId> {
        match self {
            Self::Local { .. } => None,
            Self::Field { base, .. } => Some(*base),
            Self::Index { base, .. } => Some(*base),
            Self::Deref { base, .. } => Some(*base),
            Self::Remote { .. } => None,
        }
    }

    #[must_use]
    pub fn is_composite(&self) -> bool {
        matches!(self, Self::Local { ty, .. } | Self::Field { ty, .. } | Self::Deref { ty, .. } if ty.is_composite())
    }
}

impl fmt::Display for PlaceNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local { name, .. } => write!(f, "local {name}"),
            Self::Field { base, field, .. } => write!(f, "field {base}.{field}"),
            Self::Index { base, index, .. } => write!(f, "index {base}[{index}]"),
            Self::Deref { base, .. } => write!(f, "deref *{base}"),
            Self::Remote { arg_index, .. } => write!(f, "remote arg#{arg_index}"),
        }
    }
}

/// Type information for places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceType {
    Scalar,
    Composite,
    Reference { mutable: bool },
}

impl PlaceType {
    #[must_use]
    pub const fn is_composite(self) -> bool {
        matches!(self, Self::Composite)
    }

    #[must_use]
    pub const fn is_reference(self) -> bool {
        matches!(self, Self::Reference { .. })
    }

    #[must_use]
    pub const fn is_mutable_ref(self) -> bool {
        matches!(self, Self::Reference { mutable: true })
    }
}

/// Index expression for array/slice access.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlaceIndex {
    Constant(usize),
    Variable(String),
    Expr(String),
}

impl fmt::Display for PlaceIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Constant(i) => write!(f, "{i}"),
            Self::Variable(v) => write!(f, "{v}"),
            Self::Expr(e) => write!(f, "({e})"),
        }
    }
}

/// Borrow projection node representing an active borrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowNode {
    pub id: BorrowId,
    pub kind: BorrowKind,
    pub place: PlaceId,
    pub extent: BorrowExtent,
    pub origin_span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowExtent {
    Lexical { start: u32, end: u32 },
    NonLexical { points: Vec<u32> },
}

impl BorrowExtent {
    #[must_use]
    pub fn contains(&self, point: u32) -> bool {
        match self {
            Self::Lexical { start, end } => point >= *start && point < *end,
            Self::NonLexical { points } => points.contains(&point),
        }
    }
}

/// Projection node combining place and borrow information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionNode {
    Place(PlaceNode),
    Borrow(BorrowNode),
}

/// Source span for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(file: u32, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }

    #[must_use]
    pub const fn dummy() -> Self {
        Self { file: 0, start: 0, end: 0 }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}:{}", self.file, self.start)
    }
}