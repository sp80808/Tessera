//! Differential test of the phase pipeline (CST -> HIR -> resolve -> typeck ->
//! TIR) against the bootstrap bridge it replaces (`tessera_syntax::to_tir`,
//! where resolution and typing were fused). Random TC sources vary trivia,
//! comments, redundant parentheses, names, types, literals and syntax damage.
//!
//! - Acceptance: the pipeline accepts exactly when the bridge accepts **and**
//!   the bridge's TIR passes the TIR verifier (the bridge accepts duplicate
//!   parameters, which TIR forbids; the pipeline rejects them up front).
//! - Accepted programs: identical TIR and identical provenance tables.
//! - Rejected programs: the bridge's (single) error offset is among the
//!   pipeline's diagnostics, which may report more independent problems.

use tessera_phases::{DiagnosticSet, FileId};
use tessera_sema::TirOutput;
use tessera_tir::{ModuleProvenance, TirModule, verify_module};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

const NAMES: &[&str] = &["a", "b", "c", "x"];

fn ty(rng: &mut Rng) -> &'static str {
    match rng.below(40) {
        0 => "bool",
        1 => "int32",
        _ => "i64",
    }
}

/// Names come from the declared parameters, occasionally from outside them.
fn expr(rng: &mut Rng, params: &[&str], depth: usize, out: &mut Vec<String>) {
    let atom = |rng: &mut Rng, out: &mut Vec<String>| match rng.below(40) {
        0 => out.push("99999999999999999999".to_owned()),
        1 | 2 => out.push("q".to_owned()),
        n if n < 15 || params.is_empty() => out.push(rng.below(1000).to_string()),
        _ => out.push(rng.pick(params).to_owned()),
    };
    if depth == 0 {
        atom(rng, out);
        return;
    }
    let terms = 1 + rng.below(3);
    for t in 0..terms {
        if t > 0 {
            out.push("+".to_owned());
        }
        if rng.below(3) == 0 {
            out.push("(".to_owned());
            expr(rng, params, depth - 1, out);
            out.push(")".to_owned());
        } else {
            atom(rng, out);
        }
    }
}

fn program(rng: &mut Rng) -> String {
    let mut toks: Vec<String> = vec!["f".into(), rng.pick(&["add", "f", "g"]).into(), "(".into()];
    let mut params = Vec::new();
    for i in 0..rng.below(4) {
        if i > 0 {
            toks.push(",".into());
        }
        let name = rng.pick(NAMES);
        params.push(name);
        toks.push(name.into());
        toks.push(":".into());
        toks.push(ty(rng).into());
    }
    toks.extend([")".into(), ">".into(), ty(rng).into(), "=".into()]);
    expr(rng, &params, 3, &mut toks);
    // Occasional syntax damage: drop or duplicate one token.
    match rng.below(14) {
        0 => {
            let i = rng.below(toks.len());
            toks.remove(i);
        }
        1 => {
            let i = rng.below(toks.len());
            let t = toks[i].clone();
            toks.insert(i, t);
        }
        _ => {}
    }
    let mut src = String::new();
    if rng.below(4) == 0 {
        src.push_str("// leading\n");
    }
    for (i, t) in toks.iter().enumerate() {
        // `f` and the name must stay apart; elsewhere trivia is optional.
        let sep = match rng.below(6) {
            0 => " ",
            1 => "\n  ",
            2 if i > 1 => " // c\n",
            _ if i == 1 => " ",
            _ => "",
        };
        if i > 0 {
            src.push_str(sep);
        }
        src.push_str(t);
    }
    src.push('\n');
    src
}

fn pipeline(src: &str) -> (TirOutput, DiagnosticSet) {
    let parsed = tessera_syntax::cst::parse_file(FileId(0), src);
    let hir = tessera_hir::lower(&parsed.value, src);
    let sema = tessera_sema::analyze(&hir.value);
    let mut diagnostics = parsed.diagnostics;
    diagnostics.extend(hir.diagnostics);
    diagnostics.extend(sema.diagnostics);
    (sema.value.tir, diagnostics)
}

/// The bridge's result, and the byte offset of its error if any.
fn bridge(src: &str) -> Result<(TirModule, ModuleProvenance), Option<usize>> {
    use tessera_syntax::SyntaxError as E;
    let offset = |e: &E| match e {
        E::Unexpected { at, .. }
        | E::UnknownType { at, .. }
        | E::UnboundVar { at, .. }
        | E::TypeMismatch { at, .. }
        | E::IntOutOfRange { at }
        | E::TrailingInput { at }
        | E::NestingTooDeep { at } => Some(*at),
        E::EmptyProgram => None,
    };
    let (func, spans) = tessera_syntax::parse_with_spans(src).map_err(|e| offset(&e))?;
    let tir = tessera_syntax::to_tir(&func, &spans).map_err(|e| offset(&e))?;
    Ok((
        TirModule { funcs: vec![tir] },
        ModuleProvenance {
            funcs: vec![tessera_syntax::tir_provenance(&spans)],
        },
    ))
}

#[derive(Default, Debug)]
struct Tally {
    accepted: usize,
    rejected: usize,
    bridge_unverified: usize,
    multi: usize,
}

#[test]
fn pipeline_agrees_with_the_bootstrap_bridge() {
    let mut rng = Rng(0x00DD_BA11_5EED);
    let mut tally = Tally::default();
    for case in 0..20_000 {
        let src = program(&mut rng);
        let (tir, diags) = pipeline(&src);
        let accepted = !diags.has_errors();
        match bridge(&src) {
            Ok((module, prov)) if verify_module(&module).is_empty() => {
                assert!(
                    accepted,
                    "case {case}: bridge accepts, pipeline rejects\n{src}\n{diags:#?}"
                );
                assert_eq!(tir.module, module, "case {case}: TIR differs\n{src}");
                assert_eq!(
                    tir.provenance, prov,
                    "case {case}: provenance differs\n{src}"
                );
                tally.accepted += 1;
            }
            Ok((module, _)) => {
                assert!(
                    !accepted,
                    "case {case}: pipeline accepts TIR the verifier rejects\n{src}\n{}",
                    module.to_text()
                );
                tally.bridge_unverified += 1;
            }
            Err(at) => {
                assert!(
                    !accepted,
                    "case {case}: pipeline accepts what the bridge rejects\n{src}"
                );
                if let Some(at) = at {
                    let starts: Vec<usize> = diags
                        .iter()
                        .map(|d| d.at.primary_span().start as usize)
                        .collect();
                    assert!(
                        starts.contains(&at),
                        "case {case}: bridge error at {at} not among pipeline diagnostics {starts:?}\n{src}\n{diags:#?}"
                    );
                }
                tally.rejected += 1;
                if diags
                    .iter()
                    .filter(|d| d.severity == tessera_phases::Severity::Error)
                    .count()
                    > 1
                {
                    tally.multi += 1;
                }
            }
        }
    }
    // Not vacuous: every branch is exercised substantially.
    assert!(tally.accepted > 3_000, "{tally:?}");
    assert!(tally.rejected > 3_000, "{tally:?}");
    assert!(tally.bridge_unverified > 100, "{tally:?}");
    assert!(tally.multi > 1_000, "{tally:?}");
}
