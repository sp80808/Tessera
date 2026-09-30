use std::{env, ffi::OsString, fs, process::ExitCode};

use tessera_db::{Database, SourceFile, byte_len, line_count, parse, source_units};
use tessera_phases::{DiagnosticSet, Severity};
use tessera_syntax::{expand, fmt, lexer};

mod pipeline;

use pipeline::Input;

fn usage() -> ExitCode {
    eprintln!(
        "usage: tsr <file.tes> | tsr fmt <file.tes> | tsr tir <file.tes>\n       \
         tsr tokens <file.tes>   (scaffold: lossless token dump)\n       \
         tsr check <file.tes>    (scaffold: syntax diagnostics only; no name/type checking yet)\n       \
         tsr mir --overflow=wrapping|trapping <file.tes|file.tir>\n       \
         tsr run --overflow=wrapping|trapping <file.tes|file.tir> [FUNCTION] [ARG...]\n       \
         \x20        (reference MIR interpreter; FUNCTION is required when the file has several)\n       \
         `.tir` files are read as TIR, anything else as TC. `--overflow` has no default:\n       \
         integer overflow semantics are open question O1."
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
    let db = Database::default();
    let file = SourceFile::new(
        &db,
        path.to_string_lossy().into_owned(),
        text.to_owned(),
    );
    let out = parse(&db, file);
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

/// `tsr mir` / `tsr run`: `--overflow` may appear anywhere; everything else
/// is positional (`FILE [FUNCTION] [ARG...]`, only `run` takes the rest).
fn run_lowering(command: &str, rest: Vec<OsString>) -> ExitCode {
    let mut overflow = None;
    let mut positional = Vec::new();
    let mut rest = rest.into_iter();
    while let Some(arg) = rest.next() {
        let text = arg.to_string_lossy().into_owned();
        let value = if let Some(v) = text.strip_prefix("--overflow=") {
            v.to_owned()
        } else if text == "--overflow" {
            let Some(v) = rest.next() else {
                return usage();
            };
            v.to_string_lossy().into_owned()
        } else {
            positional.push(arg);
            continue;
        };
        let Some(mode) = pipeline::parse_overflow(&value) else {
            eprintln!("tsr: {}", pipeline::OVERFLOW_REQUIRED);
            return ExitCode::from(2);
        };
        overflow = Some(mode);
    }
    let Some(overflow) = overflow else {
        eprintln!("tsr: {}", pipeline::OVERFLOW_REQUIRED);
        return ExitCode::from(2);
    };
    let mut positional = positional.into_iter();
    let Some(path) = positional.next() else {
        return usage();
    };
    let extra: Vec<String> = positional
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if command == "mir" && !extra.is_empty() {
        return usage();
    }
    let text = match read_source(&path) {
        Ok(text) => text,
        Err(code) => return code,
    };
    let path = path.to_string_lossy().into_owned();
    let mir = match pipeline::lower(&path, &text, Input::from_path(&path), overflow) {
        Ok(mir) => mir,
        Err(rendered) => {
            eprintln!("{}", rendered.trim_end());
            return ExitCode::from(1);
        }
    };
    if command == "mir" {
        print!("{}", tessera_mir::dump(&mir));
        return ExitCode::SUCCESS;
    }
    // `run [FUNCTION] [ARG...]`: FUNCTION is present iff the first extra
    // argument names a function of the module.
    let (name, args) = match extra.split_first() {
        Some((first, args)) if mir.funcs.iter().any(|f| &f.name == first) => {
            (Some(first.as_str()), args)
        }
        _ => (None, extra.as_slice()),
    };
    let result = pipeline::entry(&mir, name).and_then(|f| pipeline::run(&mir, f, args));
    match result {
        Ok(value) => {
            println!("{value}");
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
    if first == "mir" || first == "run" {
        let command = first.to_string_lossy().into_owned();
        return run_lowering(&command, args.collect());
    }
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
        let db = Database::default();
        let file = SourceFile::new(&db, "t.tes".to_owned(), text.to_owned());
        let out = parse(&db, file);
        let rendered = render("t.tes", text, &out.diagnostics);
        assert_eq!(
            rendered,
            "t.tes:2:1: error[E-syntax-expected]: expected expression, found end of input\n"
        );
    }
}
