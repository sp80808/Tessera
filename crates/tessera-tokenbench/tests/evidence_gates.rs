//! Evidence gates (issue #1, "no syntax proposal can be marked stable without a
//! benchmark artifact"). Two documents make syntax claims; both are checked
//! against the committed benchmark artifacts on every `cargo test --workspace`:
//!
//! 1. `docs/architecture/syntax-lexicon.md`: a register row whose `Grammar`
//!    column is anything other than `no` / `ignored` / `n/a` (the grammar
//!    accepts the lexeme) must carry benchmark evidence or a named, dated
//!    exemption from [`EXEMPT`]. Exemptions expire: once a row gains evidence,
//!    or stops being accepted, its entry must be deleted (same idea as
//!    `KNOWN_UNWIRED` in `tessera-phases/tests/architecture.rs`).
//! 2. `docs/rfcs/NNNN-*.md`: a `Status:` of `experimental` or `accepted` needs
//!    at least one resolving evidence reference. `draft` (and the terminal
//!    `rejected` / `superseded`) RFCs are not gated.
//!
//! An evidence reference is `bench:<corpus_hash prefix>/<program id>`. It
//! resolves when the prefix (>= 12 lowercase hex characters) is a prefix of
//! `corpus_hash` in `bench/results/baseline.json` and the program id exists in
//! `bench/corpus/corpus.json`. That proves the claim names the *current*
//! committed artifact; it does not prove the artifact supports the claim or
//! that it covers enough tokenizer families. Regenerating the baseline after a
//! corpus change changes the hash and therefore invalidates every reference,
//! which is deliberate: the evidence must be re-read against the new artifact.

mod common;

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;
use tessera_syntax::lexer::TokenKind;

const MIN_PREFIX: usize = 12;

/// What an evidence reference can resolve against.
struct Artifacts {
    corpus_hash: String,
    programs: BTreeSet<String>,
}

impl Artifacts {
    fn committed() -> Self {
        let baseline = common::committed_baseline();
        let corpus_path = common::bench_dir().join("corpus").join("corpus.json");
        let corpus: Value =
            serde_json::from_str(&fs::read_to_string(corpus_path).expect("corpus.json"))
                .expect("corpus.json is valid JSON");
        Self {
            corpus_hash: baseline["corpus_hash"]
                .as_str()
                .expect("baseline corpus_hash")
                .to_owned(),
            programs: corpus["programs"]
                .as_array()
                .expect("programs")
                .iter()
                .map(|p| p["id"].as_str().expect("program id").to_owned())
                .collect(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct BenchRef<'a> {
    prefix: &'a str,
    program: &'a str,
}

/// Parse `bench:<lowercase hex prefix, >= 12>/<program id [a-z0-9_]+>`.
fn parse_ref(token: &str) -> Result<BenchRef<'_>, String> {
    let shape = || format!("`{token}` is not `bench:<corpus hash prefix>/<program id>`");
    let rest = token.strip_prefix("bench:").ok_or_else(shape)?;
    let (prefix, program) = rest.split_once('/').ok_or_else(shape)?;
    if !prefix
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        return Err(format!("`{token}`: hash prefix must be lowercase hex"));
    }
    if prefix.len() < MIN_PREFIX {
        return Err(format!(
            "`{token}`: hash prefix must be at least {MIN_PREFIX} hex characters"
        ));
    }
    if program.is_empty()
        || !program
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(format!("`{token}`: program id must match [a-z0-9_]+"));
    }
    Ok(BenchRef { prefix, program })
}

fn resolve(token: &str, artifacts: &Artifacts) -> Result<(), String> {
    let r = parse_ref(token)?;
    if !artifacts.corpus_hash.starts_with(r.prefix) {
        return Err(format!(
            "`{token}`: hash prefix does not match baseline corpus_hash `{}...`",
            &artifacts.corpus_hash[..MIN_PREFIX]
        ));
    }
    if !artifacts.programs.contains(r.program) {
        return Err(format!(
            "`{token}`: no program `{}` in bench/corpus/corpus.json",
            r.program
        ));
    }
    Ok(())
}

/// Concrete references in free text: `bench:` followed by a hex digit, then
/// the maximal run of `[A-Za-z0-9_/]`. Placeholders such as `bench:<hash>/..`
/// are not concrete and are ignored.
fn concrete_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("bench:") {
        let after = &rest[i + "bench:".len()..];
        if after.chars().next().is_some_and(|c| c.is_ascii_hexdigit()) {
            let len = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '/'))
                .unwrap_or(after.len());
            out.push(format!("bench:{}", &after[..len]));
        }
        rest = after;
    }
    out
}

