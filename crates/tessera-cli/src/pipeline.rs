//! `tsr mir` / `tsr run`: source -> TIR -> MIR, and running MIR.
//!
//! Pure driver glue: every function returns a value or a rendered error
//! string; printing and exit codes stay in `main`.
//!
//! Two inputs reach TIR today:
//! - `.tir` text, read by the standalone TIR reader with provenance into the
//!   `.tir` file (contract TIR-2: a hand-written file can be checked and
//!   lowered);
//! - anything else is TC, through the bootstrap bridge
//!   (`tessera_syntax::{to_tir, tir_provenance}`, TEMPORARY(#19)) until the
//!   HIR/sema path produces TIR.

use tessera_mir::interp::{self, Limits, Value};
use tessera_mir::{FuncId, LowerOptions, MirFunction, MirModule, OverflowMode, lower_module};
use tessera_phases::FileId;
use tessera_tir::{ModuleProvenance, TirModule};

use crate::render;

/// Which front end reads the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Tc,
    Tir,
}

impl Input {
    /// `.tir` files are TIR; everything else is TC.
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        if path.ends_with(".tir") {
            Self::Tir
        } else {
            Self::Tc
        }
    }
}

/// `--overflow` values. There is no default: integer overflow semantics are
/// open question O1, so the user states the behavior they want.
#[must_use]
pub fn parse_overflow(value: &str) -> Option<OverflowMode> {
    match value {
        "wrapping" => Some(OverflowMode::Wrapping),
        "trapping" => Some(OverflowMode::Trapping),
        _ => None,
    }
}

pub const OVERFLOW_REQUIRED: &str = "--overflow=wrapping|trapping is required: integer overflow semantics are not decided yet (open question O1)";

/// Source text -> TIR plus provenance, or a rendered error.
pub fn load_tir(text: &str, input: Input) -> Result<(TirModule, ModuleProvenance), String> {
    match input {
        Input::Tir => TirModule::parse_with_provenance(FileId(0), text).map_err(|e| e.to_string()),
        Input::Tc => {
            let (func, spans) =
                tessera_syntax::parse_with_spans(text).map_err(|e| e.to_string())?;
            let tir = tessera_syntax::to_tir(&func, &spans).map_err(|e| e.to_string())?;
            let provenance = ModuleProvenance {
                funcs: vec![tessera_syntax::tir_provenance(&spans)],
            };
            Ok((TirModule { funcs: vec![tir] }, provenance))
        }
    }
}

/// Source text -> verified MIR, or rendered diagnostics (`path` names the
/// file in them).
pub fn lower(
    path: &str,
    text: &str,
    input: Input,
    overflow: OverflowMode,
) -> Result<MirModule, String> {
    let (tir, provenance) = load_tir(text, input).map_err(|e| format!("{path}: {e}"))?;
    let out = lower_module(&tir, &provenance, &LowerOptions { overflow });
    if !out.diagnostics.is_empty() {
        return Err(render(path, text, &out.diagnostics));
    }
    let findings = tessera_mir::verify_module(&out.value);
    if !findings.is_empty() {
        // Lowering produced MIR its own verifier rejects: a compiler bug.
        let lines: Vec<String> = findings
            .iter()
            .map(|f| format!("internal error: MIR verifier: {f}"))
            .collect();
        return Err(lines.join("\n"));
    }
    Ok(out.value)
}

/// The function to run: `name` if given, else the module's only function.
pub fn entry(mir: &MirModule, name: Option<&str>) -> Result<FuncId, String> {
    let id = |i: usize| FuncId(u32::try_from(i).unwrap_or(u32::MAX));
    match name {
        Some(name) => mir
            .funcs
            .iter()
            .position(|f| f.name == name)
            .map(id)
            .ok_or_else(|| format!("no function `{name}`")),
        None if mir.funcs.len() == 1 => Ok(id(0)),
        None => {
            let names: Vec<&str> = mir.funcs.iter().map(|f| f.name.as_str()).collect();
            Err(format!(
                "the module has several functions; name one of: {}",
                names.join(", ")
            ))
        }
    }
}

/// Command-line arguments typed by `func`'s parameters: integers in decimal,
/// booleans as `true`/`false`.
pub fn parse_args(func: &MirFunction, raw: &[String]) -> Result<Vec<Value>, String> {
    if raw.len() != func.params.len() {
        return Err(format!(
            "`{}` takes {} arguments, got {}",
            func.name,
            func.params.len(),
            raw.len()
        ));
    }
    func.params
        .iter()
        .zip(raw)
        .map(|(param, text)| match func.local(*param).map(|d| d.ty) {
            Some(tessera_mir::TirType::I64) => text
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| format!("`{text}` is not an i64")),
            Some(tessera_mir::TirType::Bool) => match text.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err(format!("`{text}` is not a bool (true or false)")),
            },
            None => Err(format!("parameter {param} has no declaration")),
        })
        .collect()
}

