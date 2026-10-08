//! End-to-end `tsr mir` / `tsr run`: argument handling, output and exit codes
//! of the real binary.

use std::path::PathBuf;
use std::process::Command;

fn tsr(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tsr"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .expect("runs tsr");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fixture(name: &str, text: &str) -> String {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, text).expect("writes fixture");
    path.to_string_lossy().into_owned()
}

const BOOTSTRAP: &str = "examples/bootstrap.tes";

#[test]
fn overflow_mode_is_required_because_o1_is_open() {
    for cmd in ["mir", "run"] {
        let (code, _, err) = tsr(&[cmd, BOOTSTRAP]);
        assert_eq!(code, 2, "{cmd}");
        assert!(err.contains("open question O1"), "{err}");
        let (code, _, _) = tsr(&[cmd, "--overflow=sometimes", BOOTSTRAP]);
        assert_eq!(code, 2, "{cmd}");
    }
}

#[test]
fn mir_prints_the_cfg_of_tc_source() {
    let (code, out, err) = tsr(&["mir", "--overflow=trapping", BOOTSTRAP]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.starts_with("fn add(_1: i64, _2: i64) -> i64 {"),
        "{out}"
    );
    assert!(
        out.contains("_0 = Add.trapping(copy _1, copy _2);  // src 0:23..26"),
        "{out}"
    );
    let (code, _, _) = tsr(&["mir", "--overflow=trapping", BOOTSTRAP, "extra"]);
    assert_eq!(code, 2);
}

#[test]
fn run_prints_the_value_or_names_the_trap() {
    assert_eq!(
        tsr(&["run", "--overflow=trapping", BOOTSTRAP, "40", "2"]),
        (0, "42\n".to_owned(), String::new())
    );
    let max = i64::MAX.to_string();
    let (code, out, err) = tsr(&["run", "--overflow", "trapping", BOOTSTRAP, &max, "1"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("trap: integer overflow"), "{err}");
    assert_eq!(
        tsr(&["run", "--overflow=wrapping", BOOTSTRAP, &max, "1"]).1,
        format!("{}\n", i64::MIN)
    );
    let (code, _, err) = tsr(&["run", "--overflow=wrapping", BOOTSTRAP, "1"]);
    assert_eq!(code, 1);
    assert!(err.contains("takes 2 arguments"), "{err}");
}

#[test]
fn run_handwritten_tir_with_calls_and_branches() {
    let path = fixture(
        "sum.tir",
        "; sum(n) = n + (n - 1) + ... + 0\n\
         (func sum (param n i64) (return i64) (body\n\
           (if i64 (eq (var n i64) (int 0 i64)) (int 0 i64)\n\
             (add i64 (var n i64) (call sum i64 (add i64 (var n i64) (int -1 i64)))))))\n\
         (func main (return i64) (body (call sum i64 (int 1000 i64))))\n",
    );
    assert_eq!(
        tsr(&["run", "--overflow=trapping", &path, "main"]).1,
        "500500\n"
    );
    assert_eq!(
        tsr(&["run", "--overflow=trapping", &path, "sum", "3"]).1,
        "6\n"
    );
    let (code, _, err) = tsr(&["run", "--overflow=trapping", &path]);
    assert_eq!(code, 1);
    assert!(err.contains("name one of: sum, main"), "{err}");
    let (code, _, err) = tsr(&["run", "--overflow=trapping", &path, "sum", "20000"]);
    assert_eq!(code, 1);
    assert!(err.contains("call depth limit exceeded"), "{err}");
}

#[test]
fn errors_point_into_the_file_they_come_from() {
    let tc = fixture("bad.tes", "f add(a:i64)>i64=a+b\n");
    let (code, _, err) = tsr(&["mir", "--overflow=trapping", &tc]);
    assert_eq!(code, 1);
    assert!(
        err.contains("bad.tes:1:20: error[E-resolve-unbound-name]: unbound variable `b`"),
        "{err}"
    );

    let tir = fixture(
        "bad.tir",
        "(func f (return i64)\n  (body (add i64 (int 1 i64) (bool true))))\n",
    );
    let (code, _, err) = tsr(&["run", "--overflow=trapping", &tir]);
    assert_eq!(code, 1);
    assert!(
        err.contains("bad.tir:2:9: error[E-mir-ill-formed-tir]"),
        "{err}"
    );
}

/// `tsr check` runs syntax, HIR, resolution and type checking and reports
/// every independent problem at once, in source order, without cascades.
/// Indented lines are `help:`/`suggestion` follow-ups to the line above.
#[test]
fn check_reports_every_independent_problem_once() {
    let (code, out, err) = tsr(&["check", BOOTSTRAP]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "ok: 1 function(s) checked (syntax, names, types)\n");

    let path = fixture("multi.tes", "f add(a:i64,a:i64)>bool=b+c+\n");
    let (code, _, err) = tsr(&["check", &path]);
    assert_eq!(code, 1);
    let codes: Vec<&str> = err
        .lines()
        .filter(|l| !l.starts_with("  "))
        .map(|l| {
            l.split("error[")
                .nth(1)
                .and_then(|r| r.split(']').next())
                .unwrap_or(l)
        })
        .collect();
    assert_eq!(
        codes,
        [
            "E-resolve-duplicate-param",
            "E-resolve-unknown-type",
            "E-resolve-unbound-name",
            "E-resolve-unbound-name",
            "E-syntax-expected"
        ],
        "{err}"
    );
}

#[test]
fn tir_expands_tc_through_the_phases() {
    let (code, out, err) = tsr(&["tir", BOOTSTRAP]);
    assert_eq!((code, err.as_str()), (0, ""));
    let golden = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bootstrap.tir"
    ))
    .expect("golden");
    assert_eq!(out.trim_end(), golden.trim_end());
    let bad = fixture("unbound.tes", "f f(a:i64)>i64=a+q\n");
    let (code, out, err) = tsr(&["tir", &bad]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.contains(":1:18: error[E-resolve-unbound-name]"),
        "{err}"
    );
}

