# RFC 0001 — Working candidate: multi-item files and call expressions

Status: draft (working candidate for the compiler slice; **not accepted**, spelling **not** promoted)  
Authors: Tessera implementation push (lead agent), for review by the maintainer  
Date: 2026-09-30

## Summary

Extend the bootstrap TC subset (`examples/bootstrap.tes`) with the two smallest constructs that create **dependencies between functions**:

1. a file is a sequence of `f` items instead of exactly one;
2. an expression may be a call `g(e1,e2,...)` of a function defined in the same file.

Everything else is unchanged: `i64` only, `+` only, no new lexemes (`(`, `)`, `,` already exist).

## Motivation

Two roadmap gates cannot be demonstrated with a single function per file:

- **#9 (incremental queries)** must show that *unrelated work is reused after a body edit* and that *dependent work is invalidated after a signature edit*. Without a caller there is no dependent; "signature edit invalidates dependents" is unfalsifiable.
- **#22 (MIR)** and **#20 (resolution)** need calls to exercise call-signature checking, argument evaluation order and function-level name resolution; TIR (v0.1) and MIR already model `call`, but no TC program can reach them.

This is a limitation of the *witness*, not a claim that this is the best call syntax.

## Non-goals

`let`/bindings, `if`/`else`, loops, tuples/structs, borrows, `bool` in TC, imports/modules across files, generics, methods, named/default arguments, closures, operator precedence beyond left-assoc `+`. Each of these is its own evidence-gated proposal (see `docs/research/token-efficiency.md`, candidate matrix).

## Detailed design (working candidate)

```
File     := Item*
Item     := 'f' NAME '(' [Param (',' Param)*] ')' '>' TYPE '=' Expr
Expr     := Primary ('+' Primary)*                         // left-associative, unchanged
Primary  := INT | NAME | NAME '(' [Expr (',' Expr)*] ')' | '(' Expr ')'
```

- A call's callee must be a NAME immediately followed (modulo trivia) by `(`. Since juxtaposition is otherwise not an expression form, there is no ambiguity with `+` chains or with the next item (an item always starts with the parser keyword `f`).
- Item boundaries are token-based, not line-based. The canonical formatter emits one item per line, separated by `\n`, no blank lines.
- Canonical call spelling: `g(x,y)`, no spaces.
- Semantics: the callee resolves to an item of the same file (B3). Arity and argument types are checked against the callee's **signature only** (B4), never its body — the change firewall of the phase contract (`compiler-phases.md` §8). A call's type is the callee's declared return type. Recursion is allowed.
- Expansion law (surface -> TIR): `g(e1,e2)` expands to `(call g T e1' e2')` where `T` is the callee's return type and each `ei'` is the expanded argument (TIR `call` node, already defined and verified in TIR v0.1).
- HIR: `Expr::Call { callee: Path, args }`; resolution yields `Res::Def(ItemId)` for the callee.

## TC / TIR / TCap / TCG / TMT impact

- TC: grammar above only. The lexicon register (`docs/architecture/syntax-lexicon.md`) gains no lexeme rows; the *grammar* column for `(`, `)`, `,` is extended and marked *working candidate*.
- TIR: none (`call` exists in v0.1). MIR: none (`Call` exists).
- TCap/TCG/TMT: unaffected; TCG may later use call edges as stable `ItemId`-to-`ItemId` facts.

## Token impact

To be measured with `tess-tokenbench` (#1) before promotion, not asserted here:

- add `parsed` corpus variants with calls, alongside Rust/C/Zig/Odin equivalents (all of which spell calls `g(x,y)`), across every tokenizer available;
- compare against the alternatives below on tokens, cross-tokenizer dispersion and grammar-boundary alignment;
- rewrite-stability (whitespace around `(`/`,`) collapses under the canonical formatter.

## Semantic and safety impact

None beyond scalar `i64` calls: no ownership, borrows or effects are involved. Integer-overflow behavior of `+` remains open (O1). Call evaluation order is left-to-right (fixed by the MIR lowering; provisional).

## Alternatives

1. **Status quo** (one function per file, no calls): keeps the grammar minimal but makes #9's signature-edit witness impossible at the source level.
2. **Juxtaposition** (`g x y`): fewer punctuation tokens, but interacts with `+` and future binding syntax; needs a precedence rule.
3. **Prefix S-expression** (`(g x y)`): trivially unambiguous, conflicts with parenthesized grouping unless grouping is dropped.
4. **Named colon form** (`g:x,y`): compact, collides with the parameter-type colon.
5. **Dedicated call sigil** (`@g(x,y)`): removes any ambiguity with grouping at one extra token.

None is preferred on evidence yet; `g(x,y)` is the working candidate only because it is the form shared by every reference language in the corpus (lowest measured surprise for models trained on them — a hypothesis to be tested, not a result).

## Verification plan

- Parser: golden CST for multi-item and call inputs; error recovery for `g(`, `g(1,`, `g(,)`, missing item body; no-panic fuzz targets updated; exact source round trip.
- HIR: reformatting invariance (`g( x , y )` == `g(x,y)`), stable `ItemId` for untouched items across edits, provenance totality.
- Sema: arity/type/unknown-callee/duplicate-item diagnostics with spans; callee bodies never read.
- TIR/MIR: `TC -> TIR -> TC` round trip (canonical); TIR verifier accepts; MIR differential interpreter test.
- Incremental (#9): body-only edit of `g` leaves the caller's `typeck`/`tir` results reused; signature edit of `g` re-runs exactly its callers; recomputation counts are asserted, not just timed.

## Falsification / reversal criteria

Reject or replace the spelling if any of these holds:

- a rival candidate (2–5) lowers **median** tokens and does not raise cross-tokenizer dispersion or lower grammar alignment across all available model families;
- a model-generation/repair experiment shows materially worse compile@1 for this form than for a rival;
- it constrains a later binding, precedence or method-call proposal (checked when those RFCs are written).

The *capability* (multi-item files + calls with signature-only checking) is independent of the spelling and is what the compiler slice actually needs; a spelling change touches only lexer-adjacent parser code and the formatter.

## Migration / compatibility

Additive: every program accepted today is accepted with identical HIR/TIR; `examples/bootstrap.tes` and `.tir` are unchanged.

## Evidence

- Phase contract `docs/architecture/compiler-phases.md` §2.4, §3 B3/B4, §8 (signature-only reads as the incremental firewall).
- Issue #9 acceptance criteria ("Signature-vs-body invalidation behavior has tests").
- Token evidence: pending `tess-tokenbench` baseline (`bench/results/baseline.json`); to be linked here when promoted to *experimental*.
