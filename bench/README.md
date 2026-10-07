# Tokenbench corpus and baselines

- `corpus/` — paired Tessera / Rust (and other) micro-programs.
- `tokenizers.json` — tokenizer manifest. HF entries with `sha256` + `revision` are fetchable; Llama stays unpinned (gated license).
- `tokenizers/` — **gitignored** vocabulary files. Populate with `./scripts/fetch-tokenizers.sh`.
- `results/baseline.json` — **embedded-only** (OpenAI tiktoken) hermetic baseline used by unit tests.
- `results/baseline-portfolio.json` — full measured portfolio after fetch (OpenAI + pinned HF families). CI `tokenbench` gates on this file.

Regenerate hermetic baseline:

```bash
cargo run -p tessera-tokenbench --release -- run \
  --tokenizers-dir /tmp/no-tokenizers \
  --out-json bench/results/baseline.json \
  --out-md bench/results/baseline.md
```

Regenerate portfolio baseline (after fetch):

```bash
./scripts/fetch-tokenizers.sh
cargo run -p tessera-tokenbench --release -- run \
  --out-json bench/results/baseline-portfolio.json \
  --out-md bench/results/baseline-portfolio.md
```

Grammar promotion rules: `docs/grammar-promotion.md`.
