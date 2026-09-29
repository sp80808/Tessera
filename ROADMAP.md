# Tessera roadmap

Tessera is currently in **research/bootstrap**. The near-term goal is not feature breadth; it is to validate whether the core premise survives measurement.

## Milestone 0 — Research baseline

Status: active / substantially documented.

Goals:
- establish evidence ledger and research process;
- define TC/TIR/TCG/TMT/TCap boundaries;
- document falsification criteria;
- survey code tokenization, repository context, ownership and compiler architecture.

Relevant issues: #7, #8.

Exit gate:
- every major architectural claim is either sourced, explicitly hypothetical, or backed by a local experiment.

## Milestone 1 — Token + syntax laboratory

Priority: highest.

Issues:
- #1 tokenizer portfolio benchmark and grammar-alignment dashboard;
- #2 minimal TC grammar, formatter and lossless TC↔TIR round-trip;
- #10 TMT pack/unpack and token-sugar experiment;
- #11 grammar-constrained generation adapter.

Deliverable:
a small executable corpus represented in Rust/C/Zig/Tessera candidates with reproducible token counts and model-generation experiments.

Exit gates:
- canonical formatting is deterministic;
- TC↔TIR round-trip is exact for the supported subset;
- tokenizer results cover multiple model families;
- no syntax is accepted because of character count alone;
- TMT remains strictly derived and reversible.

## Milestone 2 — Semantic safety core

Issues:
- #3 affine ownership + borrowing + effects;
- #12 TCap capability graph and diagnostics.

Implementation order:
1. owned affine values;
2. deterministic drop;
3. immutable borrow;
4. lexical borrow extent;
5. reborrow;
6. mutable borrow;
7. field-sensitive ownership;
8. non-lexical shortening only after the simpler model works.

Exit gates:
- positive/negative semantic tests;
- explicit TIR for every hidden operation;
- graph-native ownership diagnostics;
- no heuristic ownership reconstruction.

## Milestone 3 — Incremental compiler skeleton

Issues:
- #9 Salsa-backed incremental semantic query engine;
- #4 Cranelift codegen spike.

Target workspace:

```
crates/
  tessera-db
  tessera-syntax
  tessera-tir
  tessera-sema
  tessera-context
  tessera-codegen-cranelift
  tessera-cli
```

Exit gates:
- small TC program parses -> TIR -> MIR -> native code;
- edits invalidate only relevant semantic queries;
- query logs prove reuse;
- native benchmark baseline exists against equivalent Rust/C.

## Milestone 4 — Context-native harness

Issues:
- #5 context tile parser/lattice;
- #6 compiler-derived TCG + context packets;
- #7 research adapters/evidence quarantine;
- #13 active context workspace + structure-first benchmark.

Exit gates:
- task packet is reproducible from repo revision + task + profile;
- evidence has provenance/freshness;
- external material cannot silently change compiler semantics;
- graph/context benchmark compares against plain file retrieval;
- context operations are auditable.

## Milestone 5 — Integrated agent/compiler prototype

Deliver one end-to-end task:

```
issue/task
 -> TCG localization
 -> context projection
 -> optional TMT
 -> model patch
 -> TC parse/check
 -> TCap diagnostics
 -> tests
 -> graph/evidence update
```

Success is measured by **total tokens/cost/time to a correct verified patch**, not compression ratio alone.

## What is intentionally deferred

- large standard library;
- package registry;
- self-hosting;
- advanced async syntax;
- macro system;
- stable ABI;
- LLVM backend;
- sophisticated NLL edge cases;
- custom model/tokenizer training.

These should not distract from validating the core hypotheses.
