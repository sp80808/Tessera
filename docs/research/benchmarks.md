# Tessera benchmark methodology

Tessera's claims must be measurable. "Looks concise" and "feels fast" are not acceptance criteria.

## Benchmark dimensions

### A. Source/model token efficiency

Per implementation:
- UTF-8 bytes;
- non-whitespace chars;
- compiler lexical tokens;
- AST nodes;
- TIR nodes;
- tokens under every supported LLM tokenizer;
- median and p90 token count across tokenizers.

Compare against Rust, Zig, C, C++ and Odin.

### B. Model coding quality

For each supported model:
- generate from specification;
- translate from Rust/C/Zig to Tessera;
- repair compiler errors;
- patch repository task;
- explain TC via TIR;
- round-trip TC -> TIR -> TC.

Measure:
- compile@1;
- test pass@1;
- repair turns;
- input tokens;
- output tokens;
- wall-clock inference if available;
- semantic regression rate.

### C. Representation robustness

Inspired by TokDrift, apply semantic-preserving transformations to non-canonical inputs:
- whitespace changes;
- identifier renaming;
- equivalent parenthesization;
- import order;
- formatting.

The canonical formatter should collapse these to one stable representation.

Measure model-output divergence before and after canonicalization.

### D. Runtime

Use representative kernels:
- integer parsing;
- hash table;
- arena allocator;
- file IO;
- TCP echo;
- matrix multiply;
- audio/DSP loop;
- SPSC queue;
- JSON tokenizer/parser;
- generic collection map/filter;
- enum state machine;
- FFI call.

Metrics:
- throughput;
- latency;
- allocations;
- peak RSS;
- binary size;
- startup time.

### E. Compiler performance

- cold compile;
- incremental compile;
- peak memory;
- lines/tokens per second;
- graph update time;
- TIR expansion time.

### F. Context harness

Tasks:
- locate relevant symbol;
- identify correct API version;
- diagnose known failure;
- repo-level bug repair;
- add feature spanning multiple modules;
- research unknown dependency behavior.

Compare retrieval modes:
1. full repository dump;
2. vector retrieval;
3. structural graph;
4. graph + context tiles;
5. graph + tiles + budgeted compression.

Metrics:
- task success;
- context precision/recall where labeled;
- prompt tokens;
- stale-fact rate;
- incorrect external-API use;
- graph build/update time;
- verification cost.

## Token budgets

Every harness benchmark should run multiple budgets, e.g.:
- 512;
- 1k;
- 2k;
- 4k;
- 8k tokens.

The goal is to learn a **quality/token frontier**, not merely maximize task score.

## Reproducibility artifact

Each experiment records:

```
experiment_id
repo_revision
compiler_revision
model
model_revision
tokenizer
prompt_template_hash
context_packet_hash
target
benchmark
seed
results
```

## Promotion gate for language syntax

A syntax change must not enter stable TC solely because it saves characters.

Require:
1. median token improvement across tokenizer suite;
2. no statistically meaningful compile/test-generation regression on the benchmark corpus;
3. parser remains deterministic;
4. TIR expansion remains lossless;
5. diagnostics do not become materially more expensive;
6. formatter canonicalization remains unique.

## Promotion gate for context features

A context tile or graph feature must show one of:
- improved task success at equal token budget;
- fewer tokens at equal task success;
- fewer stale/hallucinated API uses;
- better fault localization;
- better reproducibility/auditability.

Otherwise it stays experimental.
