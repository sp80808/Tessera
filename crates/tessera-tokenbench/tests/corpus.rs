//! Corpus soundness: files, labels, parser-verified status, candidate consistency.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use tessera_phases::FileId;
use tessera_syntax::cst;
use tessera_tokenbench::corpus::{self, Corpus, Lang, LoadedCorpus, Program, Status, Variant};

fn load() -> LoadedCorpus {
    corpus::load(&common::bench_dir().join("corpus")).expect("load corpus")
}

#[test]
fn corpus_is_sound_and_every_file_is_referenced() {
    let dir = common::bench_dir().join("corpus");
    let loaded = load();
    let problems = corpus::validate(&loaded);
    assert!(
        problems.is_empty(),
        "corpus problems:\n{}",
        problems.join("\n")
    );
    let stray = corpus::unreferenced_files(&dir, &loaded);
    assert!(
        stray.is_empty(),
        "files not referenced by corpus.json: {stray:?}"
    );
    // every referenced file exists (load reads them all) and is non-empty
    for (path, text) in &loaded.files {
        assert!(!text.trim().is_empty(), "{path} is empty");
        assert!(
            text.ends_with('\n') && !text.ends_with("\n\n"),
            "{path}: exactly one trailing newline"
        );
        assert!(
            text.is_ascii(),
            "{path}: corpus text is ASCII so byte and char counts agree"
        );
    }
}

#[test]
fn corpus_shape_follows_the_selection_criteria() {
    let loaded = load();
    let programs = &loaded.corpus.programs;
    assert!(
        (12..=16).contains(&programs.len()),
        "{} programs",
        programs.len()
    );
    let categories: BTreeSet<_> = programs.iter().map(|p| p.category.as_str()).collect();
    assert!(categories.len() >= 8, "categories: {categories:?}");
    for p in programs {
        let langs: BTreeSet<Lang> = p.variants.iter().map(|v| v.lang).collect();
        for lang in Lang::ALL {
            assert!(langs.contains(&lang), "{} lacks {}", p.id, lang.as_str());
        }
        assert!(
            p.variants
                .iter()
                .any(|v| v.candidate.as_deref() == Some("A")),
            "{} lacks candidate A",
            p.id
        );
        assert!(p.semantic_units > 0);
    }
    let parsed = programs
        .iter()
        .flat_map(|p| &p.variants)
        .filter(|v| v.status == Some(Status::Parsed))
        .count();
    assert!(
        parsed >= 1,
        "at least one program must be expressible in the current parser"
    );
}

#[test]
fn parsed_variants_parse_cleanly_with_the_real_parser() {
    let loaded = load();
    let mut checked = 0;
    for p in &loaded.corpus.programs {
        for v in p
            .variants
            .iter()
            .filter(|v| v.status == Some(Status::Parsed))
        {
            let text = loaded.text(v);
            let out = cst::parse_file(FileId(0), text);
            assert!(
                out.diagnostics.is_empty(),
                "{} is labelled parsed but has diagnostics: {:?}",
                v.file,
                out.diagnostics
                    .iter()
                    .map(|d| d.message.clone())
                    .collect::<Vec<_>>()
            );
            assert_eq!(out.value.reconstruct(text), text, "CST is lossless");
            // parsed variants are already in the one canonical spelling
            assert_eq!(
                tessera_syntax::fmt(text).expect("fmt"),
                text.trim_end(),
                "{}",
                v.file
            );
            checked += 1;
        }
    }
    assert!(checked >= 3, "only {checked} parsed variants");
}

