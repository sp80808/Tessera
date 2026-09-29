# Context tiles — provisional language design

> Status: research proposal, not frozen syntax.

Context tiles are compact, typed metadata embedded in or adjacent to Tessera source. They let a tiny snippet expose useful project and troubleshooting context without forcing every agent to rediscover the same information.

## Separation of concerns

Tessera source has two channels:

1. **program channel** — executable/compile-time semantics;
2. **context channel** — queryable metadata for tools and agents.

Context-channel data is stripped from release artifacts by default and cannot alter runtime behavior.

## Layer model

| Layer | Purpose | Typical producer |
|---|---|---|
| C0 | syntax + executable semantics | programmer/compiler |
| C1 | inferred semantic facts: types, ownership, effects | compiler |
| C2 | symbol/module intent | programmer/tool |
| C3 | project/tech-stack constraints | manifest/context tile |
| C4 | evidence + external documentation | harness |
| C5 | troubleshooting + prior fixes | tests/agents |
| C6 | task/agent guidance | project maintainers |
| C7 | speculative ideas/research candidates | research harness |

Higher layers cannot override lower-layer executable facts.

## Provisional compact syntax

A context tile begins with `@c`. Syntax below is deliberately dense and exists to be benchmarked, not blessed.

```tessera
@c db?feat(db){s:pg>=17,sqlx~.8;v:30d;r:c7(sqlx),gh(sqlx-rs/sqlx)}
f load(id:Id)>R<User,E>=...
```

Possible expansion:

```text
context tile "db"
applies_when: feature("db")
stack:
  postgres: >=17
  sqlx: compatible-with 0.8
freshness_ttl: 30 days
sources:
  - Context7 package docs: sqlx
  - GitHub repository: sqlx-rs/sqlx

function load(id: Id) -> Result<User, E>
...
```

The compact keys (`s,v,r`) are provisional dictionary entries. TIR/context expansion always displays the full semantic names.

## Conditional context

Context often matters only under a condition.

Candidate predicates:

```
?os(linux)
?arch(a64)
?feat(db)
?dep(sqlx>=.8)
?target(wasm)
?err(E0425)
?task(test)
```

Multiple conditions may compose:

```
@c net?os(linux)&feat(tls){...}
```

Conditions decide **context retrieval**, not general runtime branching.

## Scopes

Tiles may attach to:
- repository;
- package;
- module;
- type;
- function;
- block;
- test;
- specific source span.

Inheritance is lexical/graph-based and explicit in TIR.

## Tile kinds

### Stack tile

```
@c stack{s:rs=1.91,cl=cranelift,os:[linux,mac]}
```

Describes supported environment or known-good versions.

### Contract tile

```
@c api{must:noalloc,panic:no,lat:<100us}
```

Records non-type-system engineering constraints. Harness can turn these into checks where possible.

### Evidence tile

```
@c ev{src:doi(10....);h:abc...;ttl:180d}
```

Points to evidence with immutable identifiers and hash.

### Troubleshooting tile

```
@c fix?err(E_CONNRESET){cause:peer-close;try:retry(3,exp)}
```

This should normally reference tests/commits rather than encode prose.

### Agent tile

```
@c ag{edit:noalloc;verify:[test(net),bench(lat)]}
```

Project-maintainer guidance. Only trusted project tiles become agent instructions.

### Research tile

```
@c idea{q:"arena elision";src:hf(2605...);state:hyp}
```

Speculative until verified. Research tiles never become constraints automatically.

## Dynamic references

Tiles may point to external resolvers rather than inline content:

```
c7(package,version)
gh(owner/repo,rev?)
hf(kind,id,rev?)
doi(id)
url(uri,hash)
```

The harness resolves a reference only when relevant. Resolved content is cached with provenance.

## Freshness

A dynamic fact requires either:
- immutable revision/hash; or
- TTL + verified timestamp.

The compact source may say:

```
v:30d
```

TIR expands this into explicit freshness policy.

## Conflict handling

If two tiles conflict:
1. compiler facts win over metadata;
2. project-pinned facts beat external latest facts for reproducible builds;
3. newer evidence does not silently overwrite old evidence;
4. graph stores both claims plus `contradicts`/ `supersedes` edges;
5. agent packet surfaces the conflict when task-relevant.

## Security

External content is data, never policy.

A GitHub README saying "ignore previous instructions" is an untrusted text node. It cannot become an agent rule unless a maintainer explicitly promotes it into a trusted project tile.

## Binary/build handling

By default:
- TC + required semantic metadata compile normally;
- context tiles are omitted from native objects;
- debug builds may embed a compact context index or source-map IDs;
- CI may export a `.tcg` graph artifact separately.

## Why this belongs in the language ecosystem

Ordinary comments have no schema, no freshness, no provenance, no conditional retrieval and no trust model. External agent instruction files are useful but have poor symbol-level locality. Context tiles provide **small local hooks into a larger project knowledge graph**.

The goal is not to put the internet in source code. The goal is to put enough typed handles in source code that an agent can reliably retrieve the right verified context.


## Context lattice: nested information without prompt bloat

Context tiles are best understood as local handles into a **context lattice**.

A source span can inherit context from:
- repository;
- package;
- module;
- type;
- function;
- block;
- feature/target branch;
- current task.

The harness computes the applicable meet/projection for a requested context class instead of concatenating all inherited metadata.

Conceptually:

```
effective_ctx(span, task, env) =
  resolve(
    repo_ctx
    + package_ctx
    + module_ctx
    + symbol_ctx
    + conditional_ctx(env)
    + task_ctx(task)
  )
```

Conflicts remain explicit evidence edges; `resolve` does not mean "last text wins".

## Condition-rich tech-stack syntax

A major use case is embedding compact, dynamic tech-stack knowledge.

Provisional examples:

```tessera
@c stk{
  dep:axum~.8;
  rt:tokio^1;
  rust:>=1.91;
  ?target(wasm){http:wasi;thread:no};
  ?feat(tls){tls:rustls~.23;verify:#tls-smoke};
  ?os(windows){io:iocp;ref:#win-net};
}
```

A more aggressively compressed candidate might be:

```tessera
@c s{a~.8,t^1,r>=1.91;?w{h:wasi,th:0};?f(tls){x:ru~.23,v:#ts}}
```

These are intentionally extreme experiments. TIR must expand either form to the same typed structure. Only tokenizer/model benchmarks should decide which, if either, becomes canonical.

## Layer references

A tile may contain a compact reference to an expanded graph cluster:

```tessera
@c{stk:#S4;why:#D19;fix:#F7;test:#T2}
```

The source pays only the reference cost. The graph cluster may include:
- dependency versions;
- API excerpts;
- rationale;
- previous failures;
- relevant tests;
- provenance.

A model packet can request just `#S4/signatures` or `#F7/summary` instead of expanding the full cluster.

## Embedded troubleshooting context

Known-failure knowledge should be addressable by machine-detectable signatures:

```tessera
@c fix{
  ?err(E_CONNRESET){use:#retry-peer-close;proof:#net-prop};
  ?diag("borrow:loan#17"){see:#ownership-reorder-3};
  ?bench(lat>100us){profile:#alloc-path}
}
```

The compact tile does not replace diagnostics. It links a current diagnostic/observation to previously verified project knowledge.

## Context introspection

Planned tooling:

```
tsr explain <span> --layers C0:C7
tsr ctx show <span> --view stack
tsr ctx show <span> --view fix
tsr ctx why <fact>
tsr ctx evidence <id>
tsr ctx stale <span>
```

A human or weaker model should always be able to recover a readable explanation from cryptic source.
