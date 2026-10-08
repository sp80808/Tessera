//! One diagnostic record, three renderings (issue #23).
//!
//! Compiler phases produce [`DiagnosticSet`]s and never print. This module
//! turns one set into [`Record`]s, which carry every fact any renderer shows
//! (code, severity, phase, span, line/column, message, `help`, `fixes`), and
//! then renders the *same* records as:
//!
//! - `human`: `path:line:col: error[CODE]: message`, with indented `help:` and
//!   `suggestion` lines (what `tsr check` has always printed);
//! - `dense`: one terse line per diagnostic for agent repair loops;
//! - `json`: the `tessera.diagnostics/v0` document.
//!
//! The JSON form of one record is exactly a `tessera.witness/v0` diagnostic,
//! so a consumer parses one shape whichever command produced it.

use serde_json::{Value as Json, json};
use tessera_phases::{Diagnostic, DiagnosticSet, Phase, Severity};

use crate::advice::{self, Fix, Suggestion};
use crate::line_col;

pub const SCHEMA: &str = "tessera.diagnostics/v0";

/// A diagnostic with every fact a renderer may show resolved against the
/// source text. Byte offsets; `line`/`col` are 1-based (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub phase: &'static str,
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
    pub help: Option<String>,
    pub fixes: Vec<Fix>,
}

/// `--diagnostic=` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Dense,
    Json,
}

impl Format {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "human" => Some(Self::Human),
            "dense" => Some(Self::Dense),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

#[must_use]
pub const fn severity(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    }
}

/// `d` without advice.
#[must_use]
pub fn plain(text: &str, d: &Diagnostic) -> Record {
    let span = d.at.primary_span();
    let (start, end) = (span.start as usize, span.end as usize);
    let (line, col) = line_col(text, start);
    Record {
        phase: d.phase.name(),
        severity: severity(d.severity),
        code: d.code,
        message: d.message.clone(),
        start,
        end,
        line,
        col,
        help: None,
        fixes: Vec::new(),
    }
}

/// The records of a TC front-end run, with `help`/`fixes` from [`advice`],
/// the `E-syntax-foreign` summary first when the source reads as another
/// language, and the whole-file suggestions that pass `check` (only when
/// there are errors).
#[must_use]
pub fn advised(text: &str, diagnostics: &DiagnosticSet) -> (Vec<Record>, Vec<Suggestion>) {
    let report = advice::report(text, diagnostics);
    let mut records: Vec<Record> = diagnostics
        .iter()
        .zip(&report.advice)
        .map(|(d, a)| Record {
            help: a.help.clone(),
            fixes: a.fixes.clone(),
            ..plain(text, d)
        })
        .collect();
    if let Some((start, end, message)) = &report.foreign {
        let (line, col) = line_col(text, *start);
        records.insert(
            0,
            Record {
                phase: Phase::Syntax.name(),
                severity: "error",
                code: advice::FOREIGN_CODE,
                message: message.clone(),
                start: *start,
                end: *end,
                line,
                col,
                help: Some(format!(
                    "a TC program is one function: `{}`",
                    advice::TEMPLATE
                )),
                fixes: Vec::new(),
            },
        );
    }
    let suggestions = if diagnostics.has_errors() {
        advice::suggestions(text, &report.advice)
    } else {
        Vec::new()
    };
    (records, suggestions)
}

fn fix_json(f: &Fix) -> Json {
    json!({
        "span": { "start": f.start, "end": f.end },
        "replacement": f.replacement,
        "label": f.label,
    })
}

/// One record as JSON: the `tessera.witness/v0` diagnostic shape.
#[must_use]
pub fn to_json(r: &Record) -> Json {
    json!({
        "phase": r.phase,
        "severity": r.severity,
        "code": r.code,
        "message": r.message,
        "span": { "start": r.start, "end": r.end },
        "line": r.line,
        "col": r.col,
        "help": r.help,
        "fixes": r.fixes.iter().map(fix_json).collect::<Vec<_>>(),
    })
}

/// `path:line:col: severity[CODE]: message`, then `  help:` and, after all
/// diagnostics, `  suggestion (...; passes check):` lines.
#[must_use]
pub fn human(path: &str, records: &[Record], suggestions: &[Suggestion]) -> String {
    let mut out = String::new();
    for r in records {
        out.push_str(&format!(
            "{path}:{}:{}: {}[{}]: {}\n",
            r.line, r.col, r.severity, r.code, r.message
        ));
        if let Some(help) = &r.help {
            out.push_str(&format!("  help: {help}\n"));
        }
    }
    for s in suggestions {
        out.push_str(&format!(
            "  suggestion ({}; passes check): {}",
            s.label, s.source
        ));
    }
    out
}

/// One line per diagnostic, `line:col CODE message`, with `; help: ...` when
/// there is help; warnings and notes carry their severity, errors do not.
/// Each checked suggestion follows as `= <source>` on its own line.
#[must_use]
pub fn dense(records: &[Record], suggestions: &[Suggestion]) -> String {
    let mut out = String::new();
    for r in records {
        out.push_str(&format!("{}:{} {}", r.line, r.col, r.code));
        if r.severity != "error" {
            out.push_str(&format!(" ({})", r.severity));
        }
        out.push(' ');
        out.push_str(&r.message);
        if let Some(help) = &r.help {
            out.push_str("; help: ");
            out.push_str(help);
        }
        out.push('\n');
    }
    for s in suggestions {
        out.push_str("= ");
        out.push_str(s.source.trim_end());
        out.push('\n');
    }
    out
}