#[test]
fn unparsed_variants_are_labelled_and_really_rejected() {
    let loaded = load();
    let mut unparsed = 0;
    for p in &loaded.corpus.programs {
        for v in p.variants.iter().filter(|v| v.lang == Lang::Tessera) {
            match v.status {
                Some(Status::Parsed) => {}
                Some(Status::Unparsed) => {
                    let out = cst::parse_file(FileId(0), loaded.text(v));
                    assert!(
                        !out.diagnostics.is_empty(),
                        "{} is labelled unparsed but the parser accepts it: promote it",
                        v.file
                    );
                    assert!(
                        v.notes.starts_with("unparsed:"),
                        "{}: notes must say what is unparsed",
                        v.file
                    );
                    unparsed += 1;
                }
                None => panic!("{} is a tessera variant without a status", v.file),
            }
        }
    }
    assert!(unparsed > 0);
}

#[test]
fn non_tessera_variants_carry_no_candidate_or_status() {
    let loaded = load();
    for p in &loaded.corpus.programs {
        for v in p.variants.iter().filter(|v| v.lang != Lang::Tessera) {
            assert!(v.candidate.is_none() && v.status.is_none(), "{}", v.file);
        }
    }
}

#[test]
fn candidate_labels_are_used_consistently() {
    let loaded = load();
    let defs = &loaded.corpus.candidates;
    let mut used: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &loaded.corpus.programs {
        let anchor = p
            .variants
            .iter()
            .find(|v| v.candidate.as_deref() == Some("A"))
            .map(|v| loaded.text(v))
            .expect("anchor");
        for v in p.variants.iter().filter(|v| v.lang == Lang::Tessera) {
            let label = v.candidate.as_deref().expect("label");
            let def = defs
                .get(label)
                .unwrap_or_else(|| panic!("{}: undefined label {label}", v.file));
            *used.entry(label).or_default() += 1;
            let text = loaded.text(v);
            // Only the label's own axis may deviate from the anchor's spelling.
            for (axis, values) in corpus::features(text) {
                for value in values {
                    assert_eq!(
                        def.axes.get(axis).map(String::as_str),
                        Some(value),
                        "{}: `{value}` on axis `{axis}` contradicts label {label}",
                        v.file
                    );
                }
            }
            if label != "A" {
                assert_ne!(text, anchor, "{} duplicates the anchor", v.file);
                assert!(
                    corpus::features(text).contains_key(def.differs_from_anchor.as_str()),
                    "{}: label {label} never exercises `{}`",
                    v.file,
                    def.differs_from_anchor
                );
            }
        }
    }
    for label in defs.keys() {
        assert!(
            used.contains_key(label.as_str()),
            "label {label} is defined but never used"
        );
    }
    // one substitution per label, and no two labels mean the same thing
    let anchor = &defs["A"].axes;
    let mut seen = BTreeSet::new();
    for (label, def) in defs {
        assert!(
            seen.insert(def.axes.clone()),
            "label {label} duplicates another label's axes"
        );
        if label != "A" {
            let diff: Vec<_> = def
                .axes
                .iter()
                .filter(|(k, v)| anchor.get(*k) != Some(*v))
                .collect();
            assert_eq!(
                diff.len(),
                1,
                "{label} must differ from A on exactly one axis"
            );
            assert_eq!(diff[0].0, &def.differs_from_anchor);
        }
    }
}

// ---- validate() must catch mislabelling: synthetic corpora ----

fn base_defs() -> Corpus {
    let mut corpus = load().corpus;
    corpus.programs.clear();
    corpus
}

fn synthetic(tessera: &[(&str, Status, &str)]) -> LoadedCorpus {
    let mut corpus = base_defs();
    let mut files = BTreeMap::new();
    let mut variants = Vec::new();
    for (lang, name) in [
        (Lang::Rust, "r.rs"),
        (Lang::C, "c.c"),
        (Lang::Zig, "z.zig"),
        (Lang::Odin, "o.odin"),
    ] {
        variants.push(Variant {
            lang,
            candidate: None,
            file: name.to_owned(),
            status: None,
            notes: String::new(),
        });
        files.insert(name.to_owned(), "x\n".to_owned());
    }
    for (i, (label, status, text)) in tessera.iter().enumerate() {
        let file = format!("t{i}.tes");
        variants.push(Variant {
            lang: Lang::Tessera,
            candidate: Some((*label).to_owned()),
            file: file.clone(),
            status: Some(*status),
            notes: "unparsed: synthetic".to_owned(),
        });
        files.insert(file, format!("{text}\n"));
    }
    corpus.programs.push(Program {
        id: "synthetic".to_owned(),
        category: "test".to_owned(),
        description: "synthetic".to_owned(),
        semantic_units: 1,
        semantic_units_note: "n/a".to_owned(),
        variants,
    });
    LoadedCorpus {
        corpus,
        files,
        hash: String::new(),
    }
}

