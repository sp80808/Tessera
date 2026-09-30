//! Lossless lexer for the provisional TC subset (issue #18 slice).
//!
//! Contract (LEX-1..3, see `docs/architecture/compiler-phases.md`):
//! - every input byte belongs to exactly one token, in order, with no gaps or
//!   overlaps, so concatenating token texts reproduces the source exactly;
//! - whitespace and comments are explicit trivia tokens, never dropped;
//! - unrecognized input becomes an [`TokenKind::Error`] token spanning one
//!   whole UTF-8 character; lexing itself never fails and never panics.
//!
//! Token kinds describe the *provisional* grammar only and are replaceable
//! with it. Keywords are not distinguished here: `f` is an `Ident` and the
//! parser decides what it means, so grammar experiments do not touch the lexer.

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Whitespace,
    /// `//` up to, but not including, the line break.
    LineComment,
    Ident,
    Int,
    Colon,
    Comma,
    LParen,
    RParen,
    Gt,
    Eq,
    Plus,
    // Provisional punctuation lexicon: recognized so diagnostics can name it,
    // NOT accepted by any grammar. Multi-character operators (`==`, `->`, `&&`)
    // are deliberately absent: they are grammar decisions (#1/#2) and are
    // formed by the parser from adjacent single-character tokens.
    Minus,
    Star,
    Lt,
    Bang,
    Amp,
    Dot,
    Semi,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    /// One character the lexicon does not recognize.
    Error,
}

impl TokenKind {
    /// Whitespace and comments: preserved, never semantically meaningful.
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(self, Self::Whitespace | Self::LineComment)
    }

    /// Every kind, in declaration order (used by the lexicon-register test).
    pub const ALL: [TokenKind; 23] = [
        Self::Whitespace,
        Self::LineComment,
        Self::Ident,
        Self::Int,
        Self::Colon,
        Self::Comma,
        Self::LParen,
        Self::RParen,
        Self::Gt,
        Self::Eq,
        Self::Plus,
        Self::Minus,
        Self::Star,
        Self::Lt,
        Self::Bang,
        Self::Amp,
        Self::Dot,
        Self::Semi,
        Self::LBrace,
        Self::RBrace,
        Self::LBracket,
        Self::RBracket,
        Self::Error,
    ];

    /// Human description for diagnostics ("expected X, found Y").
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Whitespace => "whitespace",
            Self::LineComment => "comment",
            Self::Ident => "identifier",
            Self::Int => "integer",
            Self::Colon => "`:`",
            Self::Comma => "`,`",
            Self::LParen => "`(`",
            Self::RParen => "`)`",
            Self::Gt => "`>`",
            Self::Eq => "`=`",
            Self::Plus => "`+`",
            Self::Minus => "`-`",
            Self::Star => "`*`",
            Self::Lt => "`<`",
            Self::Bang => "`!`",
            Self::Amp => "`&`",
            Self::Dot => "`.`",
            Self::Semi => "`;`",
            Self::LBrace => "`{`",
            Self::RBrace => "`}`",
            Self::LBracket => "`[`",
            Self::RBracket => "`]`",
            Self::Error => "unrecognized character",
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Whitespace => "Whitespace",
            Self::LineComment => "LineComment",
            Self::Ident => "Ident",
            Self::Int => "Int",
            Self::Colon => "Colon",
            Self::Comma => "Comma",
            Self::LParen => "LParen",
            Self::RParen => "RParen",
            Self::Gt => "Gt",
            Self::Eq => "Eq",
            Self::Plus => "Plus",
            Self::Minus => "Minus",
            Self::Star => "Star",
            Self::Lt => "Lt",
            Self::Bang => "Bang",
            Self::Amp => "Amp",
            Self::Dot => "Dot",
            Self::Semi => "Semi",
            Self::LBrace => "LBrace",
            Self::RBrace => "RBrace",
            Self::LBracket => "LBracket",
            Self::RBracket => "RBracket",
            Self::Error => "Error",
        }
    }
}

/// A token as a byte range `[start, end)` into the source it was lexed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    /// The exact source text of this token.
    #[must_use]
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
}

fn take_while(bytes: &[u8], mut pos: usize, pred: impl Fn(u8) -> bool) -> usize {
    while pos < bytes.len() && pred(bytes[pos]) {
        pos += 1;
    }
    pos
}

/// Lex `src` into a gap-free token sequence covering every byte.
#[must_use]
pub fn lex(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < bytes.len() {
        let start = pos;
        let c = bytes[pos];
        let kind = if c.is_ascii_whitespace() {
            pos = take_while(bytes, pos, |b| b.is_ascii_whitespace());
            TokenKind::Whitespace
        } else if c == b'/' && bytes.get(pos + 1) == Some(&b'/') {
            pos = take_while(bytes, pos, |b| b != b'\n');
            TokenKind::LineComment
        } else if c.is_ascii_alphabetic() || c == b'_' {
            pos = take_while(bytes, pos, |b| b.is_ascii_alphanumeric() || b == b'_');
            TokenKind::Ident
        } else if c.is_ascii_digit() {
            pos = take_while(bytes, pos, |b| b.is_ascii_digit());
            TokenKind::Int
        } else {
            let punct = match c {
                b':' => Some(TokenKind::Colon),
                b',' => Some(TokenKind::Comma),
                b'(' => Some(TokenKind::LParen),
                b')' => Some(TokenKind::RParen),
                b'>' => Some(TokenKind::Gt),
                b'=' => Some(TokenKind::Eq),
                b'+' => Some(TokenKind::Plus),
                b'-' => Some(TokenKind::Minus),
                b'*' => Some(TokenKind::Star),
                b'<' => Some(TokenKind::Lt),
                b'!' => Some(TokenKind::Bang),
                b'&' => Some(TokenKind::Amp),
                b'.' => Some(TokenKind::Dot),
                b';' => Some(TokenKind::Semi),
                b'{' => Some(TokenKind::LBrace),
                b'}' => Some(TokenKind::RBrace),
                b'[' => Some(TokenKind::LBracket),
                b']' => Some(TokenKind::RBracket),
                _ => None,
            };
            if let Some(kind) = punct {
                pos += 1;
                kind
            } else {
                // `pos` is always on a char boundary here: every branch above
                // consumes only ASCII bytes.
                pos += src[pos..].chars().next().map_or(1, char::len_utf8);
                TokenKind::Error
            }
        };
        out.push(Token {
            kind,
            start,
            end: pos,
        });
    }
    out
}

