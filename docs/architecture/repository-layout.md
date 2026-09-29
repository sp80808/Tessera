# Repository layout

This document defines the intended project shape so early implementation work does not create incompatible parallel structures.

## Current

```
/
├── .github/
├── docs/
│   ├── architecture/
│   ├── research/
│   ├── rfcs/
│   └── spec/
├── skills/
│   └── tessera/
├── AGENTS.md
├── CONTRIBUTING.md
├── README.md
└── ROADMAP.md
```

## Compiler bootstrap target

Issue #14 should grow the repository toward:

```
/
├── crates/
│   ├── tessera-db/
│   ├── tessera-syntax/
│   ├── tessera-tir/
│   ├── tessera-sema/
│   ├── tessera-context/
│   └── tessera-cli/
├── tests/
│   ├── syntax/
│   ├── semantics/
│   ├── roundtrip/
│   └── integration/
├── benchmarks/
│   ├── corpus/
│   ├── tokenizers/
│   ├── runtime/
│   └── context/
└── examples/
```

Later:
- `tessera-codegen-cranelift`;
- optional LLVM backend;
- LSP/editor crates;
- model adapters outside compiler-semantic crates.

## Dependency boundaries

### tessera-syntax
Owns:
- lexical grammar;
- lossless CST;
- parser recovery;
- canonical formatting primitives;
- source spans.

Does not own type semantics.

### tessera-db
Owns:
- incremental inputs;
- query orchestration;
- stable/interned identities.

Must not perform web/model I/O.

### tessera-tir
Owns explicit semantic representation and serialization/versioning experiments.

### tessera-sema
Owns:
- name resolution;
- inference/checking;
- effects;
- ownership/borrow rules;
- TCap derivation.

### tessera-context
Owns:
- context tiles;
- TCG;
- evidence records;
- task projections;
- context packing interfaces.

It may consume compiler facts but may not redefine them.

### tessera-cli
Thin command surface. Avoid putting semantics here.

## External integrations

Research adapters, LLM clients and model/tokenizer-specific code should eventually live in clearly non-semantic crates/tools, for example:

```
tools/
  tessera-research/
  tessera-model-adapters/
```

This preserves a compiler that builds and checks code completely offline.

## Artifact policy

Generated artifacts:
- `.tessera/cache/`
- TMT packets;
- local TCG snapshots;
- benchmark output;
- build products.

Committed fixtures are permitted when required for reproducible tests, but generated output should not become source of truth.
