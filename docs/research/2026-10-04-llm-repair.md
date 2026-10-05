# Research pass 3: what makes a model repair an unfamiliar language

Date: 2026-10-04. Question: given the first Lattice repair runs, which
compiler and harness changes most improve how often a model reaches a
`tsr`-verified patch, and at what token cost?

## What the runs showed

Lattice's `examples/tessera-repair` drives three broken one-function TC
programs to a `tsr witness` pass plus `tsr run` cases.

- A local 7B coder model wrote C/Rust-style programs (braces, `return`, `;`)
  and kept doing so after `tsr` rejected them: 2/5 on the syntax-error seed,
  no better than random choice among the same candidates.
- Hosted frontier models fixed the same seeds in one round.
- The feedback the model saw for a Rust-style program was eleven cascading
  `E-syntax-expected` errors, none of which said "this is Rust, TC spells it
  `f NAME(P:i64)>i64=EXPR`". Rejected attempts were listed without the error
  each one produced.

## What the literature says

| Finding | Source | Consequence for Tessera/Lattice |
|---|---|---|
| Self-repair is bottlenecked by feedback quality: better feedback on the same model raises repair rates more than extra samples do. | Olausson et al., *Is Self-Repair a Silver Bullet for Code Generation?*, ICLR 2024 ([arXiv:2306.09896](https://arxiv.org/abs/2306.09896)) | Diagnostics should say what to write, not only what token was expected. |
| Models generating a very-low-resource language drift into a familiar "parent" language; deterministically repairing parent-language output into the target language recovers much of the gap. | Mora et al., *Synthetic Programming Elicitation (SPEAC)*, NeurIPS 2024 ([arXiv:2406.03636](https://arxiv.org/abs/2406.03636)) | Detect foreign syntax and offer the TC reading of it, checked by the compiler. |
| Putting the DSL's BNF in the prompt improves DSL generation over examples alone. | Wang et al., *Grammar Prompting for Domain-Specific Language Generation with LLMs*, NeurIPS 2023 ([arXiv:2305.19234](https://arxiv.org/abs/2305.19234)) | `tsr grammar` emits the accepted grammar; the harness puts it in the prompt. |
| Grammar-constrained decoding removes syntax errors outright for models served by engines that accept a grammar. | Ugare et al., *SynCode* ([arXiv:2403.01632](https://arxiv.org/abs/2403.01632)); Dong et al., *XGrammar* ([arXiv:2411.15100](https://arxiv.org/abs/2411.15100)) | `tsr grammar --format=gbnf|lark` for llama.cpp / vLLM-style backends; canonical spelling only. |
| Execution/compiler feedback paired with the attempt that produced it drives self-debugging. | Chen et al., *Teaching Large Language Models to Self-Debug*, ICLR 2024 ([arXiv:2304.05128](https://arxiv.org/abs/2304.05128)) | Show each rejected attempt with its own diagnostics and failing cases. |
| A compiler-in-the-loop with concise error rendering fixes most Rust compile errors. | Deligiannis et al., *RustAssistant*, ICSE 2025 ([arXiv:2308.05177](https://arxiv.org/abs/2308.05177)) | Render the first errors with source line, caret and help, not raw JSON. |
| Agent performance depends heavily on the agent-computer interface: concise, actionable tool feedback (e.g. a linter on every edit) beats verbose output. | Yang et al., *SWE-agent*, NeurIPS 2024 ([arXiv:2405.15793](https://arxiv.org/abs/2405.15793)) | Cap and de-cascade diagnostics shown to the model. |

## Changes made in this pass

Tessera:

1. `tsr witness` diagnostics carry `help` and machine-applicable `fixes`, an
   `E-syntax-foreign` diagnostic summarizes foreign syntax first, and the
   document lists whole-file `suggestions` that already pass `tsr check`
   (see `docs/spec/witness.md`). Suggestions are compiler-checked, never
   behaviour-checked: the caller's tests decide, as with any candidate.
2. `tsr check` prints the same `help:` lines and suggestions.
3. `tsr grammar [--format=ebnf|gbnf|lark]` prints the grammar the parser
   accepts.

Lattice (`packages/tessera`): try verified suggestions before spending model
tokens, show each rejected attempt with its own rendered diagnostics, and put
`tsr grammar` in the repair prompt.

## Not done, and why

- Constrained decoding in Lattice: the hosted providers in use (OpenRouter,
  Anthropic) take JSON schemas, not CFGs, and the candidate is a JSON string
  field. The grammar is published for backends that can use it.
- Supporting parent-language syntax in TC itself: the canonical-form
  invariant (one spelling per construct) stays; foreign input is read only to
  *suggest* TC.
- pass@k: each repair run is already multi-round with a fixed seed; the
  comparison reports solved/runs per arm, which is pass@1 over seeds.
