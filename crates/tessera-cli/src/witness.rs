//! `tsr witness`: one compiler run as versioned, machine-readable evidence
//! (issue #41), so an orchestrator such as Lattice can store and replay the
//! result without scraping human CLI text.
//!
//! The document is `tessera.witness/v0` (see `docs/spec/witness.md`). Its
//! `outcome` separates the four cases a caller must not confuse:
//! - `pass`: every requested phase ran and reported no errors;
//! - `fail`: the compiler rejected the input, with structured diagnostics;
//! - `unsupported`: the requested phase is not implemented, so there is no
//!   result to report (never a fabricated one);
//! - `tool_error`: the run itself broke (unreadable input, a compiler
//!   invariant violated), which says nothing about the program.
//!
//! Everything but `timing` and `invocation.path` is a pure function of the
//! tool build and the source bytes; `result_id` hashes exactly that part, so
//! repeated runs of one fixture produce one `result_id`.

use std::time::Instant;

use serde_json::{Map, Value as Json, json};
use sha2::{Digest, Sha256};
use tessera_mir::{LowerOptions, OverflowMode, lower_module};
use tessera_phases::{Diagnostic, DiagnosticSet, Phase, Severity};
use tessera_syntax::lexer;
use tessera_tir::{ModuleProvenance, TirModule};

use crate::advice;
use crate::diagnostics;
use crate::line_col;
use crate::pipeline::{self, Input};

pub const SCHEMA: &str = "tessera.witness/v0";

/// The furthest phase the caller asks to be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Front end through TIR: syntax, names, types.
    Check,
    /// [`Target::Check`], then lowering to MIR and the MIR verifier.
    Mir(OverflowMode),
    /// Code generation: not implemented yet.
    Backend,
}

impl Target {
    pub fn parse(name: &str, overflow: Option<OverflowMode>) -> Result<Self, String> {
        match (name, overflow) {
            ("check", None) => Ok(Self::Check),
            ("mir", Some(mode)) => Ok(Self::Mir(mode)),
            ("mir", None) => Err(pipeline::OVERFLOW_REQUIRED.to_owned()),
            ("backend", None) => Ok(Self::Backend),
            ("check" | "backend", Some(_)) => Err(format!(
                "--overflow only applies to --phase=mir, not `{name}`"
            )),
            _ => Err(format!(
                "unknown phase `{name}` (expected check, mir or backend)"
            )),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Mir(_) => "mir",
            Self::Backend => "backend",
        }
    }

    const fn overflow(self) -> Option<&'static str> {
        match self {
            Self::Mir(OverflowMode::Wrapping) => Some("wrapping"),
            Self::Mir(OverflowMode::Trapping) => Some("trapping"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    Unsupported,
    ToolError,
}

impl Outcome {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unsupported => "unsupported",
            Self::ToolError => "tool_error",
        }
    }

    /// `tsr witness` exit status; 2 stays reserved for usage errors.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Unsupported => 3,
            Self::ToolError => 4,
        }
    }
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The two OpenAI vocabularies embedded in `tiktoken-rs`: real tokenizer
/// counts, not estimates. They measure cost under those vocabularies only.
fn tokenizer_counts(text: &str) -> Json {
    json!({
        "cl100k_base": tiktoken_rs::cl100k_base_singleton().encode_ordinary(text).len(),
        "o200k_base": tiktoken_rs::o200k_base_singleton().encode_ordinary(text).len(),
    })
}

/// One representation of the program and what it costs to send.
fn representation(text: &str, tessera_tokens: Option<usize>) -> Json {
    json!({
        "sha256": sha256_hex(text.as_bytes()),
        "bytes": text.len(),
        "tessera_tokens": tessera_tokens,
        "tokenizers": tokenizer_counts(text),
    })
}

/// Accumulates the evidence of one run.
struct Run<'a> {
    text: &'a str,
    phases: Vec<(&'static str, &'static str)>,
    diagnostics: Vec<Json>,
    artifacts: Map<String, Json>,
    /// Name, text and Tessera lexer token count of each representation;
    /// tokenized after the compiler run so `timing.compile_us` excludes it.
    representations: Vec<(&'static str, String, Option<usize>)>,
    error: Option<String>,
    suggestions: Vec<advice::Suggestion>,
}

