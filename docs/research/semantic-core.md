# Semantic core and memory-safety direction

## Objective

Tessera should support low-level systems work with deterministic resource management and no mandatory tracing garbage collector.

The working model is **affine ownership + explicit borrowing + capabilities**.

This borrows the core insight of Rust—compile-time restrictions can make ownership and aliasing safe without runtime GC—while allowing Tessera to seek a much denser surface notation.

## Core categories

Semantic categories, independent of final surface syntax:

- `copy` — freely duplicable value.
- `own` — affine owned resource.
- `shr` — shared read-only loan.
- `mut` — exclusive loan.
- `raw` — unchecked pointer/address.
- `cap` — capability authorizing restricted operations.

## Working invariants

1. A non-copy value has at most one owner.
2. Ownership can move.
3. Shared loans may coexist.
4. Exclusive loans exclude other conflicting access.
5. Destruction happens deterministically at end of liveness/scope.
6. Unsafe operations require an explicit capability boundary.
7. FFI/layout rules are explicit.
8. Allocation is never implicit merely because an abstraction is used.
9. Concurrency safety is expressed through types/capabilities rather than a global runtime.

## Evidence and precedent

The Rust Reference documents moves, references, mutable references, raw operations and unsafe blocks as language-level mechanisms.

Reference: https://doc.rust-lang.org/reference/

Marshall & Orchard, *Functional Ownership through Fractional Uniqueness* (PACMPL 2024), develops ownership/borrowing ideas through graded/fractional uniqueness, reinforcing that ownership can be modeled as a principled type discipline.

DOI: https://doi.org/10.1145/3649848

KRust provides an earlier formal executable semantics effort for Rust and documents the role of ownership, moves and borrows in its safety story.

DOI: https://doi.org/10.1109/TASE.2018.00014

## Compact surface hypotheses

Not final:

```tessera
x:=make()   # own
y:=^x       # explicit move if needed
a:=&y       # shared borrow
b:=&!y      # exclusive borrow
```

A key experiment is whether moves should usually remain inferred or be surfaced with a one-token/sigil marker. Explicit moves cost tokens but may improve model reasoning and diagnostics.

## Effects

Tessera should research a compact effect/capability system for:
- allocation;
- IO;
- blocking;
- panic/abort;
- unsafe memory;
- FFI;
- async/suspension.

The agent-facing TIR should always expand inferred effects, even if TC omits them.

Example intent expansion:

```
function read_frame
effects: io, allocation:none, unsafe:none, suspend:none
borrows:
  input: shared for call duration
returns:
  owned Frame
```

## Unsafe

Unsafe is an explicit proof boundary, never "turn off the checker globally."

TIR must list:
- every unsafe operation;
- required preconditions;
- the enclosing capability;
- affected memory objects;
- whether the compiler can verify any obligations automatically.

## FFI

C ABI support is a v0 requirement:
- `repr(C)`-equivalent layout;
- fixed-width integer types;
- raw pointers;
- extern functions/statics;
- explicit calling conventions;
- predictable symbol names/export control.

## Open questions

- lexical lifetimes vs non-lexical liveness model;
- region inference representation;
- whether shared aliasing rules match Rust exactly;
- destructive moves vs explicit consume operators;
- capability inference;
- panic/unwind strategy;
- async state-machine semantics;
- aliasing rules suitable for LLVM optimization without exposing accidental undefined behavior;
- how much borrow detail should be encoded directly in TC versus TIR only.
