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


## Additional 2026 tokenizer and repository-reasoning evidence

### Source-Attributed BPE — ACL 2026

Pavel Chizhov, Egor Bogomolov, Ivan P. Yamshchikov. *From Where Words Come: Efficient Regularization of Code Tokenizers Through Source Attribution*.

Why it matters: shows code-tokenizer vocabulary can overfit to source/repository imbalance and create under-trained tokens; motivates multi-repository, multi-language token benchmarking for Tessera.

- https://aclanthology.org/2026.acl-long.1812/

### Stop Taking Tokenizers for Granted — EACL 2026

Sawsan Alqahtani et al. *Stop Taking Tokenizers for Granted: They Are Core Design Decisions in Large Language Models*.

Why it matters: reinforces treating tokenizer design/behavior as a first-class model-system decision rather than preprocessing trivia.

- https://aclanthology.org/2026.eacl-long.394/

### RepoReason — ACL 2026

Jia Li, Yuxin Su, Michael R. Lyu. *From Laboratory to Real-World Applications: Benchmarking Agentic Code Reasoning at the Repository Level*.

Why it matters: introduces white-box repository reasoning diagnostics and reports integration width as a major bottleneck in evaluated frontier models. Tessera should therefore measure not only context tokens but how many disconnected facts a model must integrate.

- https://aclanthology.org/2026.acl-long.399/

### GALLa — ACL 2025

Ziyin Zhang et al. *GALLa: Graph Aligned Large Language Models for Improved Source Code Understanding*.

Why it matters: emphasizes semantic program information such as data flow that ordinary source text does not explicitly provide to code LLMs.

- https://aclanthology.org/2025.acl-long.676/

### Knowledge Graph Based Repository-Level Code Generation — 2025

Mihir Athale, Vishal Vaddina.

Why it matters: repository code represented structurally as a graph can improve context-aware retrieval/generation versus flatter retrieval baselines in the reported experiments.

- https://arxiv.org/abs/2505.14394

## GitHub reconnaissance — 2026-09-29

Searches for repository graph agents, token-efficient languages and Rust/Cranelift language implementations surfaced the following **implementation leads**, not authoritative evidence:

- https://github.com/r3tr0-afk/repo-graph-rag-agent
- https://github.com/mikekonan/cograph
- https://github.com/Cheesecaster/RepoCortex
- https://github.com/robertkarlsson2-design/ION
- https://github.com/AeroForger/Sydrogen
- https://github.com/fajarkraton/fajar-lang
- https://github.com/agam-lang/agam

Future research agents should inspect architecture, activity, license and actual implementation before borrowing patterns.

## Hugging Face connector status

The 2026-09-29 environment exposed Hugging Face search actions in connector metadata, but calls to paper/model/space search returned a runtime "tool not found" error. Existing Hugging Face paper links above remain useful navigational references, but no claim in this research pass should be described as having been validated through the live HF search connector.


## Context engineering and AI-native language precedents

### Context Engineering for AI Agents in Open-Source Software — 2025

Seyedmoein Mohsenimofidi, Matthias Galster, Christoph Treude, Sebastian Baltes.

Why it matters: studies agent-context/configuration files across 466 open-source projects and reports substantial variation in descriptive, prescriptive, prohibitive, explanatory and conditional information. Tessera's Context Lattice is a proposal to make those context classes scoped, typed, queryable and freshness-aware rather than relying only on free-form prompt files.

- https://arxiv.org/abs/2510.21413

### toke — AI-oriented compiled language

toke is an independent compiled-language project explicitly optimized for LLM code generation. Its current documentation emphasizes a small grammar, one canonical form, structured diagnostics and measured token/model gates. Importantly, the project also documents negative/superseded measurements and a corrected Pass@1 denominator, which is a good precedent for Tessera's evidence discipline.

Useful Tessera comparisons:
- canonical-form grammar and bounded parsing;
- native compilation;
- tokenizer benchmark methodology;
- structured repair-loop diagnostics;
- source/documentation separation.

Do **not** copy its self-reported token reductions into Tessera claims; reproduce equivalent measurements independently.

- https://tokelang.dev/
- https://tokelang.dev/docs/learn/01-why-toke
- tokenizer artifact: https://huggingface.co/karwalski/toke-tokenizer

### ION — token-efficient AI-native transpiled language

ION describes itself as a token-efficient AI-native programming language compiling to JavaScript, TypeScript and Python. It is especially useful as a contrast case because Tessera targets low-level native systems semantics rather than a compact transpilation surface.

- https://github.com/robertkarlsson2-design/ION

### Qwen3-Coder — long-context agentic code-model target

Qwen's current Hugging Face model cards describe Qwen3-Coder variants as agentic coding models with 256K native context (with larger extended-context configurations). Tessera should include at least one Qwen3-Coder tokenizer/model in the harness benchmark portfolio, while still testing whether graph retrieval beats indiscriminate long-context stuffing.

- https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct

## Research principle added from comparable projects

End-to-end agent efficiency should be measured as more than source tokens. A useful experimental decomposition is:

```
cost(task) =
  input_tokens
  + output_tokens
  + repair_tokens
  + retrieved_context_tokens
  + verification_cost
```

and quality must be tracked simultaneously. A language that saves 30% source tokens but causes substantially more repair turns is not a successful LLM-native language.
