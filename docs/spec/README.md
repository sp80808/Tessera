# Tessera provisional specifications

These documents define **candidate contracts**, not stable language behavior.

## Current proposals

- [Context tiles](context-tiles.md)
- [Tessera Model Transport](model-transport.md)
- [Tessera Capability Graph](ownership-capability-graph.md)
- [`tsr witness` evidence (`tessera.witness/v0`)](witness.md)
- [`tsr check` diagnostics (`tessera.diagnostics/v0`)](diagnostics.md)

## Missing specs intentionally blocked on experiments

The following should not be frozen until the linked implementation/research issues produce data:

- TC lexical grammar — #1, #2
- TIR schema — #2, #3
- effect system — #3
- module/import syntax — #2
- ABI/layout — #3, #4
- TCG serialization — #6
- transport profile format — #10

## Promotion rule

A proposal becomes normative only when:
1. an RFC records the decision and alternatives;
2. tests/benchmarks support it;
3. implementation exists for the accepted subset;
4. the status is changed explicitly.

Syntax examples elsewhere remain illustrative.
