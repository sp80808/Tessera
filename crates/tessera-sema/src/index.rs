//! Workspace module index (contract B3 "module index", issue #20, consumed by
//! the incremental engine of issue #9).
//!
//! A [`ModuleIndex`] is the deterministic table of every function a workspace
//! defines, built from per-file **signature summaries** and nothing else:
//!
//! - a [`Signature`] is `name + parameter types + return type`, taken from the
//!   resolved signature facts ([`crate::ResolvedFn`]); a function *body* is never
//!   read, so a body-only edit cannot change a summary (the change firewall of
//!   `docs/architecture/incremental-query-engine.md`);
//! - summaries and the index contain **no spans** (PROV-3): reformatting a file
//!   leaves them equal, which is what lets an incremental engine stop
//!   invalidation at the signature. Where a duplicate is, is joined in only
//!   when diagnostics are rendered ([`ModuleIndex::duplicate_diagnostics`]),
//!   from a caller-supplied provenance lookup;
//! - the identity of an entry is its [`ItemId`] (file + name + disambiguator),
//!   never an offset.
//!
//! Everything here is a pure function of its arguments: same summaries in any
//! order give the same table, with entries ordered by `(name, path, id)` and no
//! hash iteration.
//!
//! # What cannot be tested yet
//!
//! The TC grammar has no `import`/`use`, no calls, no visibility and no nested
//! modules, so:
//!
//! - every function is "exported" and lives in one flat, workspace-wide
//!   namespace; there is no scope for a name to shadow another;
//! - nothing *resolves through* the index yet (no path expression names a
//!   function), so the index cannot cause a dependent file to invalidate; it is
//!   only compared and reported;
//! - import-edge invalidation ("an imported module changed") is therefore
//!   unobservable and unmeasured. The first grammar with calls or imports must
//!   add the resolve-through-index path and the tests for it.
//!
//! What is tested: ordering independence, rename, cross-file duplicates and the
//! signature-only equality rule.

use std::collections::BTreeMap;

use tessera_hir::ItemId;
use tessera_phases::{Diagnostic, DiagnosticSet, FileId, Phase, Provenance};

use crate::Ty;
use crate::resolve::ResolvedModule;

/// The externally visible shape of a function. Equality of two signatures is
/// exactly equality of name, parameter types and return type: no span, no body.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Signature {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
}

impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let params: Vec<&str> = self.params.iter().map(|t| t.as_str()).collect();
        write!(f, "{}({})>{}", self.name, params.join(","), self.ret)
    }
}

/// One function of one file: its identity, where it sits in the file's item
/// list, and its [`Signature`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnSummary {
    pub id: ItemId,
    /// Index of the item in the file's item list (parallel to the HIR
    /// provenance tables, so a caller can find the item's span). Structural,
    /// not an offset.
    pub ordinal: u32,
    pub sig: Signature,
}

/// What the workspace index needs to know about one file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileSummary {
    pub file: FileId,
    pub path: String,
    /// In source order.
    pub fns: Vec<FnSummary>,
}

impl FileSummary {
    /// Summarize the signatures of one resolved file.
    ///
    /// A function is listed iff it is well-formed, has a name, and is the
    /// *first* definition of that name in its file. Later same-named items of
    /// the same file are not listed: [`crate::resolve`] already reports them
    /// (`E-resolve-duplicate-item`, "already defined in this file"), and
    /// listing them again would report one mistake twice. Functions whose
    /// signature contains [`Ty::Error`] are listed with that type: the name
    /// exists even if its types are broken.
    #[must_use]
    pub fn from_resolved(path: impl Into<String>, res: &ResolvedModule) -> Self {
        let fns = res
            .funcs
            .iter()
            .enumerate()
            .filter(|(_, f)| f.well_formed && !f.item.name.is_empty() && f.item.disambiguator == 0)
            .map(|(i, f)| FnSummary {
                id: f.item.clone(),
                ordinal: u32::try_from(i).unwrap_or(u32::MAX),
                sig: Signature {
                    name: f.item.name.clone(),
                    params: f.params.clone(),
                    ret: f.ret,
                },
            })
            .collect();
        Self {
            file: res.file,
            path: path.into(),
            fns,
        }
    }
}