impl<'a> Run<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            phases: Vec::new(),
            diagnostics: Vec::new(),
            artifacts: Map::new(),
            representations: Vec::new(),
            error: None,
            suggestions: Vec::new(),
        }
    }

    fn diagnostic(&mut self, d: &Diagnostic) {
        self.diagnostics
            .push(diagnostics::to_json(&diagnostics::plain(self.text, d)));
    }

    /// `help`/`fixes` on each diagnostic of rejected TC, an
    /// `E-syntax-foreign` diagnostic first when the source reads as another
    /// language, and the whole-file `suggestions` that pass `check`. Replaces
    /// the plain records [`Self::phases_ran`] pushed for the same set.
    fn advise_tc(&mut self, set: &DiagnosticSet) {
        let (records, suggestions) = diagnostics::advised(self.text, set);
        self.diagnostics = records.iter().map(diagnostics::to_json).collect();
        self.suggestions = suggestions;
    }

    /// Record `phases` as run against `diagnostics`; true when none of them
    /// reported an error.
    fn phases_ran(&mut self, phases: &[Phase], diagnostics: &DiagnosticSet) -> bool {
        for d in diagnostics.iter() {
            self.diagnostic(d);
        }
        let mut ok = true;
        for &phase in phases {
            let failed = diagnostics
                .iter()
                .any(|d| d.phase == phase && d.severity == Severity::Error);
            ok &= !failed;
            self.phases
                .push((phase.name(), if failed { "fail" } else { "pass" }));
        }
        ok
    }

    /// The TC front end; tolerant phases all run, so independent mistakes are
    /// all reported at once.
    fn front_tc(&mut self) -> Option<(TirModule, ModuleProvenance)> {
        let tessera_tokens = lexer::lex(self.text)
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .count();
        self.representations
            .push(("source", self.text.to_owned(), Some(tessera_tokens)));
        let out = pipeline::check_tc(self.text);
        let phases = [
            Phase::Syntax,
            Phase::Hir,
            Phase::Resolve,
            Phase::Typeck,
            Phase::Tir,
        ];
        let ok = self.phases_ran(&phases, &out.diagnostics);
        if !ok {
            self.advise_tc(&out.diagnostics);
        }
        ok.then_some((out.value.module, out.value.provenance))
    }

    /// A hand-written `.tir` file: the TIR reader, then the TIR verifier.
    fn front_tir(&mut self) -> Option<(TirModule, ModuleProvenance)> {
        self.representations
            .push(("source", self.text.to_owned(), None));
        let parsed = TirModule::parse_with_provenance(tessera_phases::FileId(0), self.text);
        let (module, provenance) = match parsed {
            Ok(parsed) => parsed,
            Err(e) => {
                let at = e.at.min(self.text.len());
                let (line, col) = line_col(self.text, at);
                self.diagnostics.push(json!({
                    "phase": Phase::Tir.name(),
                    "severity": "error",
                    "code": "E-tir-parse",
                    "message": e.message,
                    "span": { "start": at, "end": at },
                    "line": line,
                    "col": col,
                    "help": null,
                    "fixes": [],
                }));
                self.phases.push((Phase::Tir.name(), "fail"));
                return None;
            }
        };
        let findings = tessera_tir::verify_module(&module);
        for f in &findings {
            // TIR verifier findings name a function and node, not a span.
            self.diagnostics.push(json!({
                "phase": Phase::Tir.name(),
                "severity": "error",
                "code": "E-tir-verify",
                "message": f.to_string(),
                "span": null,
                "line": null,
                "col": null,
                "help": null,
                "fixes": [],
            }));
        }
        self.phases.push((
            Phase::Tir.name(),
            if findings.is_empty() { "pass" } else { "fail" },
        ));
        findings.is_empty().then_some((module, provenance))
    }

    /// The canonical TIR text: the compiler's own representation of the
    /// checked program, its identity, and whether it reads back to the same
    /// module (reversibility, not just a hash).
    fn tir_artifact(&mut self, module: &TirModule) {
        let text = module.to_text();
        let roundtrip = TirModule::parse(&text).is_ok_and(|back| &back == module);
        self.artifacts.insert(
            "tir".to_owned(),
            json!({
                "sha256": sha256_hex(text.as_bytes()),
                "bytes": text.len(),
                "functions": module.funcs.len(),
                "roundtrip": roundtrip,
            }),
        );
        self.representations.push(("tir", text, None));
    }

    /// Lower to MIR and verify it. MIR the verifier rejects is a compiler bug,
    /// reported as a tool error rather than a verdict on the program.
    fn mir(&mut self, tir: &TirModule, provenance: &ModuleProvenance, overflow: OverflowMode) {
        let out = lower_module(tir, provenance, &LowerOptions { overflow });
        if !self.phases_ran(&[Phase::Mir], &out.diagnostics) {
            return;
        }
        let findings = tessera_mir::verify_module(&out.value);
        if !findings.is_empty() {
            let lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
            self.error = Some(format!(
                "internal error: MIR verifier: {}",
                lines.join("; ")
            ));
            return;
        }
        let dump = tessera_mir::dump(&out.value);
        self.artifacts.insert(
            "mir".to_owned(),
            json!({
                "sha256": sha256_hex(dump.as_bytes()),
                "bytes": dump.len(),
                "functions": out.value.funcs.len(),
            }),
        );
    }

    fn execute(&mut self, input: Input, target: Target) -> Outcome {
        if target == Target::Backend {
            self.phases.push((Phase::Backend.name(), "unsupported"));
            self.error = Some("phase `backend` is not implemented yet".to_owned());
            return Outcome::Unsupported;
        }
        let front = match input {
            Input::Tc => self.front_tc(),
            Input::Tir => self.front_tir(),
        };
        let Some((tir, provenance)) = front else {
            if let Target::Mir(_) = target {
                self.phases.push((Phase::Mir.name(), "not_run"));
            }
            return Outcome::Fail;
        };
        self.tir_artifact(&tir);
        if let Target::Mir(overflow) = target {
            self.mir(&tir, &provenance, overflow);
        }
        if self.error.is_some() {
            Outcome::ToolError
        } else if self.phases.iter().any(|&(_, status)| status == "fail") {
            Outcome::Fail
        } else {
            Outcome::Pass
        }
    }
}

