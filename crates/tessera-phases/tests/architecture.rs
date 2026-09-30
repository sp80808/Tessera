//! Mechanical checks for `docs/architecture/compiler-phases.md` (issue #17).
//!
//! These read manifests and sources as text (no cargo invocation, no new
//! dependencies) and fail with the invariant ID from the phase contract.
//!
//! Adding a crate under `crates/` requires registering it in `LAYERS` *and*
//! placing it in the contract; that friction is deliberate.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Allowed intra-workspace dependencies per crate (contract INV-DEP-1).
/// Crates marked "planned" do not exist yet; their rows tell #19-#22 which
/// edges are legal when they are created.
const LAYERS: &[(&str, &[&str])] = &[
    ("tessera-phases", &[]),
    // TEMPORARY(#19): the bootstrap witness lowers AST -> TIR inside
    // tessera-syntax. Target row is ["tessera-phases"]; the CST crate must not
    // know TIR. Remove `tessera-tir` here when #19 moves lowering out.
    ("tessera-syntax", &["tessera-phases", "tessera-tir"]),
    ("tessera-tir", &["tessera-phases"]),
    ("tessera-tcap", &["tessera-phases", "tessera-tir"]),
    // planned
    ("tessera-hir", &["tessera-phases", "tessera-syntax"]),
    (
        "tessera-sema",
        &["tessera-phases", "tessera-hir", "tessera-tir"],
    ),
    ("tessera-mir", &["tessera-phases", "tessera-tir"]),
    (
        "tessera-codegen-cranelift",
        &["tessera-phases", "tessera-mir"],
    ),
    // dev tool, not a compiler phase: measures TC lexemes against real tokenizers
    ("tessera-tokenbench", &["tessera-phases", "tessera-syntax"]),
    (
        "tessera-context",
        &[
            "tessera-phases",
            "tessera-hir",
            "tessera-sema",
            "tessera-tir",
            "tessera-tcap",
        ],
    ),
    // query host and driver: may see the whole pipeline
    (
        "tessera-db",
        &[
            "tessera-phases",
            "tessera-syntax",
            "tessera-hir",
            "tessera-sema",
            "tessera-tir",
            "tessera-tcap",
            "tessera-mir",
            "tessera-codegen-cranelift",
        ],
    ),
    (
        "tessera-cli",
        &[
            "tessera-phases",
            "tessera-db",
            "tessera-syntax",
            "tessera-hir",
            "tessera-sema",
            "tessera-tir",
            "tessera-tcap",
            "tessera-mir",
            "tessera-codegen-cranelift",
            "tessera-context",
        ],
    ),
];

/// The only crate allowed to name backend libraries (INV-BACKEND-1).
const BACKEND_CRATE: &str = "tessera-codegen-cranelift";
const BACKEND_MARKERS: &[&str] = &["cranelift", "inkwell", "llvm-sys", "llvm_sys"];

/// Network/model clients are outside deterministic phases (INV-PURE-1).
const NETWORK_MARKERS: &[&str] = &[
    "reqwest",
    "hyper",
    "ureq",
    "tokio",
    "async-std",
    "isahc",
    "curl",
    "tungstenite",
    "async-openai",
    "rig-core",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

struct Krate {
    name: String,
    dir: PathBuf,
    deps: BTreeSet<String>,
}

fn crates() -> Vec<Krate> {
    let mut out = Vec::new();
    let mut dirs: Vec<_> = fs::read_dir(workspace_root().join("crates"))
        .expect("crates dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).expect("manifest");
        let (name, deps) = parse_manifest(&manifest);
        out.push(Krate { name, dir, deps });
    }
    out
}

/// Minimal manifest reader: package name plus every key in any
/// `dependencies`-flavoured table.
fn parse_manifest(text: &str) -> (String, BTreeSet<String>) {
    let mut name = String::new();
    let mut deps = BTreeSet::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix('[') {
            section = rest.trim_end_matches(']').trim_matches('[').to_owned();
            // `[dependencies.foo]` style
            if let Some((table, dep)) = section.split_once('.') {
                if table.ends_with("dependencies") {
                    deps.insert(dep.to_owned());
                }
            }
            continue;
        }
        if section == "package" {
            if let Some(v) = line.strip_prefix("name") {
                if let Some((_, v)) = v.split_once('=') {
                    name = v.trim().trim_matches('"').to_owned();
                }
            }
        } else if section.ends_with("dependencies") && !line.is_empty() && !line.starts_with('#') {
            if let Some((key, _)) = line.split_once('=') {
                deps.insert(key.trim().to_owned());
            }
        }
    }
    (name, deps)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("read_dir")
        .map(|e| e.expect("entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            // skip macOS AppleDouble sidecars (`._lib.rs`) created on external volumes
            && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("._"))
        {
            out.push(path);
        }
    }
}