/// `tsr witness` on `fixture`: exit code and the parsed evidence document.
fn witness(args: &[&str]) -> (i32, serde_json::Value) {
    let mut full = vec!["witness"];
    full.extend_from_slice(args);
    let (code, out, err) = tsr(&full);
    assert_eq!(err, "", "witness writes evidence to stdout only");
    let doc = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    (code, doc)
}

/// The issue #41 witness fixtures through the real binary: outcome, exit
/// code and structured diagnostics, never terminal prose.
#[test]
fn witness_fixtures_report_compiler_evidence() {
    let cases = [
        ("examples/witness/pass.tes", 0, "pass", vec![]),
        (
            "examples/witness/syntax_error.tes",
            1,
            "fail",
            vec!["E-syntax-expected"],
        ),
        (
            "examples/witness/semantic_error.tes",
            1,
            "fail",
            vec!["E-resolve-unbound-name"],
        ),
    ];
    for (path, want_code, want_outcome, want_codes) in cases {
        let (code, doc) = witness(&[path]);
        assert_eq!(
            (code, doc["outcome"].as_str()),
            (want_code, Some(want_outcome)),
            "{path}"
        );
        assert_eq!(doc["schema"], "tessera.witness/v0");
        assert_eq!(doc["tool"]["version"], env!("CARGO_PKG_VERSION"));
        let commit = doc["tool"]["commit"].as_str().expect("commit");
        assert!(commit == "unknown" || commit.len() == 40, "{commit}");
        let codes: Vec<&str> = doc["diagnostics"]
            .as_array()
            .expect("array")
            .iter()
            .map(|d| d["code"].as_str().expect("code"))
            .collect();
        assert_eq!(codes, want_codes, "{path}");
    }
    let (code, doc) = witness(&[
        "--phase=mir",
        "--overflow=trapping",
        "examples/witness/pass.tes",
    ]);
    assert_eq!((code, doc["outcome"].as_str()), (0, Some("pass")));
    assert_eq!(doc["artifacts"]["mir"]["functions"], 1);
}

#[test]
fn witness_marks_unimplemented_phases_unsupported() {
    let (code, doc) = witness(&["--phase", "backend", "examples/witness/pass.tes"]);
    assert_eq!((code, doc["outcome"].as_str()), (3, Some("unsupported")));
    assert_eq!(doc["artifacts"], serde_json::json!({}));
}

#[test]
fn witness_reports_unreadable_input_as_a_tool_error() {
    let (code, doc) = witness(&["examples/witness/does-not-exist.tes"]);
    assert_eq!((code, doc["outcome"].as_str()), (4, Some("tool_error")));
    assert!(
        doc["error"]
            .as_str()
            .is_some_and(|e| e.contains("failed to read"))
    );
}

#[test]
fn witness_usage_errors_are_not_evidence() {
    let (code, out, err) = tsr(&["witness", "--phase=mir", "examples/witness/pass.tes"]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("open question O1"), "{err}");
    assert_eq!(
        tsr(&["witness", "--phase=link", "examples/witness/pass.tes"]).0,
        2
    );
    assert_eq!(tsr(&["witness"]).0, 2);
}

