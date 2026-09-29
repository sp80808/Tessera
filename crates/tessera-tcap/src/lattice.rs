//! Capability lattice for TCap.
//!
//! The four capabilities form a partial order (not total):
//! - E (Exclusive): full authority — read, write, move, borrow
//! - R (Read): read + shared borrow
//! - W (Write): write where permitted (e.g., partial init)
//! - 0 (None): no access

use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Exclusive,
    Read,
    Write,
    None,
}

impl Capability {
    /// Returns true if this capability allows reading.
    #[must_use]
    pub const fn can_read(self) -> bool {
        matches!(self, Self::Exclusive | Self::Read)
    }

    /// Returns true if this capability allows writing.
    #[must_use]
    pub const fn can_write(self) -> bool {
        matches!(self, Self::Exclusive | Self::Write)
    }

    /// Returns true if this capability allows moving.
    #[must_use]
    pub const fn can_move(self) -> bool {
        matches!(self, Self::Exclusive)
    }

    /// Returns true if this capability allows shared borrowing.
    #[must_use]
    pub const fn can_share(self) -> bool {
        matches!(self, Self::Exclusive | Self::Read)
    }

    /// Returns true if this capability allows mutable borrowing.
    #[must_use]
    pub const fn can_loan_mut(self) -> bool {
        matches!(self, Self::Exclusive)
    }

    /// Canonical string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exclusive => "E",
            Self::Read => "R",
            Self::Write => "W",
            Self::None => "0",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Capability state for a place at a program point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityState {
    /// The capability at this point.
    pub capability: Capability,
    /// Outstanding borrows that affect this place.
    pub borrows: Vec<BorrowRef>,
    /// Whether this place is partially moved (some fields moved out).
    pub is_partial: bool,
    /// For composite places, the capabilities of individual fields.
    pub field_states: HashMap<String, CapabilityState>,
}

impl CapabilityState {
    #[must_use]
    pub fn new(capability: Capability) -> Self {
        Self {
            capability,
            borrows: Vec::new(),
            is_partial: false,
            field_states: HashMap::new(),
        }
    }

    #[must_use]
    pub fn exclusive() -> Self {
        Self::new(Capability::Exclusive)
    }

    #[must_use]
    pub fn read() -> Self {
        Self::new(Capability::Read)
    }

    #[must_use]
    pub fn write() -> Self {
        Self::new(Capability::Write)
    }

    #[must_use]
    pub fn none() -> Self {
        Self::new(Capability::None)
    }

    /// Check if this state is compatible with another (for joins).
    #[must_use]
    pub fn compatible_with(&self, other: &Self) -> bool {
        if self.is_partial || other.is_partial {
            return false;
        }
        match (self.capability, other.capability) {
            (Capability::None, _) | (_, Capability::None) => true,
            (Capability::Read, Capability::Read) => true,
            (Capability::Exclusive, Capability::Exclusive) => true,
            (Capability::Write, Capability::Write) => true,
            _ => false,
        }
    }

    /// Join two compatible states (used at control flow merges).
    #[must_use]
    pub fn join(&self, other: &Self) -> Option<Self> {
        if !self.compatible_with(other) {
            return None;
        }
        let capability = match (self.capability, other.capability) {
            (Capability::None, c) | (c, Capability::None) => c,
            (c1, c2) if c1 == c2 => c1,
            _ => return None,
        };
        Some(Self::new(capability))
    }
}

/// Reference to an active borrow.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BorrowRef {
    pub id: BorrowId,
    pub kind: BorrowKind,
    pub place: PlaceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorrowKind {
    Shared,
    Mutable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorrowId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaceId(pub u32);

impl fmt::Display for BorrowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl fmt::Display for PlaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}