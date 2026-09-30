//! Shared helpers for the integration tests. Every test is hermetic: it reads
//! only files inside the repository and never depends on user-placed vocab
//! files (`bench/tokenizers/` is deliberately not consulted).

#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::Value;
use tessera_tokenbench::bench::{self, Results, RunOptions};
use tessera_tokenbench::{report_json, report_md};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn bench_dir() -> PathBuf {
    repo_root().join("bench")
}

/// Scratch space cargo provides for integration tests.
pub fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Options for a run that can only see embedded tokenizers: the tokenizers
/// directory does not exist, so every file-based tokenizer must be `skipped`.
pub fn hermetic_options() -> RunOptions {
    RunOptions {
        corpus_dir: bench_dir().join("corpus"),
        manifest_path: bench_dir().join("tokenizers.json"),
        tokenizers_dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-tokenizers-dir"),
        corpus_revision: None,
    }
}

pub fn fresh_results() -> Results {
    bench::run(&hermetic_options()).expect("benchmark run")
}

pub fn fresh_json_text() -> String {
    report_json::to_json(&fresh_results()).to_pretty_string()
}

pub fn fresh_markdown() -> String {
    report_md::to_markdown(&fresh_results())
}

pub fn fresh_json() -> Value {
    serde_json::from_str(&fresh_json_text()).expect("results are valid JSON")
}

pub fn committed_baseline_path() -> PathBuf {
    bench_dir().join("results").join("baseline.json")
}

pub fn committed_baseline() -> Value {
    let text = std::fs::read_to_string(committed_baseline_path()).expect("committed baseline.json");
    serde_json::from_str(&text).expect("baseline is valid JSON")
}

/// `(program, candidate)` of the first parsed tessera variant in `doc`.
pub fn first_parsed(doc: &Value) -> (usize, usize) {
    for (pi, p) in doc["programs"]
        .as_array()
        .expect("programs")
        .iter()
        .enumerate()
    {
        for (vi, v) in p["variants"]
            .as_array()
            .expect("variants")
            .iter()
            .enumerate()
        {
            if v["lang"] == "tessera" && v["status"] == "parsed" {
                return (pi, vi);
            }
        }
    }
    panic!("no parsed tessera variant");
}
