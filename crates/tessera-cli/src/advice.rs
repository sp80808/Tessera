//! Repair advice for `tsr witness`: `help` text and machine-applicable
//! `fixes` per diagnostic, and whole-file `suggestions` the front end has
//! already accepted.
//!
//! Models writing TC fall back on the languages they know: `fn`/`def`/`int`
//! headers, `->`, braces, `return`, `;` (Mora et al., SPEAC, 2024, see
//! `docs/research/2026-10-04-llm-repair.md`). Self-repair is bounded by the
//! quality of the feedback (Olausson et al., 2024), so a diagnostic names the
//! TC spelling instead of only the token the parser expected next.
//!
//! A suggestion is offered only when the rewritten file passes `check`: it is
//! compiler-checked, never behaviour-checked. Whether it is *correct* is still
//! for the caller's tests to decide; several alternatives may be offered.

use tessera_phases::{Diagnostic, DiagnosticSet, Phase, Severity};

use crate::pipeline;

/// One edit: replace `start..end` (bytes) with `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    pub label: String,
}

/// What to tell the caller about one diagnostic. `fixes` are alternatives:
/// apply at most one of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Advice {
    pub help: Option<String>,
    pub fixes: Vec<Fix>,
}

/// A complete replacement file that passes `tsr check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub source: String,
    pub label: String,
}

/// The one spelling TC accepts, quoted in help text.
pub const TEMPLATE: &str = "f NAME(P:i64,...)>i64=EXPR";

/// Most alternatives offered for one unbound name, and most combined files.
const MAX_ALTERNATIVES: usize = 4;
const MAX_SUGGESTIONS: usize = 8;

// ---------------------------------------------------------------------------
// A lenient scanner: it reads TC and the languages models confuse it with.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Int(String),
    Punct(&'static str),
    /// Anything else (a character no rule below knows).
    Other,
}

#[derive(Debug, Clone)]
struct Lexeme {
    tok: Tok,
    start: usize,
    end: usize,
}

const PUNCT: &[&str] = &[
    "->", "=>", "::", "==", "(", ")", "{", "}", "[", "]", ",", ":", ";", "=", ">", "<", "+", "-",
    "*", "/", "%",
];

fn scan(text: &str) -> Vec<Lexeme> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if text[i..].starts_with("//") || c == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Lexeme {
                tok: Tok::Ident(text[start..i].to_owned()),
                start,
                end: i,
            });
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            out.push(Lexeme {
                tok: Tok::Int(text[start..i].to_owned()),
                start,
                end: i,
            });
        } else if let Some(p) = PUNCT.iter().find(|p| text[i..].starts_with(**p)) {
            out.push(Lexeme {
                tok: Tok::Punct(p),
                start: i,
                end: i + p.len(),
            });
            i += p.len();
        } else {
            let width = text[i..].chars().next().map_or(1, char::len_utf8);
            out.push(Lexeme {
                tok: Tok::Other,
                start: i,
                end: i + width,
            });
            i += width;
        }
    }
    out
}

/// Header keywords of other languages, as written before a function name.
const FOREIGN_KEYWORDS: &[&str] = &[
    "fn", "def", "func", "function", "fun", "pub", "static", "let", "const", "auto", "inline",
];

/// Type names models write for `i64`.
const FOREIGN_TYPES: &[&str] = &[
    "int", "i8", "i16", "i32", "i128", "isize", "u8", "u16", "u32", "u64", "usize", "long",
    "int64", "int64_t", "Int", "Long", "number", "integer", "num",
];

fn is_type_name(name: &str) -> bool {
    name == "i64" || FOREIGN_TYPES.contains(&name)
}

fn ident(l: &Lexeme) -> Option<&str> {
    match &l.tok {
        Tok::Ident(s) => Some(s),
        _ => None,
    }
}

fn is(l: &Lexeme, p: &str) -> bool {
    matches!(l.tok, Tok::Punct(q) if q == p)
}

