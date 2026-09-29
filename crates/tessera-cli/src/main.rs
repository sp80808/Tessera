use std::{env, fs, process::ExitCode};

use tessera_db::{byte_len, line_count, source_units, Database, SourceFile};

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();

    let Some(path) = args.next() else {
        eprintln!("usage: tsr <file.tes>");
        return ExitCode::from(2);
    };

    if args.next().is_some() {
        eprintln!("usage: tsr <file.tes>");
        return ExitCode::from(2);
    }

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("tsr: failed to read {:?}: {error}", path);
            return ExitCode::from(1);
        }
    };

    let db = Database::default();
    let file = SourceFile::new(&db, path.to_string_lossy().into_owned(), text);

    println!(
        "{} bytes | {} lines | {} bootstrap units",
        byte_len(&db, file),
        line_count(&db, file),
        source_units(&db, file)
    );

    ExitCode::SUCCESS
}
