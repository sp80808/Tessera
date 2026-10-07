# Grammar promotion (gated by tokenbench)

No TC syntax proposal is marked **stable** without a `tess-tokenbench` artifact.

## Required evidence

1. Run against the checked-in corpus:

```bash
./scripts/fetch-tokenizers.sh   # optional: measure pinned HF families
cargo run -p tessera-tokenbench -- run
cargo run -p tessera-tokenbench -- check \
  --baseline bench/results/baseline-portfolio.json \
  --results bench/results/latest.json
```

2. Attach or link the JSON (+ Markdown) report from that run.
3. The report must identify tokenizer versions/revisions (embedded crate encodings and/or pinned HF `sha256` + `revision`).
4. At least **four distinct model tokenizer families** must be `measured` (OpenAI tiktoken encodings count as one vendor portfolio; HF families such as Qwen / StarCoder2 / DeepSeek / Mistral count separately).
5. CI `tokenbench` job must stay green on the proposal branch (regression gate ≤ 2% median/worst growth on `parsed` Tessera variants).

## Why shorter can still lose

Model-token cost is not character count. A visually compact spelling can:

- expand under an older or code-specialized vocabulary;
- split grammar units across subword boundaries (TokDrift-style sensitivity);
- raise tokens-to-verified-success if models need more repair turns.

Promotion is a **Pareto** decision across representation cost, verified task success, rewrite robustness, and tokenizer/family diversity — not a single scalar win on one encoding.

## Related

- Issue [#1](https://github.com/sp80808/Tessera/issues/1)
- Research notes: `docs/research/token-efficiency.md`
