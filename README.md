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

The working invariant is that TC can be expanded into explicit intent and canonicalized back without semantic loss.

## Context-native tooling

Tessera also explores **context tiles**: compact typed metadata attached to repositories, modules or symbols. A tile can describe conditional tech-stack facts, API/version constraints, evidence, prior failures/fixes or agent verification requirements.

The runtime program does not silently depend on research metadata. External context is provenance-tracked, freshness-aware and treated as untrusted until verified.

## Start here

- [Core research dossier](docs/research/README.md)
- [Token efficiency research](docs/research/token-efficiency.md)
- [Context graph + autonomous harness](docs/research/context-graph-harness.md)
- [Context lattice / layered conditional metadata](docs/research/context-lattice.md)
- [Context tile language proposal](docs/spec/context-tiles.md)
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
- context harness: graph retrieval + token-budgeted compression + provenance/freshness verification;
- research loop: GitHub + Hugging Face + papers + official docs, with findings quarantined until validated.

## Status

Research/bootstrap stage. Syntax examples in the docs are **provisional experiments**, not a stable language specification.

The next milestone is to build the tokenizer benchmark and grammar experiment harness before freezing v0 syntax.
