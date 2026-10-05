use std::{env, ffi::OsString, fs, process::ExitCode};

use tessera_db::{Database, SourceFile, byte_len, line_count, source_units};
use tessera_phases::{DiagnosticSet, Severity};
use tessera_syntax::{fmt, lexer};

mod advice;
mod grammar;
mod pipeline;
mod witness;

use pipeline::Input;

fn usage() -> ExitCode {
    eprintln!(
        "usage: tsr <file.tes> | tsr fmt <file.tes> | tsr tir <file.tes>\n       \
         tsr tokens <file.tes>   (scaffold: lossless token dump)\n       \
         tsr check <file.tes>    (syntax, name and type diagnostics, all at once)\n       \
         tsr witness [--phase=check|mir|backend] [--overflow=wrapping|trapping] <file>\n       \
         \x20        (one run as tessera.witness/v0 JSON evidence on stdout; exit 0 pass,\n       \
         \x20         1 fail, 3 unsupported phase, 4 tool error)\n       \
         tsr grammar [--format=ebnf|gbnf|lark]   (the TC grammar, for prompts and\n       \
         \x20        constrained decoding)\n       \
         tsr --version\n       \
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

/// `render`, with the foreign-syntax summary first and each diagnostic's
/// `help` line from [`advice`].
fn render_with_help(path: &str, text: &str, diagnostics: &DiagnosticSet) -> String {
    let report = advice::report(text, diagnostics);
    let mut out = String::new();
    if let Some((start, _, message)) = &report.foreign {
        let (line, col) = line_col(text, *start);
        out.push_str(&format!(
            "{path}:{line}:{col}: error[{}]: {message}\n  help: a TC program is one function: `{}`\n",
            advice::FOREIGN_CODE,
            advice::TEMPLATE
        ));
    }
    let rendered = render(path, text, diagnostics);
    for (line, advice) in rendered.lines().zip(&report.advice) {
        out.push_str(line);
        out.push('\n');
        if let Some(help) = &advice.help {
            out.push_str(&format!("  help: {help}\n"));
        }
    }
    out
}

fn run_check(text: &str, path: &std::ffi::OsStr) -> ExitCode {
    let out = pipeline::check_tc(text);
    eprint!(
        "{}",
        render_with_help(&path.to_string_lossy(), text, &out.diagnostics)
    );
    if out.diagnostics.has_errors() {
        let report = advice::report(text, &out.diagnostics);
        for s in advice::suggestions(text, &report.advice) {
            eprint!("  suggestion ({}; passes check): {}", s.label, s.source);
        }
    }
    if out.diagnostics.has_errors() {
        ExitCode::from(1)
    } else {
        let n = out.value.module.funcs.len();
        println!("ok: {n} function(s) checked (syntax, names, types)");
        ExitCode::SUCCESS
    }
}

/// `tsr tir`: TC through the phases to TIR text.
fn run_tir(text: &str, path: &std::ffi::OsStr) -> ExitCode {
    let path = path.to_string_lossy();
    match pipeline::load_tir(&path, text, Input::Tc) {
        Ok((module, _)) => {
            println!("{}", module.to_text());
            ExitCode::SUCCESS
        }
        Err(rendered) => {
            eprintln!("{}", rendered.trim_end());
            ExitCode::from(1)
        }
    }
}

fn run_fmt(text: &str) -> ExitCode {
    match fmt(text) {
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

/// `tsr witness [--phase=P] [--overflow=MODE] FILE`.
fn run_witness(rest: Vec<OsString>) -> ExitCode {
    let mut phase = "check".to_owned();
    let mut overflow = None;
    let mut path = None;
    let mut rest = rest.into_iter();
    while let Some(arg) = rest.next() {
        let text = arg.to_string_lossy().into_owned();
        let (flag, inline) = match text.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_owned(), Some(value.to_owned()))
            }
            _ => (text.clone(), None),
        };
        if flag != "--phase" && flag != "--overflow" {
            if path.replace(arg).is_some() {
                return usage();
            }
            continue;
        }
        let Some(value) = inline.or_else(|| rest.next().map(|v| v.to_string_lossy().into_owned()))
        else {
            return usage();
        };
        if flag == "--phase" {
            phase = value;
        } else {
            let Some(mode) = pipeline::parse_overflow(&value) else {
                eprintln!("tsr: {}", pipeline::OVERFLOW_REQUIRED);
                return ExitCode::from(2);
            };
            overflow = Some(mode);
        }
    }
    let Some(path) = path else {
        return usage();
    };
    let target = match witness::Target::parse(&phase, overflow) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("tsr: {error}");
            return ExitCode::from(2);
        }
    };
    let source =
        fs::read_to_string(&path).map_err(|error| format!("failed to read input: {error}"));
    let display = path.to_string_lossy();
    let (outcome, doc) =
        witness::witness(&display, source.as_deref().map_err(Clone::clone), target);
    match serde_json::to_string_pretty(&doc) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("tsr: failed to serialize witness: {error}");
            return ExitCode::from(witness::Outcome::ToolError.exit_code());
        }
    }
    ExitCode::from(outcome.exit_code())
}

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();

    let Some(first) = args.next() else {
        return usage();
    };
    if first == "--version" || first == "-V" {
        println!("{}", witness::version_line());
        return ExitCode::SUCCESS;
    }
    if first == "grammar" {
        let format = match args.next() {
            None => "ebnf".to_owned(),
            Some(arg) => match arg.to_string_lossy().strip_prefix("--format=") {
                Some(format) => format.to_owned(),
                None => return usage(),
            },
        };
        if args.next().is_some() {
            return usage();
        }
        let Some(text) = grammar::grammar(&format) else {
            eprintln!("tsr: unknown grammar format `{format}` (expected ebnf, gbnf or lark)");
            return ExitCode::from(2);
        };
        print!("{text}");
        return ExitCode::SUCCESS;
    }
    if first == "witness" {
        return run_witness(args.collect());
    }
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
            Some("tir") => run_tir(&text, &path),
            _ => run_fmt(&text),
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
    use tessera_phases::FileId;
    use tessera_syntax::cst::parse_file;

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
