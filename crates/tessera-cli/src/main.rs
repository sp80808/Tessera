use std::{env, fs, process::ExitCode};

use tessera_db::{Database, SourceFile, byte_len, line_count, source_units};
use tessera_syntax::{expand, fmt};

fn usage() -> ExitCode {
    eprintln!("usage: tsr <file.tes> | tsr fmt <file.tes> | tsr tir <file.tes>");
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
    if first == "fmt" || first == "tir" {
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
        return run_expand(&text, first == "tir");
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
