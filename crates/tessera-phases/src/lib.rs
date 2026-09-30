//! Phase-handoff vocabulary for the Tessera compiler (issue #17).
//!
//! This crate owns only what *every* boundary in
//! `docs/architecture/compiler-phases.md` must agree on: byte spans, source
//! provenance, structured diagnostics and the "value plus diagnostics" result
//! shape. It deliberately contains no IR, no parser and no global state, so
//! every phase output built from it can be a pure, comparable Salsa value.
//!
//! Nothing here fixes TC surface syntax; spans are byte offsets into source
//! text and are meaningful only within one revision of one file.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

/// Identity of one source file input (interned by the driver/database).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// Half-open byte range `[start, end)` in one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Build a span. `start > end` is normalized by swapping so a span is
    /// always well-formed; callers cannot construct an inverted range.
    #[must_use]
    pub const fn new(file: FileId, start: u32, end: u32) -> Self {
        if start <= end {
            Self { file, start, end }
        } else {
            Self {
                file,
                start: end,
                end: start,
            }
        }
    }

    #[must_use]
    pub const fn len(&self) -> u32 {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// `true` if `other` lies entirely inside `self` (same file).
    #[must_use]
    pub const fn contains(&self, other: Span) -> bool {
        self.file.0 == other.file.0 && self.start <= other.start && other.end <= self.end
    }

    /// Smallest span covering both, or `None` across files.
    #[must_use]
    pub fn cover(self, other: Span) -> Option<Span> {
        (self.file == other.file).then(|| Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        })
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}..{}", self.file.0, self.start, self.end)
    }
}

/// Where a semantic entity came from in TC source.
///
/// There is intentionally **no** "unknown" variant: every entity that can
/// carry a diagnostic must trace to TC source (invariant PROV-1). Compiler
/// synthesized entities point at the source that caused them and say why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provenance {
    /// Written directly in source.
    Source(Span),
    /// Introduced by the compiler (desugaring, explicit drop, inferred type
    /// made explicit, ...). `origin` is the causing source span.
    Synthesized { origin: Span, why: &'static str },
}

impl Provenance {
    /// The span a diagnostic should point at.
    #[must_use]
    pub const fn primary_span(&self) -> Span {
        match self {
            Self::Source(span) | Self::Synthesized { origin: span, .. } => *span,
        }
    }

    #[must_use]
    pub const fn is_synthesized(&self) -> bool {
        matches!(self, Self::Synthesized { .. })
    }
}

/// Compiler phase that produced a fact or diagnostic. Order is pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    Syntax,
    Hir,
    Resolve,
    Typeck,
    Tir,
    Mir,
    Backend,
}

impl Phase {
    /// Every phase in pipeline order.
    pub const ALL: [Phase; 7] = [
        Phase::Syntax,
        Phase::Hir,
        Phase::Resolve,
        Phase::Typeck,
        Phase::Tir,
        Phase::Mir,
        Phase::Backend,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Hir => "hir",
            Self::Resolve => "resolve",
            Self::Typeck => "typeck",
            Self::Tir => "tir",
            Self::Mir => "mir",
            Self::Backend => "backend",
        }
    }

    /// The phase this one lowers into, or `None` for the last.
    #[must_use]
    pub const fn next(self) -> Option<Phase> {
        match self {
            Self::Syntax => Some(Self::Hir),
            Self::Hir => Some(Self::Resolve),
            Self::Resolve => Some(Self::Typeck),
            Self::Typeck => Some(Self::Tir),
            Self::Tir => Some(Self::Mir),
            Self::Mir => Some(Self::Backend),
            Self::Backend => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// One structured diagnostic. Rendering to text is a driver concern (#23);
/// compiler phases only produce this value and never print or exit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub phase: Phase,
    pub severity: Severity,
    /// Stable machine code, e.g. `"E-syntax-unexpected-token"`.
    pub code: &'static str,
    pub message: String,
    pub at: Provenance,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        phase: Phase,
        code: &'static str,
        message: impl Into<String>,
        at: Provenance,
    ) -> Self {
        Self {
            phase,
            severity: Severity::Error,
            code,
            message: message.into(),
            at,
        }
    }

    fn sort_key(&self) -> (Span, Phase, Severity, &'static str, &str) {
        (
            self.at.primary_span(),
            self.phase,
            self.severity,
            self.code,
            self.message.as_str(),
        )
    }
}

/// Deterministically ordered diagnostics: iteration order is a pure function
/// of the *contents*, never of pass execution order or hash iteration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct DiagnosticSet {
    items: Vec<Diagnostic>,
}

impl DiagnosticSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert keeping canonical order (stable for equal keys).
    pub fn push(&mut self, diagnostic: Diagnostic) {
        let key = diagnostic.sort_key();
        let at = self
            .items
            .partition_point(|d| d.sort_key().cmp(&key) != Ordering::Greater);
        self.items.insert(at, diagnostic);
    }

    pub fn extend(&mut self, other: DiagnosticSet) {
        for diagnostic in other.items {
            self.push(diagnostic);
        }
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }
}

/// Result of one phase: a value **and** diagnostics, never an early exit.
///
/// `value` is always present even when `diagnostics.has_errors()`; erroneous
/// input yields a structurally valid value with explicit error nodes
/// (invariant DIAG-1), so later phases and editors can keep working.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhaseOutput<T> {
    pub value: T,
    pub diagnostics: DiagnosticSet,
}