/// Index of the token closing the bracket opened at `open`.
fn matching(lex: &[Lexeme], open: usize, left: &str, right: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, l) in lex.iter().enumerate().skip(open) {
        if is(l, left) {
            depth += 1;
        } else if is(l, right) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// The header as written: function name and parameter names. Reads TC and
/// C/Rust/Go/Python/TypeScript-style headers alike.
struct Header {
    name: String,
    params: Vec<String>,
    /// Index of the `)` closing the parameter list.
    close: usize,
}

fn header(lex: &[Lexeme]) -> Option<Header> {
    let open = lex.iter().position(|l| is(l, "("))?;
    let name = ident(lex.get(open.checked_sub(1)?)?)?.to_owned();
    let close = matching(lex, open, "(", ")")?;
    let mut params = Vec::new();
    for group in lex[open + 1..close].split(|l| is(l, ",")) {
        if group.is_empty() {
            continue;
        }
        let names: Vec<&str> = group.iter().filter_map(ident).collect();
        let param = if let Some(colon) = group.iter().position(|l| is(l, ":")) {
            // `a: int` (TC, Rust, Python, TypeScript)
            group[..colon].iter().rev().find_map(ident)?
        } else {
            match names.as_slice() {
                // `a int` (Go) vs `int a` (C)
                [first, last] if is_type_name(last) && !is_type_name(first) => *first,
                [.., last] => *last,
                [] => return None,
            }
        };
        params.push(param.to_owned());
    }
    Some(Header {
        name,
        params,
        close,
    })
}

/// The body expression's tokens: after `=`, inside `{...}` or after a
/// Python-style `:`, with `return` and `;` dropped. `None` unless only
/// identifiers, integers, `+` and parentheses remain (all TC has).
fn body(lex: &[Lexeme], close: usize) -> Option<Vec<&Lexeme>> {
    let rest = &lex[close + 1..];
    let tokens: &[Lexeme] = if let Some(open) = rest.iter().position(|l| is(l, "{")) {
        let end = matching(rest, open, "{", "}")?;
        if rest[end + 1..].iter().any(|l| !is(l, ";")) {
            return None;
        }
        &rest[open + 1..end]
    } else if let Some(eq) = rest.iter().position(|l| is(l, "=")) {
        &rest[eq + 1..]
    } else {
        &rest[rest.iter().rposition(|l| is(l, ":"))? + 1..]
    };
    let out: Vec<&Lexeme> = tokens
        .iter()
        .filter(|l| ident(l) != Some("return") && !is(l, ";"))
        .collect();
    let tc = out.iter().all(|l| {
        matches!(l.tok, Tok::Ident(_) | Tok::Int(_)) || is(l, "+") || is(l, "(") || is(l, ")")
    });
    (tc && !out.is_empty()).then_some(out)
}

fn token_text<'a>(text: &'a str, l: &Lexeme) -> &'a str {
    &text[l.start..l.end]
}

/// Read a program written in a TC-like or foreign syntax and print it in TC.
/// No checking here; see [`suggestions`].
fn rewrite(text: &str) -> Option<String> {
    let lex = scan(text);
    let head = header(&lex)?;
    let body: String = body(&lex, head.close)?
        .iter()
        .map(|l| token_text(text, l))
        .collect();
    let params: Vec<String> = head.params.iter().map(|p| format!("{p}:i64")).collect();
    Some(format!(
        "f {}({})>i64={body}\n",
        head.name,
        params.join(",")
    ))
}

/// Foreign constructs in `text`, each with the TC spelling. Empty for TC.
#[must_use]
pub fn foreign_markers(text: &str) -> Vec<(usize, usize, String)> {
    let lex = scan(text);
    let open = lex.iter().position(|l| is(l, "(")).unwrap_or(lex.len());
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let mut note = |l: &Lexeme, key: &str, message: String| {
        if !seen.iter().any(|k| k == key) {
            seen.push(key.to_owned());
            out.push((l.start, l.end, message));
        }
    };
    for (i, l) in lex.iter().enumerate() {
        match (&l.tok, ident(l)) {
            (_, Some(word)) if i < open && FOREIGN_KEYWORDS.contains(&word) => note(
                l,
                "keyword",
                format!("functions start with `f`, not `{word}`"),
            ),
            (_, Some(word)) if is_type_name(word) && word != "i64" => {
                note(l, "type", format!("the only type is `i64`, not `{word}`"));
            }
            (_, Some("return")) => note(
                l,
                "return",
                "there is no `return`: the expression after `=` is the result".to_owned(),
            ),
            (Tok::Punct("->"), _) => {
                note(
                    l,
                    "arrow",
                    "the return type is written `>i64`, not `->`".to_owned(),
                );
            }
            (Tok::Punct("{" | "}"), _) => note(
                l,
                "braces",
                "there are no braces: the body is one expression after `=`".to_owned(),
            ),
            (Tok::Punct(";"), _) => note(l, "semicolon", "there are no `;`".to_owned()),
            (Tok::Punct(":"), _)
                if i > open && lex.get(i + 1).is_none_or(|n| ident(n) == Some("return")) =>
            {
                note(l, "colon-body", "the body follows `=`, not `:`".to_owned());
            }
            _ => {}
        }
    }
    out
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            row.push(
                (prev[j + 1] + 1)
                    .min(row[j] + 1)
                    .min(prev[j] + usize::from(ca != *cb)),
            );
        }
        prev = row;
    }
    prev[b.len()]
}