fn tool() -> Json {
    let dirty = match env!("TSR_GIT_DIRTY") {
        "true" => Json::Bool(true),
        "false" => Json::Bool(false),
        _ => Json::Null,
    };
    json!({
        "name": "tsr",
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("TSR_GIT_COMMIT"),
        "dirty": dirty,
    })
}

/// `tsr --version`.
#[must_use]
pub fn version_line() -> String {
    let dirty = if env!("TSR_GIT_DIRTY") == "true" {
        ", dirty"
    } else {
        ""
    };
    format!(
        "tsr {} (commit {}{dirty})",
        env!("CARGO_PKG_VERSION"),
        env!("TSR_GIT_COMMIT")
    )
}

/// `result_id`: SHA-256 of the document minus its run-specific fields
/// (`timing`, `invocation.path`, `result_id` itself), serialized as compact
/// JSON with object keys sorted. Consumers (Lattice) recompute it to check a
/// stored document, so that serialization is part of the contract.
fn result_id(doc: &Json) -> String {
    let mut stable = doc.clone();
    if let Some(obj) = stable.as_object_mut() {
        obj.remove("timing");
        obj.remove("result_id");
        if let Some(inv) = obj.get_mut("invocation").and_then(Json::as_object_mut) {
            inv.remove("path");
        }
    }
    format!("sha256:{}", sha256_hex(stable.to_string().as_bytes()))
}

