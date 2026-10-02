# `tsr witness`: compiler evidence for orchestrators (`tessera.witness/v0`)

Status: **provisional contract** (issue #41). Field names may change only with
a new `schema` value.

`tsr witness` runs the real compiler on one file and prints a single JSON
document on stdout. An orchestrator (Lattice, through TAP) stores and replays
that document instead of scraping `tsr check` prose. A model's opinion never
substitutes for it: `outcome` comes only from Tessera execution.

## One command

```sh
cargo run -q -p tessera-cli -- witness examples/witness/pass.tes
```

```text
tsr witness [--phase=check|mir|backend] [--overflow=wrapping|trapping] FILE
```

- `--phase=check` (default): syntax, HIR, name resolution, type checking, TIR.
- `--phase=mir`: `check`, then MIR lowering and the MIR verifier. Requires
  `--overflow` because integer overflow semantics are open question O1.
- `--phase=backend`: not implemented; always `unsupported`.
- `FILE` ending in `.tir` is read as TIR (TIR reader + TIR verifier instead of
  the TC front end); anything else as TC.

## Outcomes and exit codes

| `outcome`     | exit | meaning                                                         |
|---------------|------|-----------------------------------------------------------------|
| `pass`        | 0    | every requested phase ran and reported no errors                |
| `fail`        | 1    | the compiler rejected the input; see `diagnostics`              |
| `unsupported` | 3    | the requested phase is not implemented; no result is fabricated |
| `tool_error`  | 4    | the run broke (unreadable input, a compiler invariant violated) |

Exit 2 is a usage error: no document is printed, the message goes to stderr.

## Document

```jsonc
{
  "schema": "tessera.witness/v0",
  "tool": { "name": "tsr", "version": "0.0.1", "commit": "<40-hex or unknown>", "dirty": false },
  "invocation": { "phase": "check", "overflow": null, "input": "tc", "path": "examples/witness/pass.tes" },
  "outcome": "pass",
  "source": { "sha256": "<hex>", "bytes": 27 },          // null on tool_error before reading
  "phases": [ { "phase": "syntax", "status": "pass" }, ... ],
  "diagnostics": [   // from syntax_error.tes; empty for pass.tes
    { "phase": "syntax", "severity": "error", "code": "E-syntax-expected",
      "message": "expected expression, found end of input",
      "span": { "start": 26, "end": 26 }, "line": 2, "col": 1 }
  ],
  "artifacts": {
    "tir": { "sha256": "<hex>", "bytes": 92, "functions": 1, "roundtrip": true },
    "mir": { "sha256": "<hex>", "bytes": 230, "functions": 1 }   // --phase=mir only
  },
  "representations": {
    "source": { "sha256": "<hex>", "bytes": 27, "tessera_tokens": 17,
                "tokenizers": { "cl100k_base": 14, "o200k_base": 14 } },
    "tir":    { "sha256": "<hex>", "bytes": 92, "tessera_tokens": null,
                "tokenizers": { "cl100k_base": 37, "o200k_base": 37 } }
  },
  "error": null,
  "result_id": "sha256:<hex>",
  "timing": { "compile_us": 382, "total_us": 1067600 }
}
```

- `tool.commit` / `tool.dirty` are recorded at build time from git
  (`TSR_GIT_COMMIT` / `TSR_GIT_DIRTY` override them for builds without
  `.git`). `dirty: true` means the binary was built from uncommitted changes;
  `null` means it could not be determined.
- `phases[].status` is `pass`, `fail`, `not_run` (an earlier phase failed and
  this one needs its output) or `unsupported`. The TC front-end phases are
  tolerant and all run, so a later phase may `pass` on what an earlier one
  recovered; `outcome` is the verdict.
- `diagnostics[].span` is a byte range into the source; `line`/`col` are
  1-based, `col` in bytes. TIR verifier findings carry no span (`null`).
- `artifacts.tir` exists only when the front end passed. `roundtrip` says the
  canonical TIR text parses back to the identical module.
- `representations` are measurements, not claims: byte counts, Tessera lexer
  tokens (non-trivia, TC source only) and real counts under the two
  vocabularies embedded in `tiktoken-rs`. No compression ratio is reported;
  compare representations yourself and only across the same fixture.
- `result_id` hashes the document without `timing`, `invocation.path` and
  itself. Identical source bytes, phase, overflow mode and `tsr` build give
  an identical `result_id` and an identical document apart from `timing`.
- `timing.compile_us` covers the compiler phases only; `total_us` adds
  tokenizer counting (the tokenizer tables load once per process). Timing is
  never part of `result_id`.

## Fixtures

`examples/witness/` holds the issue #41 fixtures, exercised end to end by
`crates/tessera-cli/tests/cli.rs`:

| fixture                       | outcome                        |
|-------------------------------|--------------------------------|
| `pass.tes`                    | `pass`                         |
| `syntax_error.tes`            | `fail` (`E-syntax-expected`)   |
| `semantic_error.tes`          | `fail` (`E-resolve-unbound-name`) |
| `pass.tes --phase=backend`    | `unsupported`                  |

TC currently has only the `i64` type, so no TC source reaches a type-check
error; `semantic_error.tes` fails name resolution, the semantic phase TC can
reach today. A type-error fixture lands when TC gains a second type.
