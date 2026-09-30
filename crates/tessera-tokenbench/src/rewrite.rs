//! TOKDRIFT-style semantic-preserving rewrites of `parsed` Tessera variants.
//!
//! Every generated text is validated against the real parser and formatter:
//! - `Layout` rewrites (spaces, newlines, comments, redundant parentheses)
//!   must parse cleanly and `tessera_syntax::fmt` must canonicalize them back
//!   to exactly the canonical spelling: the "one spelling" property of `tsr fmt`.
//! - `Rename` rewrites (alpha-renaming) must parse cleanly and be canonical
//!   themselves, and they must NOT collapse to the original: renaming is not
//!   something a whitespace formatter can undo, and that is reported honestly.
//!
//! Rewrites that would leave the text unchanged are dropped.

use std::collections::BTreeMap;

use tessera_syntax::lexer::{self, Token, TokenKind};

use crate::corpus::parser_diagnostics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteKind {
    Layout,
    Rename,
}

impl RewriteKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Rename => "rename",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rewrite {
    pub name: &'static str,
    pub kind: RewriteKind,
    pub text: String,
}

#[derive(Default)]
struct Edit {
    pre: &'static str,
    replace: Option<String>,
    post: &'static str,
}

fn rebuild(src: &str, tokens: &[Token], mut edit: impl FnMut(usize, &Token) -> Edit) -> String {
    let mut out = String::with_capacity(src.len() + 16);
    for (i, token) in tokens.iter().enumerate() {
        let e = edit(i, token);
        out.push_str(e.pre);
        match e.replace {
            Some(text) => out.push_str(&text),
            None => out.push_str(token.text(src)),
        }
        out.push_str(e.post);
    }
    out
}

fn spaced(src: &str, tokens: &[Token], kinds: &[TokenKind], before: bool, after: bool) -> String {
    rebuild(src, tokens, |_, t| {
        if kinds.contains(&t.kind) {
            Edit {
                pre: if before { " " } else { "" },
                post: if after { " " } else { "" },
                ..Edit::default()
            }
        } else {
            Edit::default()
        }
    })
}

fn after_token(src: &str, tokens: &[Token], kind: TokenKind, post: &'static str) -> String {
    rebuild(src, tokens, |_, t| {
        if t.kind == kind {
            Edit {
                post,
                ..Edit::default()
            }
        } else {
            Edit::default()
        }
    })
}

/// Names of parameters: identifiers directly followed by `:` before the first `)`.
fn param_names(src: &str, tokens: &[Token]) -> Vec<String> {
    let sig: Vec<&Token> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    let mut names = Vec::new();
    let Some(open) = sig.iter().position(|t| t.kind == TokenKind::LParen) else {
        return names;
    };
    for pair in sig[open..].windows(2) {
        if pair[0].kind == TokenKind::RParen {
            break;
        }
        if pair[0].kind == TokenKind::Ident && pair[1].kind == TokenKind::Colon {
            names.push(pair[0].text(src).to_owned());
        }
    }
    names
}

/// All applicable rewrites of the canonical `src`, in a fixed order.
#[must_use]
pub fn generate(src: &str) -> Vec<Rewrite> {
    let tokens = lexer::lex(src);
    let first_eq = tokens.iter().position(|t| t.kind == TokenKind::Eq);
    let last_sig = tokens.iter().rposition(|t| !t.kind.is_trivia());
    let params = param_names(src, &tokens);
    let body = src.trim_end();

    let layout = |name: &'static str, text: String| Rewrite {
        name,
        kind: RewriteKind::Layout,
        text,
    };
    let mut out = vec![
        layout(
            "space_after_comma",
            after_token(src, &tokens, TokenKind::Comma, " "),
        ),
        layout(
            "space_after_colon",
            after_token(src, &tokens, TokenKind::Colon, " "),
        ),
        layout(
            "space_around_binary_ops",
            spaced(
                src,
                &tokens,
                &[TokenKind::Plus, TokenKind::Eq, TokenKind::Gt],
                true,
                true,
            ),
        ),
        layout(
            "space_inside_parens",
            rebuild(src, &tokens, |_, t| match t.kind {
                TokenKind::LParen => Edit {
                    post: " ",
                    ..Edit::default()
                },
                TokenKind::RParen => Edit {
                    pre: " ",
                    ..Edit::default()
                },
                _ => Edit::default(),
            }),
        ),
        layout(
            "all_punct_spaced",
            spaced(
                src,
                &tokens,
                &[
                    TokenKind::Colon,
                    TokenKind::Comma,
                    TokenKind::LParen,
                    TokenKind::RParen,
                    TokenKind::Gt,
                    TokenKind::Eq,
                    TokenKind::Plus,
                ],
                true,
                true,
            ),
        ),
        layout(
            "trailing_comment",
            format!("{body} // trailing note{}", &src[body.len()..]),
        ),
        layout("leading_comment", format!("// leading note\n{src}")),
        layout(
            "newline_before_body",
            after_token(src, &tokens, TokenKind::Eq, "\n"),
        ),
        layout(
            "newline_indent_body",
            after_token(src, &tokens, TokenKind::Eq, "\n    "),
        ),
        layout(
            "newline_after_comma",
            after_token(src, &tokens, TokenKind::Comma, "\n"),
        ),
        layout(
            "redundant_parens_body",
            rebuild(src, &tokens, |i, _| {
                if Some(i) == first_eq {
                    Edit {
                        post: "(",
                        ..Edit::default()
                    }
                } else if Some(i) == last_sig {
                    Edit {
                        post: ")",
                        ..Edit::default()
                    }
                } else {
                    Edit::default()
                }
            }),
        ),
        layout(
            "redundant_parens_atoms",
            rebuild(src, &tokens, |i, t| {
                let in_body = first_eq.is_some_and(|eq| i > eq);
                if in_body && matches!(t.kind, TokenKind::Ident | TokenKind::Int) {
                    Edit {
                        pre: "(",
                        post: ")",
                        ..Edit::default()
                    }
                } else {
                    Edit::default()
                }
            }),
        ),
    ];
    let rename = |name: &'static str, text: String| Rewrite {
        name,
        kind: RewriteKind::Rename,
        text,
    };
    out.push(rename(
        "rename_verbose",
        rebuild(src, &tokens, |_, t| {
            let text = t.text(src);
            if t.kind == TokenKind::Ident && text != "f" && text != "i64" {
                Edit {
                    replace: Some(format!("{text}_val")),
                    ..Edit::default()
                }
            } else {
                Edit::default()
            }
        }),
    ));
    let positional: BTreeMap<&str, String> = params
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), format!("p{i}")))
        .collect();
    out.push(rename(
        "rename_positional",
        rebuild(src, &tokens, |_, t| match positional.get(t.text(src)) {
            Some(new) if t.kind == TokenKind::Ident => Edit {
                replace: Some(new.clone()),
                ..Edit::default()
            },
            _ => Edit::default(),
        }),
    ));
    out.retain(|r| r.text != src);
    out
}

