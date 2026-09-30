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

/// A deliberately trivial derived query that depends on other queries.
#[salsa::tracked(returns(copy))]
pub fn source_units(db: &dyn Db, file: SourceFile) -> usize {
    byte_len(db, file) + line_count(db, file)
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

        file.set_text(&mut db).to("f a()=1\nf b()=2".to_owned());

        assert_eq!(line_count(&db, file), 2);
        assert_ne!(source_units(&db, file), before);
    }

    #[test]
    fn empty_source_has_zero_lines() {
        let db = Database::default();
        let file = SourceFile::new(&db, "empty.tes".to_owned(), String::new());

        assert_eq!(byte_len(&db, file), 0);
        assert_eq!(line_count(&db, file), 0);
        assert_eq!(source_units(&db, file), 0);
    }
}
