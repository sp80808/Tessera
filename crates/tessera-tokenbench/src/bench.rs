//! The benchmark run: corpus x tokenizers -> typed [`Results`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::adapter::{self, Provenance, SlotState, TokenizerSlot};
use crate::corpus::{self, CandidateDef, Lang, LoadedCorpus, Status};
use crate::metrics::{self, Alignment, Stats};
use crate::rewrite::{self, RewriteKind};

pub const RESULTS_SCHEMA_VERSION: u32 = 1;
pub const TOOL_NAME: &str = "tess-tokenbench";

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub corpus_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub tokenizers_dir: PathBuf,
    /// Caller-provided corpus revision label (never derived by shelling out).
    pub corpus_revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TokenizerRecord {
    pub id: String,
    pub family: String,
    pub vendor: String,
    pub notes: String,
    pub provenance: Provenance,
    /// `Some(reason)` when the tokenizer could not be measured.
    pub skipped: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VariantRecord {
    pub program: String,
    pub lang: Lang,
    pub candidate: Option<String>,
    pub file: String,
    pub status: Option<Status>,
    pub notes: String,
    pub bytes: usize,
    pub chars: usize,
    pub non_ws_chars: usize,
    pub lexer_tokens: Option<usize>,
    pub lexer_error_tokens: Option<usize>,
    pub parser_diagnostics: Option<usize>,
    pub tir_nodes: Option<usize>,
    /// Model tokens per measured tokenizer id.
    pub tokens: BTreeMap<String, usize>,
    pub stats: Option<Stats>,
    pub model_density: Option<f64>,
    pub median_tokens_per_tir_node: Option<f64>,
    /// Tessera variants only.
    pub alignment: BTreeMap<String, Alignment>,
}

impl VariantRecord {
    /// `tessera/A`, `rust`, ...
    #[must_use]
    pub fn label(&self) -> String {
        match &self.candidate {
            Some(c) => format!("{}/{c}", self.lang.as_str()),
            None => self.lang.as_str().to_owned(),
        }
    }

    #[must_use]
    pub fn is_parsed_tessera(&self) -> bool {
        self.lang == Lang::Tessera && self.status == Some(Status::Parsed)
    }
}

#[derive(Debug, Clone)]
pub struct ProgramRecord {
    pub id: String,
    pub category: String,
    pub description: String,
    pub semantic_units: u32,
    pub semantic_units_note: String,
    pub variants: Vec<VariantRecord>,
}

#[derive(Debug, Clone)]
pub struct RewriteTok {
    pub tokens: usize,
    /// `tokens(rewrite) - tokens(canonical)`.
    pub delta: i64,
    /// Grammar units whose segmentation (internal cuts or boundary alignment at
    /// either edge) differs from the canonical spelling; `None` when the two
    /// spellings do not share a unit sequence.
    pub resegmented_units: Option<usize>,
    /// Subset view: units whose internal cuts alone differ.
    pub split_changed_units: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RewriteRecord {
    pub name: &'static str,
    pub kind: RewriteKind,
    pub bytes_delta: i64,
    pub fmt_collapses_to_canonical: bool,
    pub per_tokenizer: BTreeMap<String, RewriteTok>,
}

#[derive(Debug, Clone)]
pub struct RewriteSet {
    pub program: String,
    pub candidate: String,
    pub canonical_tokens: BTreeMap<String, usize>,
    pub grammar_units: usize,
    pub items: Vec<RewriteRecord>,
}

#[derive(Debug, Clone)]
pub struct Results {
    pub corpus_hash: String,
    pub corpus_revision: Option<String>,
    pub semantic_units_definition: String,
    pub candidates: BTreeMap<String, CandidateDef>,
    pub tokenizers: Vec<TokenizerRecord>,
    pub programs: Vec<ProgramRecord>,
    pub rewrites: Vec<RewriteSet>,
}

impl Results {
    /// Ids of tokenizers that were actually measured, in manifest order.
    #[must_use]
    pub fn measured_ids(&self) -> Vec<&str> {
        self.tokenizers
            .iter()
            .filter(|t| t.skipped.is_none())
            .map(|t| t.id.as_str())
            .collect()
    }

    /// Distinct vendors among measured tokenizers, sorted.
    #[must_use]
    pub fn measured_vendors(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .tokenizers
            .iter()
            .filter(|t| t.skipped.is_none())
            .map(|t| t.vendor.as_str())
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Load the corpus, validate it, resolve tokenizers and run the benchmark.
///
/// # Errors
/// Corpus or manifest problems, corpus validation failures (a `parsed` label
/// the parser does not confirm, inconsistent candidate labels, ...), or a
/// tokenizer backend failure.
pub fn run(opts: &RunOptions) -> Result<Results, String> {
    let loaded = corpus::load(&opts.corpus_dir)?;
    let manifest = adapter::load_manifest(&opts.manifest_path)?;
    let slots = adapter::build_slots(&manifest, &opts.tokenizers_dir);
    run_with(&loaded, &slots, opts.corpus_revision.clone())
}

/// Run against an already loaded corpus and resolved tokenizers.
///
/// # Errors
/// See [`run`].
pub fn run_with(
    loaded: &LoadedCorpus,
    slots: &[TokenizerSlot],
    corpus_revision: Option<String>,
) -> Result<Results, String> {
    let problems = corpus::validate(loaded);
    if !problems.is_empty() {
        return Err(format!(
            "corpus validation failed:\n  {}",
            problems.join("\n  ")
        ));
    }
    let mut programs = Vec::new();
    let mut rewrites = Vec::new();
    for program in &loaded.corpus.programs {
        let mut variants = Vec::new();
        for variant in &program.variants {
            let text = loaded.text(variant);
            let record =
                measure_variant(program.semantic_units, &program.id, variant, text, slots)?;
            if record.is_parsed_tessera() {
                rewrites.push(rewrite_set(&program.id, &record, text, slots)?);
            }
            variants.push(record);
        }
        programs.push(ProgramRecord {
            id: program.id.clone(),
            category: program.category.clone(),
            description: program.description.clone(),
            semantic_units: program.semantic_units,
            semantic_units_note: program.semantic_units_note.clone(),
            variants,
        });
    }
    Ok(Results {
        corpus_hash: loaded.hash.clone(),
        corpus_revision,
        semantic_units_definition: loaded.corpus.semantic_units_definition.clone(),
        candidates: loaded.corpus.candidates.clone(),
        tokenizers: slots
            .iter()
            .map(|s| TokenizerRecord {
                id: s.id.clone(),
                family: s.family.clone(),
                vendor: s.vendor.clone(),
                notes: s.notes.clone(),
                provenance: s.provenance.clone(),
                skipped: match &s.state {
                    SlotState::Ready(_) => None,
                    SlotState::Skipped(reason) => Some(reason.clone()),
                },
            })
            .collect(),
        programs,
        rewrites,
    })
}

/// Count of TIR expression nodes (`int`, `bool`, `var`, `add`, `eq`, `and`,
/// `not`) in the function body of a parsed Tessera text.
fn tir_expression_nodes(text: &str) -> Option<usize> {
    let tir = tessera_syntax::expand(text).ok()?;
    let heads = ["int", "bool", "var", "add", "eq", "and", "not"];
    let count = tir
        .match_indices('(')
        .filter(|(i, _)| {
            let head: String = tir[i + 1..]
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            heads.contains(&head.as_str())
        })
        .count();
    Some(count)
}

fn measure_variant(
    semantic_units: u32,
    program: &str,
    variant: &corpus::Variant,
    text: &str,
    slots: &[TokenizerSlot],
) -> Result<VariantRecord, String> {
    let is_tessera = variant.lang == Lang::Tessera;
    let mut tokens = BTreeMap::new();
    let mut alignment = BTreeMap::new();
    for slot in slots {
        let Some(encoder) = slot.encoder() else {
            continue;
        };
        let ranges = encoder
            .encode_ranges(text)
            .map_err(|e| format!("{} on `{}`: {e}", slot.id, variant.file))?;
        tokens.insert(slot.id.clone(), ranges.len());
        if is_tessera {
            alignment.insert(slot.id.clone(), metrics::alignment(text, &ranges));
        }
    }
    let counts: Vec<usize> = tokens.values().copied().collect();
    let stats = metrics::stats(&counts);
    let (lexer_tokens, lexer_error_tokens) = if is_tessera {
        let (n, errors) = metrics::lexer_counts(text);
        (Some(n), Some(errors))
    } else {
        (None, None)
    };
    let parsed = variant.status == Some(Status::Parsed);
    let tir_nodes = if parsed {
        tir_expression_nodes(text)
    } else {
        None
    };
    Ok(VariantRecord {
        program: program.to_owned(),
        lang: variant.lang,
        candidate: variant.candidate.clone(),
        file: variant.file.clone(),
        status: variant.status,
        notes: variant.notes.clone(),
        bytes: text.len(),
        chars: text.chars().count(),
        non_ws_chars: text.chars().filter(|c| !c.is_whitespace()).count(),
        lexer_tokens,
        lexer_error_tokens,
        parser_diagnostics: is_tessera.then(|| corpus::parser_diagnostics(text)),
        tir_nodes,
        model_density: stats
            .as_ref()
            .filter(|s| s.median > 0.0)
            .map(|s| f64::from(semantic_units) / s.median),
        median_tokens_per_tir_node: match (&stats, tir_nodes) {
            (Some(s), Some(n)) if n > 0 => Some(s.median / n as f64),
            _ => None,
        },
        stats,
        tokens,
        alignment,
    })
}

fn rewrite_set(
    program: &str,
    canonical: &VariantRecord,
    text: &str,
    slots: &[TokenizerSlot],
) -> Result<RewriteSet, String> {
    let mut canonical_ranges = BTreeMap::new();
    for slot in slots {
        if let Some(encoder) = slot.encoder() {
            let ranges = encoder.encode_ranges(text)?;
            canonical_ranges.insert(slot.id.as_str(), ranges);
        }
    }
    let canonical_units = metrics::unit_sequence(text);
    let mut items = Vec::new();
    for rw in rewrite::generate(text) {
        let fmt_collapses =
            rewrite::validate(text, &rw).map_err(|e| format!("program `{program}`: {e}"))?;
        let same_units = metrics::unit_sequence(&rw.text) == canonical_units;
        let mut per_tokenizer = BTreeMap::new();
        for slot in slots {
            let Some(encoder) = slot.encoder() else {
                continue;
            };
            let ranges = encoder.encode_ranges(&rw.text)?;
            let base = &canonical_ranges[slot.id.as_str()];
            let compared = same_units.then(|| {
                let before = metrics::segmentations(text, base);
                let after = metrics::segmentations(&rw.text, &ranges);
                let pairs = || before.iter().zip(&after);
                (
                    pairs().filter(|(a, b)| a != b).count(),
                    pairs()
                        .filter(|(a, b)| a.internal_cuts != b.internal_cuts)
                        .count(),
                )
            });
            per_tokenizer.insert(
                slot.id.clone(),
                RewriteTok {
                    tokens: ranges.len(),
                    delta: i64::try_from(ranges.len()).unwrap_or(i64::MAX)
                        - i64::try_from(base.len()).unwrap_or(i64::MAX),
                    resegmented_units: compared.map(|c| c.0),
                    split_changed_units: compared.map(|c| c.1),
                },
            );
        }
        items.push(RewriteRecord {
            name: rw.name,
            kind: rw.kind,
            bytes_delta: i64::try_from(rw.text.len()).unwrap_or(i64::MAX)
                - i64::try_from(text.len()).unwrap_or(i64::MAX),
            fmt_collapses_to_canonical: fmt_collapses,
            per_tokenizer,
        });
    }
    Ok(RewriteSet {
        program: program.to_owned(),
        candidate: canonical.candidate.clone().unwrap_or_default(),
        canonical_tokens: canonical.tokens.clone(),
        grammar_units: canonical.lexer_tokens.unwrap_or(0),
        items,
    })
}
