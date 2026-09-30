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
