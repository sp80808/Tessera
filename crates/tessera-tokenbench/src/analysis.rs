//! Derived comparisons over [`Results`]: candidate pairs, groups, alignment and
//! rewrite summaries. Both the JSON and the Markdown renderers read these, so
//! the two outputs cannot disagree.

use std::collections::BTreeMap;

use crate::bench::{ProgramRecord, Results, VariantRecord};
use crate::corpus::Lang;
use crate::metrics::Alignment;
use crate::rewrite::RewriteKind;

/// One non-anchor Tessera candidate compared with anchor `A` of the same program.
#[derive(Debug, Clone)]
pub struct Pair {
    pub program: String,
    pub candidate: String,
    pub anchor_bytes: usize,
    pub candidate_bytes: usize,
    pub anchor_non_ws: usize,
    pub candidate_non_ws: usize,
    /// tokenizer id -> (anchor tokens, candidate tokens)
    pub per_tokenizer: BTreeMap<String, (usize, usize)>,
}

impl Pair {
    /// Candidate is shorter in bytes yet costs MORE model tokens.
    #[must_use]
    pub fn shorter_but_costlier_on(&self) -> Vec<&str> {
        self.select(|shorter, _, (a, c)| shorter && c > a)
    }

    /// Candidate is shorter in bytes yet saves NO model tokens (ties).
    #[must_use]
    pub fn shorter_but_no_cheaper_on(&self) -> Vec<&str> {
        self.select(|shorter, _, (a, c)| shorter && c == a)
    }

    /// Candidate is longer in bytes yet costs FEWER model tokens.
    #[must_use]
    pub fn longer_but_cheaper_on(&self) -> Vec<&str> {
        self.select(|_, longer, (a, c)| longer && c < a)
    }

    fn select(&self, pred: impl Fn(bool, bool, (usize, usize)) -> bool) -> Vec<&str> {
        let shorter = self.candidate_bytes < self.anchor_bytes;
        let longer = self.candidate_bytes > self.anchor_bytes;
        self.per_tokenizer
            .iter()
            .filter(|&(_, &counts)| pred(shorter, longer, counts))
            .map(|(id, _)| id.as_str())
            .collect()
    }
}

fn tessera<'a>(program: &'a ProgramRecord, label: &str) -> Option<&'a VariantRecord> {
    program
        .variants
        .iter()
        .find(|v| v.lang == Lang::Tessera && v.candidate.as_deref() == Some(label))
}

#[must_use]
pub fn pairs(results: &Results) -> Vec<Pair> {
    let mut out = Vec::new();
    for program in &results.programs {
        let Some(anchor) = tessera(program, "A") else {
            continue;
        };
        for v in &program.variants {
            let label = match (&v.lang, v.candidate.as_deref()) {
                (Lang::Tessera, Some(l)) if l != "A" => l,
                _ => continue,
            };
            let per_tokenizer = anchor
                .tokens
                .iter()
                .filter_map(|(id, &a)| v.tokens.get(id).map(|&c| (id.clone(), (a, c))))
                .collect();
            out.push(Pair {
                program: program.id.clone(),
                candidate: label.to_owned(),
                anchor_bytes: anchor.bytes,
                candidate_bytes: v.bytes,
                anchor_non_ws: anchor.non_ws_chars,
                candidate_non_ws: v.non_ws_chars,
                per_tokenizer,
            });
        }
    }
    out
}

/// A candidate's totals against anchor `A` over exactly the programs that have it.
#[derive(Debug, Clone)]
pub struct VsAnchor {
    pub candidate: String,
    pub programs: Vec<String>,
    pub anchor_bytes: usize,
    pub candidate_bytes: usize,
    /// tokenizer id -> (anchor total, candidate total)
    pub per_tokenizer: BTreeMap<String, (usize, usize)>,
}

#[must_use]
pub fn vs_anchor(results: &Results) -> Vec<VsAnchor> {
    let mut map: BTreeMap<String, VsAnchor> = BTreeMap::new();
    for pair in pairs(results) {
        let e = map
            .entry(pair.candidate.clone())
            .or_insert_with(|| VsAnchor {
                candidate: pair.candidate.clone(),
                programs: Vec::new(),
                anchor_bytes: 0,
                candidate_bytes: 0,
                per_tokenizer: BTreeMap::new(),
            });
        e.programs.push(pair.program.clone());
        e.anchor_bytes += pair.anchor_bytes;
        e.candidate_bytes += pair.candidate_bytes;
        for (id, (a, c)) in pair.per_tokenizer {
            let t = e.per_tokenizer.entry(id).or_insert((0, 0));
            t.0 += a;
            t.1 += c;
        }
    }
    map.into_values().collect()
}

/// Totals for one language (or one Tessera candidate) over the programs that have it.
#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub programs: usize,
    pub bytes: usize,
    pub tokens: BTreeMap<String, usize>,
    pub mean_dispersion: f64,
    pub max_dispersion: f64,
    pub max_dispersion_program: String,
}

fn group_rank(name: &str) -> (usize, String) {
    let lang_rank = ["rust", "c", "zig", "odin"]
        .iter()
        .position(|l| *l == name)
        .unwrap_or(4);
    (lang_rank, name.to_owned())
}

