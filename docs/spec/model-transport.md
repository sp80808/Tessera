# Tessera Model Transport (TMT) — provisional specification

Status: research proposal

## Purpose

TMT is a reversible, compiler-generated representation optimized for model communication. It is **not a second accepted source language** and it does not change program semantics.

Why separate it from TC:

- TC needs long-term grammatical stability.
- Different model tokenizers reward different byte sequences.
- high-cardinality shorthand can reduce tokens but would bloat the human/compiler language;
- repair, explanation and generation may need different representations.

Invariant:

```
semantics(unpack(pack(TIR, profile))) == semantics(TIR)
```

Where possible:

```
canonical_TC(unpack(pack(TC, profile))) == canonical_TC(TC)
```

## Profiles

A profile is versioned:

```
profile {
  id
  tessera_version
  task_class
  tokenizer_family
  dictionary_revision
  transforms[]
}
```

Initial task classes:
- `gen`
- `repair`
- `explain`
- `repo`
- `diag`

## Transform classes

### 1. Symbol dictionary

Replace repeated long identifiers with scoped stable handles.

```
$0=parse_config
$1=Config
f $0(s:&str)>R<$1,E>=...
```

The dictionary is transported once per packet.

### 2. Type/effect interning

Repeated types/effect sets receive packet-local IDs.

```
!0={alloc:0,io:0,panic:0}
T3=R<Config,ParseE>
```

### 3. Pattern sugar

Frequent TIR patterns may have reversible IDs.

Conceptual:

```
%17(x,y)
```

must expand to exactly one normalized TIR template with typed placeholders.

Pattern sugars are mined from corpora but only admitted to a profile after:
- frequency threshold;
- token savings across target tokenizer(s);
- exact inverse;
- semantic test corpus;
- collision/ambiguity test.

### 4. Structural elision

Task-irrelevant subtrees may be replaced with typed summaries, but only in context packets — never when the model is expected to edit the elided region.

Example:

```
#body:hash/signature/effects
```

## Never elide

For an editable symbol:
- ownership transfer points;
- unsafe operations;
- effect boundaries;
- target/feature conditions affecting semantics;
- unresolved overload/generic choices;
- invariants required by the task.

## Grammar-constrained generation bridge

The compiler should expose a grammar-state API usable by model harnesses.

Conceptual:

```
state = parser.start()
state.accept(bytes)
state.valid_terminals()
state.accepting()
```

A model adapter may precompute tokenizer-token -> grammar transition relations.

The compiler itself must not depend on a particular model tokenizer.

## Metrics

For every profile measure:

```
token_saving
generation_success
compile_success
tests_passed
repair_turns
decode_overhead
roundtrip_failures
cross_model_variance
```

A transport transform that saves 20% prompt tokens but increases repair iterations is a regression unless total cost/success improves.

## Storage

Profiles and dictionaries are build/tooling artifacts.

Suggested paths:

```
.tessera/transport/<profile>.json
.tessera/cache/transport/
```

Canonical source control should not require committing generated TMT.

## Security

TMT is mechanically generated from trusted compiler state and selected verified context. External web/research text must never define executable transport transforms automatically.