/// One entry of the [`ModuleIndex`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol {
    /// Sorted first so the derived order is "by path".
    pub path: String,
    pub id: ItemId,
    pub ordinal: u32,
    pub sig: Signature,
}

/// Deterministic workspace symbol table: function name to its definitions,
/// definitions ordered by path (then id). A name with more than one definition
/// is a conflict; every definition stays in the table so each can be reported.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ModuleIndex {
    symbols: BTreeMap<String, Vec<Symbol>>,
}

impl ModuleIndex {
    /// Build the index from file summaries. The order of `files` does not
    /// matter: the result depends only on the set of summaries.
    #[must_use]
    pub fn build(files: &[FileSummary]) -> Self {
        let mut symbols: BTreeMap<String, Vec<Symbol>> = BTreeMap::new();
        for file in files {
            for func in &file.fns {
                symbols
                    .entry(func.sig.name.clone())
                    .or_default()
                    .push(Symbol {
                        path: file.path.clone(),
                        id: func.id.clone(),
                        ordinal: func.ordinal,
                        sig: func.sig.clone(),
                    });
            }
        }
        for defs in symbols.values_mut() {
            defs.sort();
        }
        Self { symbols }
    }

    /// Definitions of `name`, ordered by path; empty if there is none.
    #[must_use]
    pub fn lookup(&self, name: &str) -> &[Symbol] {
        self.symbols.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every name with its definitions, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[Symbol])> {
        self.symbols.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Number of distinct names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Names with more than one definition, in name order.
    pub fn conflicts(&self) -> impl Iterator<Item = (&str, &[Symbol])> {
        self.iter().filter(|(_, defs)| defs.len() > 1)
    }

    /// The table without identities: `(name, path, signature)` in table order.
    /// Independent of `FileId` assignment, so two workspaces that differ only
    /// in the order files were registered compare equal here.
    #[must_use]
    pub fn contents(&self) -> Vec<(String, String, Signature)> {
        self.symbols
            .iter()
            .flat_map(|(name, defs)| {
                defs.iter()
                    .map(move |s| (name.clone(), s.path.clone(), s.sig.clone()))
            })
            .collect()
    }

    /// `E-resolve-duplicate-item` for every definition of every conflicting
    /// name: one diagnostic per definition (so both sides of a clash are
    /// reported), located by `at`, which maps an entry to the provenance of its
    /// item (normally the item span from the file's HIR provenance).
    ///
    /// This is the report-time join of semantic facts and provenance
    /// (contract §2.4): the index itself holds no spans. The result is in
    /// canonical [`DiagnosticSet`] order, independent of file order.
    #[must_use]
    pub fn duplicate_diagnostics(&self, at: impl Fn(&Symbol) -> Provenance) -> DiagnosticSet {
        let mut out = DiagnosticSet::new();
        for (name, defs) in self.conflicts() {
            for (i, def) in defs.iter().enumerate() {
                let others: Vec<String> = defs
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, d)| format!("`{}`", d.path))
                    .collect();
                out.push(Diagnostic::error(
                    Phase::Resolve,
                    "E-resolve-duplicate-item",
                    format!("`{name}` is also defined in {}", others.join(", ")),
                    at(def),
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use tessera_hir::ItemKind;
    use tessera_phases::Span;

    use super::*;
    use crate::{Res, ResolvedFn};

    fn id(file: u32, name: &str, disambiguator: u32) -> ItemId {
        ItemId {
            file: FileId(file),
            kind: ItemKind::Fn,
            name: name.to_owned(),
            disambiguator,
        }
    }

    fn rfn(file: u32, name: &str, params: &[Ty], ret: Ty) -> ResolvedFn {
        ResolvedFn {
            item: id(file, name, 0),
            well_formed: true,
            declarations_ok: true,
            params: params.to_vec(),
            ret,
            names: BTreeMap::new(),
        }
    }

    fn module(file: u32, funcs: Vec<ResolvedFn>) -> ResolvedModule {
        ResolvedModule {
            file: FileId(file),
            funcs,
        }
    }

    fn summary(file: u32, path: &str, name: &str, params: &[Ty], ret: Ty) -> FileSummary {
        FileSummary::from_resolved(path, &module(file, vec![rfn(file, name, params, ret)]))
    }

    fn at(def: &Symbol) -> Provenance {
        // A stand-in span that differs per definition.
        Provenance::Source(Span::new(
            def.id.file,
            10 * def.id.file.0,
            10 * def.id.file.0 + 5,
        ))
    }

    #[test]
    fn table_is_keyed_by_name_and_sorted_by_path() {
        let a = summary(0, "b.tes", "add", &[Ty::I64, Ty::I64], Ty::I64);
        let b = summary(1, "a.tes", "inc", &[Ty::I64], Ty::I64);
        let index = ModuleIndex::build(&[a, b]);
        let names: Vec<&str> = index.iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["add", "inc"]);
        assert_eq!(index.len(), 2);
        assert_eq!(index.lookup("add")[0].path, "b.tes");
        assert_eq!(index.lookup("inc")[0].sig.to_string(), "inc(i64)>i64");
        assert!(index.lookup("missing").is_empty());
        assert_eq!(index.conflicts().count(), 0);
    }

    #[test]
    fn file_order_changes_ids_but_not_contents() {
        // The same two files registered in opposite orders get different
        // `FileId`s; names, paths and signatures are identical.
        let forward = ModuleIndex::build(&[
            summary(0, "a.tes", "add", &[Ty::I64, Ty::I64], Ty::I64),
            summary(1, "b.tes", "inc", &[Ty::I64], Ty::I64),
        ]);
        let backward = ModuleIndex::build(&[
            summary(0, "b.tes", "inc", &[Ty::I64], Ty::I64),
            summary(1, "a.tes", "add", &[Ty::I64, Ty::I64], Ty::I64),
        ]);
        assert_eq!(forward.contents(), backward.contents());
        assert_ne!(
            forward, backward,
            "FileIds differ, so the identities differ"
        );
        assert_eq!(forward.lookup("add")[0].id.file, FileId(0));
        assert_eq!(backward.lookup("add")[0].id.file, FileId(1));

        // Presenting the same summaries in another order changes nothing at all.
        let (a, b) = (
            summary(0, "a.tes", "add", &[Ty::I64], Ty::I64),
            summary(1, "b.tes", "inc", &[Ty::I64], Ty::I64),
        );
        assert_eq!(
            ModuleIndex::build(&[a.clone(), b.clone()]),
            ModuleIndex::build(&[b, a])
        );
    }

    #[test]
    fn rename_moves_the_entry() {
        let before = ModuleIndex::build(&[summary(0, "a.tes", "add", &[Ty::I64], Ty::I64)]);
        let after = ModuleIndex::build(&[summary(0, "a.tes", "plus", &[Ty::I64], Ty::I64)]);
        assert_eq!(before.lookup("add").len(), 1);
        assert!(before.lookup("plus").is_empty());
        assert!(after.lookup("add").is_empty());
        assert_eq!(after.lookup("plus")[0].path, "a.tes");
        assert_eq!(
            after.lookup("plus")[0].id.path(),
            "fn/plus",
            "identity follows the name"
        );
        assert_ne!(before, after);
    }

    #[test]
    fn a_duplicate_across_files_is_reported_once_per_definition() {
        let a = summary(0, "a.tes", "add", &[Ty::I64], Ty::I64);
        let b = summary(1, "b.tes", "add", &[Ty::I64, Ty::I64], Ty::I64);
        let c = summary(2, "c.tes", "other", &[], Ty::I64);
        let index = ModuleIndex::build(&[b.clone(), c.clone(), a.clone()]);
        assert_eq!(index.conflicts().count(), 1);
        let diagnostics = index.duplicate_diagnostics(at);
        let got: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.code, d.at.primary_span().file.0, d.message.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "E-resolve-duplicate-item",
                    0,
                    "`add` is also defined in `b.tes`"
                ),
                (
                    "E-resolve-duplicate-item",
                    1,
                    "`add` is also defined in `a.tes`"
                ),
            ]
        );
        // Deterministic: the same diagnostics whatever order the files came in.
        assert_eq!(
            ModuleIndex::build(&[a, b, c]).duplicate_diagnostics(at),
            diagnostics
        );
        // Differing signatures do not make it a different name.
        assert_eq!(index.lookup("add")[0].sig.params.len(), 1);
        assert_eq!(index.lookup("add")[1].sig.params.len(), 2);
    }

