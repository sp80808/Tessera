//! Results -> deterministic JSON (`schema_version` 1).

use std::collections::BTreeMap;

use crate::analysis::{self, Pair};
use crate::bench::{
    ProgramRecord, RESULTS_SCHEMA_VERSION, Results, RewriteSet, TOOL_NAME, TokenizerRecord,
    VariantRecord,
};
use crate::corpus::Lang;
use crate::json::Json;
use crate::metrics::{Alignment, Stats};

/// Metric definitions embedded in every results file so it is self-describing.
#[must_use]
pub fn definitions() -> Json {
    Json::obj([
        (
            "bytes_chars_non_ws",
            Json::str(
                "UTF-8 bytes, Unicode scalar values, and non-whitespace scalar values of the file text (one trailing newline included in every file).",
            ),
        ),
        (
            "lexer_tokens",
            Json::str(
                "Non-trivia tokens from tessera_syntax::lexer::lex (tessera variants only; null otherwise). Characters the provisional lexicon does not name (`^`, `?`) are Error tokens, counted in lexer_error_tokens.",
            ),
        ),
        (
            "tokens",
            Json::str(
                "Model tokens per measured tokenizer from ordinary encoding of the file text: no special tokens, no BOS/EOS.",
            ),
        ),
        (
            "stats.median",
            Json::str(
                "Median over measured tokenizers (mean of the two middle values when their number is even).",
            ),
        ),
        (
            "stats.p90",
            Json::str(
                "Nearest-rank 90th percentile over measured tokenizers: sorted[ceil(0.9*n)-1]. Equals worst when n < 10.",
            ),
        ),
        (
            "stats.dispersion",
            Json::str("(worst - min) / median over measured tokenizers; 0 when the median is 0."),
        ),
        (
            "model_density",
            Json::str(
                "semantic_units / stats.median. semantic_units is defined in `semantic_units_definition`.",
            ),
        ),
        (
            "tir_nodes",
            Json::str(
                "TIR expression nodes (int, bool, var, add, eq, and, not) in tessera_syntax::expand output; parsed variants only.",
            ),
        ),
        (
            "alignment.intact",
            Json::str(
                "Grammar units (non-trivia lexer tokens) with no model-token boundary strictly inside their byte range.",
            ),
        ),
        (
            "alignment.exact",
            Json::str("Grammar units whose byte range is exactly one model token."),
        ),
        (
            "alignment.merged",
            Json::str(
                "Adjacent grammar-unit pairs covered by a single model token (it contains the last byte of the first unit and the first byte of the second). Denominator: units - 1.",
            ),
        ),
        (
            "rewrites.delta",
            Json::str(
                "tokens(rewritten text) - tokens(canonical text) for one tokenizer; stable means delta == 0.",
            ),
        ),
        (
            "rewrites.resegmented_units",
            Json::str(
                "Grammar units whose model-token segmentation differs from the canonical spelling: internal cuts, or whether a token boundary sits at the unit's start or end (a unit that now merges with a different neighbour counts). null when the rewrite changes the unit sequence.",
            ),
        ),
        (
            "rewrites.split_changed_units",
            Json::str("Subset view of resegmented_units: units whose internal cuts alone differ."),
        ),
        (
            "rewrites.fmt_collapses_to_canonical",
            Json::str(
                "tessera_syntax::fmt(rewrite) == fmt(canonical). Layout rewrites must; rename rewrites must not.",
            ),
        ),
    ])
}

fn stats_json(stats: Option<&Stats>) -> Json {
    stats.map_or(Json::Null, |s| {
        Json::obj([
            ("median", Json::Float(s.median)),
            ("p90", Json::int(s.p90)),
            ("worst", Json::int(s.worst)),
            ("min", Json::int(s.min)),
            ("dispersion", Json::Float(s.dispersion)),
        ])
    })
}

fn alignment_json(a: &Alignment) -> Json {
    Json::obj([
        ("units", Json::int(a.units)),
        ("intact", Json::int(a.intact)),
        ("exact", Json::int(a.exact)),
        ("pairs", Json::int(a.pairs)),
        ("merged", Json::int(a.merged)),
        ("intact_fraction", Json::Float(a.intact_fraction())),
        ("exact_fraction", Json::Float(a.exact_fraction())),
        ("merged_fraction", Json::Float(a.merged_fraction())),
    ])
}

fn tokens_json(tokens: &BTreeMap<String, usize>) -> Json {
    Json::obj(tokens.iter().map(|(k, v)| (k.clone(), Json::int(*v))))
}