/// The first backtick-quoted word of a diagnostic message.
fn quoted(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let len = message[start..].find('`')?;
    Some(&message[start..start + len])
}

/// Help and fixes for one front-end diagnostic of `text`.
#[must_use]
pub fn advise(text: &str, code: &str, message: &str, start: usize, end: usize) -> Advice {
    let lex = scan(text);
    let mut params = header(&lex).map(|h| h.params).unwrap_or_default();
    let mut seen = Vec::new();
    params.retain(|p| {
        let first = !seen.contains(p);
        seen.push(p.clone());
        first
    });
    let at = text.get(start..).unwrap_or("");
    let fix = |start, end, replacement: &str, label: String| Fix {
        start,
        end,
        replacement: replacement.to_owned(),
        label,
    };
    match code {
        "E-resolve-unbound-name" => {
            let Some(name) = quoted(message) else {
                return Advice::default();
            };
            if name == "return" {
                return Advice {
                    help: Some("TC has no `return`: the expression after `=` is the result".into()),
                    fixes: vec![fix(start, end, "", "delete `return`".into())],
                };
            }
            let best = params.iter().map(|p| edit_distance(name, p)).min();
            let limit = (name.chars().count() / 2).max(1);
            let close: Vec<&String> = params
                .iter()
                .filter(|p| Some(edit_distance(name, p)) == best && best <= Some(limit))
                .take(MAX_ALTERNATIVES)
                .collect();
            let listed = if params.is_empty() {
                "the function has no parameters".to_owned()
            } else {
                format!("parameters: {}", params.join(", "))
            };
            let help = match close.as_slice() {
                [] => format!("`{name}` is not a parameter; {listed}"),
                [one] => format!("`{name}` is not a parameter; did you mean `{one}`? ({listed})"),
                many => format!(
                    "`{name}` is not a parameter; did you mean one of {}? ({listed})",
                    many.iter()
                        .map(|p| format!("`{p}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            Advice {
                help: Some(help),
                fixes: close
                    .iter()
                    .map(|p| fix(start, end, p, format!("use parameter `{p}`")))
                    .collect(),
            }
        }
        "E-resolve-unknown-type" => Advice {
            help: Some("TC v0 has one type, `i64`".into()),
            fixes: vec![fix(start, end, "i64", "use `i64`".into())],
        },
        "E-syntax-trailing-input" => {
            let op = at.chars().next();
            match op {
                Some(';') => Advice {
                    help: Some("TC has no `;`".into()),
                    fixes: vec![fix(start, start + 1, "", "delete `;`".into())],
                },
                Some(c @ ('*' | '-' | '/' | '%')) => Advice {
                    help: Some(format!(
                        "TC v0 has only the `+` operator; `{c}` is not available"
                    )),
                    fixes: Vec::new(),
                },
                _ => Advice {
                    help: Some(format!("a file is exactly one function: `{TEMPLATE}`")),
                    fixes: Vec::new(),
                },
            }
        }
        "E-syntax-expected" if at.starts_with("->") => Advice {
            help: Some("the return type is written `>i64`, not `->i64`".into()),
            fixes: vec![fix(start, start + 2, ">", "replace `->` with `>`".into())],
        },
        "E-syntax-expected" if message.starts_with("expected `:`") => Advice {
            help: Some("every parameter needs a type: `NAME:i64`".into()),
            fixes: vec![fix(start, start, ":i64", "add `:i64`".into())],
        },
        "E-syntax-expected" if message.starts_with("expected `f`") => Advice {
            help: Some(format!("a TC function is written `{TEMPLATE}`")),
            fixes: Vec::new(),
        },
        "E-syntax-expected" if message.starts_with("expected expression") => Advice {
            help: Some(if at.starts_with('-') {
                "TC v0 has no `-` (no subtraction or negative literals); EXPR is integers, parameters and `+`".into()
            } else {
                "an operand is missing: EXPR is integers, parameters and `+`, e.g. `a+b`".into()
            }),
            fixes: Vec::new(),
        },
        _ => Advice::default(),
    }
}

/// Advice for a rejected TC file: an `E-syntax-foreign` summary when it
/// reads as another language, and [`advise`] for each diagnostic. With a
/// foreign summary, syntax diagnostics after the first are parser cascades
/// and get no advice of their own.
#[derive(Debug, Default)]
pub struct Report {
    /// `(start, end, message)` of the foreign-syntax summary.
    pub foreign: Option<(usize, usize, String)>,
    /// One per diagnostic, in the set's order.
    pub advice: Vec<Advice>,
}

pub const FOREIGN_CODE: &str = "E-syntax-foreign";

#[must_use]
pub fn report(text: &str, diagnostics: &DiagnosticSet) -> Report {
    let is_syntax = |d: &Diagnostic| d.phase == Phase::Syntax && d.severity == Severity::Error;
    let markers = foreign_markers(text);
    let foreign = match markers.first() {
        Some(&(start, end, _)) if diagnostics.iter().any(is_syntax) => {
            let found: Vec<&str> = markers.iter().map(|(_, _, m)| m.as_str()).collect();
            Some((
                start,
                end,
                format!("this is not TC syntax: {}", found.join("; ")),
            ))
        }
        _ => None,
    };
    let mut first_syntax = true;
    let advice = diagnostics
        .iter()
        .map(|d| {
            if is_syntax(d) {
                let cascade = foreign.is_some() && !first_syntax;
                first_syntax = false;
                if cascade {
                    return Advice::default();
                }
            }
            let span = d.at.primary_span();
            advise(
                text,
                d.code,
                &d.message,
                span.start as usize,
                span.end as usize,
            )
        })
        .collect();
    Report { foreign, advice }
}

/// Apply non-overlapping `fixes` (overlaps keep the earliest-listed one).
fn apply(text: &str, fixes: &[&Fix]) -> String {
    let mut chosen: Vec<&Fix> = Vec::new();
    for f in fixes {
        let overlaps = chosen.iter().any(|c| {
            (f.start < c.end && c.start < f.end) || (f.start == c.start && f.end == c.end)
        });
        if !overlaps {
            chosen.push(f);
        }
    }
    chosen.sort_by_key(|f| std::cmp::Reverse(f.start));
    let mut out = text.to_owned();
    for f in chosen {
        out.replace_range(f.start..f.end, &f.replacement);
    }
    out
}

/// `candidate`, canonically formatted, if the front end accepts it.
fn checked(candidate: &str) -> Option<String> {
    if pipeline::check_tc(candidate).diagnostics.has_errors() {
        return None;
    }
    tessera_syntax::fmt(candidate)
        .ok()
        .map(|s| format!("{}\n", s.trim_end()))
}

/// Whole-file repairs of a rejected `text`, each accepted by `tsr check`:
/// first the TC reading of a foreign-syntax program, then every combination
/// of the per-diagnostic fix alternatives (`advice`), in order.
#[must_use]
pub fn suggestions(text: &str, advice: &[Advice]) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let mut push = |source: Option<String>, label: String| {
        let Some(source) = source else { return };
        if source.trim() != text.trim()
            && !out.iter().any(|s| s.source == source)
            && out.len() < MAX_SUGGESTIONS
        {
            out.push(Suggestion { source, label });
        }
    };
    if !foreign_markers(text).is_empty() {
        push(
            rewrite(text).and_then(|s| checked(&s)),
            "rewrite in TC syntax".into(),
        );
    }
    let groups: Vec<&[Fix]> = advice
        .iter()
        .map(|a| a.fixes.as_slice())
        .filter(|f| !f.is_empty())
        .collect();
    let mut index = vec![0usize; groups.len()];
    for _ in 0..MAX_SUGGESTIONS * 2 {
        if groups.is_empty() {
            break;
        }
        let picked: Vec<&Fix> = groups.iter().zip(&index).map(|(g, &i)| &g[i]).collect();
        let label = picked
            .iter()
            .map(|f| f.label.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        push(checked(&apply(text, &picked)), label);
        // Next combination, last group fastest.
        let mut k = groups.len();
        loop {
            if k == 0 {
                return out;
            }
            k -= 1;
            index[k] += 1;
            if index[k] < groups[k].len() {
                break;
            }
            index[k] = 0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(text: &str) -> Vec<String> {
        let out = pipeline::check_tc(text);
        let report = report(text, &out.diagnostics);
        suggestions(text, &report.advice)
            .into_iter()
            .map(|s| s.source)
            .collect()
    }

    #[test]
    fn foreign_headers_are_read_as_tc() {
        let want = vec!["f add(a:i64,b:i64)>i64=a+b\n".to_owned()];
        for text in [
            "fn add(a: i64, b: i64) -> i64 { a + b }",
            "fn add(a: i64, b: i64) -> i64 {\n    return a + b;\n}\n",
            "int add(int a, int b) { return a + b; }",
            "func add(a int, b int) int { return a + b }",
            "func add(a, b int) int { return a + b }",
            "def add(a, b):\n    return a + b\n",
            "def add(a: int, b: int) -> int:\n    return a + b\n",
            "function add(a: number, b: number): number { return a + b; }",
            "fun add(a: Int, b: Int): Int = a + b",
            "f add(a:i64,b:i64)->i64=a+b",
            "f add(a:i64,b:i64)>i64=return a+b;",
        ] {
            assert_eq!(sources(text), want, "{text}");
        }
    }

    #[test]
    fn no_suggestion_outside_tc() {
        // `-` and `*` have no TC spelling; nothing is invented for them.
        assert!(sources("fn sub(a: i64, b: i64) -> i64 { a - b }").is_empty());
        assert!(sources("f add(a:i64,b:i64)>i64=a*b").is_empty());
        assert!(sources("f add(a:i64,b:i64)>i64=a+").is_empty());
    }

    #[test]
    fn unbound_names_offer_each_closest_parameter() {
        assert_eq!(
            sources("f add(a:i64,b:i64)>i64=a+c\n"),
            vec![
                "f add(a:i64,b:i64)>i64=a+a\n".to_owned(),
                "f add(a:i64,b:i64)>i64=a+b\n".to_owned()
            ]
        );
        assert_eq!(
            sources("f add(left:i64,right:i64)>i64=left+rigth\n"),
            vec!["f add(left:i64,right:i64)>i64=left+right\n".to_owned()]
        );
        let advice = advise(
            "f g(x:i64)>i64=zzzz",
            "E-resolve-unbound-name",
            "unbound variable `zzzz`",
            15,
            19,
        );
        assert!(advice.fixes.is_empty(), "too far from any parameter");
        assert_eq!(
            advice.help.as_deref(),
            Some("`zzzz` is not a parameter; parameters: x")
        );
    }

    #[test]
    fn types_and_untyped_params_are_fixed() {
        assert_eq!(
            sources("f add(a:int,b:int)>int=a+b\n"),
            vec!["f add(a:i64,b:i64)>i64=a+b\n".to_owned()]
        );
        assert_eq!(
            sources("f add(a,b)>i64=a+b\n"),
            vec!["f add(a:i64,b:i64)>i64=a+b\n".to_owned()]
        );
    }

    #[test]
    fn markers_name_the_tc_spelling_and_tc_has_none() {
        let found: Vec<String> = foreign_markers("fn add(a: i32) -> i32 { return a; }")
            .into_iter()
            .map(|(_, _, m)| m)
            .collect();
        assert_eq!(found.len(), 6, "{found:?}");
        assert!(found[0].contains("start with `f`"));
        assert!(foreign_markers("f add(a:i64,b:i64)>i64=(a+b)+1 // ok\n").is_empty());
        assert!(foreign_markers("f add(a:i64,b:i64)>i64=a+c\n").is_empty());
    }

    /// Arbitrary text never panics, and every suggestion passes `check`.
    #[test]
    fn advice_is_total_and_suggestions_are_checked() {
        const PIECES: &[&str] = &[
            "f", "fn", "def", "int", "i64", "return", "add", "a", "b", "c", "(", ")", "{", "}",
            ":", ";", ",", "=", "+", "-", "->", "*", " ", "\n", "#", "//", "é", "0", "12", "_x",
        ];
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % n as u64).unwrap_or(0)
        };
        for round in 0..3000 {
            let len = next(24);
            let mut text: String = (0..len).map(|_| PIECES[next(PIECES.len())]).collect();
            if round % 3 == 0 {
                text = format!("fn add(a: i64, b: i64) -> i64 {{ {text} }}");
            }
            for source in sources(&text) {
                assert!(
                    !pipeline::check_tc(&source).diagnostics.has_errors(),
                    "{text:?} -> {source:?}"
                );
            }
        }
    }
}