const GOOD_A: &str = "f add(a:i64,b:i64)>i64=a+b";

#[test]
fn validate_accepts_a_wellformed_synthetic_corpus() {
    let problems = corpus::validate(&synthetic(&[("A", Status::Parsed, GOOD_A)]));
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn validate_rejects_parsed_label_on_text_the_parser_rejects() {
    let bad = synthetic(&[("A", Status::Parsed, "f add(a:i64,b:i64)>i64={a+b}")]);
    let problems = corpus::validate(&bad).join("\n");
    assert!(problems.contains("labelled parsed"), "{problems}");
}

#[test]
fn validate_rejects_unparsed_label_on_text_the_parser_accepts() {
    let bad = synthetic(&[("A", Status::Unparsed, GOOD_A)]);
    let problems = corpus::validate(&bad).join("\n");
    assert!(problems.contains("promote it to parsed"), "{problems}");
}

#[test]
fn validate_rejects_inconsistent_candidate_labels() {
    // label B means "function form NAME:(..)"; this text uses the `f` keyword form and
    // a `let` bind, which belong to A / E.
    let mixed = synthetic(&[
        ("A", Status::Parsed, GOOD_A),
        ("B", Status::Unparsed, "f calc(a:i64)>i64={let s=a+1;s}"),
    ]);
    let problems = corpus::validate(&mixed).join("\n");
    assert!(
        problems.contains("uses `f-keyword` on axis `fn` but B defines `name-colon-sig`"),
        "{problems}"
    );
    assert!(
        problems.contains("uses `let` on axis `bind` but B defines `walrus`"),
        "{problems}"
    );
    // a candidate that never uses the construct it is supposed to vary
    let unexercised = synthetic(&[
        ("A", Status::Parsed, GOOD_A),
        ("B", Status::Unparsed, "struct P{x:i64}"),
    ]);
    assert!(
        corpus::validate(&unexercised)
            .join("\n")
            .contains("never exercises its axis")
    );
    // an undefined label
    let undefined = synthetic(&[
        ("A", Status::Parsed, GOOD_A),
        ("Z", Status::Unparsed, "calc:(a:i64)>i64=a"),
    ]);
    assert!(
        corpus::validate(&undefined)
            .join("\n")
            .contains("not defined")
    );
    // a candidate identical to the anchor
    let dup = synthetic(&[
        ("A", Status::Parsed, GOOD_A),
        ("B", Status::Unparsed, GOOD_A),
    ]);
    assert!(
        corpus::validate(&dup)
            .join("\n")
            .contains("identical to candidate `A`")
    );
}

#[test]
fn validate_rejects_missing_language_and_missing_anchor() {
    let mut no_anchor = synthetic(&[("A", Status::Parsed, GOOD_A)]);
    no_anchor.corpus.programs[0]
        .variants
        .retain(|v| v.candidate.as_deref() != Some("A"));
    assert!(
        corpus::validate(&no_anchor)
            .join("\n")
            .contains("missing tessera candidate `A`")
    );
    let mut no_zig = synthetic(&[("A", Status::Parsed, GOOD_A)]);
    no_zig.corpus.programs[0]
        .variants
        .retain(|v| v.lang != Lang::Zig);
    assert!(
        corpus::validate(&no_zig)
            .join("\n")
            .contains("missing `zig`")
    );
}
