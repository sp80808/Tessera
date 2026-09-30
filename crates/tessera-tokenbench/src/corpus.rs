//! Corpus schema, loading, identity hash and validation.
//!
//! The corpus is `corpus.json` plus the per-variant files it references.
//! Validation is shared by `run` (so results can never present unverified text
//! as validated Tessera) and by the test-suite.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tessera_phases::FileId;
use tessera_syntax::cst;
use tessera_syntax::lexer::{self, TokenKind};

pub const CORPUS_SCHEMA_VERSION: u32 = 1;
pub const CORPUS_FILE: &str = "corpus.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Tessera,
    Rust,
    C,
    Zig,
    Odin,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::Rust, Lang::C, Lang::Zig, Lang::Odin, Lang::Tessera];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tessera => "tessera",
            Self::Rust => "rust",
            Self::C => "c",
            Self::Zig => "zig",
            Self::Odin => "odin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The current parser accepts the text with no diagnostics.
    Parsed,
    /// Candidate spelling for a construct the parser does not implement yet.
    Unparsed,
}

impl Status {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parsed => "parsed",
            Self::Unparsed => "unparsed",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDef {
    pub description: String,
    /// Spelling chosen per syntax axis (`fn`, `bind`, `excl_borrow`, `move`, `match`).
    pub axes: BTreeMap<String, String>,
    /// The one axis on which this label differs from anchor `A` (empty for `A`).
    pub differs_from_anchor: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub lang: Lang,
    pub candidate: Option<String>,
    pub file: String,
    pub status: Option<Status>,
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub id: String,
    pub category: String,
    pub description: String,
    pub semantic_units: u32,
    pub semantic_units_note: String,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub schema_version: u32,
    pub semantic_units_definition: String,
    pub candidates: BTreeMap<String, CandidateDef>,
    pub programs: Vec<Program>,
}

/// A parsed `corpus.json` with every referenced file read into memory.
#[derive(Debug, Clone)]
pub struct LoadedCorpus {
    pub corpus: Corpus,
    /// Referenced files keyed by their relative path (posix separators).
    pub files: BTreeMap<String, String>,
    /// Hex SHA-256 identity of `corpus.json` plus all referenced files.
    pub hash: String,
}

impl LoadedCorpus {
    /// Source text of a variant.
    ///
    /// # Panics
    /// Never for a corpus produced by [`load`], which reads every referenced file.
    #[must_use]
    pub fn text(&self, variant: &Variant) -> &str {
        self.files.get(&variant.file).map_or("", String::as_str)
    }
}

/// A relative path with normal components only (no `..`, roots or backslashes).
#[must_use]
pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Deterministic identity: SHA-256 over `(path, length, bytes)` records in
/// sorted path order. Paths are the relative strings used in `corpus.json`.
#[must_use]
pub fn hash_entries(entries: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in entries {
        hasher.update(b"file\0");
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes.len().to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes);
        hasher.update(b"\n");
    }
    hex(&hasher.finalize())
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Read `<dir>/corpus.json` and every file it references.
///
/// # Errors
/// Missing/unreadable files, invalid JSON, unsupported schema version,
/// unsafe or duplicate file references. Messages contain only relative paths.
pub fn load(dir: &Path) -> Result<LoadedCorpus, String> {
    let json_bytes = fs::read(dir.join(CORPUS_FILE))
        .map_err(|e| format!("cannot read {CORPUS_FILE} in corpus dir: {e}"))?;
    let corpus: Corpus =
        serde_json::from_slice(&json_bytes).map_err(|e| format!("invalid {CORPUS_FILE}: {e}"))?;
    if corpus.schema_version != CORPUS_SCHEMA_VERSION {
        return Err(format!(
            "unsupported corpus schema_version {} (expected {CORPUS_SCHEMA_VERSION})",
            corpus.schema_version
        ));
    }
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(CORPUS_FILE.to_owned(), json_bytes);
    let mut files = BTreeMap::new();
    for program in &corpus.programs {
        for variant in &program.variants {
            if !safe_relative(&variant.file) || variant.file == CORPUS_FILE {
                return Err(format!(
                    "program `{}`: unsafe file reference `{}`",
                    program.id, variant.file
                ));
            }
            if files.contains_key(&variant.file) {
                return Err(format!("file `{}` is referenced twice", variant.file));
            }
            let bytes = fs::read(dir.join(&variant.file)).map_err(|e| {
                format!(
                    "program `{}`: cannot read `{}`: {e}",
                    program.id, variant.file
                )
            })?;
            let text = String::from_utf8(bytes.clone())
                .map_err(|_| format!("`{}` is not valid UTF-8", variant.file))?;
            entries.insert(variant.file.clone(), bytes);
            files.insert(variant.file.clone(), text);
        }
    }
    Ok(LoadedCorpus {
        corpus,
        files,
        hash: hash_entries(&entries),
    })
}

/// Files under `dir` that `corpus.json` does not reference (sidecar files such
/// as macOS `._*` AppleDouble and `.DS_Store` are ignored). Sorted.
#[must_use]
pub fn unreferenced_files(dir: &Path, loaded: &LoadedCorpus) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("._") || name == ".DS_Store" {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                let rel: Vec<_> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(rel.join("/"));
            }
        }
    }
    let mut all = Vec::new();
    walk(dir, dir, &mut all);
    all.sort();
    all.retain(|p| p != CORPUS_FILE && p != ".gitattributes" && !loaded.files.contains_key(p));
    all
}