impl<T> PhaseOutput<T> {
    #[must_use]
    pub fn clean(value: T) -> Self {
        Self {
            value,
            diagnostics: DiagnosticSet::new(),
        }
    }

    #[must_use]
    pub fn with(value: T, diagnostics: DiagnosticSet) -> Self {
        Self { value, diagnostics }
    }

    /// May the next phase lower this? Errors do not stop lowering (tolerant
    /// phases), but a pipeline stage that requires clean input checks this.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        !self.diagnostics.has_errors()
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> PhaseOutput<U> {
        PhaseOutput {
            value: f(self.value),
            diagnostics: self.diagnostics,
        }
    }
}

/// Side table from a phase's stable IDs to source provenance.
///
/// Semantic trees carry IDs, not spans; spans live here. Editing whitespace
/// therefore changes this table but not the semantic value, which lets an
/// incremental engine cut off recomputation of dependents (see
/// "Provenance side tables" in the phase contract). `BTreeMap` keeps
/// iteration deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceMap<Id: Ord> {
    entries: BTreeMap<Id, Provenance>,
}

impl<Id: Ord> Default for ProvenanceMap<Id> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<Id: Ord + Copy> ProvenanceMap<Id> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: Id, provenance: Provenance) {
        self.entries.insert(id, provenance);
    }

    #[must_use]
    pub fn get(&self, id: Id) -> Option<Provenance> {
        self.entries.get(&id).copied()
    }

    /// IDs among `ids` with no provenance: the PROV-1 violation set. Empty
    /// means the map is total over `ids`.
    pub fn missing(&self, ids: impl IntoIterator<Item = Id>) -> Vec<Id> {
        ids.into_iter()
            .filter(|id| !self.entries.contains_key(id))
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id, Provenance)> + '_ {
        self.entries.iter().map(|(id, p)| (*id, *p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: FileId = FileId(0);

    fn span(start: u32, end: u32) -> Span {
        Span::new(F, start, end)
    }

    #[test]
    fn span_is_always_well_formed() {
        assert_eq!(span(9, 3), span(3, 9));
        assert_eq!(span(3, 9).len(), 6);
        assert!(span(4, 4).is_empty());
    }

    #[test]
    fn span_cover_and_contains() {
        let whole = span(0, 26);
        let body = span(23, 26);
        assert!(whole.contains(body));
        assert!(!body.contains(whole));
        assert_eq!(span(0, 5).cover(span(20, 26)), Some(whole));
        assert_eq!(span(0, 5).cover(Span::new(FileId(1), 0, 5)), None);
    }

    #[test]
    fn provenance_always_names_a_source_span() {
        let origin = span(23, 26);
        let synth = Provenance::Synthesized {
            origin,
            why: "explicit-drop",
        };
        assert_eq!(synth.primary_span(), origin);
        assert!(synth.is_synthesized());
        assert!(!Provenance::Source(origin).is_synthesized());
    }

    #[test]
    fn phases_form_one_linear_chain() {
        let mut seen = vec![Phase::ALL[0]];
        while let Some(next) = seen.last().and_then(|p| p.next()) {
            seen.push(next);
        }
        assert_eq!(seen, Phase::ALL.to_vec());
        assert!(Phase::ALL.windows(2).all(|w| w[0] < w[1]));
    }

    fn diag(code: &'static str, start: u32) -> Diagnostic {
        Diagnostic::error(
            Phase::Syntax,
            code,
            "m",
            Provenance::Source(span(start, start + 1)),
        )
    }

    #[test]
    fn diagnostic_order_ignores_insertion_order() {
        let items = [diag("b", 9), diag("a", 3), diag("c", 3), diag("a", 0)];
        let mut forward = DiagnosticSet::new();
        for d in items.iter().cloned() {
            forward.push(d);
        }
        let mut backward = DiagnosticSet::new();
        for d in items.iter().rev().cloned() {
            backward.push(d);
        }
        assert_eq!(forward, backward);
        let codes: Vec<_> = forward
            .iter()
            .map(|d| (d.at.primary_span().start, d.code))
            .collect();
        assert_eq!(codes, vec![(0, "a"), (3, "a"), (3, "c"), (9, "b")]);
    }

    #[test]
    fn output_keeps_value_alongside_errors() {
        let mut ds = DiagnosticSet::new();
        ds.push(diag("x", 1));
        let out = PhaseOutput::with(42_u32, ds);
        assert!(!out.is_clean());
        assert_eq!(out.map(|v| v + 1).value, 43);
        assert!(PhaseOutput::clean(()).is_clean());
    }

    #[test]
    fn provenance_map_reports_missing_ids_deterministically() {
        let mut map = ProvenanceMap::new();
        map.insert(1_u32, Provenance::Source(span(0, 1)));
        map.insert(3_u32, Provenance::Source(span(2, 3)));
        assert_eq!(map.missing([1, 2, 3, 4]), vec![2, 4]);
        assert_eq!(map.get(3), Some(Provenance::Source(span(2, 3))));
        let ids: Vec<_> = map.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![1, 3]);
    }
}