    #[test]
    fn three_way_duplicate_names_the_other_two() {
        let files: Vec<_> = ["a.tes", "b.tes", "c.tes"]
            .iter()
            .enumerate()
            .map(|(i, p)| summary(i as u32, p, "f", &[], Ty::I64))
            .collect();
        let diagnostics = ModuleIndex::build(&files).duplicate_diagnostics(at);
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "`f` is also defined in `b.tes`, `c.tes`",
                "`f` is also defined in `a.tes`, `c.tes`",
                "`f` is also defined in `a.tes`, `b.tes`",
            ]
        );
    }

    #[test]
    fn no_duplicates_no_diagnostics() {
        let index = ModuleIndex::build(&[
            summary(0, "a.tes", "add", &[], Ty::I64),
            summary(1, "b.tes", "inc", &[], Ty::I64),
        ]);
        assert!(index.duplicate_diagnostics(at).is_empty());
        assert!(ModuleIndex::default().is_empty());
    }

    #[test]
    fn summaries_compare_by_signature_only() {
        let base = summary(0, "a.tes", "add", &[Ty::I64, Ty::I64], Ty::I64);

        // Different body facts (which names resolved to what) do not matter:
        // the summary is built from signature fields only.
        let mut with_body = rfn(0, "add", &[Ty::I64, Ty::I64], Ty::I64);
        with_body
            .names
            .insert(tessera_hir::ExprId(0), Res::Local(tessera_hir::LocalId(1)));
        with_body.declarations_ok = false;
        assert_eq!(
            base,
            FileSummary::from_resolved("a.tes", &module(0, vec![with_body]))
        );

        // Signature fields do.
        for other in [
            summary(0, "a.tes", "plus", &[Ty::I64, Ty::I64], Ty::I64),
            summary(0, "a.tes", "add", &[Ty::I64], Ty::I64),
            summary(0, "a.tes", "add", &[Ty::I64, Ty::Bool], Ty::I64),
            summary(0, "a.tes", "add", &[Ty::I64, Ty::I64], Ty::Bool),
        ] {
            assert_ne!(base, other);
            assert_ne!(base.fns[0].sig, other.fns[0].sig);
        }
        // `Signature` has no span field, so equal signatures from two
        // different places compare equal by construction.
        assert_eq!(
            base.fns[0].sig,
            summary(7, "z.tes", "add", &[Ty::I64, Ty::I64], Ty::I64).fns[0].sig
        );
    }

    #[test]
    fn only_first_well_formed_named_definitions_are_listed() {
        let mut dup = rfn(0, "add", &[], Ty::I64);
        dup.item = id(0, "add", 1);
        let mut malformed = rfn(0, "bad", &[], Ty::I64);
        malformed.well_formed = false;
        let mut unnamed = rfn(0, "", &[], Ty::I64);
        unnamed.item = id(0, "", 0);
        let broken = rfn(0, "broken", &[Ty::Error], Ty::Error);
        let res = module(
            0,
            vec![rfn(0, "add", &[], Ty::I64), dup, malformed, unnamed, broken],
        );
        let summary = FileSummary::from_resolved("a.tes", &res);
        let listed: Vec<(&str, u32)> = summary
            .fns
            .iter()
            .map(|f| (f.sig.name.as_str(), f.ordinal))
            .collect();
        assert_eq!(listed, [("add", 0), ("broken", 4)]);
        assert_eq!(summary.fns[1].sig.ret, Ty::Error);
    }
}
