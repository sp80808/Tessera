# Provisional syntax lexicon register

Status: **provisional**. This register lists what the *lexer* recognizes and what the *grammar* accepts. It exists so lexicon growth is visible and cannot silently become language design.

## Rules

- The lexer's vocabulary is **not** the language. Recognizing a character lets diagnostics name it (`unexpected `-``) instead of reporting an opaque error; it does not accept it.
- Adding a lexeme *kind* is cheap and reversible. **Accepting a lexeme in the grammar** is a language-syntax change and requires tokenizer + model-quality evidence (#1/#2, AGENTS.md rule) and, for structural forms, an RFC.
- Multi-character operators (`==`, `->`, `&&`, `::`) are **not** lexemes. Whether one exists, and how it tokenizes across model families, is a grammar decision; the parser would form it from adjacent single-character tokens (byte-adjacent, no trivia between).
- Keywords are parser decisions. The lexer emits `Ident`; `f` is a keyword only inside the current parser.
- Every `TokenKind` must have a row here (enforced by `crates/tessera-syntax/tests/lexicon_register.rs`).

## Register

`Grammar` = accepted by the current tiny grammar (`examples/bootstrap.tes`). `Evidence` refers to #1/#2 benchmarks; none of the rows below has been promoted.

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
- Candidate lexemes for competing grammars (#1/#2) should be added as new rows with Grammar = **no** first, and promoted only with evidence.
