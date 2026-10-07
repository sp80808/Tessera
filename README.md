# Tessera

**An LLM-native systems programming language experiment.**

Tessera is designed to explore whether a programming language can combine:

- Rust-class native systems performance and static safety;
- aggressively token-efficient canonical source for LLMs;
- deterministic, lossless intent expansion for debugging;
- repository-level context as a typed, queryable knowledge graph;
- agent harnesses that retrieve, verify, compress and update project knowledge.

Canonical Tessera source is allowed to be difficult for humans to read. **Readability is a tooling/view concern; debuggability and semantic recoverability are hard requirements.**

## Core model

Tessera is being designed around three representations:

| Representation | Purpose |
|---|---|
| **TC — Tessera Compact** | canonical, aggressively token-dense source |
| **TIR — Tessera Intent IR** | explicit, lossless semantic/debug expansion |
| **TCG — Tessera Context Graph** | project, dependency, evidence, troubleshooting and agent context |
| **TMT — Tessera Model Transport** | reversible task/model-specific token compression; never canonical source |
| **TCap — capability graph view** | compiler-derived ownership/borrow transitions for checking and diagnostics |

The working invariant is that TC can be expanded into explicit intent and canonicalized back without semantic loss.

## Context-native tooling

Tessera also explores **context tiles**: compact typed metadata attached to repositories, modules or symbols. A tile can describe conditional tech-stack facts, API/version constraints, evidence, prior failures/fixes or agent verification requirements.

The runtime program does not silently depend on research metadata. External context is provenance-tracked, freshness-aware and treated as untrusted until verified.

## Start here

- [Documentation map](docs/README.md)
- [Roadmap](ROADMAP.md)
- [Contributing](CONTRIBUTING.md)
- [Core research dossier](docs/research/README.md)
- [Token efficiency research](docs/research/token-efficiency.md)
- [Context graph + autonomous harness](docs/research/context-graph-harness.md)
- [Context lattice / layered conditional metadata](docs/research/context-lattice.md)
- [Context tile language proposal](docs/spec/context-tiles.md)
- [Tessera Model Transport proposal](docs/spec/model-transport.md)
- [Ownership capability graph proposal](docs/spec/ownership-capability-graph.md)
- [`tsr witness`: machine-readable compiler evidence](docs/spec/witness.md) (`cargo run -q -p tessera-cli -- witness examples/witness/pass.tes`)
- [Incremental semantic query-engine architecture](docs/architecture/incremental-query-engine.md)
- [Research pass 2: transport, active context, ownership graphs](docs/research/2026-09-29-pass2.md)
- [Research pass 3: what makes a model repair an unfamiliar language](docs/research/2026-10-04-llm-repair.md)
- [Semantic core / memory safety](docs/research/semantic-core.md)
- [Compiler architecture](docs/research/compiler-architecture.md)
- [Benchmark methodology](docs/research/benchmarks.md)
- [Evidence ledger / related work](docs/research/references.md)
- [Tessera agent skill](skills/tessera/SKILL.md)

## Current design direction

- compiler implementation: Rust;
- initial codegen candidate: Cranelift;
- later/optional optimizing backend: LLVM;
- memory model: affine ownership + shared/exclusive borrowing + explicit unsafe capabilities;
- canonical formatter: exactly one TC spelling;
- debugging: dense diagnostics plus expanded TIR diagnostics;
- context harness: structure-first graph retrieval + active context workspace + token-budgeted compression + provenance/freshness verification;
- compiler/context architecture: shared incremental semantic query database;
- model transport: reversible compiler-generated compression separate from canonical TC;
- ownership diagnostics: TCap capability graph derived from the formal semantic core;
- research loop: GitHub + Hugging Face + papers + official docs, with findings quarantined until validated.

## Repository state

The repository now separates research, provisional specification, architecture decisions and future normative RFCs. Open issues are organized as experimentally gated implementation slices rather than a feature wishlist.

See [ROADMAP.md](ROADMAP.md) for the current sequence: token/syntax laboratory → safety core → incremental compiler → context-native harness → integrated agent/compiler prototype.

## Status

Research/bootstrap stage. Syntax examples in the docs are **provisional experiments**, not a stable language specification.

The next milestone is to build the tokenizer benchmark and grammar experiment harness before freezing v0 syntax.
