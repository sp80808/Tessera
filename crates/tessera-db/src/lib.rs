//! Incremental semantic database bootstrap for Tessera.
//!
//! This crate intentionally starts smaller than the language. The first goal is
//! to prove the architecture: source files are explicit inputs and all derived
//! compiler/context state is a deterministic tracked query.

#[salsa::db]
pub trait Db: salsa::Database {}

#[salsa::db]
#[derive(Clone, Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl Db for Database {}

/// Mutable source input owned by the outer compiler driver.
#[salsa::input]
pub struct SourceFile {
    #[returns(deref)]
    pub path: String,
    #[returns(deref)]
    pub text: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct LexicalStats {
    tokens: usize,
    identifiers: usize,
    numbers: usize,
    punctuation: usize,
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn compound_punctuation_len(bytes: &[u8], index: usize) -> Option<usize> {
    const COMPOUND: [&[u8]; 11] = [
        b"..=", b"...", b"->", b"=>", b"::", b":=", b"==", b"!=", b"<=", b">=", b"&&",
    ];
    const COMPOUND_OR: &[u8] = b"||";

    for punct in COMPOUND {
        if bytes[index..].starts_with(punct) {
            return Some(punct.len());
        }
    }

    if bytes[index..].starts_with(COMPOUND_OR) {
        return Some(COMPOUND_OR.len());
    }

    None
}

fn lexical_stats(text: &str) -> LexicalStats {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut stats = LexicalStats::default();

    while index < bytes.len() {
        let byte = bytes[index];

        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        if is_ident_start(byte) {
            index += 1;
            while index < bytes.len() && is_ident_continue(bytes[index]) {
                index += 1;
            }
            stats.tokens += 1;
            stats.identifiers += 1;
            continue;
        }

        if byte.is_ascii_digit() {
            index += 1;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
                index += 1;
            }
            stats.tokens += 1;
            stats.numbers += 1;
            continue;
        }

        if let Some(len) = compound_punctuation_len(bytes, index) {
            index += len;
            stats.tokens += 1;
            stats.punctuation += 1;
            continue;
        }

        if byte.is_ascii_punctuation() {
            index += 1;
            stats.tokens += 1;
            stats.punctuation += 1;
            continue;
        }

        let next_char_len = text[index..]
            .chars()
            .next()
            .map_or(1, core::primitive::char::len_utf8);
        index += next_char_len;
        stats.tokens += 1;
    }

    stats
}

/// Smallest possible tracked source query.
#[salsa::tracked(returns(copy))]
pub fn byte_len(db: &dyn Db, file: SourceFile) -> usize {
    file.text(db).len()
}

/// Kept separate so query dependency/reuse behavior is easy to observe.
#[salsa::tracked(returns(copy))]
pub fn line_count(db: &dyn Db, file: SourceFile) -> usize {
    let text = file.text(db);
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

/// Bootstrap lexical-token count for syntax/tokenization experiments.
#[salsa::tracked(returns(copy))]
pub fn lexeme_count(db: &dyn Db, file: SourceFile) -> usize {
    lexical_stats(file.text(db)).tokens
}

#[salsa::tracked(returns(copy))]
pub fn identifier_count(db: &dyn Db, file: SourceFile) -> usize {
    lexical_stats(file.text(db)).identifiers
}

#[salsa::tracked(returns(copy))]
pub fn number_count(db: &dyn Db, file: SourceFile) -> usize {
    lexical_stats(file.text(db)).numbers
}

#[salsa::tracked(returns(copy))]
pub fn punctuation_count(db: &dyn Db, file: SourceFile) -> usize {
    lexical_stats(file.text(db)).punctuation
}

/// A deliberately trivial derived query that depends on other queries.
#[salsa::tracked(returns(copy))]
pub fn source_units(db: &dyn Db, file: SourceFile) -> usize {
    byte_len(db, file) + line_count(db, file) + lexeme_count(db, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use salsa::Setter;

    #[test]
    fn updating_source_invalidates_dependent_queries() {
        let mut db = Database::default();
        let file = SourceFile::new(&db, "a.tes".to_owned(), "f a()=1".to_owned());

        assert_eq!(line_count(&db, file), 1);
        let before = source_units(&db, file);
        let before_lexemes = lexeme_count(&db, file);

        file.set_text(&mut db).to("f a()=1\nf b()=2".to_owned());

        assert_eq!(line_count(&db, file), 2);
        assert_eq!(lexeme_count(&db, file), 12);
        assert_ne!(source_units(&db, file), before);
        assert_ne!(lexeme_count(&db, file), before_lexemes);
    }

    #[test]
    fn empty_source_has_zero_lines() {
        let db = Database::default();
        let file = SourceFile::new(&db, "empty.tes".to_owned(), String::new());

        assert_eq!(byte_len(&db, file), 0);
        assert_eq!(line_count(&db, file), 0);
        assert_eq!(lexeme_count(&db, file), 0);
        assert_eq!(identifier_count(&db, file), 0);
        assert_eq!(number_count(&db, file), 0);
        assert_eq!(punctuation_count(&db, file), 0);
        assert_eq!(source_units(&db, file), 0);
    }

    #[test]
    fn lexical_counts_match_bootstrap_rules() {
        let db = Database::default();
        let file = SourceFile::new(&db, "lex.tes".to_owned(), "f a(x)=x+1\nb:=a(2)".to_owned());

        assert_eq!(lexeme_count(&db, file), 15);
        assert_eq!(identifier_count(&db, file), 6);
        assert_eq!(number_count(&db, file), 2);
        assert_eq!(punctuation_count(&db, file), 7);
    }
}
