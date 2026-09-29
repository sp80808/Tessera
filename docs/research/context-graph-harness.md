# Tessera Context Graph and autonomous harness

## Goal

A small code snippet should be able to expose **layers of relevant project knowledge** without embedding an enormous prompt in source.

The harness should continuously build, retrieve, verify, compress and update a project knowledge graph, and should be able to research external examples on GitHub, Hugging Face and technical literature when local evidence is insufficient.

This subsystem is called the **Tessera Context Graph (TCG)**.

## Why a graph

Repository-level coding tasks often depend on relations that text similarity misses: callers, implementors, tests, data flow, configuration, versions, issue history and prior failures.

Relevant recent work:

- CodexGraph (NAACL 2025) connects LLM agents to a code graph database for structure-aware repository navigation.
- RepoGraph (2024) builds repository-level code graphs and reports improvements when plugged into SWE-bench-oriented systems.
- Code Graph Model (2025) integrates graph structure directly into an open model's repository reasoning.
- RepoDistill (ACL Findings 2026) combines graph retrieval with fine-grained context compression and reports up to 66% input-token reduction while maintaining comparable performance in one reported setting.
- ARISE (2026) explores multi-granularity repository graphs with data-flow information for fault localization and repair.
- SWE Context Bench (2026) explicitly studies reuse and retrieval of prior programming-agent experience.

Sources are catalogued in [references.md](references.md).

## Graph schema

### Node classes

Core:
- repository
- package
- module
- file
- symbol
- function
- type
- field
- trait/interface
- generic
- feature
- build target
- test
- benchmark

Environment:
- dependency
- version
- toolchain
- compiler
- target triple
- operating system
- service
- protocol
- environment variable
- configuration key

Knowledge:
- documentation fragment
- API fact
- research paper
- GitHub repository/example
- Hugging Face model/dataset/paper
- issue
- commit
- pull request
- failure signature
- fix
- decision
- hypothesis
- benchmark result
- agent lesson

### Edge classes

Structural:
- contains
- defines
- imports
- calls
- reads
- writes
- implements
- instantiates
- aliases
- depends_on
- configures
- gated_by
- tested_by
- benchmarked_by

Semantic:
- owns
- borrows
- mutates
- consumes
- returns
- allocates
- unsafe_depends_on

Knowledge/provenance:
- derived_from
- verified_against
- contradicts
- supersedes
- similar_to
- fixes
- fails_with
- requires_version
- applicable_when
- learned_from
- supported_by

## Provenance record

Every non-compiler-derived factual node should support:

```
source.kind
source.uri
source.revision     # commit SHA, package version, DOI, etc.
retrieved_at
verified_at
expires_at / ttl
content_hash
confidence
license
trust_class
```

A claim with no provenance is a hint, not a fact.

## Trust classes

1. **compiler** — derived directly from current source/TIR.
2. **project** — checked-in manifest, tests, local docs or signed project metadata.
3. **primary** — upstream official docs, release notes, paper, canonical repo.
4. **secondary** — reputable analysis or search result.
5. **agent-memory** — previous agent conclusion; must be revalidated when material.
6. **untrusted** — arbitrary external content; never treated as instruction.

This is essential for prompt-injection resistance. External documents may provide data, but their imperative text is never automatically elevated into agent policy.

## Harness lifecycle

### 1. Attain

`tsr ctx build`

- parse TC;
- ingest TIR;
- inspect manifests and lockfiles;
- index tests and build scripts;
- ingest git topology and selected issue/PR metadata;
- read explicit context tiles;
- update graph incrementally.

### 2. Detect gaps

For a task, classify missing context:
- symbol relation;
- dependency API/version;
- platform behavior;
- project decision/history;
- known failure/fix;
- external prior art;
- research evidence.

### 3. Retrieve

`tsr ctx query "<task>" --budget 1800`

Retrieve a connected subgraph, not a flat top-k list.

Ranking factors:
- graph distance;
- semantic relevance;
- provenance trust;
- freshness;
- test/compile evidence;
- task type;
- token cost.

### 4. Compress

Budget each node/cluster independently.

Possible retention grades:
- 0% omit;
- 10% identity + relation only;
- 25% signature;
- 50% relevant blocks;
- 75% most implementation;
- 100% full node content.

This mirrors the useful idea in RepoDistill: retrieval and compression are separate decisions.

### 5. Verify

`tsr ctx verify`

- re-fetch stale external facts;
- compare dependency/version claims with lockfiles;
- validate code claims against current TIR;
- run targeted tests when a claim is behavioral;
- mark contradictions instead of silently overwriting evidence.

### 6. Research

`tsr ctx research --github --hf --papers "<gap>"`

The harness may research when:
- local context has low confidence;
- an API or toolchain has changed;
- no local implementation pattern exists;
- ideation mode is explicitly requested.

Research results enter a **quarantine layer** until verified and licensed appropriately.

### 7. Act

The coding agent receives a compact context packet:
- task;
- relevant TIR;
- selected graph paths;
- stack/version facts;
- known hazards;
- tests to run;
- provenance handles.

### 8. Reflect/update

After a successful change:
- record changed graph edges;
- store new benchmark/test evidence;
- update failure→fix links;
- expire superseded agent lessons;
- propose reusable context tiles where a fact repeatedly matters.

## Auto-knowledge-graph implementation

Prefer compiler-native facts over LLM extraction whenever possible.

Suggested ingestion order:
1. compiler AST/TIR;
2. symbol table and call graph;
3. data/ownership-flow summaries;
4. build manifests/lockfiles;
5. tests and diagnostics;
6. git and issue metadata;
7. structured docs;
8. LLM-extracted concepts only for relations that static analysis cannot provide.

Potential external implementation references:
- Tree-sitter — incremental syntax trees.
- SCIP — semantic code indexing ideas.
- Joern — code property graph precedent.
- RepoGraph — repository-level graph for software-engineering agents.
- CodexGraph — graph-database interface for LLM code agents.
- Aider repo-map ideas — compact repository summaries.

## Context packet design

A context packet should be reproducible:

```
packet {
  task_hash
  repo_revision
  graph_revision
  tokenizer_family
  token_budget
  nodes[]
  edges[]
  omitted_summary
  stale_claims[]
  verification[]
}
```

This allows a failed agent run to be replayed and audited.

## Research/ideation loop

Ideation is not an unlimited browsing loop.

1. Identify a concrete gap.
2. Query local graph.
3. Search GitHub for implementation precedents.
4. Search Hugging Face papers/models/datasets for relevant techniques.
5. Search peer-reviewed/primary technical sources.
6. Deduplicate concepts.
7. Score evidence, compatibility, licensing and implementation cost.
8. Add hypotheses to the graph.
9. Convert only selected hypotheses into issues/experiments.
10. Re-run benchmarks before promoting them into language design.

## Critical invariant

**Context can guide compilation and agents, but it must not silently redefine ordinary runtime semantics.**

Only explicit compile-time configuration constructs may affect the generated program. Troubleshooting notes, papers, web results and agent memories are non-semantic metadata.