/// Repeated runs of one fixture are byte-identical apart from `timing`, so
/// Lattice can compare and replay them by `result_id`.
#[test]
fn witness_is_deterministic_across_runs() {
    for path in [
        "examples/witness/pass.tes",
        "examples/witness/syntax_error.tes",
    ] {
        let strip = |mut doc: serde_json::Value| {
            doc.as_object_mut().expect("object").remove("timing");
            doc
        };
        let (_, first) = witness(&[path]);
        let (_, second) = witness(&[path]);
        assert!(
            first["result_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("sha256:"))
        );
        assert_eq!(strip(first), strip(second), "{path}");
    }
}

#[test]
fn version_names_the_commit() {
    let (code, out, _) = tsr(&["--version"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!("tsr {} (commit ", env!("CARGO_PKG_VERSION"))),
        "{out}"
    );
}

/// A program in another language's syntax gets one diagnostic naming that,
/// and a whole-file suggestion that itself passes `tsr witness`.
#[test]
fn witness_suggests_checked_repairs() {
    let (code, doc) = witness(&["examples/witness/foreign_syntax.tes"]);
    assert_eq!((code, doc["outcome"].as_str()), (1, Some("fail")));
    let first = &doc["diagnostics"][0];
    assert_eq!(first["code"], "E-syntax-foreign");
    assert!(
        first["message"]
            .as_str()
            .is_some_and(|m| m.contains("not `fn`") && m.contains("no `return`")),
        "{first}"
    );
    let suggestions = doc["suggestions"].as_array().expect("array");
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    let source = suggestions[0]["source"].as_str().expect("source");
    assert_eq!(source, "f add(a:i64,b:i64)>i64=a+b\n");
    assert_eq!(suggestions[0]["checked"], "check");
    let (code, doc) = witness(&[&fixture("suggested.tes", source)]);
    assert_eq!((code, doc["outcome"].as_str()), (0, Some("pass")), "{doc}");
    assert_eq!(doc["suggestions"], serde_json::json!([]));

    // Fix alternatives are listed per diagnostic; each combination is a suggestion.
    let (_, doc) = witness(&["examples/witness/semantic_error.tes"]);
    let unbound = &doc["diagnostics"][0];
    assert_eq!(unbound["fixes"][0]["replacement"], "a");
    assert!(
        unbound["help"]
            .as_str()
            .is_some_and(|h| h.contains("did you mean `a`"))
    );
    assert_eq!(
        doc["suggestions"][0]["source"], "f add(a:i64)>i64=a+a\n",
        "checked, not necessarily what the author meant"
    );
}

#[test]
fn check_prints_help_and_suggestions() {
    let (code, out, err) = tsr(&["check", "examples/witness/foreign_syntax.tes"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.starts_with("examples/witness/foreign_syntax.tes:1:1: error[E-syntax-foreign]"),
        "{err}"
    );
    assert!(
        err.contains("  help: a TC program is one function"),
        "{err}"
    );
    assert!(
        err.ends_with("passes check): f add(a:i64,b:i64)>i64=a+b\n"),
        "{err}"
    );
}

#[test]
fn grammar_prints_each_format() {
    for (args, marker) in [
        (&["grammar"][..], "function = \"f\""),
        (&["grammar", "--format=ebnf"], "expr     = term"),
        (&["grammar", "--format=gbnf"], "root   ::= \"f \""),
        (&["grammar", "--format=lark"], "start: \"f \""),
    ] {
        let (code, out, err) = tsr(args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
        assert!(out.contains(marker), "{args:?}: {out}");
    }
    assert_eq!(tsr(&["grammar", "--format=peg"]).0, 2);
    assert_eq!(tsr(&["grammar", "ebnf"]).0, 2);
}

/// `--diagnostic=dense|json` render the same records as the human form.
#[test]
fn check_renders_dense_and_json_diagnostics() {
    let file = "examples/witness/semantic_error.tes";
    let (code, out, err) = tsr(&["check", "--diagnostic=dense", file]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "1:20 E-resolve-unbound-name unbound variable `b` (not a parameter of `add`); \
         help: `b` is not a parameter; did you mean `a`? (parameters: a)\n\
         = f add(a:i64)>i64=a+a\n"
    );

    let (code, out, err) = tsr(&["check", file, "--diagnostic=json"]);
    assert_eq!((code, err.as_str()), (1, ""));
    let doc: serde_json::Value = serde_json::from_str(&out).expect("json document");
    assert_eq!(doc["schema"], "tessera.diagnostics/v0");
    assert_eq!(doc["ok"], false);
    assert_eq!(doc["diagnostics"][0]["code"], "E-resolve-unbound-name");
    assert_eq!(doc["diagnostics"][0]["line"], 1);
    assert_eq!(doc["suggestions"][0]["source"], "f add(a:i64)>i64=a+a\n");

    // The JSON record is the witness diagnostic, field for field.
    let (_, witness) = witness(&[file]);
    assert_eq!(doc["diagnostics"], witness["diagnostics"]);

    let (code, out, err) = tsr(&["check", "--diagnostic=json", BOOTSTRAP]);
    assert_eq!((code, err.as_str()), (0, ""));
    let doc: serde_json::Value = serde_json::from_str(&out).expect("json document");
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["diagnostics"], serde_json::json!([]));

    let (code, _, err) = tsr(&["check", "--diagnostic=prose", BOOTSTRAP]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown diagnostic format"), "{err}");
}
