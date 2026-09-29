# ADR 0001 — Keep TC, TIR, TCap, TCG and TMT distinct

Status: accepted for bootstrap architecture  
Date: 2026-09-29

## Context

Tessera optimizes canonical source for model-token efficiency while also requiring explicit debugging, ownership analysis, repository context and task/model-specific transport compression.

Trying to make one representation serve all of those purposes would couple language stability to model/tokenizer churn and context-harness policy.

## Decision

Maintain distinct representations:

- TC: canonical source.
- TIR: explicit semantic intent.
- TCap: compiler-derived ownership/borrow capability view.
- TCG: project/context/evidence graph.
- TMT: reversible task/model-specific transport.

Only TC is canonical authored source.

TIR/TCap are compiler-authoritative semantic products. TCG combines compiler facts with provenance-bearing project/external evidence. TMT is disposable and profile-specific.

## Consequences

Positive:
- model-specific compression can evolve without changing language syntax;
- debugging can be explicit even when TC is dense;
- external context cannot silently redefine runtime semantics;
- ownership diagnostics can use graph structure without making the graph the surface type system.

Cost:
- more conversion boundaries;
- round-trip/source-map testing becomes mandatory;
- versioning of derived representations is required.

## Reversal condition

Revisit only if benchmarks demonstrate that two representations can be merged without compromising canonical stability, semantic explicitness, security boundaries or measurable model efficiency.
