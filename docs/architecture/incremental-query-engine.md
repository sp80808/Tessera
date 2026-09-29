# Incremental query engine architecture

Status: implementation direction

## Thesis

Tessera should have one incremental semantic dependency system that feeds:
- compilation;
- IDE queries;
- TIR;
- TCap;
- TCG;
- model transport/context packing.

Do not maintain a compiler graph and an unrelated agent graph if the compiler can provide the authoritative dependencies.

## Reference architecture

Salsa and rustc's query model demonstrate the core pattern:
- deterministic queries;
- explicit input reads;
- memoization;
- dependency tracking;
- recompute only when an observed dependency changes.

For the prototype, evaluate Salsa directly before building custom infrastructure.

## Inputs

Inputs are mutable only from the outer driver:

```
SourceFile { path, text }
Manifest { ... }
Lockfile { ... }
Target { triple, features }
CompilerOptions { ... }
Evidence { id, revision, content_hash, trust, payload }
Task { id, objective, editable_scope, budget }
ModelProfile { tokenizer, transport_profile }
```

Network access is never performed from a tracked semantic query. A research adapter updates an `Evidence` input after retrieval/verification.

## Query DAG

Suggested first slice:

```
parse(file)
lower_ast(file)
module_index(module)
resolve(symbol)
infer_type(symbol)
infer_effects(symbol)
ownership(symbol)
capability_graph(symbol)
tir(symbol)

symbol_edges(symbol)
test_edges(symbol)
build_edges(package)
context_slice(task)
context_projection(task, view)
transport_packet(task, model)
```

## Change firewall

Prefer fine-grained projections over monolithic "whole repo" results.

Example:
- parsing change in file A invalidates A;
- exported signature change invalidates dependents;
- function body-only change should not invalidate unrelated callers' type results if signature/effects remain stable;
- evidence update invalidates only context queries that consumed that evidence;
- context packet change must never invalidate native codegen unless an explicit compile-time input changed.

## Stable IDs

Need stable identities for:
- files;
- modules;
- symbols;
- types;
- TIR operations;
- context/evidence nodes.

Use interning for repeated semantic values and stable path-based identities across sessions where possible.

## Diagnostics

Diagnostics are side outputs of relevant semantic queries, not global mutable logs.

Desired query:

```
diagnostics(file)
diagnostics(symbol)
```

Each diagnostic carries:
- compact message;
- TC spans;
- TIR node;
- TCap trace if relevant;
- context/evidence references only when they materially explain the failure.

## Lossless parsing

Tessera's near-unreadable canonical syntax makes source mapping critical.

The parser layer must preserve:
- every token/trivia span;
- malformed/incomplete source during editing;
- deterministic recovery;
- mapping from CST -> semantic nodes.

Evaluate:
1. Rowan-style immutable green tree + cheap red views;
2. custom lossless CST;
3. Tree-sitter primarily as an editor grammar, not necessarily the compiler parser.

Decision should be benchmark-driven.

## Persistence

V0 can keep the query DB in-process.

Later persistence may cache:
- parse trees;
- TIR;
- stable fingerprints;
- token counts;
- graph projections.

Cache keys must include compiler/schema/profile versions.

## Concurrency

Parallelize independent queries, but require deterministic outputs.

External research, model inference and nondeterministic tools live outside the semantic query DB and return versioned inputs.

## First implementation milestone

A minimal Rust workspace:

```
crates/
  tessera-db
  tessera-syntax
  tessera-tir
  tessera-context
  tessera-cli
```

Demonstration:
1. load two files;
2. parse/lower both;
3. query TIR;
4. edit one function body;
5. show query logs proving unrelated TIR queries were reused;
6. derive/update one TCG edge;
7. show only its affected context packet was rebuilt.

That end-to-end witness is more valuable than implementing many language constructs before the architecture is validated.
