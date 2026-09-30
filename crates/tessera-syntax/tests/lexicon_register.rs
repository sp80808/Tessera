//! Keeps `docs/architecture/syntax-lexicon.md` and the lexer in sync.

use tessera_syntax::lexer::TokenKind;

fn register() -> String {
    let path = format!(
        "{}/../../docs/architecture/syntax-lexicon.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).expect("lexicon register exists")
}

#[test]
fn every_token_kind_has_a_register_row() {
    let doc = register();
    for kind in TokenKind::ALL {
        let row = format!("| `{}` |", kind.name());
        assert!(doc.contains(&row), "lexicon register is missing {row}");
    }
}

#[test]
fn register_rows_name_only_existing_kinds() {
    let doc = register();
    let known: Vec<_> = TokenKind::ALL.iter().map(|k| k.name()).collect();
    for line in doc.lines().filter(|l| l.starts_with("| `")) {
        let name = line
            .trim_start_matches("| `")
            .split('`')
            .next()
            .unwrap_or("");
        assert!(
            known.contains(&name),
            "register row for unknown kind `{name}`"
        );
    }
}

/// Recognizing a character in the lexer must not make the grammar accept it.
#[test]
fn recognized_but_ungrammatical_lexemes_are_rejected_by_the_parser() {
    for lexeme in ["-", "*", "<", "!", "&", ".", ";", "{", "}", "[", "]"] {
        let src = format!("f x()>i64=1{lexeme}1");
        assert!(
            tessera_syntax::parse(&src).is_err(),
            "grammar accepts {lexeme:?}"
        );
        assert!(
            tessera_syntax::cst::parse_file(tessera_phases::FileId(0), &src)
                .diagnostics
                .has_errors(),
            "cst accepts {lexeme:?}"
        );
    }
}