/// Groups: each non-Tessera language, each Tessera candidate label, and
/// `tessera/A[parsed]` (the anchor restricted to variants the parser accepts).
#[must_use]
pub fn groups(results: &Results) -> Vec<Group> {
    let mut map: BTreeMap<String, Vec<&VariantRecord>> = BTreeMap::new();
    for program in &results.programs {
        for v in &program.variants {
            map.entry(v.label()).or_default().push(v);
            if v.is_parsed_tessera() {
                map.entry(format!("{}[parsed]", v.label()))
                    .or_default()
                    .push(v);
            }
        }
    }
    let mut out: Vec<Group> = map
        .into_iter()
        .map(|(name, variants)| {
            let mut tokens: BTreeMap<String, usize> = BTreeMap::new();
            for v in &variants {
                for (id, n) in &v.tokens {
                    *tokens.entry(id.clone()).or_insert(0) += n;
                }
            }
            let disp: Vec<(f64, &str)> = variants
                .iter()
                .filter_map(|v| v.stats.as_ref().map(|s| (s.dispersion, v.program.as_str())))
                .collect();
            let max = disp.iter().fold(
                (0.0_f64, ""),
                |acc, &(d, p)| if d > acc.0 { (d, p) } else { acc },
            );
            Group {
                programs: variants.len(),
                bytes: variants.iter().map(|v| v.bytes).sum(),
                tokens,
                mean_dispersion: if disp.is_empty() {
                    0.0
                } else {
                    disp.iter().map(|d| d.0).sum::<f64>() / disp.len() as f64
                },
                max_dispersion: max.0,
                max_dispersion_program: max.1.to_owned(),
                name,
            }
        })
        .collect();
    out.sort_by_key(|g| group_rank(&g.name));
    out
}

/// Summed alignment counts per Tessera group and tokenizer.
#[derive(Debug, Clone)]
pub struct AlignmentGroup {
    pub name: String,
    pub variants: usize,
    pub per_tokenizer: BTreeMap<String, Alignment>,
}

#[must_use]
pub fn alignment_groups(results: &Results) -> Vec<AlignmentGroup> {
    let mut map: BTreeMap<String, AlignmentGroup> = BTreeMap::new();
    for program in &results.programs {
        for v in program.variants.iter().filter(|v| v.lang == Lang::Tessera) {
            let mut names = vec![v.label()];
            if v.is_parsed_tessera() {
                names.push(format!("{}[parsed]", v.label()));
            }
            for name in names {
                let g = map.entry(name.clone()).or_insert_with(|| AlignmentGroup {
                    name,
                    variants: 0,
                    per_tokenizer: BTreeMap::new(),
                });
                g.variants += 1;
                for (id, a) in &v.alignment {
                    let t = g.per_tokenizer.entry(id.clone()).or_insert(Alignment {
                        units: 0,
                        intact: 0,
                        exact: 0,
                        pairs: 0,
                        merged: 0,
                    });
                    t.units += a.units;
                    t.intact += a.intact;
                    t.exact += a.exact;
                    t.pairs += a.pairs;
                    t.merged += a.merged;
                }
            }
        }
    }
    map.into_values().collect()
}

#[derive(Debug, Clone, Default)]
pub struct RewriteTokAgg {
    pub stable: usize,
    pub total: usize,
    pub min_delta: i64,
    pub max_delta: i64,
    /// (resegmented units, units compared)
    pub resegmented: (usize, usize),
    /// Units whose internal cuts alone changed.
    pub split_changed: usize,
}

#[derive(Debug, Clone)]
pub struct RewriteAgg {
    pub name: &'static str,
    pub kind: RewriteKind,
    pub variants: usize,
    pub collapsed_by_fmt: usize,
    pub per_tokenizer: BTreeMap<String, RewriteTokAgg>,
}

/// Rewrite stability per rewrite name, summed over all parsed variants.
#[must_use]
pub fn rewrite_summary(results: &Results) -> Vec<RewriteAgg> {
    let mut order: Vec<&'static str> = Vec::new();
    let mut map: BTreeMap<&'static str, RewriteAgg> = BTreeMap::new();
    for set in &results.rewrites {
        for item in &set.items {
            let agg = map.entry(item.name).or_insert_with(|| {
                order.push(item.name);
                RewriteAgg {
                    name: item.name,
                    kind: item.kind,
                    variants: 0,
                    collapsed_by_fmt: 0,
                    per_tokenizer: BTreeMap::new(),
                }
            });
            agg.variants += 1;
            agg.collapsed_by_fmt += usize::from(item.fmt_collapses_to_canonical);
            for (id, t) in &item.per_tokenizer {
                let a = agg.per_tokenizer.entry(id.clone()).or_default();
                if a.total == 0 {
                    a.min_delta = t.delta;
                    a.max_delta = t.delta;
                }
                a.total += 1;
                a.stable += usize::from(t.delta == 0);
                a.min_delta = a.min_delta.min(t.delta);
                a.max_delta = a.max_delta.max(t.delta);
                if let Some(r) = t.resegmented_units {
                    a.resegmented.0 += r;
                    a.resegmented.1 += set.grammar_units;
                }
                a.split_changed += t.split_changed_units.unwrap_or(0);
            }
        }
    }
    order.into_iter().filter_map(|n| map.remove(n)).collect()
}