/// Number of parser diagnostics for `text` (0 means the parser accepts it).
#[must_use]
pub fn parser_diagnostics(text: &str) -> usize {
    cst::parse_file(FileId(0), text).diagnostics.len()
}

/// Syntax axes a Tessera variant text actually exercises, detected lexically.
/// Only *explicit* spellings are reported: an absent construct (or an implicit
/// move) contributes nothing. Corpus convention: one item per line.
#[must_use]
pub fn features(src: &str) -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let mut out: BTreeMap<&'static str, BTreeSet<&'static str>> = BTreeMap::new();
    let mut add = |axis: &'static str, value: &'static str| {
        out.entry(axis).or_default().insert(value);
    };
    for line in src.lines() {
        let toks: Vec<_> = lexer::lex(line)
            .into_iter()
            .filter(|t| !t.kind.is_trivia())
            .collect();
        if toks.len() < 3 {
            continue;
        }
        let kind = |i: usize| toks[i].kind;
        let text = |i: usize| toks[i].text(line);
        if kind(0) == TokenKind::Ident
            && text(0) == "f"
            && kind(1) == TokenKind::Ident
            && kind(2) == TokenKind::LParen
        {
            add("fn", "f-keyword");
        } else if kind(0) == TokenKind::Ident
            && !matches!(text(0), "struct" | "enum")
            && kind(1) == TokenKind::Colon
            && kind(2) == TokenKind::LParen
        {
            add("fn", "name-colon-sig");
        }
        if line.contains(":=") {
            add("bind", "walrus");
        }
        if toks
            .iter()
            .enumerate()
            .any(|(i, t)| t.kind == TokenKind::Ident && t.text(line) == "let" && i + 1 < toks.len())
        {
            add("bind", "let");
        }
        for w in toks.windows(3) {
            let adjacent_eq = w[2].kind == TokenKind::Eq
                && line.as_bytes().get(w[2].end) != Some(&b'=')
                && w[1].end == w[2].start;
            if matches!(w[0].kind, TokenKind::LBrace | TokenKind::Semi)
                && w[1].kind == TokenKind::Ident
                && w[1].text(line) != "let"
                && adjacent_eq
            {
                add("bind", "plain-eq");
            }
        }
        if line.contains("&!") {
            add("excl_borrow", "amp-bang");
        }
        if line.contains("!&") {
            add("excl_borrow", "bang-amp");
        }
        if line.contains('^') {
            add("move", "caret");
        }
        if line.contains("?{") {
            add("match", "compact");
        }
        if toks
            .iter()
            .any(|t| t.kind == TokenKind::Ident && t.text(line) == "match")
        {
            add("match", "match-keyword");
        }
    }
    out
}

