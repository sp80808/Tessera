# `tsr check` diagnostics (`tessera.diagnostics/v0`)

Status: **provisional contract** (issue #23, first slice). Field names may
change only with a new `schema` value.

Compiler phases produce structured diagnostics and never print. The driver
resolves each one into a single record (code, severity, phase, byte span,
line/column, message, `help`, `fixes`) and renders that same record in one of
three forms, so no form can carry a fact the others lack.

```text
tsr check [--diagnostic=human|dense|json] FILE
```

Exit 0 when there are no errors, 1 when there are, 2 on a usage error.

## `human` (default, stderr)

```text
examples/witness/semantic_error.tes:1:20: error[E-resolve-unbound-name]: unbound variable `b` (not a parameter of `add`)
  help: `b` is not a parameter; did you mean `a`? (parameters: a)
  suggestion (use parameter `a`; passes check): f add(a:i64)>i64=a+a
```

## `dense` (stderr)

One line per diagnostic, `line:col CODE message`, with `; help: ...` when there
is help. Warnings and notes add `(warning)` / `(note)` after the code; errors
do not. Each checked whole-file suggestion follows as `= SOURCE`.

```text
1:20 E-resolve-unbound-name unbound variable `b` (not a parameter of `add`); help: `b` is not a parameter; did you mean `a`? (parameters: a)
= f add(a:i64)>i64=a+a
```

## `json` (stdout)

```jsonc
{
  "schema": "tessera.diagnostics/v0",
  "path": "examples/witness/semantic_error.tes",
  "ok": false,                       // false exactly when a diagnostic is an error
  "diagnostics": [
    { "phase": "resolve", "severity": "error", "code": "E-resolve-unbound-name",
      "message": "unbound variable `b` (not a parameter of `add`)",
      "span": { "start": 19, "end": 20 }, "line": 1, "col": 20,
      "help": "`b` is not a parameter; did you mean `a`? (parameters: a)",
      "fixes": [ { "span": { "start": 19, "end": 20 }, "replacement": "a",
                   "label": "use parameter `a`" } ] }
  ],
  "suggestions": [
    { "source": "f add(a:i64)>i64=a+a\n", "label": "use parameter `a`", "checked": "check" }
  ]
}
```

Each diagnostic object is exactly a [`tessera.witness/v0`](witness.md)
diagnostic, so consumers parse one shape whichever command produced it.
`checked: "check"` means the suggestion passes `tsr check`; it says nothing
about behaviour.

## Ordering and cascades

Diagnostics are in the compiler's canonical order (span, then phase, severity,
code, message), independent of pass execution order. When the source reads as
another language, an `E-syntax-foreign` summary comes first and the parser
errors after the first one carry no advice of their own.

## Not yet in this slice

- secondary spans, and HIR/TIR/MIR/TCap IDs on a record;
- an explicit `caused_by` link for cascade suppression (today only the
  foreign-syntax rule above);
- `--diagnostic` on `tsr tir`, `tsr mir` and `tsr run`, which still print the
  human form without advice.

A type mismatch cannot be produced from TC v0 source (it has one type,
`i64`), so its rendering is pinned by a unit test that builds the typeck
diagnostic directly.