fn variant_json(v: &VariantRecord) -> Json {
    Json::obj([
        ("lang", Json::str(v.lang.as_str())),
        ("candidate", Json::opt_str(v.candidate.as_deref())),
        ("file", Json::str(&v.file)),
        (
            "status",
            Json::opt_str(v.status.map(crate::corpus::Status::as_str)),
        ),
        ("notes", Json::str(&v.notes)),
        ("bytes", Json::int(v.bytes)),
        ("chars", Json::int(v.chars)),
        ("non_ws_chars", Json::int(v.non_ws_chars)),
        ("lexer_tokens", Json::opt_int(v.lexer_tokens)),
        ("lexer_error_tokens", Json::opt_int(v.lexer_error_tokens)),
        ("parser_diagnostics", Json::opt_int(v.parser_diagnostics)),
        ("tir_nodes", Json::opt_int(v.tir_nodes)),
        ("tokens", tokens_json(&v.tokens)),
        ("stats", stats_json(v.stats.as_ref())),
        (
            "model_density",
            v.model_density.map_or(Json::Null, Json::Float),
        ),
        (
            "median_tokens_per_tir_node",
            v.median_tokens_per_tir_node.map_or(Json::Null, Json::Float),
        ),
        (
            "alignment",
            Json::obj(
                v.alignment
                    .iter()
                    .map(|(k, a)| (k.clone(), alignment_json(a))),
            ),
        ),
    ])
}

fn pair_json(p: &Pair) -> Json {
    Json::obj([
        ("candidate", Json::str(&p.candidate)),
        ("vs", Json::str("A")),
        ("anchor_bytes", Json::int(p.anchor_bytes)),
        ("candidate_bytes", Json::int(p.candidate_bytes)),
        ("anchor_non_ws_chars", Json::int(p.anchor_non_ws)),
        ("candidate_non_ws_chars", Json::int(p.candidate_non_ws)),
        (
            "tokens",
            Json::obj(p.per_tokenizer.iter().map(|(id, &(a, c))| {
                (
                    id.clone(),
                    Json::obj([
                        ("anchor", Json::int(a)),
                        ("candidate", Json::int(c)),
                        (
                            "delta",
                            Json::Int(
                                i64::try_from(c).unwrap_or(0) - i64::try_from(a).unwrap_or(0),
                            ),
                        ),
                    ]),
                )
            })),
        ),
        (
            "shorter_but_costlier_on",
            Json::strs(p.shorter_but_costlier_on()),
        ),
        (
            "shorter_but_no_cheaper_on",
            Json::strs(p.shorter_but_no_cheaper_on()),
        ),
        (
            "longer_but_cheaper_on",
            Json::strs(p.longer_but_cheaper_on()),
        ),
    ])
}

fn program_json(p: &ProgramRecord, pairs: &[Pair]) -> Json {
    Json::obj([
        ("id", Json::str(&p.id)),
        ("category", Json::str(&p.category)),
        ("description", Json::str(&p.description)),
        ("semantic_units", Json::int(p.semantic_units as usize)),
        ("semantic_units_note", Json::str(&p.semantic_units_note)),
        (
            "variants",
            Json::Arr(p.variants.iter().map(variant_json).collect()),
        ),
        (
            "candidate_pairs",
            Json::Arr(
                pairs
                    .iter()
                    .filter(|pair| pair.program == p.id)
                    .map(pair_json)
                    .collect(),
            ),
        ),
    ])
}

fn tokenizer_json(t: &TokenizerRecord) -> Json {
    Json::obj([
        ("id", Json::str(&t.id)),
        ("family", Json::str(&t.family)),
        ("vendor", Json::str(&t.vendor)),
        ("notes", Json::str(&t.notes)),
        (
            "status",
            Json::str(if t.skipped.is_none() {
                "measured"
            } else {
                "skipped"
            }),
        ),
        ("skip_reason", Json::opt_str(t.skipped.as_deref())),
        ("provenance", t.provenance.to_json()),
    ])
}

