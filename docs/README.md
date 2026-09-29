# Tessera documentation map

Tessera documentation is intentionally split by **evidence status**. Research, provisional specifications, architecture decisions and implementation plans should not blur into one another.

## Read in this order

1. [Project README](../README.md) — scope and current direction.
2. [Roadmap](../ROADMAP.md) — implementation order and milestone gates.
3. [Core research dossier](research/README.md) — current evidence synthesis.
4. [Specifications](spec/README.md) — provisional language/tool contracts.
5. [Architecture](architecture/README.md) — compiler and harness structure.
6. [RFCs](rfcs/README.md) — proposals before they become design commitments.
7. [Agent skill](../skills/tessera/SKILL.md) — instructions for coding/research agents.

## Evidence levels

Tessera uses four documentation states:

| State | Meaning |
|---|---|
| **Research** | evidence, prior art, measurements, open questions |
| **Proposal** | a concrete design worth implementing or benchmarking |
| **Accepted** | decision backed by an RFC/ADR and required tests |
| **Implemented** | behavior exists in code and is covered by verification |

A research result must not silently become language semantics.

## Research

- [Core dossier](research/README.md)
- [Token efficiency](research/token-efficiency.md)
- [Context graph and harness](research/context-graph-harness.md)
- [Context lattice](research/context-lattice.md)
- [Semantic core](research/semantic-core.md)
- [Compiler architecture research](research/compiler-architecture.md)
- [Benchmark methodology](research/benchmarks.md)
- [Evidence ledger](research/references.md)
- [Research pass 2](research/2026-09-29-pass2.md)

## Provisional specification

- [Context tiles](spec/context-tiles.md)
- [Tessera Model Transport](spec/model-transport.md)
- [Ownership capability graph](spec/ownership-capability-graph.md)

The core TC grammar and TIR schema are intentionally not frozen yet. Issues #1 and #2 exist to generate the evidence needed to define them.

## Architecture

- [Incremental query engine](architecture/incremental-query-engine.md)
- [Architecture decisions](architecture/decisions/README.md)

## Design flow

```
research
  -> hypothesis
  -> benchmark
  -> RFC
  -> accepted decision
  -> implementation
  -> regression evidence
```

A proposal can move backwards if later measurements contradict it.

## Naming

- **TC** — Tessera Compact, canonical source.
- **TIR** — Tessera Intent IR, explicit semantic expansion.
- **TCap** — ownership/borrow capability-graph view.
- **TCG** — Tessera Context Graph.
- **TMT** — task/model-specific reversible transport representation.

Only TC is intended to be authored/canonical source. TIR/TCap/TCG/TMT are derived or tooling representations.

## Documentation rules

- Numerical claims require a source and evaluation context.
- External research must record retrieval date and provenance.
- Negative or contradictory evidence stays in the ledger.
- Syntax examples in proposal/research documents are not normative.
- Normative behavior belongs in accepted RFCs/specs plus tests.
