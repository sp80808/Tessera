# Evidence ledger and related work

Last research pass: 2026-09-29.

This is a curated evidence ledger, not an endorsement of every result. Tessera should prefer primary papers, official documentation and canonical repositories, then verify implementation claims against code.

## Tokenization and compact representations

### TokDrift — 2026

Yinxi Li, Yuntian Deng, Pengyu Nie. *TokDrift: When LLM Speaks in Subwords but Code Speaks in Grammar*. ACL 2026.

Why it matters: semantic-preserving formatting/tokenization changes can change code-model behavior; argues for grammar-aware treatment and a canonical Tessera source form.

- https://aclanthology.org/2026.acl-long.2199/
- code: https://github.com/uw-swag/tokdrift

### RepoDistill — 2026

Xin Yin et al. *RepoDistill: Distilling Repository Knowledge through Compression-Aware Budget Allocation and Policy Optimization*. Findings of ACL 2026.

Why it matters: separates graph retrieval from fine-grained context compression and reports reductions of up to 66% input tokens while maintaining comparable performance in a reported setup.

- https://aclanthology.org/2026.findings-acl.217/

### Compiler-tokenized compressed inputs — 2025

Zhe Li, Xinxi Lu. *Research on Compressed Input Sequences Based on Compiler Tokenization*.

Why it matters: compiler-assisted lexical compression + restoration dictionary; reported 33.7% input-token reduction.

- https://doi.org/10.3390/info16020073

### Dynamic tokenization — 2025

Darius Feher, Ivan Vulić, Benjamin Minixhofer. *Retrofitting Large Language Models with Dynamic Tokenization*. ACL 2025.

Why it matters: adaptive merging shows fixed token boundaries are not inherently optimal.

- https://aclanthology.org/2025.acl-long.1444/
- code: https://github.com/DariusFeher/dynamic-tokenization

### Token merging for code — 2025

Mootez Saad, Hao Li, Tushar Sharma, Ahmed E. Hassan. *On the Effect of Token Merging on Pre-trained Models for Code*.

Why it matters: semantic-unit merging in code models can reduce compute with task-dependent quality effects.

- https://arxiv.org/abs/2507.14423
- Hugging Face: https://huggingface.co/papers/2507.14423

### CodeBPE — 2023

Nadezhda Chirkova, Sergey Troshin. *CodeBPE: Investigating Subtokenization Options for Large Language Model Pretraining on Source Code*.

Why it matters: reported 17% average length reduction without downstream performance loss for code-aware punctuation grouping in the evaluated setup.

- https://arxiv.org/abs/2308.00683
- https://huggingface.co/papers/2308.00683

## Repository context, graphs and agents

### CodexGraph — NAACL 2025

*CodexGraph: Bridging Large Language Models and Code Repositories via Code Graph Databases*.

Why it matters: graph-database interface gives agents structural repository navigation instead of only similarity retrieval.

- https://aclanthology.org/2025.naacl-long.7/
- https://huggingface.co/papers/2408.03910
- implementation: https://github.com/modelscope/modelscope-agent/tree/master/apps/codexgraph_agent

### RepoGraph — 2024

*RepoGraph: Enhancing AI Software Engineering with Repository-level Code Graph*.

Why it matters: repository graph used as a plug-in context/navigation layer for software-engineering agents.

- https://huggingface.co/papers/2410.14684
- code: https://github.com/ozyyshr/RepoGraph

### Code Graph Model — 2025

*Code Graph Model (CGM): A Graph-Integrated Large Language Model for Repository-Level Software Engineering Tasks*.

Why it matters: integrates repository graph structure into model attention and also evaluates graph-RAG style use.

- https://huggingface.co/papers/2505.16901

### RANGER — 2025

*RANGER — Repository-Level Agent for Graph-Enhanced Retrieval*.

Why it matters: dual-stage retrieval across explicit code entities and natural-language queries.

- https://huggingface.co/papers/2509.25257

### In Line with Context — 2026

*In Line with Context: Repository-Level Code Generation via Context Inlining*.

Why it matters: emphasizes call-graph context and bidirectional dependency context.

- https://huggingface.co/papers/2601.00376

### SWE Context Bench — 2026

*SWE Context Bench: A Benchmark for Context Learning in Coding*.

Why it matters: explicitly evaluates retrieval/reuse of previous agent experience across related repository tasks.

- https://huggingface.co/papers/2602.08316

### ARISE — 2026

*ARISE: A Repository-level Graph Representation and Toolset for Agentic Fault Localization and Program Repair*.

Why it matters: multi-granularity graph with data-flow emphasis for fault localization/repair.

- https://huggingface.co/papers/2605.03117

### CodeGraph open taxonomy — 2026

*CodeGraph: Open-Taxonomy Knowledge Graph for Source Code with Wikidata Grounding*.

Why it matters: demonstrates large-scale semantic concept graphs over source code, beyond syntax-only relations.

- https://huggingface.co/papers/2609.29474

## Systems language semantics

### Rust Reference

Primary language semantics reference for ownership-adjacent behavior, references, unsafe operations and ABI/layout concepts.

- https://doc.rust-lang.org/reference/

### Functional Ownership through Fractional Uniqueness — 2024

Daniel Marshall, Dominic Orchard.

Why it matters: formal type-system direction for ownership/borrowing via graded and fractional uniqueness.

- https://doi.org/10.1145/3649848

### KRust — 2018

*KRust: A Formal Executable Semantics of Rust*.

Why it matters: executable formalization precedent for ownership/move/borrow semantics.

- https://doi.org/10.1109/TASE.2018.00014

## Compiler/tooling prior art

### Cranelift

Fast Rust-written compiler backend supporting JIT/AOT use.

- https://cranelift.dev/
- https://github.com/bytecodealliance/wasmtime/tree/main/cranelift

### LLVM

Industrial optimizing compiler infrastructure.

- https://llvm.org/docs/LangRef.html
- https://github.com/llvm/llvm-project

### QBE

Compact compiler backend intended to achieve useful native performance with a much smaller implementation surface.

- https://c9x.me/compile/

### Tree-sitter

Incremental parsing system useful as tooling precedent.

- https://github.com/tree-sitter/tree-sitter

### Joern

Code Property Graph implementation combining syntax/control/data relations.

- https://github.com/joernio/joern

### Aider

Useful repository-map precedent for providing compressed structural context to coding models.

- https://github.com/Aider-AI/aider

## Research connector status

During the 2026-09-29 research pass:
- GitHub, Context7, SciSpace, Exa, Parallel Search and Wolfram were queried successfully.
- Scite's connected account had reached its monthly MCP call limit.
- Consensus's connected account had reached its monthly search limit.

Those rate limits mean this ledger should be refreshed after reset; they are not evidence gaps that should be papered over with invented citations.

## Evidence policy for future agents

When promoting a research idea into language design:
1. record the primary source;
2. distinguish reported result from Tessera inference;
3. note evaluation population/task/model;
4. reproduce when feasible;
5. add a benchmark before freezing syntax/semantics;
6. retain contradicting evidence in the graph.
