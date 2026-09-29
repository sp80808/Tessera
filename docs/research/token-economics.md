# Token Economics and Syntax Research

## Goal

Tessera aims to minimise **model-token cost per unit of executable intent**, not characters or lines.

A useful objective is:

```
density = semantic_work / model_tokens
```

where semantic work can be approximated by a benchmark's typed AST/TIR node count, compiled behaviour, or task-specific information content.

## Why ordinary minification is insufficient

LLM tokenizers are statistical subword systems. Removing whitespace or shortening an identifier can sometimes reduce tokens, sometimes leave the count unchanged, and sometimes create worse token boundaries.

CodeBPE found that allowing punctuation combinations can shorten code sequences substantially without a quality drop in their tested setup. More aggressive merging produced larger compression but could trade off quality. This supports designing **repeated canonical operator/grammar patterns** that tokenizers are likely to encode efficiently rather than pursuing arbitrary character golf.

Li & Lu's compiler-tokenization work goes further: lexical knowledge plus a reversible contextual dictionary reduced code-generation input token count by 33.7% in their experiment. Tessera can internalise a similar idea at the language boundary: common semantic forms have short canonical surface encodings, while identifiers and context can be represented through stable local dictionaries.

Recent 2026 work strengthens the warning against optimising one fixed BPE vocabulary:
- TOKDRIFT reports model-output sensitivity to semantics-preserving tokenization changes.
- Source-Attributed BPE shows code tokenizers can overfit repository/language-specific repeated strings and create under-trained tokens.
- Secret-leakage research demonstrates that character entropy and token entropy can diverge materially.

## Tessera strategy

### 1. Grammar-level density before tokenizer tricks

Prioritise:
- expression-oriented constructs;
- implicit terminators where unambiguous;
- local type inference;
- compact affine/move markers;
- one canonical spelling per common operation;
- tuples/records with positional shorthand when names are recoverable from type context;
- effect sets encoded as compact symbols/IDs;
- stable short standard-library namespaces;
- canonical literals and numeric suffixes.

Avoid:
- many synonymous keywords;
- optional punctuation that produces several equivalent token patterns;
- indentation-sensitive semantics unless it measurably helps tokenization;
- prose-like keywords;
- repeated long generic constraints at call sites.

### 2. Local symbol dictionaries

Long identifiers are useful at project boundaries but expensive when repeated.

A source unit may define a reversible local dictionary in its metadata representation:

```
sym {
  0 = audio_buffer_frames
  1 = sample_rate_hz
  2 = interleaved_output
}
```

Compact source can reference stable local slots while the Intent IR always expands them. The compiler must preserve debug names.

This should be evaluated against a simpler rule: canonical short lexical identifiers in TC + descriptive names stored in TIR/CTX.

### 3. Structural shorthand

Repeated semantic structures should have compact grammar forms instead of macros that hide arbitrary behaviour.

Illustrative only:

```
# verbose intent
function dot(a: borrow slice<f32>, b: borrow slice<f32>) -> f32
  requires length(a) == length(b)
  pure
  vectorizable

# possible compact surface
f dot(a:&[f],b:&[f])>f ?#= !0 ^v { ... }
```

Every compact token must map to a documented TIR construct.

### 4. Context layers do not all enter the prompt

A harness should request CTX subsets by task.

Examples:
- compilation: ABI/layout/feature constraints;
- bug fix: invariants + recent failing tests + dependency versions;
- migration: tech-stack and version evidence;
- performance: hot-path profile + target architecture;
- security: unsafe boundaries + trust model + relevant advisories.

This makes token efficiency a retrieval problem as well as a syntax problem.

## Tokenizer benchmark matrix

The benchmark runner should support adapters for at least:
- tiktoken/OpenAI-style tokenizers;
- Qwen code-model tokenizer;
- Llama-family tokenizer;
- DeepSeek-family tokenizer;
- StarCoder/code-specific tokenizer;
- a Tessera-native experimental tokenizer.

Report:
- raw bytes;
- characters;
- lexical tokens;
- model tokens per tokenizer;
- median and worst-case model tokens;
- tokens per TIR node;
- tokens per compiled instruction proxy;
- tokens per benchmark task solved;
- round-trip accuracy;
- model compile-success rate.

The optimisation target should be a weighted portfolio score, not the minimum under one tokenizer.

## Strong experimental precedent: toke

The Hugging Face repository [karwalski/toke-tokenizer](https://huggingface.co/karwalski/toke-tokenizer) describes a purpose-built 16K BPE tokenizer for the terse `toke` programming language and reports a 52% average reduction versus `cl100k_base` over its benchmark set. This is not independent validation of Tessera's design, but it is a useful engineering precedent: publish the tokenizer, corpus methodology and side-by-side benchmark programs rather than making unmeasured density claims.

## Proposed benchmark corpus

Implement the same tasks in:
- Tessera TC
- Tessera TIR/readable form
- Rust
- C
- Zig
- Odin
- Go (where applicable)

Task families:
- scalar algorithms;
- collections;
- file/network I/O;
- parsing;
- SIMD/DSP;
- concurrency;
- FFI;
- allocators;
- embedded/no-std;
- unsafe pointer manipulation;
- realistic multi-module bug fixes.

## Hypotheses to test

H1. TC median token count <= 65% of idiomatic Rust for equivalent benchmark programs.

H2. TC has lower variance across model tokenizers than ad-hoc minified Rust.

H3. Intent expansion improves model debugging accuracy enough to offset its larger token count when diagnosis is requested.

H4. Context-lattice retrieval reduces whole-task prompt tokens more than syntax compression alone on repository-scale tasks.

H5. Canonical formatting reduces representation-induced model variance.

## References

- N. Chirkova, S. Troshin, [CodeBPE: Investigating Subtokenization Options for Large Language Model Pretraining on Source Code](https://arxiv.org/abs/2308.00683), 2023.
- Z. Li, X. Lu, [Research on Compressed Input Sequences Based on Compiler Tokenization](https://doi.org/10.3390/info16020073), Information 16(2):73, 2025.
- [TOKDRIFT: When LLM Speaks in Subwords but Code Speaks in Grammar](https://aclanthology.org/2026.acl-long.2199/), ACL 2026.
- P. Chizhov et al., [From Where Words Come: Efficient Regularization of Code Tokenizers Through Source Attribution](https://aclanthology.org/2026.acl-long.1812/), ACL 2026.
- M. Chen et al., [Understanding Secret Leakage Risks in Code LLMs: A Tokenization Perspective](https://aclanthology.org/2026.findings-acl.6/), Findings of ACL 2026.
- [Hugging Face Tokenizers documentation](https://huggingface.co/docs/tokenizers/).