/// Every problem found in `loaded` (empty means the corpus is sound).
#[must_use]
pub fn validate(loaded: &LoadedCorpus) -> Vec<String> {
    let mut problems = Vec::new();
    let corpus = &loaded.corpus;
    validate_candidate_defs(corpus, &mut problems);
    let mut ids = BTreeSet::new();
    for program in &corpus.programs {
        if !ids.insert(program.id.as_str()) {
            problems.push(format!("duplicate program id `{}`", program.id));
        }
        if program.id.is_empty()
            || !program
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            problems.push(format!("program id `{}` must match [a-z0-9_]+", program.id));
        }
        if program.semantic_units == 0 {
            problems.push(format!(
                "program `{}`: semantic_units must be > 0",
                program.id
            ));
        }
        validate_program(loaded, program, &mut problems);
    }
    problems
}

fn validate_candidate_defs(corpus: &Corpus, problems: &mut Vec<String>) {
    let Some(anchor) = corpus.candidates.get("A") else {
        problems.push("candidates: anchor `A` is not defined".to_owned());
        return;
    };
    if !anchor.differs_from_anchor.is_empty() {
        problems.push("candidate `A` must have an empty differs_from_anchor".to_owned());
    }
    for (label, def) in &corpus.candidates {
        if label == "A" {
            continue;
        }
        let axis = def.differs_from_anchor.as_str();
        if !anchor.axes.contains_key(axis) {
            problems.push(format!("candidate `{label}`: unknown axis `{axis}`"));
            continue;
        }
        for (key, value) in &def.axes {
            let same = anchor.axes.get(key) == Some(value);
            if (key == axis) == same {
                problems.push(format!(
                    "candidate `{label}` must differ from `A` on axis `{axis}` only (axis `{key}` is wrong)"
                ));
            }
        }
    }
}

