# Tessera RFC process

RFCs capture significant language/tooling proposals before they become commitments.

## When an RFC is required

Use an RFC for:
- grammar or semantic changes;
- ownership/effect-system changes;
- ABI/layout commitments;
- stable context-tile fields;
- stable TIR/TCG/TMT formats;
- public CLI behavior;
- architecture changes with broad implementation impact.

Small implementation details do not require an RFC.

## Lifecycle

```
draft -> experimental -> accepted | rejected | superseded
```

"Experimental" means the proposal has an implementation/benchmark but is not stable.

## Numbering

Copy [0000-template.md](0000-template.md) and use the next available number.

## Acceptance

An RFC should not be accepted on prose quality alone. It must name the evidence or implementation gate appropriate to the change.
