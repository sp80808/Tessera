# Provisional syntax lexicon register

Status: **provisional**. This register lists what the *lexer* recognizes and what the *grammar* accepts. It exists so lexicon growth is visible and cannot silently become language design.

## Rules

- The lexer's vocabulary is **not** the language. Recognizing a character lets diagnostics name it (`unexpected `-``) instead of reporting an opaque error; it does not accept it.
- Adding a lexeme *kind* is cheap and reversible. **Accepting a lexeme in the grammar** is a language-syntax change and requires tokenizer + model-quality evidence (#1/#2, AGENTS.md rule) and, for structural forms, an RFC.
- Multi-character operators (`==`, `->`, `&&`, `::`) are **not** lexemes. Whether one exists, and how it tokenizes across model families, is a grammar decision; the parser would form it from adjacent single-character tokens (byte-adjacent, no trivia between).
- Keywords are parser decisions. The lexer emits `Ident`; `f` is a keyword only inside the current parser.
- Every `TokenKind` must have a row here (enforced by `crates/tessera-syntax/tests/lexicon_register.rs`).

## Promotion rule

A row whose `Grammar` column is anything other than `no`, `ignored` or `n/a` (the grammar accepts the lexeme) must have an `Evidence` cell that is either:

1. **a benchmark reference** `bench:<corpus_hash prefix>/<program id>`: the prefix is at least 12 lowercase hex characters and must be a prefix of `corpus_hash` in `bench/results/baseline.json`, and the program id must exist in `bench/corpus/corpus.json`. Several references may be listed, separated by commas; or
2. **a named, dated exemption**: the literal text `bootstrap only`, plus an entry for that kind in the `EXEMPT` constant of `crates/tessera-tokenbench/tests/evidence_gates.rs` (kind, `since` date, reason). Today all nine accepted rows are exempt with the reason "predates the gate; revisit when #1 has >= 4 families".

Exemptions expire. The test fails if an exempt row gains a real reference, is demoted to `no`, or is deleted, until its `EXEMPT` entry is removed, so the allow-list cannot rot. A `bench:` reference that does not resolve is an error in any row, accepted or not.

What a reference proves, and what it does not: it proves the claim names the *current* committed benchmark artifact. It does not prove that the artifact supports the claim, that the program exercises the lexeme, or that the artifact covers enough tokenizer families (issue #1 asks for >= 4 vendor families; `bench/README.md` tracks that separately). Regenerating the baseline after a corpus change changes `corpus_hash` and therefore invalidates every reference, on purpose: re-read the new artifact and update the citations in the same change. Promotion also needs model-quality evidence (#2) and, for structural forms, an RFC (see `docs/rfcs/README.md`, Evidence gate).

## Register

`Grammar` = accepted by the current tiny grammar (`examples/bootstrap.tes`). `Evidence` is a benchmark reference or an explicit exemption (see Promotion rule). No row carries a benchmark reference yet: the accepted rows are all `bootstrap only` exemptions, i.e. none of the rows below has been promoted on evidence.

| Kind | Spelling | Lexer | Grammar | Evidence |
|---|---|---|---|---|
| `Whitespace` | ASCII whitespace run | trivia | ignored | n/a |
| `LineComment` | `//` to end of line (excl. newline) | trivia | ignored | n/a |
| `Ident` | `[A-Za-z_][A-Za-z0-9_]*` (ASCII only) | yes | names, types, keyword `f` | bootstrap only |
| `Int` | `[0-9]+` | yes | literal | bootstrap only |
| `Colon` | `:` | yes | param/type separator | bootstrap only |
| `Comma` | `,` | yes | param separator | bootstrap only |
| `LParen` | `(` | yes | params, grouping | bootstrap only |
| `RParen` | `)` | yes | params, grouping | bootstrap only |
| `Gt` | `>` | yes | return-type introducer | bootstrap only |
| `Eq` | `=` | yes | body introducer | bootstrap only |
| `Plus` | `+` | yes | left-assoc addition | bootstrap only |
| `Minus` | `-` | yes | **no** | none |
| `Star` | `*` | yes | **no** | none |
| `Lt` | `<` | yes | **no** | none |
| `Bang` | `!` | yes | **no** | none |
| `Amp` | `&` | yes | **no** | none |
| `Dot` | `.` | yes | **no** | none |
| `Semi` | `;` | yes | **no** | none |
| `LBrace` | `{` | yes | **no** | none |
| `RBrace` | `}` | yes | **no** | none |
| `LBracket` | `[` | yes | **no** | none |
| `RBracket` | `]` | yes | **no** | none |
| `Error` | any other single character (whole UTF-8 char) | yes | **no** | n/a |

Notes and known limits:

- `Ident` is ASCII-only; non-ASCII identifiers become `Error` tokens. Whether TC allows Unicode identifiers is undecided.
- No string, char, float, or negative-literal lexemes exist. `-` is only the `Minus` token; unary/negative literal handling is a grammar decision.
- Candidate lexemes for competing grammars (#1/#2) should be added as new rows with Grammar = **no** first, and promoted only with evidence (a `bench:` reference; a new `EXEMPT` entry is not an acceptable way to promote a new lexeme).
