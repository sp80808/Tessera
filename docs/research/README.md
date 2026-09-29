# Tessera Core Research

Status: **living research baseline**  
Last reviewed: **2026-09-29**

Tessera is an experimental systems programming language optimized for **LLM token efficiency, deterministic recoverability, and low-level native performance**. Human readability is secondary; debuggability and lossless intent translation are mandatory.

## Core architecture

Tessera separates concerns that conventional languages force into one representation:

| Layer | Purpose |
|---|---|
| **TC — Tessera Compact** | canonical, aggressively token-dense executable source |
| **TIR — Tessera Intent IR** | deterministic semantic expansion for debugging, review, translation and verification |
| **TCG — Tessera Context Graph** | repository/build/operational/knowledge graph used by harnesses |
| **CTX / Context Lattice** | typed, scoped and condition-aware metadata attached to code/project entities |
| **Verification Ledger** | provenance, freshness, confidence and invalidation state for contextual claims |
| **TMT — Tessera Model Transport** | reversible model/task-specific packing derived from TC/TIR |
| **TCap — Capability Graph** | flow-sensitive ownership/borrow capability view derived from TIR |

The executable meaning of ordinary Tessera code must never require an LLM or live network lookup.

## Non-negotiable design constraints

1. Optimize measured **model tokens**, not visual terseness.
2. Benchmark syntax across multiple model tokenizer families; never optimize around one vocabulary.
3. TC must have one deterministic canonical spelling for each construct.
4. Every omitted/inferred semantic detail must be recoverable in TIR.
5. Safe code targets affine ownership/borrowing without mandatory tracing GC.
6. Unsafe operations remain explicit and carry inspectable obligations.
7. Runtime semantics and agent/research context remain separate unless a compile-time context kind is explicitly semantic.
8. External context is untrusted until verified and must retain provenance.
9. Harnesses retrieve a task-relevant graph projection rather than dumping the repository.
10. Syntax/semantics are promoted only after tokenizer, model-success, compiler and runtime experiments.

## Research dossier

### Language / compiler
- [Token efficiency and tokenizer-aware syntax](token-efficiency.md)
- [Semantic core and memory-safety direction](semantic-core.md)
- [Compiler and runtime architecture](compiler-architecture.md)
- [Benchmark methodology](benchmarks.md)
- [Incremental semantic query-engine architecture](../architecture/incremental-query-engine.md)
- [Model transport proposal](../spec/model-transport.md)
- [Ownership capability graph proposal](../spec/ownership-capability-graph.md)
- [Research pass 2 synthesis](2026-09-29-pass2.md)

### Context-native agent architecture
- [Context Graph and autonomous harness](context-graph-harness.md)
- [Context Lattice: typed layered context](context-lattice.md)
- [Context tiles — provisional language design](../spec/context-tiles.md)

### Evidence
- [Evidence ledger and related work](references.md)

### Agent instructions
- [Tessera agent skill](../../skills/tessera/SKILL.md)
- [Repository agent rules](../../AGENTS.md)

## Strong findings guiding v0 research

- Code-aware tokenization can materially reduce source sequence length. CodeBPE reports about **17% shorter sequences without downstream performance loss** for punctuation grouping in the evaluated setup.
- Compiler-aware lexical compression is a serious precedent: Li & Lu report **33.7% fewer input tokens** with compiler tokenization and contextual restoration in their experiment.
- Tokenization is behaviorally relevant, not just a billing detail. TOKDRIFT reports prediction changes under semantics-preserving rewrites, motivating canonical source and grammar/token alignment measurement.
- Repository/language imbalance can make code-tokenizer vocabularies source-specific and under-trained; Tessera therefore needs a diverse corpus and tokenizer portfolio rather than a Tessera-only tokenizer score.
- Repository graphs can improve code-agent navigation, but retrieval is only half the problem: packing/compression and the number of disconnected facts a model must integrate also matter.
- Real-world coding-agent context files are growing organically and inconsistently. Tessera's response is typed, scoped, conditional context that can render into conventional agent files when needed.
- Independent AI-native language projects such as **toke** and **ION** validate that token-efficient language design is now an active engineering niche. Their self-reported results are useful precedents, not evidence for Tessera's own claims.

## The Context Lattice hypothesis

A small source snippet may expose several orthogonal views without embedding all expanded text:

- execution semantics;
- ownership/effects;
- exact tech-stack/version conditions;
- architectural intent/invariants;
- tests and verification evidence;
- prior failure -> fix knowledge;
- trusted agent workflow;
- quarantined research hypotheses.

The harness computes a task/environment-specific projection and expands only the required layers. This makes token efficiency a **language + retrieval + context-packing** problem.

## Required toolchain views

Planned deterministic tools:

```
tsr fmt
tsr check
tsr build
tsr tir <symbol|file>
tsr explain <symbol|span>
tsr tokens [--models ...]
tsr ctx build
tsr ctx query "<task>" --budget N
tsr ctx verify [--stale]
tsr ctx research "<gap>" --github --hf --papers
tsr ctx pack "<task>" --model <family>
```

These are design targets until implemented.

## Research workflow

```
local source/spec
 -> deterministic TIR + graph
 -> detect missing fact
 -> retrieve local evidence
 -> external research only for the gap
 -> quarantine claim
 -> verify against current project/source/test
 -> promote or reject
 -> update graph/ledger
 -> benchmark
```

## Immediate experiments

1. Build a tokenizer-suite harness and equivalent Rust/C/Zig/Odin/Tessera micro-corpus.
2. Define a tiny TC grammar and lossless TIR expansion/canonicalization round trip.
3. Prototype compiler-derived symbol/call/ownership graph export.
4. Implement CTX/context-tile parser with conditions, scope, provenance and freshness.
5. Build graph-packet retrieval under explicit token budgets.
6. Lower a safe-core subset through Cranelift and compare generated runtime/compile performance.
7. Evaluate TC-only vs TC+TIR vs TC+TCG packets on generation and debugging tasks.
8. Prototype TMT symbol/type interning and reversible pattern sugar separately from TC grammar.
9. Use one incremental query database for compiler results and context invalidation.
10. Prototype TCap graph-native ownership diagnostics.
11. Benchmark structure-first/active context against static AGENTS files and flat retrieval.
12. Test optional grammar-constrained TC generation before attempting semantic constrained decoding.

Syntax examples remain provisional until those experiments produce evidence.
