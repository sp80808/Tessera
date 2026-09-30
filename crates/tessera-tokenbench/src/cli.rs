//! Command-line front end. Returns exit codes instead of terminating the
//! process: 0 success, 1 the regression gate failed, 2 usage or I/O error.

use std::fs;
use std::path::{Path, PathBuf};

use crate::bench::{self, RunOptions};
use crate::check::{self, DEFAULT_MAX_REGRESSION_PCT};
use crate::{report_json, report_md};

pub const USAGE: &str = "\
tess-tokenbench: reproducible tokenizer benchmark for TC syntax candidates (issue #1)

USAGE:
  tess-tokenbench run [--corpus DIR] [--tokenizers-dir DIR] [--manifest FILE]
                      [--out-json PATH] [--out-md PATH] [--corpus-revision STR]
  tess-tokenbench check --baseline JSON --results JSON [--max-regression-pct N]

run    Benchmark the corpus; write deterministic JSON and Markdown.
       Defaults (relative to the working directory, normally the repo root):
         --corpus bench/corpus   --tokenizers-dir bench/tokenizers
         --manifest bench/tokenizers.json
         --out-json bench/results/latest.json   --out-md bench/results/latest.md
check  Exit 1 if any `parsed` tessera variant's median or worst token count grew
       by more than N percent (default 2), or if the corpus hash, tokenizer set
       or provenance differs from the baseline.

Exit codes: 0 ok, 1 gate failed, 2 usage or I/O error.
";

struct Flags {
    pairs: Vec<(String, String)>,
}

impl Flags {
    fn parse(args: &[String], allowed: &[&str]) -> Result<Self, String> {
        let mut pairs = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            let Some(flag) = arg.strip_prefix("--") else {
                return Err(format!("unexpected argument `{arg}`"));
            };
            let (name, value) = if let Some((n, v)) = flag.split_once('=') {
                (n.to_owned(), v.to_owned())
            } else {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| format!("flag `--{flag}` needs a value"))?;
                (flag.to_owned(), value.clone())
            };
            if !allowed.contains(&name.as_str()) {
                return Err(format!("unknown flag `--{name}`"));
            }
            pairs.push((name, value));
            i += 1;
        }
        Ok(Self { pairs })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn path(&self, name: &str, default: &str) -> PathBuf {
        PathBuf::from(self.get(name).unwrap_or(default))
    }
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn cmd_run(args: &[String]) -> Result<u8, String> {
    let flags = Flags::parse(
        args,
        &[
            "corpus",
            "tokenizers-dir",
            "manifest",
            "out-json",
            "out-md",
            "corpus-revision",
        ],
    )?;
    let opts = RunOptions {
        corpus_dir: flags.path("corpus", "bench/corpus"),
        manifest_path: flags.path("manifest", "bench/tokenizers.json"),
        tokenizers_dir: flags.path("tokenizers-dir", "bench/tokenizers"),
        corpus_revision: flags.get("corpus-revision").map(str::to_owned),
    };
    let results = bench::run(&opts)?;
    let out_json = flags.path("out-json", "bench/results/latest.json");
    let out_md = flags.path("out-md", "bench/results/latest.md");
    write_file(
        &out_json,
        &report_json::to_json(&results).to_pretty_string(),
    )?;
    write_file(&out_md, &report_md::to_markdown(&results))?;
    println!("corpus_hash {}", results.corpus_hash);
    println!(
        "tokenizers measured: {} ({} distinct vendor(s)); skipped: {}",
        results.measured_ids().len(),
        results.measured_vendors().len(),
        results.tokenizers.len() - results.measured_ids().len()
    );
    for t in results.tokenizers.iter().filter(|t| t.skipped.is_some()) {
        println!("  skipped {}: {}", t.id, t.skipped.as_deref().unwrap_or(""));
    }
    println!("wrote {}", out_json.display());
    println!("wrote {}", out_md.display());
    Ok(0)
}

fn cmd_check(args: &[String]) -> Result<u8, String> {
    let flags = Flags::parse(args, &["baseline", "results", "max-regression-pct"])?;
    let baseline = flags.get("baseline").ok_or("`check` needs --baseline")?;
    let results = flags.get("results").ok_or("`check` needs --results")?;
    let pct = match flags.get("max-regression-pct") {
        Some(s) => s
            .parse::<f64>()
            .map_err(|_| format!("--max-regression-pct `{s}` is not a number"))?,
        None => DEFAULT_MAX_REGRESSION_PCT,
    };
    let report = check::check_files(Path::new(baseline), Path::new(results), pct)?;
    if report.passed() {
        println!(
            "check passed: {} parsed tessera variant(s) within {pct}% of the baseline",
            report.checked
        );
        Ok(0)
    } else {
        eprintln!("check FAILED ({} problem(s)):", report.failures.len());
        for failure in &report.failures {
            eprintln!("  - {failure}");
        }
        Ok(1)
    }
}

/// Run the CLI on `args` (without the program name) and return the exit code.
#[must_use]
pub fn run_cli(args: &[String]) -> u8 {
    let outcome = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("check") => cmd_check(&args[1..]),
        Some("help" | "--help" | "-h") => {
            print!("{USAGE}");
            Ok(0)
        }
        _ => Err("expected a subcommand: `run` or `check` (try `help`)".to_owned()),
    };
    match outcome {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags_accept_both_spellings_and_reject_unknowns() {
        let f =
            Flags::parse(&args(&["--a", "1", "--b=2", "--a", "3"]), &["a", "b"]).expect("flags");
        assert_eq!(f.get("a"), Some("3"));
        assert_eq!(f.get("b"), Some("2"));
        assert!(Flags::parse(&args(&["--zzz", "1"]), &["a"]).is_err());
        assert!(Flags::parse(&args(&["--a"]), &["a"]).is_err());
        assert!(Flags::parse(&args(&["positional"]), &["a"]).is_err());
    }

    #[test]
    fn usage_errors_exit_2() {
        assert_eq!(run_cli(&args(&[])), 2);
        assert_eq!(run_cli(&args(&["bogus"])), 2);
        assert_eq!(run_cli(&args(&["check"])), 2);
        assert_eq!(
            run_cli(&args(&[
                "check",
                "--baseline",
                "a",
                "--results",
                "b",
                "--max-regression-pct",
                "x"
            ])),
            2
        );
    }
}
