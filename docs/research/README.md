# Tessera — core research dossier

Tessera is an experimental systems programming language designed around two unusual priorities:

1. **Native systems performance and explicit control** comparable in ambition to Rust/C/Zig-class workloads.
2. **LLM token efficiency and agent comprehension as first-class language metrics**, even when this makes canonical source difficult for humans to read.

Human readability is secondary to raw performance and model-token efficiency. **Debuggability, reversibility and intent recovery are not secondary.** Tessera therefore separates the executable language from the representations used to explain it.

## The central hypothesis

A programming language can be aggressively compressed for machine generation and reasoning if:

- the grammar is deterministic;
- the compiler produces a lossless explicit semantic view;
- one canonical compact spelling minimizes superficial variation;
- ownership, effects and unsafe behavior remain mechanically visible;
- contextual project knowledge is encoded as typed metadata rather than free-form comments;
- an agent harness retrieves only task-relevant context under a token budget;
- every external fact can carry provenance, freshness and verification status.

## Three core representations

### 1. Tessera Compact (TC)

Canonical source. Optimized for low model-token count, parser simplicity and low syntactic entropy.

TC is allowed to be visually dense and near-unreadable without tooling.

### 2. Tessera Intent IR (TIR)

Lossless semantic expansion for humans and models. TIR exposes:

- resolved names and inferred types;
- moves, borrows, regions and drops;
- implicit conversions and generic resolution;
- desugared control flow;
- unsafe obligations;
- source-to-expansion mappings;
- compile-time configuration decisions.

Invariant target:

```
canonicalize(TIR_to_TC(TC_to_TIR(x))) == canonicalize(x)
```

### 3. Tessera Context Graph (TCG)

A queryable graph of project, code, stack, evidence and troubleshooting context. Some graph facts are compiler-derived; others come from explicit context tiles, manifests, git history, tests, external documentation and research.

TCG is **not** runtime state. It exists to help tools and agents obtain the right context without flooding prompts.

## Research conclusions so far

### Tokenization

Recent code-model research makes it unsafe to equate character count with LLM efficiency. Code-aware subtokenization can reduce sequence length, while semantically equivalent formatting changes can change model behavior. Tessera must therefore benchmark **real model tokenizers**, not invent a character-density proxy.

### Repository context

Repository-level coding work increasingly benefits from structural or graph retrieval. Recent systems such as CodexGraph and RepoGraph represent repository relations explicitly. 2026 work such as RepoDistill goes further: retrieval alone is not enough; retrieved context itself should be selectively compressed under a learned or heuristic token budget.

### Context as a language primitive

Tessera will explore a compact, typed metadata syntax called **context tiles**. Tiles can attach project intent, tech-stack constraints, evidence references, failure history and agent hints to modules or symbols. Tiles are:
- conditional;
- layered;
- provenance-aware;
- independently refreshable;
- removable from production artifacts;
- excluded from runtime semantics unless explicitly used for compile-time configuration.

### Safety

The working semantic direction is an affine ownership model with explicit shared/exclusive borrows and explicit unsafe capabilities. We should borrow the *semantic strengths* of Rust without assuming Rust surface syntax is optimal for Tessera.

## Research documents

- [Token efficiency and tokenizer-aware syntax](token-efficiency.md)
- [Context graph and autonomous harness](context-graph-harness.md)
- [Embedded context tiles](../spec/context-tiles.md)
- [Semantic core and memory model](semantic-core.md)
- [Compiler and backend architecture](compiler-architecture.md)
- [Benchmark methodology](benchmarks.md)
- [Evidence ledger and related work](references.md)

## Non-goals

Tessera is not:
- a code-golf language;
- a natural-language programming language;
- a prompt format masquerading as a language;
- a tracing-GC-first application language;
- a syntax that silently downloads knowledge and changes runtime semantics;
- optimized for one proprietary tokenizer.

## Primary research questions

1. Can TC materially beat Rust, Zig, C and C++ on median model-token count across tokenizer families?
2. Does a canonical dense syntax improve or reduce code-generation correctness?
3. How much context can TCG omit while preserving repo-level task performance?
4. Can TIR make a near-unreadable TC program easier to debug than ordinary source?
5. Can context tiles reduce hallucinated API use and stale-stack mistakes without bloating prompts?
6. What is the correct trust model for self-updating context and research?
7. Can ownership semantics be expressed with fewer surface tokens than Rust while retaining equivalent safety properties?
8. Which backend gives the best bootstrap trade-off: Cranelift first, LLVM first, or dual backend?
