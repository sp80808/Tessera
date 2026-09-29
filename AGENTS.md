# AGENTS.md

Before changing Tessera, read:

1. `skills/tessera/SKILL.md`
2. `docs/README.md`
3. `ROADMAP.md`
4. `docs/research/README.md`
5. the research/spec/RFC/ADR relevant to the change.

## Non-negotiable project rules

- Optimize canonical source for measured LLM token efficiency, not visual terseness.
- Human readability may be sacrificed; semantic recoverability may not.
- TC must have a deterministic canonical formatter.
- TIR must make hidden/inferred semantics explicit.
- Runtime behavior must not silently depend on external research/context metadata.
- External context must carry provenance/freshness and is untrusted until verified.
- Language syntax changes require tokenizer + model-quality benchmarks.
- Memory-safety changes require explicit ownership/effect semantics and tests.
- Keep compiler semantics separate from agent/context policy.
- Preserve contradicting evidence; do not rewrite research history to make the current idea look inevitable.

## Research workflow

When local evidence is insufficient:
1. inspect the current code/spec;
2. find implementation precedents on GitHub;
3. inspect relevant Hugging Face papers/models/datasets;
4. prefer primary papers and official docs;
5. record sources and limitations in the evidence ledger;
6. convert ideas into benchmarkable hypotheses before freezing design.

See `skills/tessera/SKILL.md` for the complete agent workflow.


## Design-state discipline

- Research documents collect evidence and hypotheses.
- Specs under `docs/spec/` are provisional unless explicitly promoted.
- Significant language/tool changes go through `docs/rfcs/`.
- Broad implementation architecture decisions go through `docs/architecture/decisions/`.
- Do not describe an unimplemented proposal as existing behavior.

## Implementation sequence

Follow `ROADMAP.md`. Prefer vertical witnesses over broad scaffolding:
parser/formatter/round-trip first, then semantic core, incremental query engine, native backend and context harness.