fn rewrite_set_json(set: &RewriteSet) -> Json {
    Json::obj([
        ("program", Json::str(&set.program)),
        ("candidate", Json::str(&set.candidate)),
        ("grammar_units", Json::int(set.grammar_units)),
        ("canonical_tokens", tokens_json(&set.canonical_tokens)),
        (
            "rewrites",
            Json::Arr(
                set.items
                    .iter()
                    .map(|item| {
                        Json::obj([
                            ("name", Json::str(item.name)),
                            ("kind", Json::str(item.kind.as_str())),
                            ("bytes_delta", Json::Int(item.bytes_delta)),
                            (
                                "fmt_collapses_to_canonical",
                                Json::Bool(item.fmt_collapses_to_canonical),
                            ),
                            (
                                "per_tokenizer",
                                Json::obj(item.per_tokenizer.iter().map(|(id, t)| {
                                    (
                                        id.clone(),
                                        Json::obj([
                                            ("tokens", Json::int(t.tokens)),
                                            ("delta", Json::Int(t.delta)),
                                            ("stable", Json::Bool(t.delta == 0)),
                                            (
                                                "resegmented_units",
                                                Json::opt_int(t.resegmented_units),
                                            ),
                                            (
                                                "split_changed_units",
                                                Json::opt_int(t.split_changed_units),
                                            ),
                                        ]),
                                    )
                                })),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// Honest, data-derived caveats. Shared with the Markdown report.
#[must_use]
pub fn limitations(r: &Results) -> Vec<String> {
    let measured = r.measured_ids();
    let vendors = r.measured_vendors();
    let skipped: Vec<&str> = r
        .tokenizers
        .iter()
        .filter(|t| t.skipped.is_some())
        .map(|t| t.id.as_str())
        .collect();
    let tessera: Vec<&VariantRecord> = r
        .programs
        .iter()
        .flat_map(|p| p.variants.iter())
        .filter(|v| v.lang == Lang::Tessera)
        .collect();
    let unparsed = tessera.iter().filter(|v| !v.is_parsed_tessera()).count();
    let programs = r.programs.len();
    vec![
        format!(
            "Tokenizer coverage: {} tokenizer(s) measured ({}) from {} distinct vendor(s) ({}). Issue #1 asks for at least 4 real model tokenizer families; distinct vocabularies from one vendor are not distinct families, so that criterion is not met by this run. Skipped: {}.",
            measured.len(),
            measured.join(", "),
            vendors.len(),
            vendors.join(", "),
            if skipped.is_empty() { "none".to_owned() } else { skipped.join(", ") },
        ),
        "No model-quality evidence: token counts say nothing about generation, repair or comprehension accuracy (docs/research/benchmarks.md section B). A cheaper spelling is not a better spelling until model-quality benchmarks agree.".to_owned(),
        format!(
            "{unparsed} of {} tessera variants are `unparsed` candidate spellings for constructs the current parser does not implement. They are unvalidated text, not Tessera; only `parsed` variants are accepted by tessera-syntax.",
            tessera.len()
        ),
        "Constructs outside the candidate-syntax matrix (blocks, if/else, loops, struct/enum, match, return, Result/Ok/Err, calls, field access, deref, compound assignment) use benchmark-author strawman spellings (verbose keyword with compact punctuation), identical across candidates. Cross-language comparisons against unparsed Tessera text are therefore not evidence about language-level efficiency.".to_owned(),
        "Lexer token counts for unparsed variants count characters the provisional lexicon does not yet name (`^`, `?`) as Error tokens (see lexer_error_tokens).".to_owned(),
        format!(
            "The corpus is {programs} tiny programs written by one author with short identifiers: micro-snippet counts are dominated by fixed costs (imports, `package`, newlines), and there is no unseen-repository or unseen-domain split (SA-BPE concern). Identifier strategy is a separate experiment."
        ),
        "With fewer than 10 tokenizers p90 equals worst by nearest rank, and dispersion is sensitive to old vocabularies (r50k/p50k tokenize whitespace-heavy code poorly). Never optimize a syntax against one tokenizer.".to_owned(),
        "Statistical significance is not assessed: counts are deterministic for a fixed corpus and tokenizer, so the regression gate is a threshold on counts, not a hypothesis test.".to_owned(),
    ]
}

/// Serialize results to the deterministic JSON document.
#[must_use]
pub fn to_json(r: &Results) -> Json {
    let pairs = analysis::pairs(r);
    let tessera_variants: Vec<&VariantRecord> = r
        .programs
        .iter()
        .flat_map(|p| p.variants.iter())
        .filter(|v| v.lang == Lang::Tessera)
        .collect();
    let all_variants: usize = r.programs.iter().map(|p| p.variants.len()).sum();
    let measured = r.measured_ids().len();
    let groups = analysis::groups(r);
    let vs_anchor = analysis::vs_anchor(r);
    let alignment = analysis::alignment_groups(r);
    let rewrite_summary = analysis::rewrite_summary(r);
    Json::obj([
        (
            "schema_version",
            Json::Int(i64::from(RESULTS_SCHEMA_VERSION)),
        ),
        (
            "tool",
            Json::obj([
                ("name", Json::str(TOOL_NAME)),
                ("version", Json::str(env!("CARGO_PKG_VERSION"))),
            ]),
        ),
        ("corpus_hash", Json::str(&r.corpus_hash)),
        (
            "corpus_revision",
            Json::opt_str(r.corpus_revision.as_deref()),
        ),
        (
            "semantic_units_definition",
            Json::str(&r.semantic_units_definition),
        ),
        ("definitions", definitions()),
        (
            "summary",
            Json::obj([
                ("programs", Json::int(r.programs.len())),
                ("variants", Json::int(all_variants)),
                ("tessera_variants", Json::int(tessera_variants.len())),
                (
                    "tessera_parsed",
                    Json::int(
                        tessera_variants
                            .iter()
                            .filter(|v| v.is_parsed_tessera())
                            .count(),
                    ),
                ),
                ("tokenizers_measured", Json::int(measured)),
                (
                    "tokenizers_skipped",
                    Json::int(r.tokenizers.len() - measured),
                ),
                ("measured_vendors", Json::strs(r.measured_vendors())),
            ]),
        ),
        (
            "candidates",
            Json::obj(r.candidates.iter().map(|(label, def)| {
                (
                    label.clone(),
                    Json::obj([
                        ("description", Json::str(&def.description)),
                        ("differs_from_anchor", Json::str(&def.differs_from_anchor)),
                        (
                            "axes",
                            Json::obj(def.axes.iter().map(|(k, v)| (k.clone(), Json::str(v)))),
                        ),
                    ]),
                )
            })),
        ),
        (
            "tokenizers",
            Json::Arr(r.tokenizers.iter().map(tokenizer_json).collect()),
        ),
        (
            "programs",
            Json::Arr(r.programs.iter().map(|p| program_json(p, &pairs)).collect()),
        ),
        (
            "aggregate",
            Json::obj([
                (
                    "groups",
                    Json::Arr(
                        groups
                            .iter()
                            .map(|g| {
                                Json::obj([
                                    ("group", Json::str(&g.name)),
                                    ("programs", Json::int(g.programs)),
                                    ("bytes", Json::int(g.bytes)),
                                    ("tokens", tokens_json(&g.tokens)),
                                    ("mean_dispersion", Json::Float(g.mean_dispersion)),
                                    ("max_dispersion", Json::Float(g.max_dispersion)),
                                    (
                                        "max_dispersion_program",
                                        Json::str(&g.max_dispersion_program),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "candidate_vs_anchor",
                    Json::Arr(
                        vs_anchor
                            .iter()
                            .map(|v| {
                                Json::obj([
                                    ("candidate", Json::str(&v.candidate)),
                                    ("programs", Json::strs(&v.programs)),
                                    ("anchor_bytes", Json::int(v.anchor_bytes)),
                                    ("candidate_bytes", Json::int(v.candidate_bytes)),
                                    (
                                        "tokens",
                                        Json::obj(v.per_tokenizer.iter().map(|(id, &(a, c))| {
                                            (
                                                id.clone(),
                                                Json::obj([
                                                    ("anchor", Json::int(a)),
                                                    ("candidate", Json::int(c)),
                                                    (
                                                        "delta",
                                                        Json::Int(
                                                            i64::try_from(c).unwrap_or(0)
                                                                - i64::try_from(a).unwrap_or(0),
                                                        ),
                                                    ),
                                                ]),
                                            )
                                        })),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "alignment",
                    Json::Arr(
                        alignment
                            .iter()
                            .map(|g| {
                                Json::obj([
                                    ("group", Json::str(&g.name)),
                                    ("variants", Json::int(g.variants)),
                                    (
                                        "per_tokenizer",
                                        Json::obj(
                                            g.per_tokenizer
                                                .iter()
                                                .map(|(id, a)| (id.clone(), alignment_json(a))),
                                        ),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
        ),
        (
            "rewrites",
            Json::obj([
                (
                    "sets",
                    Json::Arr(r.rewrites.iter().map(rewrite_set_json).collect()),
                ),
                (
                    "summary",
                    Json::Arr(
                        rewrite_summary
                            .iter()
                            .map(|a| {
                                Json::obj([
                                    ("name", Json::str(a.name)),
                                    ("kind", Json::str(a.kind.as_str())),
                                    ("variants", Json::int(a.variants)),
                                    ("collapsed_by_fmt", Json::int(a.collapsed_by_fmt)),
                                    (
                                        "per_tokenizer",
                                        Json::obj(a.per_tokenizer.iter().map(|(id, t)| {
                                            (
                                                id.clone(),
                                                Json::obj([
                                                    ("stable", Json::int(t.stable)),
                                                    ("total", Json::int(t.total)),
                                                    ("min_delta", Json::Int(t.min_delta)),
                                                    ("max_delta", Json::Int(t.max_delta)),
                                                    (
                                                        "resegmented_units",
                                                        Json::int(t.resegmented.0),
                                                    ),
                                                    (
                                                        "split_changed_units",
                                                        Json::int(t.split_changed),
                                                    ),
                                                    ("units_compared", Json::int(t.resegmented.1)),
                                                ]),
                                            )
                                        })),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
        ),
        ("limitations", Json::strs(limitations(r))),
    ])
}
