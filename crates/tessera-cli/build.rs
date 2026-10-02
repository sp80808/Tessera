//! Records which Tessera commit `tsr` was built from, so `tsr witness`
//! evidence names the exact compiler that produced it (issue #41).
//!
//! `TSR_GIT_COMMIT` / `TSR_GIT_DIRTY` override the probe for builds without a
//! `.git` directory (source tarballs, vendored copies). Without either, the
//! commit is reported as `unknown` rather than guessed.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-env-changed=TSR_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=TSR_GIT_DIRTY");

    // Re-probe whenever HEAD moves or the tree changes, so the recorded
    // commit and dirty flag are never stale.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let git_dir = Path::new(&git_dir);
        for file in ["HEAD", "index", "packed-refs"] {
            println!("cargo:rerun-if-changed={}", git_dir.join(file).display());
        }
        if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!(
                "cargo:rerun-if-changed={}",
                git_dir.join(head_ref).display()
            );
        }
    }
    if let Some(top) = git(&["rev-parse", "--show-toplevel"]) {
        let top = Path::new(&top);
        for dir in ["crates", "Cargo.toml", "Cargo.lock"] {
            println!("cargo:rerun-if-changed={}", top.join(dir).display());
        }
    }

    let commit = std::env::var("TSR_GIT_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let dirty = std::env::var("TSR_GIT_DIRTY").ok().unwrap_or_else(|| {
        match git(&["status", "--porcelain", "--untracked-files=no"]) {
            Some(status) => (!status.is_empty()).to_string(),
            None => "unknown".to_owned(),
        }
    });
    println!("cargo:rustc-env=TSR_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=TSR_GIT_DIRTY={dirty}");
}
