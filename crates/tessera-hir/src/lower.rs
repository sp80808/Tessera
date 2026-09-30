//! `CST -> HIR` lowering (contract B1 -> B2). Placeholder: the signature is the
//! contract; the body is owned by the HIR-lowering bead (tes-s0s).

use tessera_phases::PhaseOutput;
use tessera_syntax::cst::ParsedFile;

use crate::{HirModule, HirOutput, HirProvenance};

/// Lower a parsed file to normalized HIR. Tolerant: erroneous CST yields
/// explicit error/missing nodes plus diagnostics, never a panic. Pure and
/// deterministic: same CST and source text give equal output (HIR-4).
#[must_use]
pub fn lower(parsed: &ParsedFile, _src: &str) -> PhaseOutput<HirOutput> {
    PhaseOutput::clean(HirOutput {
        module: HirModule {
            file: parsed.file,
            items: Vec::new(),
        },
        provenance: HirProvenance::default(),
    })
}
