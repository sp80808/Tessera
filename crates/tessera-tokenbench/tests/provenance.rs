//! Tokenizer provenance: versions, manifest shape, explicit skips.

mod common;

use std::fs;

use tessera_tokenbench::adapter::{
    self, HF_TOKENIZERS_VERSION, Loader, SlotState, TIKTOKEN_RS_VERSION,
};
use tessera_tokenbench::corpus::sha256_hex;

/// Version of `name` recorded in Cargo.lock (plain-text read).
fn locked_version(lock: &str, name: &str) -> Vec<String> {
    let needle = format!("name = \"{name}\"\nversion = \"");
    lock.match_indices(&needle)
        .map(|(i, _)| {
            let rest = &lock[i + needle.len()..];
            rest[..rest.find('"').expect("closing quote")].to_owned()
        })
        .collect()
}

#[test]
fn recorded_dependency_versions_match_cargo_lock() {
    let lock = fs::read_to_string(common::repo_root().join("Cargo.lock")).expect("Cargo.lock");
    assert_eq!(
        locked_version(&lock, "tiktoken-rs"),
        [TIKTOKEN_RS_VERSION],
        "TIKTOKEN_RS_VERSION is out of date: update it and regenerate the baseline"
    );
    assert_eq!(
        locked_version(&lock, "tokenizers"),
        [HF_TOKENIZERS_VERSION],
        "HF_TOKENIZERS_VERSION is out of date: update it and regenerate the baseline"
    );
}

#[test]
fn manifest_is_wellformed_and_lists_the_required_families() {
    let manifest =
        adapter::load_manifest(&common::bench_dir().join("tokenizers.json")).expect("manifest");
    let families: Vec<&str> = manifest
        .tokenizers
        .iter()
        .map(|t| t.family.as_str())
        .collect();
    for wanted in ["Qwen3-Coder", "Llama-3", "DeepSeek-Coder-V2", "StarCoder2"] {
        assert!(families.contains(&wanted), "manifest lacks {wanted}");
    }
    let embedded = manifest
        .tokenizers
        .iter()
        .filter(|t| t.loader == Loader::TiktokenEmbedded)
        .count();
    assert_eq!(embedded, 4, "r50k, p50k, cl100k, o200k");
    let mut pinned = 0usize;
    let mut unpinned = 0usize;
    for t in manifest
        .tokenizers
        .iter()
        .filter(|t| t.loader == Loader::HfTokenizersJson)
    {
        let file = t.file.as_ref().expect("file ref");
        assert_eq!(file.hub_filename, "tokenizer.json");
        assert!(file.repo_id.contains('/'), "{}", t.id);
        assert!(tessera_tokenbench::corpus::safe_relative(&file.local_path));
        match (file.sha256.as_deref(), file.revision.as_deref()) {
            (Some(sha), Some(rev)) => {
                assert_eq!(sha.len(), 64, "{} sha256 length", t.id);
                assert!(
                    rev.chars().all(|c| c.is_ascii_hexdigit()),
                    "{} revision",
                    t.id
                );
                assert!(
                    file.hub_metadata_verified,
                    "{}: pinned entries must verify hub metadata",
                    t.id
                );
                pinned += 1;
            }
            (None, None) => {
                assert!(
                    !file.hub_metadata_verified,
                    "{}: unpinned entries must not claim hub verification",
                    t.id
                );
                unpinned += 1;
            }
            _ => panic!("{}: sha256 and revision must be set together", t.id),
        }
    }
    // Four public HF vocabularies are pinned; Llama stays gated/unpinned until license acceptance.
    assert_eq!(pinned, 4, "expected four pinned HF tokenizers");
    assert_eq!(unpinned, 1, "Llama remains unpinned");
}

#[test]
fn missing_artifacts_are_explicit_skips_not_failures() {
    let results = common::fresh_results();
    let skipped: Vec<_> = results
        .tokenizers
        .iter()
        .filter(|t| t.skipped.is_some())
        .collect();
    assert_eq!(
        skipped.len(),
        5,
        "qwen, llama, deepseek, starcoder2, mistral"
    );
    for t in &skipped {
        let reason = t.skipped.as_deref().expect("reason");
        assert!(reason.contains("artifact not found"), "{}: {reason}", t.id);
        assert!(
            !reason.contains('/') || !reason.starts_with('/'),
            "reason must not embed absolute paths: {reason}"
        );
    }
    let measured = results.measured_ids();
    assert_eq!(
        measured,
        [
            "openai/r50k_base",
            "openai/p50k_base",
            "openai/cl100k_base",
            "openai/o200k_base"
        ]
    );
    assert_eq!(
        results.measured_vendors(),
        ["OpenAI"],
        "one vendor only; must not be reported as more"
    );
    // skipped tokenizers appear in the output records, not just in the CLI log
    let json = common::fresh_json();
    let listed: Vec<&str> = json["tokenizers"]
        .as_array()
        .expect("tokenizers")
        .iter()
        .filter(|t| t["status"] == "skipped")
        .map(|t| t["id"].as_str().expect("id"))
        .collect();
    assert_eq!(listed.len(), 5);
}

