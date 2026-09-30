//! Differential test of the TIR -> MIR lowering: for random well-typed TIR
//! modules and random arguments, the TIR reference evaluator and the MIR
//! interpreter must produce the same value, the same trap, or hit the call
//! depth limit at the same call, under both overflow modes.
//!
//! Only running out of fuel is exempt: fuel counts each machine's own steps,
//! so where it runs out is not comparable. The test asserts such runs stay
//! rare, and that values, traps, depth halts, calls and short-circuiting all
//! actually occur, so a generator regression cannot make the test vacuous.

mod common;

use common::{Rng, args, lower, tir_module};
use tessera_mir::interp::{self, Halt, Limits, Trap};
use tessera_mir::{FuncId, OverflowMode, verify_module};
use tessera_tir::eval::{self, EvalOptions, Overflow};
use tessera_tir::{TirExpr, TirModule, verify_module as verify_tir};

const LIMITS: Limits = Limits {
    fuel: 200_000,
    max_call_depth: 64,
};

#[derive(Default, Debug)]
struct Tally {
    values: usize,
    traps: usize,
    depth: usize,
    fuel: usize,
    modes_disagree: usize,
}

fn has(module: &TirModule, pred: fn(&TirExpr) -> bool) -> bool {
    module
        .funcs
        .iter()
        .any(|f| f.nodes().iter().any(|(_, e)| pred(e)))
}

#[test]
fn mir_interpreter_agrees_with_the_tir_evaluator() {
    let mut rng = Rng::new(0x7E55_E4A0);
    let mut tally = Tally::default();
    let (mut with_calls, mut with_and) = (0, 0);
    for case in 0..1500 {
        let tir = tir_module(&mut rng);
        assert_eq!(
            verify_tir(&tir),
            Vec::new(),
            "case {case}: generator made ill-typed TIR"
        );
        with_calls += usize::from(has(&tir, |e| matches!(e, TirExpr::Call { .. })));
        with_and += usize::from(has(&tir, |e| matches!(e, TirExpr::And { .. })));
        let wrapping = lower(&tir, OverflowMode::Wrapping);
        let trapping = lower(&tir, OverflowMode::Trapping);
        for mir in [&wrapping, &trapping] {
            assert_eq!(
                verify_module(mir),
                Vec::new(),
                "case {case}\n{}",
                tir.to_text()
            );
        }
        for (i, func) in tir.funcs.iter().enumerate() {
            let id = FuncId(u32::try_from(i).expect("small"));
            for _ in 0..3 {
                let args = args(&mut rng, func);
                let mut results = Vec::new();
                for (mir, mode, overflow) in [
                    (&wrapping, Overflow::Wrapping, "wrapping"),
                    (&trapping, Overflow::Trapping, "trapping"),
                ] {
                    let opts = EvalOptions {
                        overflow: mode,
                        limits: LIMITS,
                    };
                    let want = eval::eval(&tir, &func.name, &args, &opts);
                    let got = interp::run(mir, id, &args, &LIMITS);
                    if want == Err(Halt::OutOfFuel) || got == Err(Halt::OutOfFuel) {
                        tally.fuel += 1;
                        continue;
                    }
                    assert_eq!(
                        got,
                        want,
                        "case {case}, {}({args:?}), {overflow}\nTIR:\n{}\nMIR:\n{}",
                        func.name,
                        tir.to_text(),
                        tessera_mir::dump(mir)
                    );
                    match &want {
                        Ok(_) => tally.values += 1,
                        Err(Halt::Trap(Trap::IntegerOverflow)) => tally.traps += 1,
                        Err(Halt::CallDepthExceeded) => tally.depth += 1,
                        Err(other) => panic!("case {case}: unexpected halt {other:?}"),
                    }
                    results.push(want);
                }
                if results.len() == 2 && results[0] != results[1] {
                    tally.modes_disagree += 1;
                }
            }
        }
    }
    let runs = tally.values + tally.traps + tally.depth + tally.fuel;
    // Not vacuous: every outcome and construct shows up, fuel skips are rare.
    assert!(tally.values * 2 > runs, "{tally:?}");
    assert!(tally.traps > 50, "{tally:?}");
    assert!(tally.depth > 5, "{tally:?}");
    assert!(tally.modes_disagree > 50, "{tally:?}");
    assert!(tally.fuel * 20 < runs, "{tally:?}");
    assert!(
        with_calls > 300 && with_and > 300,
        "{with_calls} {with_and}"
    );
}

/// Lowering is a pure function of its input: same TIR, same MIR.
#[test]
fn lowering_is_deterministic() {
    let mut rng = Rng::new(0xD373_2A11);
    for _ in 0..200 {
        let tir = tir_module(&mut rng);
        for mode in [OverflowMode::Wrapping, OverflowMode::Trapping] {
            let a = lower(&tir, mode);
            let b = lower(&tir, mode);
            assert_eq!(a, b);
            assert_eq!(tessera_mir::dump(&a), tessera_mir::dump(&b));
        }
    }
}
