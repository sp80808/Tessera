//! Replays `seeds/` and `regressions/` through the same invariant functions the
//! fuzz targets call, without libFuzzer, so it runs under plain `cargo test`
//! (stable toolchain, any CI) and is the regression guard for every finding.
//!
//! ```text
//! cargo test --manifest-path crates/tessera-syntax/fuzz/Cargo.toml
//! ```
//!
//! Regression files are `regressions/<descriptive-name>.tes` (text) or `.bin`
//! (raw libFuzzer artifact). Either kind is replayed through all four targets,
//! so an artifact found by one target also guards the others.

use std::fs;
use std::path::{Path, PathBuf};

use tessera_syntax_fuzz::{cst_inv, frontend_inv, lexer_inv, structured};

/// Exactly what the four fuzz targets do with raw bytes.
fn check_all_targets(data: &[u8]) {
    lexer_inv::check_bytes(data);
    cst_inv::check_bytes(data);
    frontend_inv::check_bytes(data);
    structured::check_bytes(data);
}

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

/// Files of `name/`, sorted; macOS `._*` sidecar files and dotfiles are never
/// inputs.
fn files(name: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir(name)) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with('.'))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn seeds_pass_every_invariant() {
    let seeds = files("seeds");
    assert!(!seeds.is_empty(), "seeds/ is empty");
    for path in seeds {
        let data = fs::read(&path).expect("reads seed");
        check_all_targets(&data);
        // `*_cases.tes` hold one case per line: replay those individually too.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_cases.tes"))
        {
            for line in String::from_utf8_lossy(&data).lines() {
                check_all_targets(line.as_bytes());
            }
        }
    }
}

#[test]
fn regressions_stay_fixed() {
    for path in files("regressions") {
        let ext = path.extension().and_then(|e| e.to_str());
        if !matches!(ext, Some("tes" | "bin")) {
            continue;
        }
        let data = fs::read(&path).expect("reads regression");
        eprintln!("replaying {}", path.display());
        check_all_targets(&data);
    }
}

/// The documented limits, exactly at and just past them, through every check.
#[test]
fn nesting_and_depth_limits_hold_on_both_sides() {
    use tessera_syntax::{MAX_EXPR_DEPTH, MAX_NESTING, SyntaxError, parse};

    let chain = |links: usize| format!("f c(a:i64)>i64=a{}", "+a".repeat(links));
    let parens = |n: usize| format!("f c(a:i64)>i64={}a{}", "(".repeat(n), ")".repeat(n));

    for (src, ok) in [
        (chain(MAX_EXPR_DEPTH), true),
        (chain(MAX_EXPR_DEPTH + 1), false),
        (parens(MAX_NESTING), true),
        (parens(MAX_NESTING + 1), false),
    ] {
        check_all_targets(src.as_bytes());
        match parse(&src) {
            Ok(_) => assert!(ok, "accepted a program past the limits"),
            Err(SyntaxError::NestingTooDeep { .. }) => assert!(!ok, "rejected within the limits"),
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }
}

/// A deterministic mini-campaign: pseudo-random bytes through all four
/// targets, including the generator oracle. Not a substitute for libFuzzer;
/// it keeps the harness itself (generator, oracle) exercised on every
/// `cargo test` and catches harness bit-rot.
#[test]
fn pseudo_random_bytes_pass_every_invariant() {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..600_usize {
        let len = 8 + (next() % 600) as usize;
        let data: Vec<u8> = (0..len).map(|_| (next() >> 24) as u8).collect();
        check_all_targets(&data);
        // printable-ish bytes reach the lexer's real tokens much more often
        let text: Vec<u8> = data
            .iter()
            .map(|b| b" (),:>=+ fi64ab019\n/"[usize::from(*b) % 20])
            .collect();
        check_all_targets(&text);
        if round % 100 == 0 {
            eprintln!("round {round}");
        }
    }
}
