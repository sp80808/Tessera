# Token efficiency and tokenizer-aware syntax

## Principle

Tessera optimizes **model-token cost**, not apparent terseness.

A syntax proposal is only considered compact if it is compact after several real tokenizer families process it. Character count, compiler lexical-token count and LLM token count must be recorded separately.

## Evidence

### CodeBPE

Chirkova & Troshin, *CodeBPE: Investigating Subtokenization Options for Large Language Model Pretraining on Source Code* (2023), report that code-aware punctuation grouping reduced average sequence length by roughly 17% without downstream performance loss in their experiments. More aggressive composite merges produced larger reductions in some settings with possible quality trade-offs.

Source: https://arxiv.org/abs/2308.00683

Design implication: punctuation combinations, repeated grammar fragments and identifier forms should be chosen empirically against tokenizers.

### Compiler-tokenized compressed source

Li & Lu, *Research on Compressed Input Sequences Based on Compiler Tokenization* (2025), use a compiler lexical pass plus a restoration dictionary and report a 33.7% reduction in input token count versus their baseline.

Source: https://doi.org/10.3390/info16020073

Design implication: a reversible compiler-assisted representation can be more effective than textual minification. This strongly supports TC + TIR rather than making the only representation human-readable.

### TokDrift

Li, Deng & Nie, *TokDrift: When LLM Speaks in Subwords but Code Speaks in Grammar* (ACL 2026), evaluate semantic-preserving source rewrites across code models and show that superficial changes in tokenization can change predictions. They identify subword/grammar-boundary mismatch as a reliability issue.

Source: https://aclanthology.org/2026.acl-long.2199/

Design implication:
- canonical formatting is a model-reliability feature;
- multiple surface synonyms should be avoided;
- whitespace and identifier spelling should not vary casually;
- Tessera should maintain grammar-aware tokenization diagnostics.

### Dynamic tokenization

Feher, Vulić & Minixhofer, *Retrofitting Large Language Models with Dynamic Tokenization* (ACL 2025), show that adaptive token merging can shorten sequences while preserving most downstream quality in their evaluated settings.

Source: https://aclanthology.org/2025.acl-long.1444/

Design implication: Tessera cannot control third-party tokenizers, but its toolchain can provide **model-specific prompt packing** and investigate reversible merged representations for harness use.

### Token merging for code models

Saad et al., *On the Effect of Token Merging on Pre-trained Models for Code* (2025 preprint), merge representations belonging to the same semantic unit and report compute reductions of 1–19% across their experiments, with task-dependent quality changes.

Source: https://arxiv.org/abs/2507.14423

Design implication: compiler lexical units and semantic units are useful boundaries for compression and context packing.

## Tessera token objective

For a program or context bundle P and tokenizer set M:

```
tok(P) = median(tokens_m(P) for m in M)
var(P) = dispersion(tokens_m(P) for m in M)
```

A syntax change should be accepted only when it improves a composite score such as:

```
score =
  tok(P)
  + λ1 * var(P)
  + λ2 * generation_failure_rate(P)
  + λ3 * parse_ambiguity(P)
  + λ4 * diagnostic_expansion_cost(P)
```

Weights are benchmark policy, not language semantics.

## Canonical-source rules to test

1. Prefer a small operator alphabet built from common ASCII sequences.
2. Prefer expression-oriented grammar to repeated statement keywords.
3. Infer local types whenever semantics remain unambiguous.
4. Default to private/module-local visibility.
5. Use one canonical import path syntax.
6. Use one canonical numeric spelling.
7. Avoid optional punctuation with multiple valid spellings.
8. Avoid semantically irrelevant whitespace.
9. Consider deterministic local identifier compaction as an optional experiment.
10. Prefer grammar-recoverable omission over implicit runtime behavior.

## Candidate syntax experiment matrix

These are hypotheses, not frozen syntax.

| Concept | Verbose reference | Candidate TC forms |
|---|---|---|
| bind | `let x = v` | `x:=v`, `x=v` |
| function | `fn f(a: T) -> U` | `f(a:T)>U=`, `f:T>U` |
| shared borrow | `&x` | `&x` |
| exclusive borrow | `&mut x` | `&!x`, `!&x` |
| move | implicit Rust move | `^x` when disambiguation is valuable |
| result propagation | `expr?` | `x?` |
| match | `match x { ... }` | `x?{...}`, dedicated compact form |
| compile condition | attribute/cfg | compact predicate prefix |

Every candidate must be tokenized across the benchmark suite before adoption.

## Tokenizer suite

The benchmark harness should support at least:
- OpenAI-family tokenizers available through public tooling;
- Qwen-family tokenizer;
- Llama-family tokenizer;
- Mistral-family tokenizer;
- DeepSeek-family tokenizer when available;
- byte count and compiler lexical tokens as tokenizer-independent controls.

Never optimize a core operator solely because one tokenizer happens to encode it as one token.

## Identifier strategy

Identifiers are a major compression opportunity and a major comprehension risk.

Experiments should compare:
1. descriptive names;
2. deterministic scoped IDs (`a,b,c`);
3. semantic abbreviations;
4. compiler dictionary substitution where TC uses IDs and TIR restores names;
5. hash-derived stable IDs for cross-file symbols.

Acceptance requires:
- lower cross-model token count;
- no reduction in compile/test success for generated patches;
- perfect TIR round-trip;
- stable diagnostics.

## Token-aware formatter

`tsr fmt` should produce exactly one canonical TC representation.

A future `tsr fmt --for-model <family>` may produce **transport-only prompt representations**, but those must never become alternate accepted source syntax.

## Required experiment

Before freezing v0 grammar, create equivalent micro-programs in:
- Tessera candidate syntax;
- Rust;
- Zig;
- C;
- C++;
- Odin.

Measure both code-only and code+diagnostics token cost. A language that is cheap to write but expensive to explain may still be a poor agent language.
