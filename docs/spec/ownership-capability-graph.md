# Tessera Capability Graph (TCap) — ownership/borrow semantic view

Status: research proposal

## Goal

Represent ownership and borrowing in a compact graph that is useful to:
- the borrow checker;
- diagnostics;
- agents;
- analyses;
- visualization;
- proof tooling.

TCap is a derived TIR view, not a replacement for the formal type rules.

## Motivation

Recent work on Place Capability Graphs shows that flow-sensitive ownership/borrowing constraints can be represented as graph structure over memory places and capabilities. "From Linearity to Borrowing" provides a useful minimal order for adding borrow features to a linear core.

Tessera should use both ideas:
- **small formal core** for soundness;
- **capability graph** for implementation/explanation.

## V0 capability lattice

Provisional capabilities:

```
E  exclusive: read, write, move, shared/exclusive borrow
R  read: read + shared borrow
W  write: assignment/write where permitted
0  none: no current access
```

Do not infer a total ordering where semantics do not justify one. The graph transition rules are authoritative.

Future extensions may represent:
- atomic capability;
- pinned/no-move state;
- unsafe/raw authority;
- FFI capability;
- thread/send/sync constraints.

## Nodes

### Place

A place denotes storage:

```
local x
field x.a
index x[i]        # may require abstraction
deref *p
remote arg#0
```

Node identity includes a stable semantic place ID and program point when flow sensitivity requires it.

### Borrow/lifetime projection

A projection node represents outstanding borrow authority associated with a place/type/lifetime.

Tessera v0 should begin with lexical extents, then generalize only after tests force it.

## Edges

Initial edge kinds:

- `move A -> B`
- `share A -> r`
- `loan_mut A -> r`
- `reborrow r1 -> r2`
- `restore r -> A`
- `split A -> A.field...`
- `join fields -> A`

Every edge records:
- source span;
- TIR operation;
- before/after capability state;
- reason/constraint ID.

## Example

Source:

```tessera
f bump(x:&!i32){*x+=1}
```

Expanded conceptual graph:

```
caller.place --loan_mut--> arg_ref(E)
caller.place: 0
arg_ref: E
  |
  +-- write --> value
  |
return/end
  |
  +-- restore --> caller.place:E
```

## Partial moves

Composite ownership must be field-sensitive.

```
p : E
move p.x -> x
p.x : 0
p.y : E
p   : incomplete
assign p.x <- new
p.x : E
p   : E
```

Diagnostics should show this as capability transitions rather than only lifetime prose.

## V0 semantic implementation order

Following the research direction from linearity toward borrowing:

1. owned affine values + deterministic drop;
2. immutable borrow;
3. lexical borrow extent;
4. reborrow;
5. mutable borrow;
6. field-sensitive split/join;
7. non-lexical shortening;
8. advanced conveniences only after core stability.

Each stage requires:
- formal rule sketch;
- positive/negative tests;
- TCap transition tests;
- TIR examples;
- diagnostics snapshots.

## Agent/debug projection

`tsr explain --ownership` should be able to emit a compact trace:

```
x:E
L12 share x->r => x:R,r:R
L14 move x ! blocked: outstanding r
L16 end r => x:E
```

and a verbose explanation on request.

This is intentionally much cheaper for models than reproducing an entire borrow-checker narrative.

## Unsafe interaction

Unsafe code may bypass ordinary checks, but TCap should still represent declared/assumed authority.

An unsafe edge must carry an obligation ID:

```
raw_write p  [requires: aligned(p), live(p), unique(p)]
```

Verification tools can attach evidence to those obligations through TCG.
