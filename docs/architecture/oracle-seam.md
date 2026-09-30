# Optional oracle seam (`tess-oracle`, not implemented)

Status: **design seam only.** No oracle code exists. This page records what is already true, so a later development/testing layer (for example one backed by Wolfram Language or another CAS/solver) can be added without touching compiler correctness.

## Rule

An oracle **consumes compiler outputs**; it is never a compiler, runtime or test-harness *dependency*. Compiler correctness must not require a model, a network, or a CAS (AGENTS.md; contract INV-PURE-1). This is enforced mechanically: `crates/tessera-phases/tests/architecture.rs::no_crate_depends_on_an_external_oracle` (INV-ORACLE-1) fails if any workspace crate depends on a Wolfram/Mathematica/oracle crate, and a new crate must be registered in `LAYERS` before it can exist.

## Machine-readable outputs that exist today

| Output | Producer | Form | Deterministic |
|---|---|---|---|
| TIR text (`.tir`) | `tsr tir`, `TirModule::to_text` | S-expression, every node typed; `TirModule::parse` reads it back | yes (golden-tested) |
| TIR verifier findings | `tessera_tir::verify_module` | ordered `TirError` list | yes |
| Tokenizer benchmark | `tess-tokenbench` (#1) | JSON + Markdown, corpus content hash, tokenizer versions | yes (byte-identical reruns are tested) |

An oracle can therefore read `.tir` (an S-expression maps directly onto a CAS expression tree) and benchmark JSON without linking anything from this repository.

## Candidate uses (all hypothetical until built)

1. **Expression-equivalence checks** between two TC/TIR spellings, or between a rewrite rule's left and right sides (integer `+` is associative only under wrapping semantics; the oracle would make that visible once O1 is decided).
2. **Counterexample search** for proposed rewrite/normalization rules over the tiny integer subset.
3. **Reference checks** for small ownership-state models (#3) against a declarative model.
4. **Optimal TCG packet calculation** as an offline comparison baseline for #6/#13 heuristics.
5. **Benchmark statistics** over `tess-tokenbench` JSON (confidence intervals, dispersion tests).

## Conditions for adding one

- lives outside the compiler crates (own crate or external script), reads artifacts, writes artifacts;
- its results are recorded as evidence with tool version and input hashes; they are never inputs to semantics;
- CI for the compiler must pass with the oracle absent;
- an accepted RFC names the property it checks and the falsification criterion.