/// The `tessera.diagnostics/v0` document. `ok` is false exactly when some
/// record is an error.
#[must_use]
pub fn document(path: &str, records: &[Record], suggestions: &[Suggestion]) -> Json {
    json!({
        "schema": SCHEMA,
        "path": path,
        "ok": !records.iter().any(|r| r.severity == "error"),
        "diagnostics": records.iter().map(to_json).collect::<Vec<_>>(),
        "suggestions": suggestions.iter().map(|s| json!({
            "source": s.source,
            "label": s.label,
            "checked": "check",
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_phases::{FileId, Provenance, Span};

    use crate::pipeline;

    fn check(text: &str) -> (Vec<Record>, Vec<Suggestion>) {
        advised(text, &pipeline::check_tc(text).diagnostics)
    }

    /// Golden: one malformed parse rendered three ways from the same records.
    #[test]
    fn malformed_parse_renders_identical_facts() {
        let text = "f add(a:i64,b:i64)>i64=a+\n";
        let (records, suggestions) = check(text);
        assert_eq!(
            human("t.tes", &records, &suggestions),
            "t.tes:2:1: error[E-syntax-expected]: expected expression, found end of input\n  \
             help: an operand is missing: EXPR is integers, parameters and `+`, e.g. `a+b`\n"
        );
        assert_eq!(
            dense(&records, &suggestions),
            "2:1 E-syntax-expected expected expression, found end of input; \
             help: an operand is missing: EXPR is integers, parameters and `+`, e.g. `a+b`\n"
        );
        assert_eq!(
            document("t.tes", &records, &suggestions),
            json!({
                "schema": "tessera.diagnostics/v0",
                "path": "t.tes",
                "ok": false,
                "diagnostics": [{
                    "phase": "syntax", "severity": "error", "code": "E-syntax-expected",
                    "message": "expected expression, found end of input",
                    "span": { "start": 26, "end": 26 }, "line": 2, "col": 1,
                    "help": "an operand is missing: EXPR is integers, parameters and `+`, e.g. `a+b`",
                    "fixes": [],
                }],
                "suggestions": [],
            })
        );
    }

    /// Golden: an unresolved name, with its fix and checked suggestion.
    #[test]
    fn unresolved_name_carries_fix_and_suggestion() {
        let text = "f add(a:i64)>i64=a+b\n";
        let (records, suggestions) = check(text);
        assert_eq!(
            dense(&records, &suggestions),
            "1:20 E-resolve-unbound-name unbound variable `b` (not a parameter of `add`); \
             help: `b` is not a parameter; did you mean `a`? (parameters: a)\n\
             = f add(a:i64)>i64=a+a\n"
        );
        let doc = document("t.tes", &records, &suggestions);
        assert_eq!(doc["diagnostics"][0]["phase"], "resolve");
        assert_eq!(
            doc["diagnostics"][0]["span"],
            json!({ "start": 19, "end": 20 })
        );
        assert_eq!(
            doc["diagnostics"][0]["fixes"][0],
            json!({ "span": { "start": 19, "end": 20 }, "replacement": "a", "label": "use parameter `a`" })
        );
        assert_eq!(doc["suggestions"][0]["source"], "f add(a:i64)>i64=a+a\n");
    }

    /// Golden: a type mismatch. TC v0 has one type, so the front end cannot
    /// produce one from source; the record is built from a typeck diagnostic
    /// directly to pin how the type phase renders.
    #[test]
    fn type_mismatch_renders_through_the_same_record() {
        let text = "f add(a:i64,b:i64)>i64=a+b\n";
        let mut ds = DiagnosticSet::new();
        ds.push(Diagnostic::error(
            Phase::Typeck,
            "E-type-mismatch",
            "`+` takes two i64 operands, found i64 and bool",
            Provenance::Source(Span::new(FileId(0), 23, 26)),
        ));
        let (records, suggestions) = advised(text, &ds);
        assert_eq!(
            human("t.tes", &records, &suggestions),
            "t.tes:1:24: error[E-type-mismatch]: `+` takes two i64 operands, found i64 and bool\n"
        );
        assert_eq!(
            dense(&records, &suggestions),
            "1:24 E-type-mismatch `+` takes two i64 operands, found i64 and bool\n"
        );
        assert_eq!(to_json(&records[0])["phase"], "typeck");
    }

    #[test]
    fn clean_source_has_no_records_and_is_ok() {
        let (records, suggestions) = check("f add(a:i64,b:i64)>i64=a+b\n");
        assert!(records.is_empty() && suggestions.is_empty());
        assert_eq!(dense(&records, &suggestions), "");
        assert_eq!(document("t.tes", &records, &suggestions)["ok"], true);
    }

    #[test]
    fn dense_names_non_error_severities() {
        let mut r = plain(
            "x",
            &Diagnostic::error(
                Phase::Syntax,
                "W-x",
                "m",
                Provenance::Source(Span::new(FileId(0), 0, 1)),
            ),
        );
        r.severity = "warning";
        assert_eq!(dense(&[r], &[]), "1:1 W-x (warning) m\n");
    }

    #[test]
    fn formats_parse_by_name() {
        assert_eq!(Format::parse("dense"), Some(Format::Dense));
        assert_eq!(Format::parse("json"), Some(Format::Json));
        assert_eq!(Format::parse("human"), Some(Format::Human));
        assert_eq!(Format::parse("prose"), None);
    }
}
