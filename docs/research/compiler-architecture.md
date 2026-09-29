# Compiler and backend architecture

## Bootstrap language

Implement the reference compiler in Rust initially.

Reasons:
- mature parser/compiler ecosystem;
- memory safety for compiler implementation;
- natural interop with Cranelift;
- good LLVM bindings/ecosystem options;
- eventual self-hosting can remain a long-term milestone rather than a bootstrap constraint.

## Proposed pipeline

```
TC source
  -> lexer/parser
  -> compact AST
  -> name resolution
  -> semantic graph
  -> ownership/effect checking
  -> typed HIR
  -> TIR emitter
  -> MIR/SSA lowering
  -> backend
       -> Cranelift
       -> LLVM (later/parallel)
  -> object/link
```

The Tessera Context Graph is fed from semantic stages but is not on the critical code-generation path.

## Frontend design

### Parser

Requirements:
- deterministic grammar;
- excellent recovery despite dense syntax;
- incremental parsing support;
- stable node IDs for graph updates;
- exact source spans.

Tree-sitter is useful prior art for incremental tooling, but the reference compiler should not be forced to use it internally if a hand-written parser is simpler/faster.

### Semantic graph

Use persistent symbol IDs rather than human names as internal identity. This naturally supports:
- compact local-name experiments;
- rename-stable graph edges;
- TIR expansion;
- language server references;
- context graph integration.

### HIR/TIR split

TIR is explanatory and stable.

HIR/MIR are compiler implementation details and may change more freely.

Do not make the debugging format identical to the optimization IR.

## Backend strategy

### Cranelift first

Cranelift describes itself as a fast, secure, relatively simple code generator usable for JIT and AOT compilation. Its Rust ecosystem and object emission make it attractive for a v0 compiler.

Advantages:
- fast compile times;
- Rust-native integration;
- simpler codebase than LLVM;
- native object emission;
- JIT path useful for REPL/tests;
- WebAssembly ecosystem adjacency.

Reference: https://cranelift.dev/

### LLVM optimization backend

LLVM remains attractive when peak optimization quality, platform support or advanced optimization passes matter.

Reference: https://llvm.org/docs/LangRef.html

Recommended architecture: define backend-neutral MIR so LLVM can be added without changing Tessera source semantics.

### QBE experiment

QBE intentionally targets a smaller compiler-backend surface and describes its goal as achieving a large fraction of industrial compiler performance with much less implementation code.

Reference: https://c9x.me/compile/

Use it as an architectural comparison, not necessarily the production backend.

## Debug info

Because TC may be hard to read, debug mappings are unusually important.

Every generated instruction should be traceable through:
- native debug location;
- MIR node;
- TIR semantic node;
- TC source span;
- expanded context/ownership explanation when requested.

## Diagnostics

A diagnostic should have two forms:

### Dense

```
E31 f:12 x^ after &x@9
```

Optimized for agent loops.

### Expanded

```
error E31: value x is moved while a shared loan is still live
move: line 12
loan created: line 9
loan required until: line 14
suggestions:
  - shorten the shared loan
  - move before creating the loan
```

Agents can request dense mode; humans can request expanded mode.

## Build determinism

External context cannot affect a reproducible build unless explicitly promoted into a pinned compile configuration.

A build manifest records:
- compiler version;
- target;
- features;
- dependency lock;
- build-time context inputs that are semantically active.

Research and troubleshooting metadata are never implicit build inputs.

## Initial compiler milestones

1. lexer/parser + canonical formatter;
2. primitive types/functions/control flow;
3. TIR lossless expansion;
4. ownership checker for locals;
5. structs/enums/patterns;
6. Cranelift object emission;
7. C FFI;
8. generics/traits or compact interface mechanism;
9. context-tile parser + TCG export;
10. incremental graph/harness;
11. LLVM backend experiment;
12. self-hosting feasibility study.
