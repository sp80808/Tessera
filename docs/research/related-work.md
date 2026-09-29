# Related Work and Precedents

This is a research map, not an endorsement or dependency list.

## Token-efficient / terse language precedent

### toke

The Hugging Face project [karwalski/toke-tokenizer](https://huggingface.co/karwalski/toke-tokenizer) reports a purpose-built tokenizer for a terse programming language and publishes token-reduction measurements. Tessera should replicate the good practice—public tokenizer artifact + benchmark corpus—but independently validate methodology and include multiple third-party tokenizers.

### ION

GitHub research surfaced [robertkarlsson2-design/ION](https://github.com/robertkarlsson2-design/ION) for LLM/token-efficient language ideas. It should be inspected for syntax, compiler strategy and benchmarking before borrowing any design.

## Systems languages

### Rust

Primary precedent for affine ownership, borrowing, explicit unsafe boundaries, deterministic resource management and native performance.

### Zig

Relevant for explicit allocators, simple low-level semantics, comptime and C interop.

### Odin

Relevant for a relatively direct systems-language surface, data-oriented programming and predictable native compilation.

### C

Baseline for density, ABI interoperability, compiler maturity and the cost of manual memory safety.

## Compiler backends

### Cranelift
Prototype-first backend candidate: Rust-native, relatively approachable and designed for fast code generation.

### LLVM
Production optimisation and target-coverage reference backend.

### QBE
Minimal backend and complexity reference.

## Code tokenizer research

- [CodeBPE](https://arxiv.org/abs/2308.00683): code-specific subtokenization, punctuation grouping and sequence length.
- [Compiler-tokenized compressed input](https://doi.org/10.3390/info16020073): lexical compression + restoration dictionary.
- [TOKDRIFT](https://aclanthology.org/2026.acl-long.2199/): robustness problems from tokenization differences under semantics-preserving rewrites.
- [Source-Attributed BPE](https://aclanthology.org/2026.acl-long.1812/): repository/source imbalance and under-trained tokens.
- [Secret leakage from a tokenization perspective](https://aclanthology.org/2026.findings-acl.6/): token-level entropy can differ sharply from character-level intuition.

## Repository context / graph agents

### CoSIL

[Paper](https://arxiv.org/abs/2503.22424) | [GitHub](https://github.com/ParsifalXu/CoSIL)

Graph-guided iterative issue localisation with context pruning. Particularly relevant to Tessera's query-driven Harness Graph.

### Other GitHub projects discovered

- [r3tr0-afk/repo-graph-rag-agent](https://github.com/r3tr0-afk/repo-graph-rag-agent)
- [mikekonan/cograph](https://github.com/mikekonan/cograph)
- [artk0de/TeaRAGs-MCP](https://github.com/artk0de/TeaRAGs-MCP)
- [foyzulkarim/hikma-engine](https://github.com/foyzulkarim/hikma-engine)

Research questions for each:
- graph generated statically or by LLM?
- node/edge schema?
- incremental update?
- source provenance?
- context-budget policy?
- exact-code retrieval versus summaries?
- trust model for model-generated knowledge?

## Agent context files

[Context Engineering for AI Agents in Open-Source Software](https://arxiv.org/abs/2510.21413) studies project-level configuration/context files used by coding agents and reports considerable variation in structure/content. Tessera's Context Lattice is intended as a lower-level typed substrate that can render to AGENTS.md or tool-specific instructions rather than replacing ecosystem compatibility.

## Long-context coding models

The Hugging Face model card for [Qwen3-Coder-30B-A3B-Instruct](https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct) advertises 256K native context and agentic tool-calling support. Long context is useful, but Tessera should not treat it as a substitute for retrieval: repository context still contains stale, irrelevant and contradictory information, and tokens still have latency/cost implications.

## Distinguishing Tessera

Tessera's research direction is the intersection of:
- systems-language semantics;
- deliberately token-dense canonical syntax;
- lossless intent expansion;
- typed contextual metadata;
- compiler-derived repository graphs;
- context-budgeted coding harnesses;
- continuous evidence/freshness verification.

The combination, rather than any single mechanism, is the project hypothesis.
