//! Proves the HuggingFace `tokenizers` JSON loader path with a hand-built,
//! checked-in tokenizer (26 vocab entries), so it is exercised even though no
//! real vocabulary file is present.

#![cfg(feature = "hf")]

mod common;

use std::fs;

use tessera_tokenbench::adapter::{self, Encoder};
use tessera_tokenbench::bench;
use tessera_tokenbench::corpus::{self, sha256_hex};
use tessera_tokenbench::metrics;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tiny-tokenizer.json"
);
const SRC: &str = "f add(a:i64,b:i64)>i64=a+b\n";

fn tiny() -> Encoder {
    adapter::hf_from_bytes(&fs::read(FIXTURE).expect("fixture")).expect("load fixture")
}

#[test]
fn byte_ranges_follow_the_hand_built_merges() {
    let ranges = tiny().encode_ranges(SRC).expect("encode");
    // "f " | add | ( | a: | i64 | , | b: | i64 | )> | i64 | =a | +b | \n
    assert_eq!(
        ranges,
        [
            (0, 2),
            (2, 5),
            (5, 6),
            (6, 8),
            (8, 11),
            (11, 12),
            (12, 14),
            (14, 17),
            (17, 19),
            (19, 22),
            (22, 24),
            (24, 26),
            (26, 27)
        ]
    );
    assert_eq!(tiny().count(SRC).expect("count"), 13);
    // ranges tile the input exactly
    assert_eq!(ranges.first().map(|r| r.0), Some(0));
    assert_eq!(ranges.last().map(|r| r.1), Some(SRC.len()));
    assert!(ranges.windows(2).all(|w| w[0].1 == w[1].0));
}

#[test]
fn offsets_are_byte_offsets_for_non_ascii_input() {
    // `é` is two bytes and unknown to the tiny vocabulary: one [UNK] covering both bytes.
    let text = "aé";
    let ranges = tiny().encode_ranges(text).expect("encode");
    assert_eq!(ranges.last().map(|r| r.1), Some(text.len()), "{ranges:?}");
}

#[test]
fn alignment_metrics_are_computed_from_hf_offsets() {
    let ranges = tiny().encode_ranges(SRC).expect("encode");
    let al = metrics::alignment(SRC, &ranges);
    // 17 grammar units; merges `a:`, `b:`, `)>`, `=a`, `+b` join five adjacent pairs.
    assert_eq!((al.units, al.intact, al.exact), (17, 17, 6));
    assert_eq!((al.pairs, al.merged), (16, 5));
}

#[test]
fn the_whole_benchmark_runs_with_a_file_loaded_tokenizer_alongside_embedded_ones() {
    let bytes = fs::read(FIXTURE).expect("fixture");
    let dir = common::scratch("hf-pipeline");
    fs::write(dir.join("tiny-tokenizer.json"), &bytes).expect("copy");
    let manifest = adapter::parse_manifest(
        format!(
            r#"{{"schema_version":1,"tokenizers":[
              {{"id":"openai/cl100k_base","family":"gpt4-cl100k","vendor":"OpenAI","loader":"tiktoken-embedded",
                "embedded":{{"crate":"tiktoken-rs","encoding":"cl100k_base"}},"file":null,"notes":""}},
              {{"id":"fixture/tiny-bpe","family":"fixture","vendor":"Fixture","loader":"hf-tokenizers-json","embedded":null,
                "file":{{"repo_id":"local/hand-built","hub_filename":"tiny-tokenizer.json","local_path":"tiny-tokenizer.json",
                "sha256":"{}","revision":"hand-built-v1","approx_size_mb":null,"hub_metadata_verified":true,
                "license_note":"test fixture"}},"notes":""}}]}}"#,
            sha256_hex(&bytes)
        )
        .as_bytes(),
    )
    .expect("manifest");
    let slots = adapter::build_slots(&manifest, &dir);
    let loaded = corpus::load(&common::bench_dir().join("corpus")).expect("corpus");
    let results = bench::run_with(&loaded, &slots, None).expect("run");
    assert_eq!(
        results.measured_ids(),
        ["openai/cl100k_base", "fixture/tiny-bpe"]
    );
    assert_eq!(results.measured_vendors(), ["Fixture", "OpenAI"]);
    let add = &results.programs[0]
        .variants
        .iter()
        .find(|v| v.file.ends_with("tessera_A.tes"))
        .expect("A");
    // The tiny vocabulary tokenizes the very text it was built for in 13 tokens.
    assert_eq!(add.tokens["fixture/tiny-bpe"], 13);
    assert!(add.alignment.contains_key("fixture/tiny-bpe"));
    assert!(add.stats.is_some());
}
