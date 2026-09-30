//! Metric definitions: cross-tokenizer statistics and grammar-boundary
//! alignment. Definitions are also embedded in the results JSON (`definitions`).

use std::collections::BTreeSet;

use tessera_syntax::lexer::{self, Token, TokenKind};

/// Cross-tokenizer statistics over the model-token counts of one text.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    /// Median over tokenizers (mean of the two middle values for even n).
    pub median: f64,
    /// Nearest-rank 90th percentile: `sorted[ceil(0.9 * n) - 1]`.
    pub p90: usize,
    pub worst: usize,
    pub min: usize,
    /// `(worst - min) / median`; 0 when the median is 0.
    pub dispersion: f64,
}

/// `None` when no tokenizer was measured.
#[must_use]
pub fn stats(counts: &[usize]) -> Option<Stats> {
    if counts.is_empty() {
        return None;
    }
    let mut sorted = counts.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2] as f64
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) as f64 / 2.0
    };
    let p90_rank = (9 * n).div_ceil(10);
    let (min, worst) = (sorted[0], sorted[n - 1]);
    Some(Stats {
        median,
        p90: sorted[p90_rank - 1],
        worst,
        min,
        dispersion: if median > 0.0 {
            (worst - min) as f64 / median
        } else {
            0.0
        },
    })
}

/// Grammar-boundary alignment of one text against one tokenizer.
///
/// Grammar units are the Tessera lexer's non-trivia tokens. A model token
/// boundary is any start or end of a model-token byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    pub units: usize,
    /// Units with no model-token boundary strictly inside their byte range.
    pub intact: usize,
    /// Units whose byte range is exactly one model token.
    pub exact: usize,
    /// Adjacent (consecutive non-trivia) unit pairs.
    pub pairs: usize,
    /// Pairs covered by one model token: it contains the last byte of the
    /// first unit and the first byte of the second.
    pub merged: usize,
}

impl Alignment {
    #[must_use]
    pub fn intact_fraction(&self) -> f64 {
        ratio(self.intact, self.units)
    }

    #[must_use]
    pub fn exact_fraction(&self) -> f64 {
        ratio(self.exact, self.units)
    }

    #[must_use]
    pub fn merged_fraction(&self) -> f64 {
        ratio(self.merged, self.pairs)
    }
}

fn ratio(num: usize, den: usize) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

fn grammar_units(src: &str) -> Vec<Token> {
    lexer::lex(src)
        .into_iter()
        .filter(|t| !t.kind.is_trivia())
        .collect()
}

/// Number of grammar units (non-trivia lexer tokens) and how many of them are
/// `Error` tokens (characters the provisional lexicon does not name, such as
/// `^` and `?`).
#[must_use]
pub fn lexer_counts(src: &str) -> (usize, usize) {
    let units = grammar_units(src);
    let errors = units.iter().filter(|t| t.kind == TokenKind::Error).count();
    (units.len(), errors)
}

fn boundaries(ranges: &[(usize, usize)]) -> BTreeSet<usize> {
    ranges.iter().flat_map(|&(s, e)| [s, e]).collect()
}

#[must_use]
pub fn alignment(src: &str, ranges: &[(usize, usize)]) -> Alignment {
    let units = grammar_units(src);
    let cuts = boundaries(ranges);
    let intact = units
        .iter()
        .filter(|u| cuts.range(u.start + 1..u.end).next().is_none())
        .count();
    let exact = units
        .iter()
        .filter(|u| ranges.contains(&(u.start, u.end)))
        .count();
    let pairs = units.len().saturating_sub(1);
    let merged = units
        .windows(2)
        .filter(|w| ranges.iter().any(|&(s, e)| s < w[0].end && e > w[1].start))
        .count();
    Alignment {
        units: units.len(),
        intact,
        exact,
        pairs,
        merged,
    }
}

/// How the model tokenizer segments one grammar unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segmentation {
    /// Offsets (relative to the unit start) of boundaries strictly inside it.
    pub internal_cuts: Vec<usize>,
    /// A model-token boundary sits exactly at the unit's first byte.
    pub starts_at_boundary: bool,
    /// A model-token boundary sits exactly after the unit's last byte.
    pub ends_at_boundary: bool,
}