/// Run `func` of `mir` on command-line arguments with the default limits.
pub fn run(mir: &MirModule, func: FuncId, raw: &[String]) -> Result<Value, String> {
    let decl = mir
        .func(func)
        .ok_or_else(|| format!("no function {func}"))?;
    let args = parse_args(decl, raw)?;
    interp::run(mir, func, &args, &Limits::default())
        .map_err(|halt| format!("`{}` halted: {halt}", decl.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOTSTRAP: &str = "f add(a:i64,b:i64)>i64=a+b\n";

    fn strings(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The B6 worked witness of `docs/architecture/compiler-phases.md` §7,
    /// now produced by real code from real TC spans.
    #[test]
    fn bootstrap_witness_from_tc() {
        let mir = lower("b.tes", BOOTSTRAP, Input::Tc, OverflowMode::Trapping).expect("lowers");
        assert_eq!(
            tessera_mir::dump(&mir),
            "\
fn add(_1: i64, _2: i64) -> i64 {  // #0 src 0:0..26
    let _0: i64;  // return
    let _1: i64;  // param a
    let _2: i64;  // param b
    bb0: {
        _0 = Add.trapping(copy _1, copy _2);  // src 0:23..26
        return;  // synth(implicit-return) 0:23..26
    }
}
"
        );
        let add = entry(&mir, None).expect("only function");
        assert_eq!(run(&mir, add, &strings(&["2", "3"])), Ok(Value::Int(5)));
        assert_eq!(
            run(&mir, add, &strings(&["9223372036854775807", "1"])),
            Err("`add` halted: trap: integer overflow".to_owned())
        );
        let wrapping =
            lower("b.tes", BOOTSTRAP, Input::Tc, OverflowMode::Wrapping).expect("lowers");
        assert_eq!(
            run(&wrapping, add, &strings(&["9223372036854775807", "1"])),
            Ok(Value::Int(i64::MIN))
        );
    }

    #[test]
    fn tc_errors_are_rendered_not_lowered() {
        let err = lower(
            "b.tes",
            "f add(a:i64)>i64=a+b",
            Input::Tc,
            OverflowMode::Trapping,
        )
        .expect_err("unbound");
        assert!(err.contains("unbound variable `b`"), "{err}");
    }

    #[test]
    fn tir_input_supports_several_functions_and_bools() {
        let text = "(func pos (param x i64) (return bool) (body (not (eq (var x i64) (int 0 i64)))))\n\
                    (func main (param x i64) (return i64) (body (if i64 (call pos bool (var x i64)) (var x i64) (int 1 i64))))";
        assert_eq!(Input::from_path("m.tir"), Input::Tir);
        let mir = lower("m.tir", text, Input::Tir, OverflowMode::Trapping).expect("lowers");
        let err = entry(&mir, None).expect_err("ambiguous");
        assert!(err.contains("pos, main"), "{err}");
        let main = entry(&mir, Some("main")).expect("exists");
        assert_eq!(run(&mir, main, &strings(&["0"])), Ok(Value::Int(1)));
        assert_eq!(run(&mir, main, &strings(&["-7"])), Ok(Value::Int(-7)));
        let pos = entry(&mir, Some("pos")).expect("exists");
        assert_eq!(run(&mir, pos, &strings(&["3"])), Ok(Value::Bool(true)));
        assert!(entry(&mir, Some("nope")).is_err());
    }

    #[test]
    fn ill_typed_tir_points_into_the_tir_file() {
        let text = "(func f (return i64)\n  (body (add i64 (int 1 i64) (bool true))))";
        let err =
            lower("bad.tir", text, Input::Tir, OverflowMode::Trapping).expect_err("ill-typed");
        assert!(
            err.starts_with("bad.tir:2:9: error[E-mir-ill-formed-tir]"),
            "{err}"
        );
    }

    #[test]
    fn arguments_are_typed_by_the_signature() {
        let text = "(func f (param n i64) (param b bool) (return bool) (body (var b bool)))";
        let mir = lower("f.tir", text, Input::Tir, OverflowMode::Wrapping).expect("lowers");
        let f = &mir.funcs[0];
        assert_eq!(
            parse_args(f, &strings(&["-3", "true"])),
            Ok(vec![Value::Int(-3), Value::Bool(true)])
        );
        for bad in [&["1"][..], &["x", "true"], &["1", "1"], &["1", "true", "2"]] {
            assert!(parse_args(f, &strings(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(parse_overflow("trapping"), Some(OverflowMode::Trapping));
        assert_eq!(parse_overflow("trap"), None);
    }
}
