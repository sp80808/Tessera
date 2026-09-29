# Benchmark and Experimental Plan

Tessera should not claim token efficiency, model comprehensibility, safety or Rust-class performance until these are measured separately.

## Benchmark axes

### A. Source density

For each program:
- bytes;
- Unicode scalar count;
- lexer token count;
- AST/TIR node count;
- model-token count for each tokenizer;
- compressed context-layer token count.

Compare canonical Tessera against idiomatic and minified variants of Rust/C/Zig where useful.

### B. Model generation

For multiple model families:
- compile@1;
- test-pass@1;
- repair success;
- semantic-equivalence success;
- number of retry turns;
- input/output tokens;
- cost where measurable.

Prompt forms:
- TC only;
- TIR only;
- TC + selected CTX;
- TC + graph retrieval;
- conventional baseline language.

### C. Debugging

Inject controlled defects:
- borrow violation;
- off-by-one;
- invalid layout/FFI;
- data race;
- stale dependency assumption;
- incorrect platform branch;
- performance regression.

Measure whether:
- TC alone is enough;
- `tess why` expansion helps;
- graph-selected CTX helps;
- full-repo context hurts or helps.

### D. Native performance

Use:
- Criterion-style microbenchmarks;
- realistic kernels;
- wall time;
- throughput;
- allocations;
- peak RSS;
- binary size;
- compile time.

Compare with Rust/C/Zig implementations built with documented optimisation flags.

### E. Compiler correctness

- parser fuzzing;
- round-trip property tests;
- differential codegen;
- sanitiser/checker builds;
- random well-typed program generation;
- CTX invalidation tests.

## Token portfolio

Do not publish a single “X% fewer tokens” number without specifying tokenizer.

Minimum dashboard:
- OpenAI/tiktoken representative;
- Qwen3-Coder;
- Llama-family;
- DeepSeek-family;
- StarCoder/code family;
- Tessera-native tokenizer experiment.

Report median, min, max and per-tokenizer values.

## Initial programs

Tier 0:
- hello/exit code;
- fib;
- sum;
- structs/enums;
- error result;
- slices;
- allocator call;
- file read.

Tier 1:
- JSON subset parser;
- arena allocator;
- thread pool;
- TCP echo server;
- WAV reader;
- SIMD dot product;
- ring buffer;
- hash table.

Tier 2:
- small CLI;
- HTTP service;
- audio DSP chain;
- embedded-style no-alloc state machine;
- multi-module compiler/interpreter component.

## Context benchmarks

Construct tasks where the needed fact is:
1. in the same file;
2. in a direct dependency;
3. in a test;
4. in an issue/ADR;
5. conditioned on platform/version;
6. stale and contradicted by current code.

Compare retrieval strategies:
- full repo;
- embeddings only;
- lexical search;
- static graph;
- static graph + CTX;
- graph + CTX + model expansion.

## Reproducibility

Every result stores:
- Tessera commit SHA;
- benchmark corpus SHA;
- compiler flags;
- target CPU/OS;
- model ID and provider;
- tokenizer artifact/version;
- temperature/sampling;
- prompt template SHA;
- dependency lockfiles;
- raw result artifact.

## Success criteria for v0.1

Provisional, to be revised after baseline data:
- compiler executes Tier 0 on at least x86_64 Linux/macOS;
- TC/TIR round trip is lossless;
- safe-core ownership checks reject curated unsound examples;
- token count beats idiomatic Rust on >=80% of Tier 0 programs under the median tokenizer;
- no benchmark may regress generated native runtime >20% versus equivalent unoptimised design solely to save source tokens;
- CTX can be stripped without changing runtime semantics for non-semantic context kinds.

## Anti-gaming rules

- Do not rename baseline variables absurdly just to favour Tessera.
- Publish both idiomatic and minified baselines.
- Do not count comments/docs in one language and omit them from another.
- Use equivalent safety/error handling.
- Separate native-tokenizer results from third-party tokenizer results.
- Record failures, not only successful generations.
