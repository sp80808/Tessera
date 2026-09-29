# Context Lattice: Typed Agent and Project Context

## Motivation

Repository agents repeatedly spend tokens rediscovering facts that the project already knows:

- which framework/version is in use;
- which code path is authoritative;
- which invariant a function relies on;
- why a dependency was pinned;
- which failed attempt should not be repeated;
- whether a workaround only applies on Windows/ARM/debug builds;
- which test proves an assumption;
- which external document established a protocol detail;
- when that information was last verified.

Free-form comments and AGENTS.md-style files help, but they are difficult to scope, merge, invalidate and query automatically.

Tessera therefore treats context as a **typed side-semantic layer**.

## Principle

CTX is part of the source artifact but is usually not part of runtime semantics.

A CTX fact has:

```
id
kind
value
scope
condition
source/provenance
confidence
verified_at
expires_or_invalidation_rule
relationships
visibility
```

The same fact may be:
- compiled away;
- included in debug metadata;
- indexed into the Harness Graph;
- surfaced to an LLM only when relevant;
- made compile-affecting if its kind is explicitly semantic (for example target ABI constraints).

## Lattice model

Context is layered from broad to narrow:

```
workspace
  -> package
    -> module
      -> type
        -> function
          -> block
            -> expression
```

and orthogonally by concern:

```
stack | platform | build | invariant | intent | perf | safety
test | issue | evidence | dependency | failure | workaround | agent
```

Resolution follows scope + condition + freshness + authority.

Narrow facts may refine broader facts. Contradictions are not silently overwritten; they become graph conflicts for `tess verify`.

## Illustrative compact syntax

Not final syntax:

```
@{stk:tokio@1.48;os:linux;arch:x64}
@{inv:#buf.valid;src:test:buffer_prop;v:2026-09-29}
f mix(a:&[f],b:&[f])>[f] { ... }
```

Conditional context:

```
@{when:os=win & dep:wasapi<0.20;
  fail:#dev.reopen;
  fix:#exclusive.retry;
  src:issue:184}
```

Agent-only troubleshooting context:

```
@{agent;
  if:err=E042;
  check:[#cfg.generated,#feature.simd];
  avoid:#old.codegen.patch;
  why:#adr.17}
```

The expander might render these as:

```yaml
context:
  stack:
    - package: tokio
      version: "1.48"
  platform:
    os: linux
    arch: x86_64
  invariants:
    - id: buf.valid
      verified_by: test:buffer_prop
      checked_at: 2026-09-29
```

## Context kinds

### Semantic context
May affect compilation and therefore must be reproducible:
- target triple;
- ABI;
- CPU feature requirements;
- representation/layout contracts;
- feature gates;
- no-std/no-alloc constraints.

### Development context
Does not change the language meaning:
- architectural intent;
- issue links;
- ownership rationale;
- migration notes;
- known failures;
- performance history;
- test evidence.

### Agent context
Optimised for autonomous tools:
- preferred inspection order;
- authoritative files;
- generated-file rules;
- commands that verify a change;
- forbidden/deprecated approaches;
- retrieval hints;
- escalation conditions.

### Evidence context
Binds claims to observations:
- repository commit/file hash;
- dependency lockfile hash;
- documentation URL + retrieved date;
- paper DOI;
- benchmark artifact;
- test run ID;
- environment fingerprint.

## Dynamic tech-stack context

A core requirement is **condition-based stack information embedded with the code**.

Example use case:
A networking function changed behaviour only for:
- Windows,
- a dependency before version X,
- async runtime feature Y,
- when a compatibility flag is enabled.

Rather than a prose comment, the CTX entry can encode the predicate. A harness can ask:

```
ctx(function=connect, env=current, concern=failure)
```

and retrieve only applicable knowledge.

## Context admission

Agent-created facts should have states:

```
observed -> inferred -> verified -> trusted
                     \-> contradicted
                     \-> stale
```

Suggested policy:
- `observed`: mechanically extracted.
- `inferred`: model/tool hypothesis; never silently treated as truth.
- `verified`: supported by deterministic test, authoritative source or reproducible observation.
- `trusted`: project explicitly promotes it.
- `stale`: invalidation condition triggered or age threshold exceeded.

## Context packing

The harness can serialise context at several levels:

- **L0 execution**: code only.
- **L1 compile**: semantic CTX only.
- **L2 local agent**: symbol + direct dependency context.
- **L3 task**: graph-selected related invariants/tests/issues/evidence.
- **L4 forensic**: full provenance and history.

This is a direct token-control mechanism.

## Interaction with comments

Comments remain allowed.

Use comments for:
- human narrative;
- non-actionable explanation;
- temporary notes.

Use CTX for information a tool should be able to:
- select;
- validate;
- filter;
- expire;
- relate;
- transform;
- cite.

## Security and trust

CTX is executable *for agents* even when it is non-executable for the CPU, so it is an injection surface.

Requirements:
- provenance and trust domains;
- repository-local policy for which CTX kinds may direct agents;
- external retrieved text cannot become trusted agent instruction automatically;
- signatures/hashes for generated context caches;
- visibly separate untrusted evidence from project-authored directives;
- `tess verify --ctx` detects stale hashes and invalid predicates.

## Research connection

A 2025 study of agent configuration files across open-source projects found substantial variation in how projects encode descriptive, prescriptive, prohibitive, explanatory and conditional context. Tessera's response is to make those categories queryable data while still allowing rendered Markdown for existing agents.

Reference: [Context Engineering for AI Agents in Open-Source Software](https://arxiv.org/abs/2510.21413).
