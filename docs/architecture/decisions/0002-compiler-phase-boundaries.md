# ADR 0002 — Compiler phase boundaries and provenance model

Status: accepted for bootstrap architecture (implementation contract for #17; TC syntax remains provisional)
Date: 2026-09-30

## Context

The bootstrap frontend (`tessera-syntax`) goes source → tokens → AST → TIR in one pass, drops trivia, fails on the first error, and loses source positions during lowering. Issues #18–#24 need independent, testable boundaries, and #9 needs phase outputs that Salsa can track without hidden state. The full contract is in [compiler-phases.md](../compiler-phases.md).

## Decisions

1. **Nine boundaries**: SourceText → lossless CST → normalized HIR → resolved HIR → typed/effect HIR → TIR → MIR/CFG → backend IR → object/link. One responsibility each.
2. **Identity and provenance are separate mechanisms.** Identity is path/structure based (`ItemId`, body-local `ExprId`); provenance (byte spans) lives in side tables keyed by ID from HIR onward. Reformatting therefore cannot change semantic values, which gives incremental early cutoff.
3. **No "unknown" provenance.** Compiler-introduced nodes are `Synthesized { origin, why }` and point at the causing source span.
4. **Phases return `PhaseOutput<T>` (value + ordered diagnostics)**; they never exit, print or panic. Erroneous input yields structurally valid values with explicit error nodes.
5. **MIR lowers from TIR alone.** TIR is the single place where all inferred facts become explicit, so what the human/model reads is what executes. TIR is never an optimization IR; MIR is.
6. **Only the backend crate may name Cranelift/LLVM**; the backend consumes `&MirModule` + `&TargetSpec` only. Enforced by a dependency table test.
7. **Ownership analysis results are a side table over typed HIR**, spelled explicitly in TIR and MIR; TCap derives from them and never feeds back. Effects are typed-HIR facts.
8. **Shared vocabulary lives in one tiny crate** (`tessera-phases`: spans, provenance, diagnostics, `PhaseOutput`). No `Phase` trait, no pipeline runner, no `CompilerState` until two implementations justify them.
9. **Architecture is tested, not just described**: `tessera-phases/tests/architecture.rs` checks dependency direction, backend isolation, network/model absence, no process exit, no hidden mutable globals.

## Alternatives considered

- *Spans inside HIR nodes* (rustc/rust-analyzer style): simpler to read, but any text-moving edit changes every node and defeats cutoff. Rejected in favor of side tables; revisit only if benchmarks show map lookups dominate.
- *MIR lowered from typed HIR, TIR as a side projection:* avoids TIR needing to express control flow, but allows the explanatory artifact to drift from the executed program. Kept as the documented reversal (O3).
- *Single `CompilerState`/context object:* rejected; incompatible with pure Salsa queries and with independent testing.
- *Reusing `tessera-db` for the shared types:* rejected; the query host must not be a dependency of the IR crates.

## Consequences

- More conversion boundaries; each needs a dump format and snapshot tests.
- Provenance tables add memory and one lookup per diagnostic.
- Adding a crate requires registering it in the `LAYERS` table (deliberate friction).
- Known violations of the contract in current code are listed in compiler-phases.md §6 and tracked by issues.

## Evidence

Mechanical checks were validated by injecting a forbidden dependency edge and a forbidden call and observing the test fail with the invariant ID. Lexer and frontend property tests (25k + 20k inputs) and a hostile-nesting test found and now prevent a stack-overflow abort in the bootstrap parser.

## Reversal conditions

- Side-table provenance measurably dominates cost or complicates tooling more than it saves in invalidation (benchmark under #9).
- TIR cannot express the first control-flow construct without becoming a CFG (O3).
- Two crates need a shared phase abstraction beyond the current types.
