use std::{env, fs, path::Path, process::ExitCode};

use tessera_context::symbol_summaries;
use tessera_db::{Database, SourceFile, byte_len, line_count, source_units};
use tessera_sema::check;
use tessera_syntax::{Program, canonical_format, parse};
use tessera_tir::{lower, render};

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();

    let Some(first) = args.next() else {
        usage();
        return ExitCode::from(2);
    };

    let (command, path) = match first.to_str() {
        Some("check" | "fmt" | "tir" | "ctx") => {
            let Some(path) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            (first.to_string_lossy().into_owned(), path)
        }
        _ => ("check".to_owned(), first),
    };

    if args.next().is_some() {
        usage();
        return ExitCode::from(2);
    }

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("tsr: failed to read {:?}: {error}", path);
            return ExitCode::from(1);
        }
    };

    let parsed = parse(&text);
    if !report_syntax_errors(Path::new(&path), &parsed) {
        return ExitCode::from(1);
    }

    match command.as_str() {
        "fmt" => {
            print!("{}", canonical_format(&parsed));
            ExitCode::SUCCESS
        }
        "tir" => {
            let tir = lower(&parsed);
            print!("{}", render(&tir));
            ExitCode::SUCCESS
        }
        "ctx" => {
            let tir = lower(&parsed);
            for symbol in symbol_summaries(&tir) {
                println!(
                    "{} {} @{}..{}",
                    symbol.name, symbol.signature, symbol.source_start, symbol.source_end
                );
            }
            ExitCode::SUCCESS
        }
        "check" => check_file(Path::new(&path), text, &parsed),
        _ => unreachable!("command was normalized above"),
    }
}

fn check_file(path: &Path, text: String, parsed: &Program) -> ExitCode {
    let db = Database::default();
    let file = SourceFile::new(&db, path.display().to_string(), text);
    let tir = lower(parsed);
    let diagnostics = check(&tir);

    if diagnostics.is_empty() {
        println!(
            "ok: {} functions | {} bytes | {} lines | {} bootstrap units",
            tir.functions.len(),
            byte_len(&db, file),
            line_count(&db, file),
            source_units(&db, file)
        );
        ExitCode::SUCCESS
    } else {
        for diagnostic in diagnostics {
            eprintln!(
                "{}:{}..{}: {}",
                path.display(),
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.message
            );
        }
        ExitCode::from(1)
    }
}

fn report_syntax_errors(path: &Path, parsed: &Program) -> bool {
    if parsed.errors.is_empty() {
        return true;
    }

    for error in &parsed.errors {
        eprintln!(
            "{}:{}..{}: syntax: {}",
            path.display(),
            error.span.start,
            error.span.end,
            error.message
        );
    }
    false
}

fn usage() {
    eprintln!("usage: tsr [check|fmt|tir|ctx] <file.tes>");
    eprintln!("note: the accepted bootstrap grammar is provisional and not a stable Tessera spec");
}