fn validate_program(loaded: &LoadedCorpus, program: &Program, problems: &mut Vec<String>) {
    let corpus = &loaded.corpus;
    let pid = &program.id;
    let mut seen: BTreeSet<(Lang, Option<&str>)> = BTreeSet::new();
    for variant in &program.variants {
        if !seen.insert((variant.lang, variant.candidate.as_deref())) {
            problems.push(format!(
                "program `{pid}`: duplicate variant {:?}/{:?}",
                variant.lang, variant.candidate
            ));
        }
    }
    for lang in [Lang::Rust, Lang::C, Lang::Zig, Lang::Odin] {
        if !seen.contains(&(lang, None)) {
            problems.push(format!(
                "program `{pid}`: missing `{}` variant",
                lang.as_str()
            ));
        }
    }
    let anchor_text = program
        .variants
        .iter()
        .find(|v| v.lang == Lang::Tessera && v.candidate.as_deref() == Some("A"))
        .map(|v| loaded.text(v));
    if anchor_text.is_none() {
        problems.push(format!("program `{pid}`: missing tessera candidate `A`"));
    }
    for variant in &program.variants {
        let text = loaded.text(variant);
        if text.trim().is_empty() {
            problems.push(format!("`{}` is empty", variant.file));
        }
        if variant.lang != Lang::Tessera {
            if variant.candidate.is_some() || variant.status.is_some() {
                problems.push(format!(
                    "`{}`: only tessera variants carry candidate/status",
                    variant.file
                ));
            }
            continue;
        }
        let (Some(label), Some(status)) = (variant.candidate.as_deref(), variant.status) else {
            problems.push(format!(
                "`{}`: tessera variants need candidate and status",
                variant.file
            ));
            continue;
        };
        let diagnostics = parser_diagnostics(text);
        match status {
            Status::Parsed => {
                if diagnostics != 0 {
                    problems.push(format!("`{}` is labelled parsed but the parser reports {diagnostics} diagnostic(s)", variant.file));
                }
                match tessera_syntax::fmt(text) {
                    Ok(canonical) if canonical == text.trim_end() => {}
                    Ok(canonical) => problems.push(format!(
                        "`{}` is parsed but not canonical (fmt gives `{canonical}`)",
                        variant.file
                    )),
                    Err(e) => problems.push(format!("`{}`: fmt failed: {e:?}", variant.file)),
                }
            }
            Status::Unparsed => {
                if diagnostics == 0 {
                    problems.push(format!(
                        "`{}` is labelled unparsed but the parser now accepts it: promote it to parsed",
                        variant.file
                    ));
                }
                if variant.notes.trim().is_empty() {
                    problems.push(format!(
                        "`{}`: unparsed variants need notes saying what is unparsed",
                        variant.file
                    ));
                }
            }
        }
        let Some(def) = corpus.candidates.get(label) else {
            problems.push(format!(
                "`{}`: candidate `{label}` is not defined in `candidates`",
                variant.file
            ));
            continue;
        };
        if label != "A" && anchor_text == Some(text) {
            problems.push(format!(
                "`{}` is textually identical to candidate `A`; omit it",
                variant.file
            ));
        }
        let observed = features(text);
        for (axis, values) in &observed {
            let want = def.axes.get(*axis).map(String::as_str);
            for value in values {
                if Some(*value) != want {
                    problems.push(format!(
                        "`{}` (candidate {label}) uses `{value}` on axis `{axis}` but {label} defines `{}`",
                        variant.file,
                        want.unwrap_or("?")
                    ));
                }
            }
        }
        if label != "A" && !observed.contains_key(def.differs_from_anchor.as_str()) {
            problems.push(format!(
                "`{}` (candidate {label}) never exercises its axis `{}`",
                variant.file, def.differs_from_anchor
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_classify_each_axis() {
        let f = features("f calc(a:i64)>i64={s:=a+1;s}");
        assert!(f["fn"].contains("f-keyword") && f["bind"].contains("walrus"));
        let f = features("calc:(a:i64)>i64={s=a+1;s}");
        assert!(f["fn"].contains("name-colon-sig") && f["bind"].contains("plain-eq"));
        assert!(!f["bind"].contains("walrus"));
        let f = features("f calc(a:i64)>i64={let s=a+1;s}");
        assert_eq!(f["bind"].iter().copied().collect::<Vec<_>>(), ["let"]);
        let f = features("f b(x:!&i64){*x+=1}\nf r(v:i64)>i64={c:=v;b(!&c);c}");
        assert!(f["excl_borrow"].contains("bang-amp") && !f["excl_borrow"].contains("amp-bang"));
        assert!(!f.contains_key("move"));
        assert!(features("f a(b:B)>i64=e(^b)")["move"].contains("caret"));
        assert!(features("f n(s:St)>St=s?{A:B}")["match"].contains("compact"));
        assert!(features("f n(s:St)>St=match s{A:B}")["match"].contains("match-keyword"));
        // `+=` and `>i64=` are not binds
        assert!(!features("f t(n:i64)>i64={for i in 0..n{acc+=i};acc}").contains_key("bind"));
    }

    #[test]
    fn hash_depends_on_paths_and_bytes() {
        let mut a = BTreeMap::new();
        a.insert("x".to_owned(), b"1".to_vec());
        let mut b = a.clone();
        assert_eq!(hash_entries(&a), hash_entries(&b));
        b.insert("x".to_owned(), b"2".to_vec());
        assert_ne!(hash_entries(&a), hash_entries(&b));
        let mut c = BTreeMap::new();
        c.insert("y".to_owned(), b"1".to_vec());
        assert_ne!(hash_entries(&a), hash_entries(&c));
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        assert!(safe_relative("programs/a/rust.rs"));
        assert!(!safe_relative("../x"));
        assert!(!safe_relative("/abs/x"));
        assert!(!safe_relative("a\\b"));
        assert!(!safe_relative(""));
    }
}