#[test]
fn every_crate_is_registered_in_the_layer_table() {
    for krate in crates() {
        assert!(
            LAYERS.iter().any(|(n, _)| *n == krate.name),
            "INV-DEP-1: crate `{}` is not in LAYERS; place it in docs/architecture/compiler-phases.md first",
            krate.name
        );
    }
}

#[test]
fn workspace_dependency_direction_follows_the_contract() {
    for krate in crates() {
        let (_, allowed) = LAYERS
            .iter()
            .find(|(n, _)| *n == krate.name)
            .unwrap_or_else(|| panic!("unregistered crate {}", krate.name));
        for dep in krate.deps.iter().filter(|d| d.starts_with("tessera-")) {
            assert!(
                allowed.contains(&dep.as_str()),
                "INV-DEP-1: `{}` may not depend on `{}` (allowed: {:?})",
                krate.name,
                dep,
                allowed
            );
        }
    }
}

#[test]
fn layer_table_is_acyclic() {
    // Allowed edges must form a DAG so the table itself cannot legalize a cycle.
    fn visit(node: &str, stack: &mut Vec<String>) {
        assert!(
            !stack.iter().any(|s| s == node),
            "INV-DEP-1: cycle in LAYERS: {stack:?} -> {node}"
        );
        stack.push(node.to_owned());
        if let Some((_, deps)) = LAYERS.iter().find(|(n, _)| *n == node) {
            for dep in *deps {
                visit(dep, stack);
            }
        }
        stack.pop();
    }
    for (name, _) in LAYERS {
        visit(name, &mut Vec::new());
    }
}

