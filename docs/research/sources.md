# Research Provenance and Source Register

Last updated: 2026-09-29

## Academic / technical sources

| Source | Year | Used for |
|---|---:|---|
| [CodeBPE: Investigating Subtokenization Options for Large Language Model Pretraining on Source Code](https://arxiv.org/abs/2308.00683) | 2023 | code-specific tokenization; punctuation grouping; sequence length |
| [Functional Ownership through Fractional Uniqueness](https://doi.org/10.1145/3649848) | 2024 | ownership/borrowing as type-level discipline |
| [Research on Compressed Input Sequences Based on Compiler Tokenization](https://doi.org/10.3390/info16020073) | 2025 | compiler lexical compression; contextual restoration |
| [CoSIL: Software Issue Localization via LLM-Driven Code Repository Graph Searching](https://arxiv.org/abs/2503.22424) | 2025 | graph-guided repo context selection |
| [Context Engineering for AI Agents in Open-Source Software](https://arxiv.org/abs/2510.21413) | 2025 | real-world coding-agent context files |
| [TOKDRIFT: When LLM Speaks in Subwords but Code Speaks in Grammar](https://aclanthology.org/2026.acl-long.2199/) | 2026 | sensitivity to semantic-preserving tokenization changes |
| [From Where Words Come: Efficient Regularization of Code Tokenizers Through Source Attribution](https://aclanthology.org/2026.acl-long.1812/) | 2026 | source diversity, BPE overfitting, under-trained tokens |
| [Understanding Secret Leakage Risks in Code LLMs: A Tokenization Perspective](https://aclanthology.org/2026.findings-acl.6/) | 2026 | token-level versus character-level entropy/security |

## Official technical documentation

- [Rust Reference](https://doc.rust-lang.org/reference/)
- [Cranelift](https://cranelift.dev/)
- [Wasmtime / Cranelift](https://github.com/bytecodealliance/wasmtime/tree/main/cranelift)
- [LLVM](https://llvm.org/docs/)
- [QBE](https://c9x.me/compile/)
- [Hugging Face Tokenizers](https://huggingface.co/docs/tokenizers/)

## Repositories/projects queued for comparative study

- [sp80808/Tessera](https://github.com/sp80808/Tessera)
- [ParsifalXu/CoSIL](https://github.com/ParsifalXu/CoSIL)
- [r3tr0-afk/repo-graph-rag-agent](https://github.com/r3tr0-afk/repo-graph-rag-agent)
- [mikekonan/cograph](https://github.com/mikekonan/cograph)
- [artk0de/TeaRAGs-MCP](https://github.com/artk0de/TeaRAGs-MCP)
- [foyzulkarim/hikma-engine](https://github.com/foyzulkarim/hikma-engine)
- [robertkarlsson2-design/ION](https://github.com/robertkarlsson2-design/ION)
- [karwalski/toke-tokenizer](https://huggingface.co/karwalski/toke-tokenizer)

## Connector/research notes

Research for this baseline used GitHub, Context7, Exa, Parallel Search, SciSpace and Wolfram sources.

At the time of this research pass:
- the Scite MCP connection had reached its monthly request limit;
- the Consensus MCP connection had reached its monthly search limit;
- the installed Hugging Face connector advertised paper/model/space discovery actions but returned action-not-found errors in this session, so public Hugging Face pages were used for the limited HF examples instead.

These are **coverage limitations**, not evidence against those sources. A later research pass should revisit Scite/Consensus citation context and Hugging Face discovery.

## Evidence discipline

When adding a claim:
1. prefer primary paper/spec/docs;
2. record exact version/date where relevant;
3. distinguish a project's self-reported benchmark from independent evidence;
4. do not promote model-generated inference to fact without verification;
5. mark stale external evidence when its invalidation condition is met.
