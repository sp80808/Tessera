//! The regression gate: passes on the committed baseline, fails on drift.

mod common;

use std::fs;

use serde_json::{Value, json};
use tessera_tokenbench::check::{self, DEFAULT_MAX_REGRESSION_PCT};
use tessera_tokenbench::cli;

fn bump(doc: &mut Value, program: usize, variant: usize, metric: &str, factor: f64) {
    let stats = &mut doc["programs"][program]["variants"][variant]["stats"][metric];
    let old = stats.as_f64().expect("number");
    *stats = json!(old * factor);
}

#[test]
fn check_passes_against_the_committed_baseline() {
    let baseline = common::committed_baseline();
    let report = check::check(&baseline, &common::fresh_json(), DEFAULT_MAX_REGRESSION_PCT);
    assert!(report.passed(), "{:?}", report.failures);
    assert!(
        report.checked >= 3,
        "gate compared only {} variants",
        report.checked
    );
}

#[test]
fn committed_baseline_is_exactly_what_the_tool_produces_from_the_committed_corpus() {
    // If this fails, regenerate the baseline (command in bench/README.md) and review the diff.
    let dir = common::bench_dir().join("results");
    let json = fs::read_to_string(dir.join("baseline.json")).expect("baseline.json");
    let md = fs::read_to_string(dir.join("baseline.md")).expect("baseline.md");
    assert_eq!(
        json,
        common::fresh_json_text(),
        "bench/results/baseline.json is stale"
    );
    assert_eq!(
        md,
        common::fresh_markdown(),
        "bench/results/baseline.md is stale"
    );
}

#[test]
fn check_fails_on_an_injected_regression() {
    let baseline = common::committed_baseline();
    let mut regressed = common::fresh_json();
    let (p, v) = common::first_parsed(&regressed);
    bump(&mut regressed, p, v, "median", 1.5);
    let report = check::check(&baseline, &regressed, DEFAULT_MAX_REGRESSION_PCT);
    assert!(!report.passed());
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.contains("median regressed")),
        "{:?}",
        report.failures
    );

    let mut worst = common::fresh_json();
    bump(&mut worst, p, v, "worst", 1.5);
    let report = check::check(&baseline, &worst, DEFAULT_MAX_REGRESSION_PCT);
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.contains("worst regressed")),
        "{:?}",
        report.failures
    );
}

#[test]
fn threshold_is_respected_and_improvements_never_fail() {
    let baseline = common::committed_baseline();
    let (p, v) = common::first_parsed(&baseline);
    let mut slightly = common::fresh_json();
    bump(&mut slightly, p, v, "median", 1.01);
    assert!(
        check::check(&baseline, &slightly, 2.0).passed(),
        "1% growth is within 2%"
    );
    assert!(
        !check::check(&baseline, &slightly, 0.5).passed(),
        "1% growth exceeds 0.5%"
    );
    let mut better = common::fresh_json();
    bump(&mut better, p, v, "median", 0.5);
    bump(&mut better, p, v, "worst", 0.5);
    assert!(
        check::check(&baseline, &better, 0.0).passed(),
        "fewer tokens is never a regression"
    );
    assert!(
        !check::check(&baseline, &slightly, -1.0).passed(),
        "negative threshold is rejected"
    );
    assert!(
        !check::check(&baseline, &slightly, f64::NAN).passed(),
        "NaN threshold is rejected"
    );
}

#[test]
fn check_fails_when_the_corpus_hash_changes() {
    let baseline = common::committed_baseline();
    let mut changed = common::fresh_json();
    changed["corpus_hash"] = json!("0".repeat(64));
    let report = check::check(&baseline, &changed, DEFAULT_MAX_REGRESSION_PCT);
    assert!(!report.passed());
    assert!(
        report.failures[0].contains("corpus_hash changed"),
        "{:?}",
        report.failures
    );
}

#[test]
fn check_fails_when_the_tokenizer_set_or_provenance_changes() {
    let baseline = common::committed_baseline();
    let mut fewer = common::fresh_json();
    fewer["tokenizers"][0]["status"] = json!("skipped");
    let report = check::check(&baseline, &fewer, DEFAULT_MAX_REGRESSION_PCT);
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.contains("tokenizer set or provenance")),
        "{:?}",
        report.failures
    );

    let mut reversioned = common::fresh_json();
    reversioned["tokenizers"][0]["provenance"]["crate_version"] = json!("9.9.9");
    let report = check::check(&baseline, &reversioned, DEFAULT_MAX_REGRESSION_PCT);
    assert!(!report.passed());
}

#[test]
fn check_fails_when_a_parsed_variant_disappears_or_is_demoted() {
    let baseline = common::committed_baseline();
    let (p, v) = common::first_parsed(&baseline);
    let mut demoted = common::fresh_json();
    demoted["programs"][p]["variants"][v]["status"] = json!("unparsed");
    let report = check::check(&baseline, &demoted, DEFAULT_MAX_REGRESSION_PCT);
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.contains("no longer `parsed`")),
        "{:?}",
        report.failures
    );

    let mut removed = common::fresh_json();
    removed["programs"][p]["variants"]
        .as_array_mut()
        .expect("variants")
        .remove(v);
    let report = check::check(&baseline, &removed, DEFAULT_MAX_REGRESSION_PCT);
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.contains("missing from results")),
        "{:?}",
        report.failures
    );
}

#[test]
fn unparsed_variants_never_gate() {
    let baseline = common::committed_baseline();
    let mut noisy = common::fresh_json();
    let mut touched = 0;
    for program in noisy["programs"].as_array_mut().expect("programs") {
        for variant in program["variants"].as_array_mut().expect("variants") {
            if variant["status"] == "unparsed" {
                let stats = &mut variant["stats"];
                stats["median"] = json!(stats["median"].as_f64().expect("median") * 3.0);
                stats["worst"] = json!(stats["worst"].as_f64().expect("worst") * 3.0);
                touched += 1;
            }
        }
    }
    assert!(touched > 0);
    assert!(check::check(&baseline, &noisy, DEFAULT_MAX_REGRESSION_PCT).passed());
}

#[test]
fn cli_check_exit_codes() {
    let dir = common::scratch("cli-check");
    let baseline = common::committed_baseline_path();
    let ok = dir.join("ok.json");
    fs::write(&ok, common::fresh_json_text()).expect("write ok");
    let mut regressed = common::fresh_json();
    let (p, v) = common::first_parsed(&regressed);
    bump(&mut regressed, p, v, "median", 2.0);
    let bad = dir.join("regressed.json");
    fs::write(&bad, serde_json::to_string(&regressed).expect("json")).expect("write bad");
    let arg = |s: &std::path::Path| s.to_str().expect("utf8").to_owned();
    let run = |results: &std::path::Path, extra: &[&str]| {
        let mut args = vec![
            "check".to_owned(),
            "--baseline".to_owned(),
            arg(&baseline),
            "--results".to_owned(),
            arg(results),
        ];
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        cli::run_cli(&args)
    };
    assert_eq!(run(&ok, &[]), 0);
    assert_eq!(run(&bad, &[]), 1, "regression must exit non-zero");
    assert_eq!(
        run(&bad, &["--max-regression-pct", "500"]),
        0,
        "threshold flag is honoured"
    );
    assert_eq!(
        run(&dir.join("missing.json"), &[]),
        2,
        "unreadable results is a usage/io error"
    );
}
