use std::{env, fs, process::ExitCode};

use tessera_db::{Database, SourceFile, byte_len, line_count, source_units};
use tessera_phases::{DiagnosticSet, FileId, Severity};
use tessera_syntax::{cst::parse_file, expand, fmt, lexer};

fn usage() -> ExitCode {
    eprintln!(
        "usage: tsr <file.tes> | tsr fmt <file.tes> | tsr tir <file.tes>\n       \
         tsr tokens <file.tes>   (scaffold: lossless token dump)\n       \
         tsr check <file.tes>    (scaffold: syntax diagnostics only; no name/type checking yet)"
    );
    ExitCode::from(2)
}

fn read_source(path: &std::ffi::OsStr) -> Result<String, ExitCode> {
    fs::read_to_string(path).map_err(|error| {
        eprintln!("tsr: failed to read {path:?}: {error}");
        ExitCode::from(1)
    })
}

fn run_stats(text: &str, path: &std::ffi::OsStr) -> ExitCode {
    let db = Database::default();
    let file = SourceFile::new(&db, path.to_string_lossy().into_owned(), text.to_owned());

    println!(
        "{} bytes | {} lines | {} bootstrap units",
        byte_len(&db, file),
        line_count(&db, file),
        source_units(&db, file)
    );
    ExitCode::SUCCESS
}

/// 1-based line and column (in bytes) of `offset`; driver-side rendering only.
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text.as_bytes()[..offset];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let col = offset
        - before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1)
        + 1;
    (line, col)
}

fn render(path: &str, text: &str, diagnostics: &DiagnosticSet) -> String {
    let mut out = String::new();
    for d in diagnostics.iter() {
        let span = d.at.primary_span();
        let (line, col) = line_col(text, span.start as usize);
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        };
        out.push_str(&format!(
            "{path}:{line}:{col}: {sev}[{}]: {}\n",
            d.code, d.message
        ));
    }
    out
}

fn run_tokens(text: &str) -> ExitCode {
    print!("{}", lexer::dump(text, &lexer::lex(text)));
    ExitCode::SUCCESS
}

fn run_check(text: &str, path: &std::ffi::OsStr) -> ExitCode {
    let out = parse_file(FileId(0), text);
    eprint!(
        "{}",
        render(&path.to_string_lossy(), text, &out.diagnostics)
    );
    if out.diagnostics.has_errors() {
        ExitCode::from(1)
    } else {
        println!("ok: syntax only ({} tokens)", out.value.tokens.len());
        ExitCode::SUCCESS
    }
}

fn run_expand(text: &str, as_tir: bool) -> ExitCode {
    let result = if as_tir { expand(text) } else { fmt(text) };
    match result {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("tsr: {error}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();

    let Some(first) = args.next() else {
        return usage();
    };
    if first == "fmt" || first == "tir" || first == "tokens" || first == "check" {
        let Some(path) = args.next() else {
            return usage();
        };
        if args.next().is_some() {
            return usage();
        }
        let text = match read_source(&path) {
            Ok(text) => text,
            Err(code) => return code,
        };
        return match first.to_str() {
            Some("tokens") => run_tokens(&text),
            Some("check") => run_check(&text, &path),
            _ => run_expand(&text, first == "tir"),
        };
    }
    if args.next().is_some() {
        return usage();
    }
    let text = match read_source(&first) {
        Ok(text) => text,
        Err(code) => return code,
    };
    run_stats(&text, &first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_is_one_based_and_counts_bytes() {
        let text = "ab\ncd\n";
        assert_eq!(line_col(text, 0), (1, 1));
        assert_eq!(line_col(text, 1), (1, 2));
        assert_eq!(line_col(text, 3), (2, 1));
        assert_eq!(line_col(text, 99), (3, 1));
    }

    #[test]
    fn check_renders_recovered_diagnostics_in_source_order() {
        let text = "f x()>i64=\n";
        let out = parse_file(FileId(0), text);
        let rendered = render("t.tes", text, &out.diagnostics);
        assert_eq!(
            rendered,
            "t.tes:2:1: error[E-syntax-expected]: expected expression, found end of input\n"
        );
    }
}
