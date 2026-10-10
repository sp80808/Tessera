# Tessera language viability and integration decision — 2026-10-08

Status: research decision / falsifiable plan, not a claim that Tessera has Rust-equivalent semantics, native speed or better model-generation reliability.

## Executive choice

For the next six weeks, **focus Tessera on a compiler-backed intermediate representation, trustworthy diagnostics and language-generation experiments**, while Lattice handles the wider developer-facing orchestration product.

A new Rust-class general-purpose systems language is a **high-risk long-horizon bet**. A useful compiler evidence interface, canonical compact notation and structured graph may create value much sooner, including for existing Rust/TypeScript code via source/structure adapters. Do not make full adoption of TC a prerequisite for Lattice.

Provisional engineering split across both projects: ~25% Tessera / ~75% Lattice. Review using measured results.

## Implementation versus hypotheses

### Already present in this repository
- Narrow Tessera Compact (TC) grammar, formatter, lexer/CST/HIR/sema/TIR/MIR pipeline and reference execution for a subset.
- `tsr witness` produces versioned machine-readable compiler evidence, source/phase digests, diagnostics and result identity.
- `tsr grammar` + compiler-checked foreign-syntax and name-resolution suggestions; integrated with Lattice's repair sample.
- Multi-tokenizer measurements exist. [PR #48](https://github.com/sp80808/Tessera/pull/48) is **open** as of this decision and proposes a pinned broader tokenizer portfolio plus CI regression gate.

### Not yet established
- Rust-class memory safety over realistic ownership/borrowing/unsafe/FFI interactions.
- A validated native backend for representative systems programs, production ABI/tooling, package ecosystem or reasonable migration story.
- Lower **whole-task** model cost or higher compile@1/test@1 success than writing Rust/TS directly.
- Human adoption among working systems developers.

The README/roadmap use research-stage language: do not present full systems-language claims as delivered.

## Test four distinct propositions, not one vague 'token efficiency' metric

| Hypothesis | Baseline | Required evidence | Falsification signal |
|---|---|---|---|
| TC syntax is easier/cheaper for models | Equivalent Rust/Zig/C subset and alternate TC grammar | Tokenizer portfolio, compile@1, semantics/test@1, repair turns, all billed tokens/time | Token savings disappear across models or more model repairs negate them |
| TIR/TCG improve software work | Lexical search, ordinary repo maps, plain structured diagnostics | Gold-context retrieval, trace2code/edit2ripple recall, verified patches at fixed budget | Added index/protocol time, stale facts or false context dominate |
| `tsr witness` gives better repair feedback | Raw diagnostics, direct interpreter tests | Model repair success with **suggestions ON/OFF**, lineage by compiler vs model | Gains are entirely deterministic compiler fixes mislabeled as model improvements |
| Safety+native performance can rival Rust | Same-behavior Rust/C kernels, existing property tests | Formal memory/effect rules, differential/metamorphic tests, actual native backend timings | UB/unsoundness, missing basic capabilities or disproportionate compiler burden |

## Two-step near-term experimental plan

### Step 1: stabilize precise compiler evidence (immediate)

- Review/merge [#48 tokenizer portfolio PR](https://github.com/sp80808/Tessera/pull/48) after the documented CI/benchmark checks; it should constrain syntax evolution, not imply model superiority.
- Keep `tsr witness` result IDs canonical, deterministic and stable across runs; corruption or version mismatch must be `tool_error`, not semantic `fail`.
- Expand [#45 differential/metamorphic oracle testing](https://github.com/sp80808/Tessera/issues/45) for formatting, TIR↔TC, MIR/interpreter, alternate syntax and critical ownership cases.
- Expand [#46 raw pointer / FFI obligations](https://github.com/sp80808/Tessera/issues/46) as a specification before allowing unsafe codegen claims. Native codegen (#4) stays a **time-boxed spike**, not a requirement for evaluating compiler feedback.
- Benchmark diagnostics, independent compiler suggestions and grammar prompting separately, including direct baseline and unknown-candidate handling.

**Exit:** compiler signal trustworthy; every 'pass' is semantically bounded by explicit verifier capabilities and versioned witness.

### Step 2: build a language repair comparison and report (following 2–4 weeks)

- Use existing `examples/witness` and Lattice `examples/tessera-repair` plus new bounded functions drawn from a **held-out** corpus, not code copied from compiler suggestion templates.
- Compare three conditions for the *same spec and oracle*: Rust source, TC compact source, and TC with compiler-checked suggestions/grammar; keep model and budget fixed.
- Test at least one local small coding model and one stronger model; record actual model/tokenizer IDs, prompt hashes and error classes.
- Separate syntax failures from type/borrow failures, behavior failures and execution/tool failures. An interpreter-only run is not evidence of native-code or borrow-checker soundness.
- Register an independent verification oracle. Never use the reference solution or gold commit in the agent worktree/context.
- Record bytes + **≥4 tokenizer families** where available, compile@1, tests@1, repairs, total input/output/cache tokens, latency, billed cost, diagnostics validity, retained semantics and native compiler version.
- Report negative controls: ordinary Rust already solved perfectly, unfamiliar grammar harms smaller models, prompts where compressed identifiers obscure context, and false/ambiguous compiler suggestions.
- Treat small task counts as exploratory; do not market percent improvement without a larger representative corpus and uncertainty intervals.

**Exit:** clear, reproducible evidence whether compact TC offers end-to-end net benefit over generating conventional source directly.

## Result-dependent next moves

### If results are compelling
Incrementally broaden semantics **one proven subset at a time**, prioritizing ownership and provenance (#3/#12/#46) plus fast structured diagnostics. Native compilation (#4/#22/#24) becomes justified for real kernels **after** semantic tests; maintain a clear unsafe/FFI boundary. Keep TMT as a derived, reversible transport, not a replacement source language.

### If TC generation is worse but evidence is useful
Retain the compiler as an **agent-native correctness/diagnostics service**:
- import code structure from Rust/TypeScript into TCG (#43);
- emit normalized evidence and source maps;
- offer context indexing, compiler advice, and structure-aware edits;
- let Lattice serve normal languages without TC user adoption.

### If there is no material improvement
Freeze novel TC syntax expansion and full runtime roadmap; maintain the reliable witness prototype and publish reproducible negative findings. Continue only when a new falsifiable hypothesis appears.

## Engineering and market viability

Near-term realistic deliverable: a dependable open-source proof of compiler-mediated coding workflows / agent context exchange, useful to coding-agent authors. Long-term general systems-language adoption requires performance, safety, libraries, debugging, platform support, clear ergonomics, and a migration path **well beyond** syntax compression. Assess these separately; do not describe project-specific research as proof of commercial demand.

Tessera does not need its own full chat agent UI, search orchestrator, MCP marketplace or billing layer. Reuse Lattice for all of that.

## Research touchstones

1. [CAVEWOMAN, June 2026](https://arxiv.org/abs/2606.24083): shortened input language can backfire even when output reductions help; optimize **whole-task outcomes** before optimizing characters.
2. [PROBE coding-language benchmark, September 2026](https://doi.org/10.1007/s10664-026-10904-5): compilation failures depend on language and model; uncommon syntax/strict rules are real model-development risks.
3. [Grammar Prompting, NeurIPS 2023](https://arxiv.org/abs/2305.19234): grammar communication is worth explicit ablation.
4. [SWE-agent, NeurIPS 2024](https://arxiv.org/abs/2405.15793): tool/diagnostic interface affects downstream agent performance.
5. [Agent Retrieval Bench, July 2026](https://arxiv.org/abs/2607.24882): lexical, structural and embedding retrievers win on different tasks; avoid assuming one big context graph wins universally.

## Next actions by existing tracker

- [#1 token portfolio](https://github.com/sp80808/Tessera/issues/1) / [PR #48](https://github.com/sp80808/Tessera/pull/48): review and finish grammar-promotion gate.
- [#45 metamorphic/differential testing](https://github.com/sp80808/Tessera/issues/45): enlarge oracle correctness fixtures.
- [#43 cross-language TCG import](https://github.com/sp80808/Tessera/issues/43): route value to existing programming ecosystems.
- [#10 TMT compression experiment](https://github.com/sp80808/Tessera/issues/10): keep as derived, reversible and task-verified.
- [#3 safety core](https://github.com/sp80808/Tessera/issues/3), [#12 ownership capability diagnostics](https://github.com/sp80808/Tessera/issues/12), [#46 unsafe boundaries](https://github.com/sp80808/Tessera/issues/46): necessary before claiming Rust-class safety.
- [Lattice #39 joint dogfood](https://github.com/sp80808/Lattice/issues/39) and [#42 structural-context comparison](https://github.com/sp80808/Lattice/issues/42): use **one** evidence/evaluation authority.

Companion: [Lattice focus report](https://github.com/sp80808/Lattice/blob/main/docs/research/2026-10-08-focus-and-viability.md) after the corresponding PR merges.