fn micros(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// Witness one invocation: `source` is the file's text, or why it could not
/// be read.
#[must_use]
pub fn witness(path: &str, source: Result<&str, String>, target: Target) -> (Outcome, Json) {
    let started = Instant::now();
    let input = Input::from_path(path);
    let invocation = json!({
        "phase": target.name(),
        "overflow": target.overflow(),
        "input": match input { Input::Tc => "tc", Input::Tir => "tir" },
        "path": path,
    });
    let mut compile_us = None;
    let (outcome, body) = match source {
        Err(error) => (
            Outcome::ToolError,
            json!({
                "source": null,
                "phases": [],
                "diagnostics": [],
                "artifacts": {},
                "representations": {},
                "error": error,
                "suggestions": [],
            }),
        ),
        Ok(text) => {
            let mut run = Run::new(text);
            let outcome = run.execute(input, target);
            compile_us = Some(micros(started));
            let representations: Map<String, Json> = run
                .representations
                .iter()
                .map(|(name, text, tokens)| ((*name).to_owned(), representation(text, *tokens)))
                .collect();
            let phases: Vec<Json> = run
                .phases
                .iter()
                .map(|(phase, status)| json!({ "phase": phase, "status": status }))
                .collect();
            (
                outcome,
                json!({
                    "source": {
                        "sha256": sha256_hex(text.as_bytes()),
                        "bytes": text.len(),
                    },
                    "phases": phases,
                    "diagnostics": run.diagnostics,
                    "artifacts": run.artifacts,
                    "representations": representations,
                    "error": run.error,
                    "suggestions": run.suggestions.iter().map(|s| json!({
                        "source": s.source,
                        "sha256": sha256_hex(s.source.as_bytes()),
                        "label": s.label,
                        "checked": "check",
                    })).collect::<Vec<_>>(),
                }),
            )
        }
    };
    let mut doc = json!({
        "schema": SCHEMA,
        "tool": tool(),
        "invocation": invocation,
        "outcome": outcome.name(),
    });
    if let (Some(doc), Json::Object(body)) = (doc.as_object_mut(), body) {
        doc.extend(body);
    }
    let id = result_id(&doc);
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("result_id".to_owned(), Json::String(id));
        obj.insert(
            "timing".to_owned(),
            json!({ "compile_us": compile_us, "total_us": micros(started) }),
        );
    }
    (outcome, doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = "f add(a:i64,b:i64)>i64=a+b\n";

    fn run(path: &str, text: &str, target: Target) -> (Outcome, Json) {
        witness(path, Ok(text), target)
    }

    fn codes(doc: &Json) -> Vec<&str> {
        doc["diagnostics"]
            .as_array()
            .expect("array")
            .iter()
            .map(|d| d["code"].as_str().expect("code"))
            .collect()
    }

    #[test]
    fn valid_program_passes_with_artifacts_and_token_counts() {
        let (outcome, doc) = run("a.tes", PASS, Target::Check);
        assert_eq!(outcome, Outcome::Pass);
        assert_eq!(doc["schema"], SCHEMA);
        assert_eq!(doc["outcome"], "pass");
        assert_eq!(doc["diagnostics"], json!([]));
        assert_eq!(doc["artifacts"]["tir"]["roundtrip"], true);
        assert!(doc["artifacts"].get("mir").is_none());
        assert_eq!(doc["representations"]["source"]["bytes"], PASS.len());
        assert!(doc["representations"]["source"]["tessera_tokens"].as_u64() > Some(0));
        assert!(doc["representations"]["tir"]["tokenizers"]["o200k_base"].as_u64() > Some(0));
        let phases: Vec<&str> = doc["phases"]
            .as_array()
            .expect("array")
            .iter()
            .map(|p| p["phase"].as_str().expect("phase"))
            .collect();
        assert_eq!(phases, ["syntax", "hir", "resolve", "typeck", "tir"]);
    }

    #[test]
    fn mir_target_records_the_mir_artifact() {
        let (outcome, doc) = run("a.tes", PASS, Target::Mir(OverflowMode::Trapping));
        assert_eq!(outcome, Outcome::Pass);
        assert_eq!(doc["invocation"]["overflow"], "trapping");
        assert_eq!(doc["artifacts"]["mir"]["functions"], 1);
    }

    #[test]
    fn syntax_and_type_errors_fail_with_structured_diagnostics() {
        let (outcome, doc) = run("s.tes", "f x()>i64=\n", Target::Mir(OverflowMode::Wrapping));
        assert_eq!(outcome, Outcome::Fail);
        assert_eq!(codes(&doc), ["E-syntax-expected"]);
        assert_eq!(doc["diagnostics"][0]["phase"], "syntax");
        assert_eq!(doc["diagnostics"][0]["line"], 2);
        assert_eq!(
            doc["phases"][5],
            json!({"phase": "mir", "status": "not_run"})
        );
        assert_eq!(doc["artifacts"], json!({}));

        // TC has only `i64` today, so no TC source reaches a typeck error;
        // name resolution is the semantic phase a TC program can fail.
        let (outcome, doc) = run("t.tes", "f add(a:i64)>i64=a+b\n", Target::Check);
        assert_eq!(outcome, Outcome::Fail);
        assert_eq!(codes(&doc), ["E-resolve-unbound-name"]);
        assert_eq!(
            doc["phases"][2],
            json!({"phase": "resolve", "status": "fail"})
        );
        assert!(doc["artifacts"].get("tir").is_none());
    }

    #[test]
    fn backend_is_unsupported_not_fabricated() {
        let (outcome, doc) = run("a.tes", PASS, Target::Backend);
        assert_eq!(outcome, Outcome::Unsupported);
        assert_eq!(outcome.exit_code(), 3);
        assert_eq!(
            doc["phases"],
            json!([{"phase": "backend", "status": "unsupported"}])
        );
        assert_eq!(doc["artifacts"], json!({}));
    }

    #[test]
    fn unreadable_input_is_a_tool_error() {
        let (outcome, doc) = witness("gone.tes", Err("no such file".to_owned()), Target::Check);
        assert_eq!(outcome, Outcome::ToolError);
        assert_eq!(doc["source"], Json::Null);
        assert_eq!(doc["error"], "no such file");
    }

    #[test]
    fn tir_input_reports_parse_errors_with_a_span() {
        let (outcome, doc) = run("m.tir", "(func f (return i64)", Target::Check);
        assert_eq!(outcome, Outcome::Fail);
        assert_eq!(codes(&doc), ["E-tir-parse"]);
        assert!(doc["representations"]["source"]["tessera_tokens"].is_null());
    }

    #[test]
    fn result_id_ignores_path_and_timing_only() {
        let (_, a) = run("a.tes", PASS, Target::Check);
        let (_, b) = run("elsewhere/b.tes", PASS, Target::Check);
        assert_eq!(a["result_id"], b["result_id"]);
        let (_, c) = run("a.tes", "f add(a:i64,b:i64)>i64=a-b\n", Target::Check);
        assert_ne!(a["result_id"], c["result_id"]);
        let (_, d) = run("a.tes", PASS, Target::Mir(OverflowMode::Wrapping));
        assert_ne!(a["result_id"], d["result_id"]);
    }

    /// Independent canonical form: compact, keys sorted at every level.
    fn canonical(value: &Json) -> String {
        match value {
            Json::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let fields: Vec<String> = keys
                    .into_iter()
                    .map(|k| format!("{}:{}", Json::String(k.clone()), canonical(&map[k])))
                    .collect();
                format!("{{{}}}", fields.join(","))
            }
            Json::Array(items) => {
                let items: Vec<String> = items.iter().map(canonical).collect();
                format!("[{}]", items.join(","))
            }
            other => other.to_string(),
        }
    }

    #[test]
    fn result_id_hashes_sorted_compact_json() {
        // Guards the consumer contract: if serde_json ever preserved insertion
        // order (e.g. a dependency enabling `preserve_order`), every stored
        // result_id would stop verifying downstream.
        for (path, text) in [("a.tes", PASS), ("b.tes", "f add(a:i64,b:i64)>i64=a+\n")] {
            let (_, doc) = run(path, text, Target::Check);
            let mut stable = doc.clone();
            let obj = stable.as_object_mut().unwrap();
            obj.remove("timing");
            obj.remove("result_id");
            obj.get_mut("invocation")
                .and_then(Json::as_object_mut)
                .unwrap()
                .remove("path");
            let expected = format!("sha256:{}", sha256_hex(canonical(&stable).as_bytes()));
            assert_eq!(doc["result_id"], Json::String(expected));
        }
    }

    #[test]
    fn targets_parse_with_the_o1_rule() {
        assert_eq!(Target::parse("check", None), Ok(Target::Check));
        assert!(Target::parse("mir", None).is_err_and(|e| e.contains("O1")));
        assert!(Target::parse("check", Some(OverflowMode::Wrapping)).is_err());
        assert!(Target::parse("codegen", None).is_err());
    }
}