// ---------------------------------------------------------------- register

struct Exemption {
    kind: &'static str,
    since: &'static str,
    reason: &'static str,
}

const BOOTSTRAP_REASON: &str = "predates the gate; revisit when #1 has >= 4 families";
const BOOTSTRAP_EVIDENCE: &str = "bootstrap only";

/// Register rows the grammar already accepted before the evidence gate
/// existed. Each must keep the literal Evidence cell `bootstrap only`. Delete
/// an entry when its row gains a `bench:` reference or stops being accepted;
/// this test fails until you do.
const EXEMPT: &[Exemption] = &[
    Exemption {
        kind: "Ident",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "Int",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "Colon",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "Comma",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "LParen",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "RParen",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "Gt",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "Eq",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
    Exemption {
        kind: "Plus",
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    },
];

/// `Grammar` cells that mean "the grammar does not accept this lexeme".
fn grammar_accepts(cell: &str) -> bool {
    let norm = cell.replace('*', "").trim().to_lowercase();
    !matches!(norm.as_str(), "no" | "ignored" | "n/a")
}

/// Every violation of the promotion rule in `doc` (empty means it holds).
fn check_register(doc: &str, artifacts: &Artifacts, exempt: &[Exemption]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut accepted = BTreeSet::new();
    for line in doc.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() != 5 {
            problems.push(format!(
                "register row has {} cells, expected 5 (a `|` inside a cell?): {line}",
                cells.len()
            ));
            continue;
        }
        let kind = cells[0].trim_matches('`');
        let (grammar, evidence) = (cells[3], cells[4]);
        let is_ref_cell = evidence.contains("bench:");
        let ref_problem = is_ref_cell
            .then(|| {
                evidence
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .map(|t| t.trim_matches('`'))
                    .filter(|t| !t.is_empty())
                    .find_map(|t| resolve(t, artifacts).err())
            })
            .flatten();
        if let Some(problem) = &ref_problem {
            problems.push(format!("{kind}: evidence does not resolve: {problem}"));
        }
        if !grammar_accepts(grammar) {
            continue;
        }
        accepted.insert(kind);
        let exemption = exempt.iter().find(|e| e.kind == kind);
        match (is_ref_cell, ref_problem.is_none(), exemption) {
            // Resolved evidence: the exemption must not linger.
            (true, true, Some(_)) => problems.push(format!(
                "{kind}: has benchmark evidence now; remove its entry from EXEMPT"
            )),
            (true, _, _) => {} // fine, or already reported above
            (false, _, Some(_)) if evidence == BOOTSTRAP_EVIDENCE => {}
            (false, _, Some(_)) => problems.push(format!(
                "{kind}: is exempt but its Evidence cell is `{evidence}`, expected `{BOOTSTRAP_EVIDENCE}`"
            )),
            (false, _, None) => problems.push(format!(
                "{kind}: the grammar accepts it (`{grammar}`) but Evidence is `{evidence}`; \
                 promotion needs `bench:<corpus hash prefix>/<program id>` (docs/architecture/syntax-lexicon.md, Promotion rule)"
            )),
        }
    }
    for e in exempt {
        if !accepted.contains(e.kind) {
            problems.push(format!(
                "stale exemption: {} is not an accepted register row; remove it from EXEMPT",
                e.kind
            ));
        }
    }
    problems
}

fn register_text() -> String {
    fs::read_to_string(
        common::repo_root()
            .join("docs")
            .join("architecture")
            .join("syntax-lexicon.md"),
    )
    .expect("lexicon register")
}