#[test]
fn only_the_backend_crate_may_name_backend_libraries() {
    for krate in crates().into_iter().filter(|k| k.name != BACKEND_CRATE) {
        for dep in &krate.deps {
            assert!(
                !BACKEND_MARKERS.iter().any(|m| dep.contains(m)),
                "INV-BACKEND-1: `{}` depends on backend library `{dep}`",
                krate.name
            );
        }
        let mut files = Vec::new();
        rust_files(&krate.dir.join("src"), &mut files);
        for file in files {
            let text = fs::read_to_string(&file).expect("source").to_lowercase();
            for marker in BACKEND_MARKERS {
                assert!(
                    !text.contains(marker),
                    "INV-BACKEND-1: {} mentions `{marker}`; backend types must not leak into semantic crates",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn no_crate_depends_on_network_or_model_clients() {
    for krate in crates() {
        for dep in &krate.deps {
            assert!(
                !NETWORK_MARKERS.iter().any(|m| dep == m),
                "INV-PURE-1: `{}` depends on network/model client `{dep}`; model and web activity live outside compiler phases",
                krate.name
            );
        }
    }
}

/// An optional development/testing oracle (e.g. a Wolfram-backed `tess-oracle`)
/// may consume compiler *outputs* (TIR text, MIR dumps, benchmark JSON) from
/// outside the workspace, but no crate here may depend on one (INV-ORACLE-1).
/// A future oracle crate must be registered in `LAYERS` explicitly and may then
/// only depend on the read-only output crates, never the other way round.
const ORACLE_MARKERS: &[&str] = &["wolfram", "mathematica", "wolframscript", "tess-oracle"];

/// Source patterns forbidden in compiler-layer crates. Built with `concat!`
/// so this file does not trip its own scan.
fn forbidden_patterns() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            concat!("process", "::exit"),
            "INV-DIAG-1: compiler code must return diagnostics, not terminate the process",
        ),
        (
            concat!("static", " mut"),
            "INV-SALSA-1: no hidden mutable globals",
        ),
        (
            concat!("thread", "_local!"),
            "INV-SALSA-1: no hidden mutable globals",
        ),
        (
            concat!("lazy", "_static!"),
            "INV-SALSA-1: no hidden mutable globals",
        ),
        (
            concat!("std::", "net"),
            "INV-PURE-1: no network in compiler crates",
        ),
        (
            concat!("env", "::var"),
            "INV-PURE-1: compiler results must not depend on the process environment",
        ),
        (
            concat!("SystemTime", "::now"),
            "INV-SALSA-1: no wall-clock reads in phase code",
        ),
    ]
}

#[test]
fn compiler_sources_are_pure_and_do_not_exit() {
    for krate in crates() {
        let mut files = Vec::new();
        rust_files(&krate.dir.join("src"), &mut files);
        for file in files {
            let text = fs::read_to_string(&file).expect("source");
            for (pattern, why) in forbidden_patterns() {
                // The driver may read its own argv/env; everyone may not exit.
                if krate.name == "tessera-cli" && !pattern.contains("exit") {
                    continue;
                }
                assert!(
                    !text.contains(pattern),
                    "{why}: `{pattern}` in {}",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn no_crate_depends_on_an_external_oracle() {
    for krate in crates() {
        for dep in &krate.deps {
            assert!(
                !ORACLE_MARKERS
                    .iter()
                    .any(|m| dep.to_lowercase().contains(m)),
                "INV-ORACLE-1: `{}` depends on oracle/CAS crate `{dep}`; oracles consume outputs, they are never a dependency",
                krate.name
            );
        }
    }
}

#[test]
fn oracle_markers_are_detected_in_manifest_text() {
    let (_, deps) = parse_manifest(
        "[package]\nname = \"x\"\n[dependencies]\nwolfram-app-descriptor = \"1\"\nserde = \"1\"\n",
    );
    let hits: Vec<_> = deps
        .iter()
        .filter(|d| ORACLE_MARKERS.iter().any(|m| d.to_lowercase().contains(m)))
        .collect();
    assert_eq!(hits, ["wolfram-app-descriptor"]);
}

#[test]
fn manifest_parser_understands_all_dependency_table_styles() {
    let (name, deps) = parse_manifest(
        "[package]\nname = \"x\"\n[dependencies]\na = \"1\"\nb = { path = \"..\" }\n\
         [dev-dependencies]\nc = \"1\"\n[dependencies.d]\nversion = \"1\"\n\
         [target.'cfg(unix)'.dependencies]\ne = \"1\"\n[features]\nnot_a_dep = []\n",
    );
    assert_eq!(name, "x");
    let want: BTreeSet<String> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(deps, want);
}

/// Files known to be unreachable from their crate root, with the issue that
/// wires them. The test fails if one of them becomes declared, so the list
/// cannot go stale.
const KNOWN_UNWIRED: &[&str] = &[
    // HIR input validation for `resolve`; dead code until #20 adds a caller.
    "tessera-sema/src/input.rs",
];

/// `mod NAME;` (any visibility) declared on this line, if any.
fn declared_module(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = match line.strip_prefix("pub") {
        Some(r) if r.starts_with('(') => r.split_once(')').map_or(r, |(_, t)| t),
        Some(r) => r,
        None => line,
    };
    let name = rest.trim_start().strip_prefix("mod ")?.trim();
    name.strip_suffix(';').map(str::trim)
}

/// INV-BUILD-1: every `.rs` file under a crate's `src/` is reached by a `mod`
/// declaration. An undeclared file is never type-checked, formatted, linted
/// or tested, yet reads like implemented code: `tessera-mir`'s lowering and
/// verifier sat in that state (0 tests run) until they were wired.
#[test]
fn every_source_file_is_declared_as_a_module() {
    for krate in crates() {
        let src = krate.dir.join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let mut declared = BTreeSet::new();
        for file in &files {
            let text = fs::read_to_string(file).expect("source");
            declared.extend(text.lines().filter_map(declared_module).map(str::to_owned));
        }
        for file in &files {
            let rel = file.strip_prefix(&src).expect("under src");
            let is_root = rel.parent() == Some(Path::new(""))
                && matches!(rel.to_str(), Some("lib.rs" | "main.rs"));
            if is_root || rel.starts_with("bin") {
                continue;
            }
            let module = match file.file_stem().and_then(|s| s.to_str()) {
                Some("mod") => rel
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or_default(),
                Some(stem) => stem,
                None => continue,
            };
            let dir = krate
                .dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let key = format!("{dir}/src/{}", rel.to_string_lossy().replace('\\', "/"));
            let known = KNOWN_UNWIRED.contains(&key.as_str());
            if declared.contains(module) {
                assert!(
                    !known,
                    "INV-BUILD-1: {key} is wired now; remove it from KNOWN_UNWIRED"
                );
            } else {
                assert!(
                    known,
                    "INV-BUILD-1: {key} is not declared by any `mod {module};`, so it is never compiled or tested"
                );
            }
        }
    }
}

#[test]
fn module_declarations_are_recognized_in_every_visibility() {
    for (line, want) in [
        ("mod ir;", Some("ir")),
        ("pub mod interp;", Some("interp")),
        ("  pub(crate) mod lower;", Some("lower")),
        ("pub(in crate::x) mod deep ;", Some("deep")),
        ("mod tests {", None),
        ("pub fn module() {}", None),
        ("pub use dump::dump;", None),
    ] {
        assert_eq!(declared_module(line), want, "{line:?}");
    }
}