/// Validate one rewrite against the real parser/formatter.
///
/// Returns whether `fmt` collapses it to the canonical original.
///
/// # Errors
/// The rewrite does not parse cleanly, or violates its kind's contract.
pub fn validate(canonical: &str, rewrite: &Rewrite) -> Result<bool, String> {
    let diagnostics = parser_diagnostics(&rewrite.text);
    if diagnostics != 0 {
        return Err(format!(
            "rewrite `{}` does not parse cleanly ({diagnostics} diagnostic(s)): {:?}",
            rewrite.name, rewrite.text
        ));
    }
    let original =
        tessera_syntax::fmt(canonical).map_err(|e| format!("original does not format: {e:?}"))?;
    let rewritten = tessera_syntax::fmt(&rewrite.text)
        .map_err(|e| format!("rewrite `{}` does not format: {e:?}", rewrite.name))?;
    let collapses = rewritten == original;
    match rewrite.kind {
        RewriteKind::Layout if !collapses => Err(format!(
            "layout rewrite `{}` canonicalizes to `{rewritten}`, not `{original}`",
            rewrite.name
        )),
        RewriteKind::Rename if collapses => Err(format!(
            "rename rewrite `{}` unexpectedly canonicalizes back to the original",
            rewrite.name
        )),
        RewriteKind::Rename if rewritten != rewrite.text.trim_end() => Err(format!(
            "rename rewrite `{}` is not itself canonical (fmt gives `{rewritten}`)",
            rewrite.name
        )),
        _ => Ok(collapses),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "f add(a:i64,b:i64)>i64=a+b\n";

    #[test]
    fn every_generated_rewrite_validates() {
        let rewrites = generate(SRC);
        assert!(rewrites.len() >= 12, "expected a full rewrite set");
        for rw in &rewrites {
            let collapses = validate(SRC, rw).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(collapses, rw.kind == RewriteKind::Layout, "{}", rw.name);
            assert_ne!(rw.text, SRC, "{} is a no-op", rw.name);
        }
    }

    #[test]
    fn known_rewrites_have_the_expected_spelling() {
        let by_name: BTreeMap<_, _> = generate(SRC)
            .into_iter()
            .map(|r| (r.name, r.text))
            .collect();
        assert_eq!(
            by_name["space_after_comma"],
            "f add(a:i64, b:i64)>i64=a+b\n"
        );
        assert_eq!(
            by_name["space_around_binary_ops"],
            "f add(a:i64,b:i64) > i64 = a + b\n"
        );
        assert_eq!(
            by_name["redundant_parens_body"],
            "f add(a:i64,b:i64)>i64=(a+b)\n"
        );
        assert_eq!(
            by_name["redundant_parens_atoms"],
            "f add(a:i64,b:i64)>i64=(a)+(b)\n"
        );
        assert_eq!(
            by_name["trailing_comment"],
            "f add(a:i64,b:i64)>i64=a+b // trailing note\n"
        );
        assert_eq!(
            by_name["rename_positional"],
            "f add(p0:i64,p1:i64)>i64=p0+p1\n"
        );
        assert_eq!(
            by_name["rename_verbose"],
            "f add_val(a_val:i64,b_val:i64)>i64=a_val+b_val\n"
        );
    }

    #[test]
    fn noop_rewrites_are_dropped() {
        // One parameter: no commas, so comma rewrites would be no-ops.
        let names: Vec<_> = generate("f inc(a:i64)>i64=a+1\n")
            .iter()
            .map(|r| r.name)
            .collect();
        assert!(!names.contains(&"space_after_comma"));
        assert!(!names.contains(&"newline_after_comma"));
        assert!(names.contains(&"space_after_colon"));
    }

    #[test]
    fn a_bad_rewrite_is_rejected() {
        let bad = Rewrite {
            name: "broken",
            kind: RewriteKind::Layout,
            text: "f add(a:i64,b:i64)>i64=a+".to_owned(),
        };
        assert!(validate(SRC, &bad).is_err());
        let changes_meaning = Rewrite {
            name: "renamed_as_layout",
            kind: RewriteKind::Layout,
            text: "f add(a:i64,b:i64)>i64=a+a\n".to_owned(),
        };
        assert!(validate(SRC, &changes_meaning).is_err());
    }
}