#[test]
fn committed_register_obeys_the_promotion_rule() {
    let problems = check_register(&register_text(), &Artifacts::committed(), EXEMPT);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn exemptions_are_named_dated_and_reasoned() {
    let names: Vec<&str> = TokenKind::ALL.iter().map(|k| k.name()).collect();
    let mut seen = BTreeSet::new();
    for e in EXEMPT {
        assert!(
            names.contains(&e.kind),
            "exemption names unknown kind {}",
            e.kind
        );
        assert!(seen.insert(e.kind), "duplicate exemption for {}", e.kind);
        let b = e.since.as_bytes();
        let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
        assert!(
            b.len() == 10
                && digits(0..4)
                && b[4] == b'-'
                && digits(5..7)
                && b[7] == b'-'
                && digits(8..10),
            "{}: `since` must be YYYY-MM-DD, got `{}`",
            e.kind,
            e.since
        );
        assert!(e.reason.len() >= 20, "{}: give a real reason", e.kind);
    }
}

fn fake() -> Artifacts {
    Artifacts {
        corpus_hash: format!("{}{}", "0123456789abcdef".repeat(2), "f".repeat(32)),
        programs: ["arith_add".to_owned(), "loop_sum".to_owned()].into(),
    }
}

const GOOD: &str = "bench:0123456789ab/arith_add";

fn row(kind: &str, grammar: &str, evidence: &str) -> String {
    format!("| `{kind}` | x | yes | {grammar} | {evidence} |\n")
}

fn exempt(kind: &'static str) -> Exemption {
    Exemption {
        kind,
        since: "2026-09-30",
        reason: BOOTSTRAP_REASON,
    }
}

#[test]
fn accepting_a_lexeme_without_evidence_fails() {
    let doc = row("Minus", "binary subtraction", "none");
    let problems = check_register(&doc, &fake(), &[]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("Minus") && problems[0].contains("promotion needs"));
    // Not accepted (any spelling of "no"), so no evidence is required.
    for grammar in ["**no**", "no", "NO", "ignored", "n/a"] {
        let doc = row("Minus", grammar, "none");
        assert!(check_register(&doc, &fake(), &[]).is_empty(), "{grammar}");
    }
}

#[test]
fn a_resolving_reference_promotes_and_bad_references_do_not() {
    assert!(check_register(&row("Minus", "subtraction", GOOD), &fake(), &[]).is_empty());
    let two = format!("`{GOOD}`, bench:0123456789abcdef0123/loop_sum");
    assert!(check_register(&row("Minus", "subtraction", &two), &fake(), &[]).is_empty());
    for (evidence, needle) in [
        ("bench:ffffffffffff/arith_add", "does not match"),
        ("bench:0123456789ab/no_such_program", "no program"),
        ("bench:0123456/arith_add", "at least 12"),
        ("bench:0123456789AB/arith_add", "lowercase hex"),
        ("bench:0123456789ab", "is not `bench:"),
        ("bench:0123456789ab/Arith-Add", "[a-z0-9_]+"),
        (&*format!("{GOOD}, prose"), "is not `bench:"),
    ] {
        let problems = check_register(&row("Minus", "subtraction", evidence), &fake(), &[]);
        assert!(
            problems.iter().any(|p| p.contains(needle)),
            "{evidence}: wanted `{needle}` in {problems:?}"
        );
    }
    // A wrong reference is an error even on a row the grammar does not accept.
    let problems = check_register(
        &row("Minus", "**no**", "bench:ffffffffffff/arith_add"),
        &fake(),
        &[],
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
}

#[test]
fn exemptions_expire() {
    let ex = [exempt("Plus")];
    // Exempt and still `bootstrap only`: fine.
    assert!(
        check_register(
            &row("Plus", "left-assoc addition", BOOTSTRAP_EVIDENCE),
            &fake(),
            &ex
        )
        .is_empty()
    );
    // Exempt row that gained evidence must leave the allow-list.
    let problems = check_register(&row("Plus", "addition", GOOD), &fake(), &ex);
    assert!(
        problems.iter().any(|p| p.contains("remove its entry")),
        "{problems:?}"
    );
    // Exempt row demoted to `no`: stale.
    let problems = check_register(&row("Plus", "**no**", "none"), &fake(), &ex);
    assert!(
        problems.iter().any(|p| p.contains("stale exemption")),
        "{problems:?}"
    );
    // Exemption for a kind with no row at all: stale.
    let problems = check_register("", &fake(), &ex);
    assert!(
        problems.iter().any(|p| p.contains("stale exemption")),
        "{problems:?}"
    );
    // Exempt rows may not quietly claim other evidence text.
    let problems = check_register(&row("Plus", "addition", "measured, trust me"), &fake(), &ex);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("expected `bootstrap only`")),
        "{problems:?}"
    );
    // An exemption does not cover a different kind.
    let problems = check_register(
        &row("Minus", "subtraction", BOOTSTRAP_EVIDENCE),
        &fake(),
        &ex,
    );
    assert!(problems.iter().any(|p| p.contains("Minus")), "{problems:?}");
}

#[test]
fn malformed_register_rows_are_reported() {
    let problems = check_register("| `Plus` | x | yes | addition |\n", &fake(), &[]);
    assert!(
        problems.iter().any(|p| p.contains("expected 5")),
        "{problems:?}"
    );
}

// -------------------------------------------------------------------- RFCs

const STATUSES: [&str; 5] = [
    "draft",
    "experimental",
    "accepted",
    "rejected",
    "superseded",
];

