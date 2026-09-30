//! Invariants of `tessera_syntax::lexer::lex` (LEX-1..3).

use tessera_syntax::lexer::{self, Token, TokenKind};

use crate::show;
use crate::stats::{Counter, hit};

/// Entry point for arbitrary bytes.
///
/// Invalid UTF-8 is the driver's job (compiler-phases.md, B0): the strict path
/// simply returns for it. The lossy decoding of the same bytes is checked too,
/// because editors may pass lossily decoded text to the compiler.
pub fn check_bytes(data: &[u8]) {
    hit(Counter::LexRuns);
    match std::str::from_utf8(data) {
        Ok(src) => {
            hit(Counter::LexUtf8Valid);
            check_text(src);
        }
        Err(_) => {
            hit(Counter::LexUtf8Invalid);
            let lossy = String::from_utf8_lossy(data);
            hit(Counter::LexLossyChecked);
            check_text(&lossy);
        }
    }
}

/// The expected spelling of a fixed-spelling token kind.
fn fixed_spelling(kind: TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::Colon => ":",
        TokenKind::Comma => ",",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::Gt => ">",
        TokenKind::Eq => "=",
        TokenKind::Plus => "+",
        TokenKind::Minus => "-",
        TokenKind::Star => "*",
        TokenKind::Lt => "<",
        TokenKind::Bang => "!",
        TokenKind::Amp => "&",
        TokenKind::Dot => ".",
        TokenKind::Semi => ";",
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::Whitespace
        | TokenKind::LineComment
        | TokenKind::Ident
        | TokenKind::Int
        | TokenKind::Error => return None,
    })
}

fn starts_a_known_lexeme(c: char) -> bool {
    c.is_ascii_whitespace()
        || c.is_ascii_alphanumeric()
        || c == '_'
        || ":,()>=+-*<!&.;{}[]".contains(c)
}

/// What each token kind's text must look like (the lexer's own grammar).
fn check_kind_shape(src: &str, token: &Token, text: &str) {
    let id = "LEX-shape";
    match token.kind {
        TokenKind::Whitespace => invariant!(
            id,
            text.bytes().all(|b| b.is_ascii_whitespace()),
            "whitespace token {token:?} has non-whitespace text in {}",
            show(src)
        ),
        TokenKind::LineComment => invariant!(
            id,
            text.starts_with("//") && !text.contains('\n'),
            "comment token {token:?} malformed in {}",
            show(src)
        ),
        TokenKind::Ident => invariant!(
            id,
            text.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "ident token {token:?} malformed in {}",
            show(src)
        ),
        TokenKind::Int => invariant!(
            id,
            !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()),
            "int token {token:?} malformed in {}",
            show(src)
        ),
        TokenKind::Error => {
            let mut chars = text.chars();
            let first = chars.next();
            invariant!(
                id,
                first.is_some() && chars.next().is_none(),
                "error token {token:?} is not exactly one char in {}",
                show(src)
            );
            invariant!(
                id,
                first.is_some_and(|c| !starts_a_known_lexeme(c)),
                "error token {token:?} spells a known lexeme in {}",
                show(src)
            );
            // `/` is an error character only when it does not start `//`.
            if text == "/" {
                invariant!(
                    id,
                    !src[token.end..].starts_with('/'),
                    "lone `/` error token before another `/` in {}",
                    show(src)
                );
            }
        }
        punct => invariant!(
            id,
            fixed_spelling(punct) == Some(text),
            "punctuation token {token:?} has spelling {text:?} in {}",
            show(src)
        ),
    }
}

/// Maximal munch: runs of the same "run" kind are never split, and a comment
/// always stops in front of a line break.
fn check_adjacent(src: &str, prev: &Token, next: &Token) {
    let id = "LEX-maximal-munch";
    let same_run = matches!(
        (prev.kind, next.kind),
        (TokenKind::Whitespace, TokenKind::Whitespace)
            | (TokenKind::Ident, TokenKind::Ident)
            | (TokenKind::Ident, TokenKind::Int)
            | (TokenKind::Int, TokenKind::Int)
            | (TokenKind::LineComment, TokenKind::LineComment)
    );
    invariant!(
        id,
        !same_run,
        "tokens {prev:?} and {next:?} should have been one token in {}",
        show(src)
    );
    if prev.kind == TokenKind::LineComment {
        invariant!(
            id,
            next.kind == TokenKind::Whitespace && src[next.start..].starts_with('\n'),
            "comment {prev:?} not followed by a line break ({next:?}) in {}",
            show(src)
        );
    }
}

/// All lexer invariants for one text.
pub fn check_text(src: &str) {
    let tokens = lexer::lex(src);
    invariant!(
        "LEX-determinism",
        tokens == lexer::lex(src),
        "two lexes of {} differ",
        show(src)
    );
    if !tokens.is_empty() {
        hit(Counter::LexNonEmpty);
    }

    let mut cursor = 0usize;
    let mut rebuilt = String::with_capacity(src.len());
    for (i, token) in tokens.iter().enumerate() {
        invariant!(
            "LEX-1-contiguous",
            token.start == cursor,
            "token #{i} {token:?} starts at {} but previous ended at {cursor} in {}",
            token.start,
            show(src)
        );
        invariant!(
            "LEX-1-nonempty",
            token.end > token.start,
            "token #{i} {token:?} is empty in {}",
            show(src)
        );
        invariant!(
            "LEX-1-in-bounds",
            token.end <= src.len(),
            "token #{i} {token:?} ends past {} in {}",
            src.len(),
            show(src)
        );
        invariant!(
            "LEX-1-char-boundary",
            src.is_char_boundary(token.start) && src.is_char_boundary(token.end),
            "token #{i} {token:?} splits a char in {}",
            show(src)
        );
        let text = &src[token.start..token.end];
        check_kind_shape(src, token, text);
        if let Some(prev) = i.checked_sub(1).map(|p| &tokens[p]) {
            check_adjacent(src, prev, token);
        }
        rebuilt.push_str(text);
        cursor = token.end;
    }
    invariant!(
        "LEX-1-covers-input",
        cursor == src.len(),
        "tokens end at {cursor} but input has {} bytes: {}",
        src.len(),
        show(src)
    );
    invariant!(
        "LEX-1-lossless",
        rebuilt == src,
        "concatenated token texts differ from input {}",
        show(src)
    );
    invariant!(
        "LEX-empty",
        tokens.is_empty() == src.is_empty(),
        "token list emptiness does not match input emptiness for {}",
        show(src)
    );

    // The golden-snapshot printer must be total and one line per token
    // (`{:?}` escapes every control character, so no token text adds lines).
    let dump = lexer::dump(src, &tokens);
    invariant!(
        "LEX-dump",
        dump.lines().count() == tokens.len(),
        "dump has {} lines for {} tokens of {}",
        dump.lines().count(),
        tokens.len(),
        show(src)
    );
}