/// Segmentation of every grammar unit, in order.
#[must_use]
pub fn segmentations(src: &str, ranges: &[(usize, usize)]) -> Vec<Segmentation> {
    let cuts = boundaries(ranges);
    grammar_units(src)
        .iter()
        .map(|u| Segmentation {
            internal_cuts: cuts
                .range(u.start + 1..u.end)
                .map(|p| p - u.start)
                .collect(),
            starts_at_boundary: cuts.contains(&u.start),
            ends_at_boundary: cuts.contains(&u.end),
        })
        .collect()
}

/// The non-trivia token sequence `(kind, text)`; equal sequences mean two
/// spellings differ only in trivia.
#[must_use]
pub fn unit_sequence(src: &str) -> Vec<(TokenKind, String)> {
    grammar_units(src)
        .iter()
        .map(|t| (t.kind, t.text(src).to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_median_p90_dispersion() {
        let s = stats(&[10, 12, 14, 20]).expect("stats");
        assert!((s.median - 13.0).abs() < 1e-9);
        assert_eq!((s.min, s.worst, s.p90), (10, 20, 20));
        assert!((s.dispersion - 10.0 / 13.0).abs() < 1e-9);
        let odd = stats(&[3, 1, 2]).expect("stats");
        assert!((odd.median - 2.0).abs() < 1e-9);
        assert!(stats(&[]).is_none());
        // n = 10: p90 is the 9th smallest
        let ten: Vec<usize> = (1..=10).collect();
        assert_eq!(stats(&ten).expect("stats").p90, 9);
    }

    #[test]
    fn alignment_counts_intact_exact_and_merged() {
        // `a+b`: lexer units a, +, b. Model tokens `a+` and `b`.
        let src = "a+b";
        let al = alignment(src, &[(0, 2), (2, 3)]);
        assert_eq!((al.units, al.intact, al.exact), (3, 3, 1));
        assert_eq!((al.pairs, al.merged), (2, 1));
        // One token per byte: everything exact, nothing merged.
        let al = alignment(src, &[(0, 1), (1, 2), (2, 3)]);
        assert_eq!((al.exact, al.merged), (3, 0));
        // `abc` split as `a|bc`: the identifier is not intact.
        let al = alignment("abc", &[(0, 1), (1, 3)]);
        assert_eq!((al.units, al.intact), (1, 0));
        let seg = segmentations("abc", &[(0, 1), (1, 3)]);
        assert_eq!(seg[0].internal_cuts, vec![1]);
        assert!(seg[0].starts_at_boundary && seg[0].ends_at_boundary);
    }

    #[test]
    fn whitespace_between_units_does_not_merge_them_without_a_spanning_token() {
        // `a + b` with model tokens `a`, ` +`, ` b`: no token spans two units' bytes.
        let al = alignment("a + b", &[(0, 1), (1, 3), (3, 5)]);
        assert_eq!(al.merged, 0);
        // A token spanning `a + b` entirely would merge both pairs.
        let al = alignment("a + b", &[(0, 5)]);
        assert_eq!(al.merged, 2);
    }

    #[test]
    fn segmentation_sees_edge_merges() {
        // `a+b` as `a`,`+b` then as `a`,`+`,`b`: unit `+` changes only at its end edge.
        let merged = segmentations("a+b", &[(0, 1), (1, 3)]);
        let split = segmentations("a+b", &[(0, 1), (1, 2), (2, 3)]);
        assert_eq!(merged[0], split[0]);
        assert_ne!(merged[1], split[1]);
        assert!(merged[1].starts_at_boundary && !merged[1].ends_at_boundary);
        assert!(split[1].starts_at_boundary && split[1].ends_at_boundary);
    }

    #[test]
    fn lexer_counts_reports_error_tokens() {
        assert_eq!(lexer_counts("f x(a:i64)>i64=a"), (11, 0));
        assert_eq!(lexer_counts("x=^y?"), (5, 2));
    }
}
