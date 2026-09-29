# Research methodology and evidence policy

Tessera is unusually dependent on fast-moving LLM/compiler research. This file defines how findings enter the project.

## Source classes

1. **Primary formal/empirical research** — peer-reviewed paper or clearly identified preprint.
2. **Official implementation/docs** — upstream compiler/library/model documentation and canonical repositories.
3. **Independent reproduction** — benchmark or experiment we can rerun.
4. **Secondary analysis** — useful for discovery, not enough alone for design claims.
5. **Search/model synthesis** — hypothesis generator only until traced to evidence.

## Evidence record

For a material claim record, where available:

```
claim
source
source_kind
authors
date
doi/arxiv/revision
retrieved_at
population/corpus
models/tokenizers/toolchain
metric
reported_result
limitations
tessera_inference
reproduction_status
```

Keep `reported_result` separate from `tessera_inference`.

## Freshness

Fast-moving claims such as model behavior, APIs, dependency versions and agent benchmarks should be treated as time-scoped.

Stable mathematical/formal claims do not need arbitrary expiry, but their applicability to Tessera can still change.

## Triangulation

Important design changes should seek at least two of:
- primary paper;
- independent implementation;
- local reproduction.

Convergence is stronger when sources are methodologically independent.

## Negative evidence

Negative, null and contradictory results remain in the ledger.

Examples:
- syntax that compresses bytes but expands model tokens;
- context compression that reduces tokens but lowers task success;
- a graph retrieval technique that adds tool overhead without improving resolution;
- an ownership convenience that complicates soundness/diagnostics.

## Connector/tool provenance

Search connectors are discovery mechanisms, not authorities.

When a connector is rate-limited/unavailable:
- record the gap;
- do not invent validation;
- continue with available primary sources;
- revisit the missing source when access returns if it could materially change a decision.

## Reproduction priority

Prefer reproducing claims that directly gate Tessera architecture:

1. token reduction across tokenizer families;
2. model correctness under compact/transport representations;
3. context packet sufficiency;
4. incremental invalidation behavior;
5. ownership diagnostics;
6. native runtime/codegen performance.

## Promotion

Research -> proposal only when the mechanism is understood well enough to state a falsifiable Tessera hypothesis.

Proposal -> accepted design only after the relevant benchmark/semantic gate passes.
