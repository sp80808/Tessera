//! Results are byte-identical across runs and carry no environment leakage.

mod common;

use std::fs;

use tessera_tokenbench::bench;
use tessera_tokenbench::cli;
use tessera_tokenbench::{report_json, report_md};

#[test]
fn two_in_process_runs_produce_byte_identical_json_and_markdown() {
    let first = bench::run(&common::hermetic_options()).expect("first run");
    let second = bench::run(&common::hermetic_options()).expect("second run");
    let (json_a, json_b) = (
        report_json::to_json(&first).to_pretty_string(),
        report_json::to_json(&second).to_pretty_string(),
    );
    assert_eq!(json_a, json_b, "JSON differs between identical runs");
    assert_eq!(
        report_md::to_markdown(&first),
        report_md::to_markdown(&second)
    );
    assert!(json_a.len() > 10_000, "suspiciously small results");
}

#[test]
fn output_has_no_paths_hostnames_or_timestamps() {
    let json = common::fresh_json_text();
    let md = common::fresh_markdown();
    let root = common::repo_root().canonicalize().expect("root");
    let target = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    for (name, text) in [("json", &json), ("markdown", &md)] {
        for forbidden in [
            root.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
            "/Users/",
            "/Volumes/",
            "/home/",
            "C:\\",
        ] {
            assert!(!text.contains(forbidden), "{name} leaks `{forbidden}`");
        }
        // ISO-like dates/times
        let bytes = text.as_bytes();
        let has_date = bytes.windows(10).any(|w| {
            w[0..4].iter().all(u8::is_ascii_digit)
                && w[4] == b'-'
                && w[5..7].iter().all(u8::is_ascii_digit)
                && w[7] == b'-'
                && w[8..10].iter().all(u8::is_ascii_digit)
        });
        assert!(!has_date, "{name} contains a date-like string");
    }
}

#[test]
fn json_is_valid_and_top_level_keys_are_sorted() {
    let text = common::fresh_json_text();
    let doc: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(doc["schema_version"], 1);
    // Top-level keys appear in sorted order in the emitted text.
    let keys: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("  \"") && l.contains("\": "))
        .map(|l| l.trim_start().split('"').nth(1).expect("key"))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
    assert!(text.ends_with("}\n"));
}

#[test]
fn corpus_revision_flag_is_recorded_and_changes_only_that_field() {
    let mut opts = common::hermetic_options();
    let plain = report_json::to_json(&bench::run(&opts).expect("run")).to_pretty_string();
    opts.corpus_revision = Some("rev-test-1".to_owned());
    let tagged = report_json::to_json(&bench::run(&opts).expect("run")).to_pretty_string();
    assert!(tagged.contains("\"corpus_revision\": \"rev-test-1\""));
    assert!(plain.contains("\"corpus_revision\": null"));
    assert_eq!(
        plain.replace(
            "\"corpus_revision\": null",
            "\"corpus_revision\": \"rev-test-1\""
        ),
        tagged
    );
}

#[test]
fn run_subcommand_writes_the_same_bytes_as_the_library() {
    let dir = common::scratch("cli-run");
    let json = dir.join("out.json");
    let md = dir.join("out.md");
    let opts = common::hermetic_options();
    let args: Vec<String> = [
        "run",
        "--corpus",
        opts.corpus_dir.to_str().expect("utf8"),
        "--manifest",
        opts.manifest_path.to_str().expect("utf8"),
        "--tokenizers-dir",
        opts.tokenizers_dir.to_str().expect("utf8"),
        "--out-json",
        json.to_str().expect("utf8"),
        "--out-md",
        md.to_str().expect("utf8"),
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(cli::run_cli(&args), 0);
    assert_eq!(
        fs::read_to_string(&json).expect("json"),
        common::fresh_json_text()
    );
    assert_eq!(
        fs::read_to_string(&md).expect("md"),
        common::fresh_markdown()
    );
}

#[test]
fn run_fails_loudly_on_a_missing_corpus() {
    let args: Vec<String> = ["run", "--corpus", "definitely/not/here"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(cli::run_cli(&args), 2);
}
