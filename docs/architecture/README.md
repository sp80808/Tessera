# Tessera architecture

Architecture documents explain **how the implementation is organized**. They are distinct from language semantics and research evidence.

## Current architecture

- [Compiler phase contracts and representation invariants](compiler-phases.md) (#17)
- [Provisional syntax lexicon register](syntax-lexicon.md)
- [Incremental semantic query engine](incremental-query-engine.md)
- [Architecture decisions](decisions/README.md)

## Target dependency direction

```
syntax
  -> semantic queries
      -> TIR
      -> TCap
      -> MIR
          -> backend(s)

semantic queries
  -> compiler-derived graph
      -> TCG
          -> context projections
          -> TMT/model packets
```

External research/model inference is outside the deterministic semantic query graph.

## Rules

- compiler semantics must not depend on network availability;
- TCG should reuse compiler identities/relations rather than duplicate semantic truth;
- backends consume validated MIR, not source syntax directly;
- model-specific transport stays outside canonical language semantics;
- diagnostics must retain TC source spans and explicit semantic traces.