/// First word of the `Status:` line, lowercased, validated against the
/// lifecycle in `docs/rfcs/README.md`.
fn rfc_status(text: &str) -> Result<String, String> {
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("Status:"))
        .ok_or("no `Status:` line")?;
    let word: String = line
        .trim()
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_lowercase();
    if STATUSES.contains(&word.as_str()) {
        Ok(word)
    } else {
        Err(format!(
            "unknown status `{}` (expected one of {STATUSES:?})",
            line.trim()
        ))
    }
}

/// Violations for one RFC (empty means it passes its gate).
fn check_rfc(text: &str, artifacts: &Artifacts) -> Vec<String> {
    let status = match rfc_status(text) {
        Ok(s) => s,
        Err(e) => return vec![e],
    };
    if status != "experimental" && status != "accepted" {
        return Vec::new();
    }
    let refs = concrete_refs(text);
    if refs.is_empty() {
        return vec![format!(
            "status `{status}` requires a `bench:<corpus hash prefix>/<program id>` evidence reference"
        )];
    }
    refs.iter()
        .filter_map(|r| resolve(r, artifacts).err())
        .collect()
}

fn rfcs() -> Vec<(String, String)> {
    let dir = common::repo_root().join("docs").join("rfcs");
    let mut out: Vec<(String, String)> = fs::read_dir(dir)
        .expect("docs/rfcs")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|n| {
            !n.starts_with("._")
                && n.len() > 5
                && n.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                && n.ends_with(".md")
        })
        .map(|n| {
            let text = fs::read_to_string(common::repo_root().join("docs").join("rfcs").join(&n))
                .expect("rfc");
            (n, text)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn committed_rfcs_obey_the_evidence_gate() {
    let all = rfcs();
    assert!(
        all.len() >= 2,
        "expected the template and RFC 0001, found {}",
        all.len()
    );
    let artifacts = Artifacts::committed();
    for (name, text) in &all {
        let problems = check_rfc(text, &artifacts);
        assert!(problems.is_empty(), "docs/rfcs/{name}: {problems:?}");
    }
}

#[test]
fn rfc_gate_requires_resolving_evidence_only_for_experimental_and_accepted() {
    let art = fake();
    let with = |status: &str, body: &str| format!("# RFC 9999\n\nStatus: {status}\n\n{body}\n");
    // Drafts (and terminal states) are not gated, even citing nothing.
    for status in [
        "draft",
        "draft (working candidate)",
        "rejected",
        "superseded by 0002",
    ] {
        assert!(
            check_rfc(&with(status, "no evidence"), &art).is_empty(),
            "{status}"
        );
    }
    // Gated states need a reference...
    for status in ["experimental", "accepted"] {
        let problems = check_rfc(
            &with(status, "prose only, placeholder bench:<hash>/id"),
            &art,
        );
        assert!(
            problems.iter().any(|p| p.contains("requires")),
            "{status}: {problems:?}"
        );
        // ...that resolves; every concrete reference must.
        assert!(check_rfc(&with(status, &format!("Evidence: `{GOOD}`.")), &art).is_empty());
        let problems = check_rfc(
            &with(
                status,
                &format!("`{GOOD}` and bench:ffffffffffff/arith_add"),
            ),
            &art,
        );
        assert_eq!(problems.len(), 1, "{status}: {problems:?}");
        let problems = check_rfc(&with(status, "bench:0123456789ab/nope"), &art);
        assert!(
            problems.iter().any(|p| p.contains("no program")),
            "{problems:?}"
        );
    }
}

#[test]
fn rfc_status_must_be_recognised_so_a_typo_cannot_skip_the_gate() {
    let art = fake();
    assert!(check_rfc("# x\n\nStatus: acepted\n", &art)[0].contains("unknown status"));
    assert!(check_rfc("# x\n\nno status here\n", &art)[0].contains("no `Status:` line"));
    assert_eq!(
        rfc_status("Status: Experimental \n").as_deref(),
        Ok("experimental")
    );
}

#[test]
fn reference_extraction_ignores_placeholders_and_trailing_punctuation() {
    assert_eq!(
        concrete_refs(
            "see (bench:0123456789ab/arith_add), and `bench:abcdef012345/loop_sum`. Not bench:<hash>/x or bench:results."
        ),
        [
            "bench:0123456789ab/arith_add",
            "bench:abcdef012345/loop_sum"
        ]
    );
    assert_eq!(
        parse_ref(GOOD),
        Ok(BenchRef {
            prefix: "0123456789ab",
            program: "arith_add"
        })
    );
}
