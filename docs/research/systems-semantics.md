# Systems Semantics Research

## Performance target

“Rust-level” should mean:
- ahead-of-time native compilation;
- predictable layout and calling conventions;
- no mandatory tracing garbage collector;
- stack allocation by default where possible;
- deterministic destruction/resources;
- zero- or low-overhead abstractions;
- explicit SIMD/atomics/volatile/raw-pointer capabilities;
- C ABI interop;
- controllable allocation;
- usable in no-std/embedded-style environments.

It does **not** require copying Rust's surface syntax.

## Safety target

Tessera should make the safe subset memory-safe and data-race-resistant by construction while retaining an explicit unsafe escape hatch.

The research baseline is an **affine ownership model**:
- values are moved by default unless their type is copyable;
- at most one live mutable borrow of a location;
- multiple shared borrows allowed when mutation is excluded;
- borrows cannot outlive the referenced storage;
- destructors execute deterministically;
- raw pointers/casts/unchecked indexing/FFI assumptions require an unsafe capability.

Rust demonstrates that moves and borrowing can provide memory/thread safety without mandatory GC. Work on fractional uniqueness/graded systems suggests ownership and borrowing can also be expressed as a more regular type-level discipline, which is interesting for Tessera because regular semantics may compress better than a large set of special cases.

Reference: D. Marshall, D. Orchard, [Functional Ownership through Fractional Uniqueness](https://doi.org/10.1145/3649848), PACMPL 2024.

## Candidate compact ownership notation

Illustrative:

```
T      owned value
&T     shared borrow
&!T    exclusive/mutable borrow
*T     raw pointer
^T     pinned/address-stable owned value
```

The exact glyph set must be benchmarked for model-token cost.

## Lifetime strategy

Avoid verbose explicit lifetimes in common code.

Proposed order:
1. lexical/NLL-like inference for local borrows;
2. elision rules for common function signatures;
3. region parameters in TIR;
4. compact explicit region IDs only when inference is ambiguous or public contracts require them.

The Intent IR must always expose resolved regions even if TC omits them.

## Effects

Effects are useful for both optimisation and agent comprehension.

Candidate effect classes:
- alloc
- io
- async
- panic/trap
- unsafe
- atomic
- ffi
- blocking
- mutation of external state

Pure/no-effect code should have a compact canonical marker or be inferred and represented in TIR.

Effects can improve context retrieval: an agent investigating an allocation regression can query functions whose inferred effect includes `alloc`.

## Error model

Research both:
- algebraic result/error values for expected failure;
- traps/panic for violated invariants.

Avoid hidden exceptions as the only mechanism because they complicate low-level predictability and static effect summaries.

## Data layout

Requirements:
- explicit C-compatible layout;
- packed/aligned representations;
- tagged and niche-optimised unions where valid;
- transparent wrappers;
- size/alignment introspection;
- endian-aware primitives;
- uninitialised memory APIs confined to unsafe/verified constructs.

TIR should record concrete layout after monomorphisation/target selection.

## Generics

Start with monomorphised parametric generics for predictable performance.

Do not optimise source density by hiding expensive dynamic dispatch. Dynamic dispatch must remain semantically explicit even if the compact surface uses a small marker.

## Concurrency

Safe shared mutable state should require synchronisation/capability types.

The compiler should expose concurrency facts into HG:
- shared across tasks/threads;
- atomic;
- mutex/lock guarded;
- Send/Sync-like properties;
- potential blocking effects.

## Unsafe blocks as verification islands

Unsafe code should carry:
- explicit entry marker;
- machine-readable claimed invariants;
- optionally linked tests/proofs/evidence.

Example concept:

```
u[#ptr.aligned,#len.bound] { *p = x }
```

where `u` is the compact unsafe construct and referenced invariants are expanded by `tess why`.

This makes unsafe code terse without making its obligations invisible.

## Formalisation path

Before claiming memory safety:
1. define a small core calculus;
2. specify ownership/borrow/effect typing judgments;
3. give operational semantics for the safe core;
4. property-test and fuzz compiler transformations;
5. later mechanise critical soundness claims if the language matures.

Do not market inferred intent metadata as a proof.
