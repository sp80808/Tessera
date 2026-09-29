# Harness Graph and Autonomous Context Acquisition

## Objective

A Tessera-aware coding harness should be able to enter an unfamiliar repository and construct a useful knowledge graph without loading the repository wholesale into an LLM.

The graph is an **index of inspectable evidence**, not a replacement for source code.

## Graph node families

### Code
- workspace/package/module/file
- symbol/function/type/trait
- call site
- field/global/resource
- unsafe region
- FFI boundary
- generated artifact

### Build/runtime
- target
- feature
- dependency/version
- environment variable
- command
- platform
- deployment/runtime

### Quality
- test
- benchmark
- lint/static-analysis rule
- failure signature
- regression
- invariant

### Work
- issue
- PR
- commit
- ADR/design decision
- TODO
- migration

### Knowledge
- CTX fact
- external documentation
- paper
- API contract
- troubleshooting recipe
- agent instruction

## Edge families

Examples:

```
CALLS
IMPORTS
IMPLEMENTS
OWNS
BORROWS
MUTATES
ALLOCATES
READS
WRITES
GUARDED_BY
TESTED_BY
BENCHMARKED_BY
FAILS_WHEN
FIXED_BY
DEPENDS_ON
VALID_FOR
SUPERSEDES
DERIVED_FROM
EVIDENCED_BY
CONTRADICTS
GENERATED_FROM
MENTIONED_IN
```

Edges carry provenance and freshness.

## Automatic construction

### Pass A: deterministic
Use parser/compiler data first:
- AST/TIR;
- module graph;
- type graph;
- call graph where statically recoverable;
- ownership/effect summaries;
- source maps;
- Cargo/package/lock/build data;
- tests and benchmarks;
- Git history links.

### Pass B: repository evidence
Extract:
- issue/PR/commit references;
- config and CI commands;
- ADRs/docs;
- generated-file declarations;
- error strings and troubleshooting docs.

### Pass C: model-assisted hypotheses
Use an LLM for information that is expensive to infer mechanically:
- likely architectural roles;
- semantic relationships between distant modules;
- failure-mode clustering;
- candidate authoritative files;
- missing documentation.

Model-generated edges start as `inferred` and require evidence before promotion.

## Query-driven graph expansion

Do not fully enrich every node.

A task such as “fix device reopen on Windows” begins with:
1. lexical/symbol match;
2. direct graph neighbourhood;
3. applicable CTX predicates;
4. failing tests/issues;
5. call/data-flow expansion;
6. external research only if an unresolved fact remains.

This resembles graph-guided issue localisation rather than blind repository dumping.

CoSIL is an important precedent: it iteratively searches a function call graph and prunes context to manage LLM context limits. Tessera can improve on this by obtaining a high-quality graph directly from its own compiler plus CTX.

Reference: [CoSIL](https://arxiv.org/abs/2503.22424), implementation: [ParsifalXu/CoSIL](https://github.com/ParsifalXu/CoSIL).

## GitHub precedents worth tracking

These are inspiration/research targets, not dependencies:

- [ParsifalXu/CoSIL](https://github.com/ParsifalXu/CoSIL) — graph-guided software issue localisation.
- [r3tr0-afk/repo-graph-rag-agent](https://github.com/r3tr0-afk/repo-graph-rag-agent) — repository graph + retrieval agent prototype.
- [mikekonan/cograph](https://github.com/mikekonan/cograph) — code-oriented graph project discovered during repo research.
- [artk0de/TeaRAGs-MCP](https://github.com/artk0de/TeaRAGs-MCP) — graph/RAG/MCP-oriented repository context.
- [foyzulkarim/hikma-engine](https://github.com/foyzulkarim/hikma-engine) — code-knowledge retrieval/agent precedent.

Each should be evaluated for graph schema, incremental indexing, retrieval policy, provenance, language support, latency and failure modes before adopting ideas.

## Harness workflow

```
scan
 -> parse
 -> graph
 -> verify deterministic facts
 -> detect unknowns
 -> retrieve local evidence
 -> retrieve external evidence if needed
 -> propose inferred graph additions
 -> run tests/checks
 -> promote verified context
 -> update graph + ledger
```

## Self-updating context

A harness may update CTX/HG when:
- dependency versions change;
- source hashes invalidate an observation;
- a test begins failing;
- an issue/PR supersedes a workaround;
- documentation evidence has exceeded its TTL;
- a platform condition changes.

Updates should be explicit diffs, not silent memory.

## Context acquisition budget

Every task receives a context budget.

Candidate graph nodes are ranked by:
- direct symbol relation;
- control/data dependency;
- matching CTX condition;
- test/failure relation;
- evidence authority;
- freshness;
- semantic retrieval score;
- graph distance;
- token cost.

A simple initial utility function:

```
utility(node) =
  relevance * authority * freshness * verification
  / (1 + token_cost)
```

Later work can learn the coefficients from task success data.

## Knowledge graph portability

The graph should export to a simple stable format (JSONL initially) rather than forcing Neo4j or another server.

Suggested records:

```json
{"n":"fn:audio::mix","k":"function","src":"src/audio.ts:31-58"}
{"n":"inv:buf.same_len","k":"invariant","state":"verified"}
{"a":"fn:audio::mix","r":"GUARDED_BY","b":"inv:buf.same_len"}
```

This is intentionally friendly to:
- CLI tools;
- Git diffs;
- embeddings;
- graph stores;
- local agents;
- remote harnesses.

## Long-term idea: graph-addressable source

Tessera Compact may optionally reference verified context IDs directly:

```
f mix(...) ... ?#buf.same_len !#fx.pure
```

Those IDs resolve through TIR/CTX, making terse source semantically rich without repeating long prose.
