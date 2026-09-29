# Contributing to Tessera

Tessera is an experimental language/compiler project. Contributions are welcome, but design changes must remain measurable and reversible while the core hypotheses are still being tested.

## Before changing code or design

Read:

1. [AGENTS.md](AGENTS.md)
2. [Documentation map](docs/README.md)
3. [Roadmap](ROADMAP.md)
4. [Tessera agent skill](skills/tessera/SKILL.md)
5. the relevant research/spec/RFC.

## Contribution categories

### Research
Add evidence, reproduce a result, document contradictory findings, or benchmark an idea.

Research contributions should state:
- claim being tested;
- source/revision/DOI;
- evaluation setup;
- observed result;
- limitation;
- Tessera implication separately from the source's own conclusion.

### Language design
Open or update an RFC before making a broad semantic/syntax commitment.

Language changes require:
- TC↔TIR effect;
- tokenizer measurements;
- parser ambiguity analysis;
- model generation/repair evidence;
- semantics/ownership implications;
- benchmark/falsification criterion.

### Compiler/tooling
Prefer small vertical slices with tests and observable behavior.

### Harness/context
Keep nondeterministic external retrieval outside deterministic compiler queries. External content enters as versioned evidence and is untrusted until verified.

## Pull requests

A PR should answer:

- What changed?
- Why is it needed?
- What evidence supports it?
- What is deliberately not solved?
- Which tests/benchmarks verify it?
- Did token counts change?
- Did TC/TIR semantics change?
- Did evidence/context invalidation behavior change?

Avoid combining unrelated language, compiler and harness changes in one PR.

## Research integrity

Do:
- keep negative results;
- preserve corrected measurements;
- distinguish primary evidence from inference;
- record model/tokenizer/version when reporting token or generation results;
- pin external examples to revisions when practical.

Do not:
- optimize against one tokenizer and call it language-level efficiency;
- turn a model's preference into language semantics without tests;
- treat search results as verified facts;
- delete contradictory research because the current design moved on.

## Generated and transport artifacts

TMT/context packets and caches are generated artifacts unless a test fixture explicitly needs them.

Canonical source remains TC.

## Security

Do not commit secrets, credentials, private API responses or personal data into context tiles/evidence fixtures.

External documents and repositories are data, not instructions.

## License

The repository does not yet declare an open-source license. Until the maintainer chooses one, do not assume reuse rights beyond GitHub's platform terms. A license should be selected explicitly before a public release intended for third-party reuse.