/// Deterministic debug print: one `Kind start..end "escaped text"` per line.
/// This is the golden-snapshot format for the token stream.
#[must_use]
pub fn dump(src: &str, tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        let _ = writeln!(
            out,
            "{} {}..{} {:?}",
            token.kind.name(),
            token.start,
            token.end,
            token.text(src)
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny deterministic PRNG so property tests need no dependency.
    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn assert_lossless(src: &str) {
        let tokens = lex(src);
        let mut expect_start = 0;
        let mut rebuilt = String::new();
        for token in &tokens {
            assert_eq!(token.start, expect_start, "gap/overlap in {src:?}");
            assert!(token.end > token.start, "empty token in {src:?}");
            assert!(
                src.is_char_boundary(token.start) && src.is_char_boundary(token.end),
                "token splits a char in {src:?}"
            );
            rebuilt.push_str(token.text(src));
            expect_start = token.end;
        }
        assert_eq!(expect_start, src.len(), "uncovered tail in {src:?}");
        assert_eq!(rebuilt, src, "round-trip mismatch");
    }

    #[test]
    fn bootstrap_golden_token_stream() {
        let src = "f add(a:i64,b:i64)>i64=a+b\n";
        assert_eq!(
            dump(src, &lex(src)),
            "\
Ident 0..1 \"f\"
Whitespace 1..2 \" \"
Ident 2..5 \"add\"
LParen 5..6 \"(\"
Ident 6..7 \"a\"
Colon 7..8 \":\"
Ident 8..11 \"i64\"
Comma 11..12 \",\"
Ident 12..13 \"b\"
Colon 13..14 \":\"
Ident 14..17 \"i64\"
RParen 17..18 \")\"
Gt 18..19 \">\"
Ident 19..22 \"i64\"
Eq 22..23 \"=\"
Ident 23..24 \"a\"
Plus 24..25 \"+\"
Ident 25..26 \"b\"
Whitespace 26..27 \"\\n\"
"
        );
    }

    #[test]
    fn trivia_is_preserved_not_dropped() {
        let src = "// lead\nf x()>i64=1 // tail";
        let kinds: Vec<_> = lex(src).iter().map(|t| t.kind).collect();
        assert_eq!(kinds.first(), Some(&TokenKind::LineComment));
        assert_eq!(kinds.last(), Some(&TokenKind::LineComment));
        assert!(lex(src).iter().any(|t| t.kind == TokenKind::Whitespace));
        assert_lossless(src);
    }

    #[test]
    fn unknown_input_becomes_one_error_token_per_char() {
        let src = "a𝑥b";
        let tokens = lex(src);
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [TokenKind::Ident, TokenKind::Error, TokenKind::Ident]
        );
        assert_eq!(tokens[1].text(src), "𝑥");
        assert_lossless(src);
    }

    #[test]
    fn edge_cases_are_lossless() {
        for src in [
            "",
            " ",
            "/",
            "//",
            "// c",
            "a//",
            "\r\n",
            "\u{0}",
            "é",
            "😀😀",
            "f a(b:i64)>i64=\u{FEFF}b",
            "99999999999999999999999",
            "\t\n\r  ",
            "/ /",
        ] {
            assert_lossless(src);
        }
    }

    #[test]
    fn random_text_is_always_lossless_and_never_panics() {
        const ALPHABET: &[&str] = &[
            "f", "a", "_", "0", "9", " ", "\n", "\r", "\t", "/", "//", "(", ")", ":", ",", ">",
            "=", "+", "-", "é", "𝑥", "😀", "\u{0}", "\u{7f}", "i64",
        ];
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        for _ in 0..20_000 {
            let len = (rng.next() % 40) as usize;
            let src: String = (0..len)
                .map(|_| ALPHABET[(rng.next() % ALPHABET.len() as u64) as usize])
                .collect();
            assert_lossless(&src);
        }
    }

    #[test]
    fn random_bytes_lossily_decoded_are_lossless() {
        let mut rng = XorShift(0xD1B5_4A32_D192_ED03);
        for _ in 0..5_000 {
            let len = (rng.next() % 64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
            assert_lossless(&String::from_utf8_lossy(&bytes));
        }
    }

    #[test]
    fn provisional_punctuation_lexes_as_single_char_tokens() {
        let src = "-*<!&.;{}[]";
        let kinds: Vec<_> = lex(src).iter().map(|t| t.kind).collect();
        assert_eq!(kinds.len(), src.len());
        assert!(!kinds.contains(&TokenKind::Error));
        // multi-char operators are NOT lexemes: `->` is two tokens
        let arrow: Vec<_> = lex("->").iter().map(|t| t.kind).collect();
        assert_eq!(arrow, [TokenKind::Minus, TokenKind::Gt]);
    }

    #[test]
    fn all_lists_every_kind_once() {
        let mut names: Vec<_> = TokenKind::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TokenKind::ALL.len());
    }
}