#[test]
fn embedded_tokenizers_record_crate_and_version() {
    let json = common::fresh_json();
    for t in json["tokenizers"]
        .as_array()
        .expect("tokenizers")
        .iter()
        .filter(|t| t["status"] == "measured")
    {
        assert_eq!(t["provenance"]["crate"], "tiktoken-rs");
        assert_eq!(t["provenance"]["crate_version"], TIKTOKEN_RS_VERSION);
        assert!(t["provenance"]["encoding"].is_string());
    }
}

#[test]
fn every_measured_variant_has_a_count_from_every_measured_tokenizer() {
    let results = common::fresh_results();
    let ids = results.measured_ids();
    for p in &results.programs {
        for v in &p.variants {
            assert_eq!(v.tokens.len(), ids.len(), "{}/{}", p.id, v.file);
            assert!(v.tokens.values().all(|&n| n > 0));
            let s = v.stats.as_ref().expect("stats");
            let min = *v.tokens.values().min().expect("min");
            let max = *v.tokens.values().max().expect("max");
            assert_eq!((s.min, s.worst), (min, max));
            assert!(s.median >= min as f64 && s.median <= max as f64);
        }
    }
}

#[cfg(feature = "hf")]
mod file_artifacts {
    use super::*;
    use std::path::Path;

    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tiny-tokenizer.json"
    );

    fn manifest_for(sha: &str, local_path: &str) -> adapter::Manifest {
        let text = format!(
            r#"{{"schema_version":1,"tokenizers":[{{"id":"fixture/tiny","family":"fixture","vendor":"Fixture",
            "loader":"hf-tokenizers-json","embedded":null,"file":{{"repo_id":"local/hand-built",
            "hub_filename":"tiny-tokenizer.json","local_path":"{local_path}","sha256":{sha},"revision":"hand-built-v1",
            "approx_size_mb":null,"hub_metadata_verified":true,"license_note":"test fixture"}},"notes":""}}]}}"#
        );
        adapter::parse_manifest(text.as_bytes()).expect("manifest")
    }

    fn state_of(dir: &Path, sha: Option<&str>) -> String {
        let sha = sha.map_or("null".to_owned(), |s| format!("\"{s}\""));
        let slots = adapter::build_slots(&manifest_for(&sha, "tiny-tokenizer.json"), dir);
        match &slots[0].state {
            SlotState::Ready(_) => "ready".to_owned(),
            SlotState::Skipped(reason) => format!("skipped: {reason}"),
        }
    }

    #[test]
    fn pinned_matching_file_is_measured_and_everything_else_is_an_explicit_skip() {
        let dir = common::scratch("artifacts");
        let bytes = fs::read(FIXTURE).expect("fixture");
        let good = sha256_hex(&bytes);
        fs::write(dir.join("tiny-tokenizer.json"), &bytes).expect("copy fixture");

        assert_eq!(state_of(&dir, Some(&good)), "ready");

        let unpinned = state_of(&dir, None);
        assert!(
            unpinned.contains("manifest sha256 is null") && unpinned.contains(&good),
            "{unpinned}"
        );

        let wrong = "0".repeat(64);
        let mismatch = state_of(&dir, Some(&wrong));
        assert!(mismatch.contains("sha256 mismatch"), "{mismatch}");

        let empty = common::scratch("artifacts-empty");
        let _ = fs::remove_file(empty.join("tiny-tokenizer.json"));
        assert!(state_of(&empty, Some(&good)).contains("artifact not found"));

        // A pinned file that is not a tokenizer is a skip, not a panic.
        let garbage_dir = common::scratch("artifacts-garbage");
        fs::write(garbage_dir.join("tiny-tokenizer.json"), b"{not a tokenizer").expect("write");
        let garbage_sha = sha256_hex(b"{not a tokenizer");
        assert!(state_of(&garbage_dir, Some(&garbage_sha)).contains("failed to load"));

        // Path traversal in local_path is refused.
        let slots = adapter::build_slots(&manifest_for("null", "../escape.json"), &dir);
        assert!(
            matches!(&slots[0].state, SlotState::Skipped(r) if r.contains("unsafe local_path"))
        );
    }

    #[test]
    fn measured_file_tokenizer_records_sha_and_revision() {
        let dir = common::scratch("artifacts-prov");
        let bytes = fs::read(FIXTURE).expect("fixture");
        let sha = sha256_hex(&bytes);
        fs::write(dir.join("tiny-tokenizer.json"), &bytes).expect("copy fixture");
        let slots = adapter::build_slots(
            &manifest_for(&format!("\"{sha}\""), "tiny-tokenizer.json"),
            &dir,
        );
        let p = &slots[0].provenance;
        assert_eq!(p.sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(p.revision.as_deref(), Some("hand-built-v1"));
        assert_eq!(
            p.loader_crate.as_deref(),
            Some(format!("tokenizers {HF_TOKENIZERS_VERSION}").as_str())
        );
        let json = p.to_json().to_pretty_string();
        assert!(json.contains(&sha));
    }
}
