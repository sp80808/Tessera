//! Canonical HIR text dump (contract §2.6): S-expression, IDs printed as paths,
//! no spans inline, spans in a separate `provenance:` section. Placeholder: the
//! signature is the contract; the body is owned by the HIR-lowering bead.

use crate::{HirModule, HirProvenance};

/// Deterministic snapshot text for goldens and humans.
#[must_use]
pub fn dump(_module: &HirModule, _provenance: &HirProvenance) -> String {
    String::new()
}
