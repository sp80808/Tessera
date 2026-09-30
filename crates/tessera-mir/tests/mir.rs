//! MIR lowering snapshots, verifier findings, stack safety and verifier
//! soundness against the interpreter. Public API only.

mod common;

use common::{Rng, lower, tir_module};
use tessera_mir::interp::{self, Halt, Limits, Value};
use tessera_mir::{
    BlockId, Const, FuncId, LocalDecl, LocalId, LocalKind, LocalState, LowerOptions, MirErrorKind,
    MirModule, Operand, OverflowMode, Rvalue, Stmt, StmtKind, TermKind, Terminator, TirType, dump,
    lower_module, verify_module,
};
use tessera_phases::{FileId, Provenance, Span};
use tessera_tir::{FunctionProvenance, ModuleProvenance, TirModule};

const TRAP: LowerOptions = LowerOptions {
    overflow: OverflowMode::Trapping,
};

fn lower_text(text: &str) -> MirModule {
    let (tir, prov) = TirModule::parse_with_provenance(FileId(0), text).expect("parses");
    let out = lower_module(&tir, &prov, &TRAP);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(verify_module(&out.value), Vec::new());
    out.value
}

#[test]
fn bootstrap_fixture_lowers_to_one_block() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bootstrap.tir"
    ))
    .expect("fixture");
    let mir = lower_text(&text);
    assert_eq!(
        dump(&mir),
        "\
fn add(_1: i64, _2: i64) -> i64 {  // #0 src 0:0..92
    let _0: i64;  // return
    let _1: i64;  // param a
    let _2: i64;  // param b
    bb0: {
        _0 = Add.trapping(copy _1, copy _2);  // src 0:57..90
        return;  // synth(implicit-return) 0:57..90
    }
}
"
    );
    let run = |a, b| {
        interp::run(
            &mir,
            FuncId(0),
            &[Value::Int(a), Value::Int(b)],
            &Limits::default(),
        )
    };
    assert_eq!(run(2, 3), Ok(Value::Int(5)));
    assert_eq!(
        run(i64::MAX, 1),
        Err(Halt::Trap(interp::Trap::IntegerOverflow))
    );
}

/// Entry-call checks happen in the same order as in the TIR evaluator, so the
/// two executors report the same halt even for invalid calls.
#[test]
fn entry_checks_match_the_tir_evaluator() {
    let text = "(func f (param a i64) (return i64) (body (var a i64)))";
    let tir = TirModule::parse(text).expect("parses");
    let mir = lower_text(text);
    let zero = Limits {
        fuel: 10,
        max_call_depth: 0,
    };
    for args in [vec![], vec![Value::Bool(true)], vec![Value::Int(1)]] {
        let opts = tessera_tir::eval::EvalOptions {
            overflow: tessera_tir::eval::Overflow::Trapping,
            limits: zero,
        };
        let want = tessera_tir::eval::eval(&tir, "f", &args, &opts);
        let got = interp::run(&mir, FuncId(0), &args, &zero);
        assert_eq!(
            std::mem::discriminant(&got.unwrap_err()),
            std::mem::discriminant(&want.unwrap_err()),
            "{args:?}"
        );
    }
}

/// `let` binders, the short-circuit diamond of `and`, the `if` diamond and a
/// call by `FuncId`: every compiler-made jump names why it exists.
#[test]
fn control_flow_snapshot() {
    let mir = lower_text(
        "(func f (param c bool) (param x i64) (return i64) (body \
           (let y i64 (add i64 (var x i64) (int 1 i64)) \
             (if i64 (and (var c bool) (eq (var y i64) (int 2 i64))) \
               (call g i64 (var y i64)) (int 0 i64)))))\n\
         (func g (param n i64) (return i64) (body (var n i64)))",
    );
    let got = dump(&mir);
    let body: Vec<&str> = got
        .lines()
        .map(|l| l.split("  //").next().unwrap_or(l).trim_end())
        .collect();
    assert_eq!(
        body.join("\n"),
        "\
fn f(_1: bool, _2: i64) -> i64 {
    let _0: i64;
    let _1: bool;
    let _2: i64;
    let _3: i64;
    let _4: bool;
    bb0: {
        _3 = Add.trapping(copy _2, 1_i64);
        branch(copy _1) -> [then: bb1, else: bb2];
    }
    bb1: {
        _4 = Eq(copy _3, 2_i64);
        goto -> bb3;
    }
    bb2: {
        _4 = false;
        goto -> bb3;
    }
    bb3: {
        branch(copy _4) -> [then: bb4, else: bb5];
    }
    bb4: {
        _0 = call g#1(copy _3);
        goto -> bb6;
    }
    bb5: {
        _0 = 0_i64;
        goto -> bb6;
    }
    bb6: {
        return;
    }
}

fn g(_1: i64) -> i64 {
    let _0: i64;
    let _1: i64;
    bb0: {
        _0 = copy _1;
        return;
    }
}"
    );
    for why in [
        "var y",
        "synth(and-short-circuit)",
        "synth(and-short-circuit-false)",
        "synth(and-join)",
        "synth(if-join)",
        "synth(implicit-return)",
    ] {
        assert!(got.contains(why), "missing {why:?} in\n{got}");
    }
}

#[test]
fn wrapping_mode_is_stated_on_every_add() {
    let text = "(func f (param a i64) (return i64) (body (add i64 (add i64 (var a i64) (int 1 i64)) (int 2 i64))))";
    let (tir, prov) = TirModule::parse_with_provenance(FileId(0), text).expect("parses");
    let mir = lower_module(
        &tir,
        &prov,
        &LowerOptions {
            overflow: OverflowMode::Wrapping,
        },
    )
    .value;
    let got = dump(&mir);
    assert_eq!(got.matches("Add.wrapping(").count(), 2, "{got}");
    assert!(!got.contains("trapping"));
    assert_eq!(
        interp::run(&mir, FuncId(0), &[Value::Int(i64::MAX)], &Limits::default()),
        Ok(Value::Int(i64::MIN + 2))
    );
}

#[test]
fn ill_formed_tir_is_diagnosed_and_not_lowered() {
    let (tir, prov) = TirModule::parse_with_provenance(
        FileId(0),
        "(func f (return i64) (body (add i64 (int 1 i64) (bool true))))",
    )
    .expect("parses");
    let out = lower_module(&tir, &prov, &TRAP);
    assert!(out.value.funcs.is_empty());
    let codes: Vec<_> = out.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E-mir-ill-formed-tir"]);
    // It points at the offending node in the .tir text, not at offset 0.
    let span = out
        .diagnostics
        .iter()
        .next()
        .expect("one")
        .at
        .primary_span();
    assert!(span.start > 0, "{span}");
}

/// Missing provenance is a diagnostic, but the MIR is still total.
#[test]
fn missing_provenance_is_diagnosed_but_lowering_stays_total() {
    let tir = TirModule::parse(
        "(func f (param a i64) (return i64) (body (add i64 (var a i64) (int 1 i64))))",
    )
    .expect("parses");
    let out = lower_module(&tir, &ModuleProvenance::default(), &TRAP);
    assert_eq!(out.value.funcs.len(), 1);
    assert!(
        out.diagnostics
            .iter()
            .all(|d| d.code == "E-mir-missing-provenance")
    );
    assert!(!out.diagnostics.is_empty());
    assert_eq!(verify_module(&out.value), Vec::new());

    // A partial table: one diagnostic per node without an entry.
    let span = Span::new(FileId(0), 0, 1);
    let mut nodes = tessera_phases::ProvenanceMap::new();
    nodes.insert(tessera_tir::TirNodeId(0), Provenance::Source(span));
    let partial = ModuleProvenance {
        funcs: vec![FunctionProvenance {
            func: Provenance::Source(span),
            params: vec![Provenance::Source(span)],
            nodes,
        }],
    };
    let out = lower_module(&tir, &partial, &TRAP);
    assert_eq!(out.diagnostics.len(), 2, "{:?}", out.diagnostics);
}

/// Everything downstream of the TIR reader is iterative: the deepest trees
/// it accepts lower, verify, dump and run on a default 2 MiB test stack.
#[test]
fn deepest_accepted_tir_survives_every_mir_consumer() {
    let depth = tessera_tir::MAX_TIR_DEPTH;
    let mut chain = "(int 1 i64)".to_owned();
    let mut ifs = "(int 1 i64)".to_owned();
    for _ in 1..depth {
        chain = format!("(add i64 {chain} (int 1 i64))");
        ifs = format!("(if i64 (bool true) {ifs} (int 0 i64))");
    }
    for (body, want, blocks) in [(chain, depth, 1), (ifs, 1, 3 * (depth - 1) + 1)] {
        let mir = lower_text(&format!("(func f (return i64) (body {body}))"));
        assert_eq!(mir.funcs[0].blocks.len(), blocks);
        assert!(dump(&mir).len() > depth);
        let want = Value::Int(i64::try_from(want).expect("fits"));
        assert_eq!(
            interp::run(&mir, FuncId(0), &[], &Limits::default()),
            Ok(want)
        );
    }
}

// ---------- verifier: one finding per rule ----------

/// `f(c, x) = if c { x + 1 } else { g(x) }`, `g(n) = n`.
fn base() -> MirModule {
    lower_text(
        "(func f (param c bool) (param x i64) (return i64) (body \
           (if i64 (var c bool) (add i64 (var x i64) (int 1 i64)) (call g i64 (var x i64)))))\n\
         (func g (param n i64) (return i64) (body (var n i64)))",
    )
}

fn src() -> Provenance {
    Provenance::Source(Span::new(FileId(0), 0, 1))
}

fn assign(dest: u32, rvalue: Rvalue) -> Stmt {
    Stmt {
        kind: StmtKind::Assign(LocalId(dest), rvalue),
        provenance: src(),
    }
}

fn add(lhs: Operand, rhs: Operand) -> Rvalue {
    Rvalue::Binary {
        op: tessera_mir::BinOp::Add,
        overflow: Some(OverflowMode::Trapping),
        lhs,
        rhs,
    }
}

fn temp(m: &mut MirModule, ty: TirType) -> u32 {
    let f = &mut m.funcs[0];
    f.locals.push(LocalDecl {
        ty,
        name: None,
        kind: LocalKind::Temp,
    });
    u32::try_from(f.locals.len() - 1).expect("small")
}

type Mutation = fn(&mut MirModule);
type Expect = fn(&MirErrorKind) -> bool;

fn cases() -> Vec<(&'static str, Mutation, Expect)> {
    use MirErrorKind as K;
    vec![
        (
            "no blocks",
            |m| m.funcs[0].blocks.clear(),
            |k| matches!(k, K::NoBlocks),
        ),
        (
            "no return place",
            |m| m.funcs[0].locals.clear(),
            |k| matches!(k, K::MissingReturnPlace),
        ),
        (
            "return place type",
            |m| m.funcs[0].locals[0].ty = TirType::Bool,
            |k| matches!(k, K::ReturnTypeMismatch { .. }),
        ),
        (
            "local kind",
            |m| m.funcs[0].locals[1].kind = LocalKind::Temp,
            |k| matches!(k, K::LocalKindMismatch { .. }),
        ),
        (
            "params out of order",
            |m| m.funcs[0].params.swap(0, 1),
            |k| matches!(k, K::ParamNotSequential { .. }),
        ),
        (
            "dangling local",
            |m| m.funcs[0].blocks[1].stmts[0] = assign(99, Rvalue::Use(Operand::Copy(LocalId(2)))),
            |k| matches!(k, K::LocalOutOfRange { .. }),
        ),
        (
            "dangling block",
            |m| m.funcs[0].blocks[1].terminator.kind = TermKind::Goto(BlockId(99)),
            |k| matches!(k, K::BlockOutOfRange { .. }),
        ),
        (
            "unreachable block",
            |m| {
                let copy = m.funcs[0].blocks[3].clone();
                m.funcs[0].blocks.push(copy);
            },
            |k| matches!(k, K::UnreachableBlock),
        ),
        (
            "assignment type",
            |m| {
                m.funcs[0].blocks[1].stmts[0] = assign(
                    0,
                    Rvalue::Binary {
                        op: tessera_mir::BinOp::Eq,
                        overflow: None,
                        lhs: Operand::Copy(LocalId(2)),
                        rhs: Operand::Const(Const::Int(1)),
                    },
                );
            },
            |k| matches!(k, K::AssignTypeMismatch { .. }),
        ),
        (
            "operand type",
            |m| {
                m.funcs[0].blocks[1].stmts[0] = assign(
                    0,
                    add(Operand::Copy(LocalId(1)), Operand::Const(Const::Int(1))),
                );
            },
            |k| matches!(k, K::OperandTypeMismatch { .. }),
        ),
        (
            "branch on i64",
            |m| {
                m.funcs[0].blocks[0].terminator.kind = TermKind::Branch {
                    cond: Operand::Copy(LocalId(2)),
                    then_bb: BlockId(1),
                    else_bb: BlockId(2),
                };
            },
            |k| matches!(k, K::BranchCondNotBool { .. }),
        ),
        (
            "add without overflow mode",
            |m| {
                if let StmtKind::Assign(_, Rvalue::Binary { overflow, .. }) =
                    &mut m.funcs[0].blocks[1].stmts[0].kind
                {
                    *overflow = None;
                }
            },
            |k| matches!(k, K::MissingOverflowMode),
        ),
        (
            "eq with overflow mode",
            |m| {
                m.funcs[0].blocks[1].stmts.push(Stmt {
                    kind: StmtKind::Assign(
                        LocalId(1),
                        Rvalue::Binary {
                            op: tessera_mir::BinOp::Eq,
                            overflow: Some(OverflowMode::Wrapping),
                            lhs: Operand::Const(Const::Int(1)),
                            rhs: Operand::Const(Const::Int(1)),
                        },
                    ),
                    provenance: src(),
                });
            },
            |k| matches!(k, K::UnexpectedOverflowMode { .. }),
        ),
        (
            "unknown callee",
            |m| {
                if let StmtKind::Assign(_, Rvalue::Call { callee, .. }) =
                    &mut m.funcs[0].blocks[2].stmts[0].kind
                {
                    *callee = FuncId(9);
                }
            },
            |k| matches!(k, K::UnknownCallee { .. }),
        ),
        (
            "call arity",
            |m| {
                if let StmtKind::Assign(_, Rvalue::Call { args, .. }) =
                    &mut m.funcs[0].blocks[2].stmts[0].kind
                {
                    args.clear();
                }
            },
            |k| matches!(k, K::ArityMismatch { .. }),
        ),
        (
            "call argument type",
            |m| {
                if let StmtKind::Assign(_, Rvalue::Call { args, .. }) =
                    &mut m.funcs[0].blocks[2].stmts[0].kind
                {
                    args[0] = Operand::Copy(LocalId(1));
                }
            },
            |k| matches!(k, K::ArgTypeMismatch { .. }),
        ),
        (
            "empty synthesized reason",
            |m| {
                m.funcs[0].blocks[3].terminator.provenance = Provenance::Synthesized {
                    origin: Span::new(FileId(0), 0, 1),
                    why: "",
                };
            },
            |k| matches!(k, K::EmptySynthReason),
        ),
        (
            "use before init",
            |m| {
                let t = temp(m, TirType::I64);
                m.funcs[0].blocks[1].stmts[0] = assign(0, Rvalue::Use(Operand::Copy(LocalId(t))));
            },
            |k| {
                matches!(
                    k,
                    K::UseNotInitialized {
                        state: LocalState::Uninit,
                        ..
                    }
                )
            },
        ),
        (
            "use after move",
            |m| {
                m.funcs[0].blocks[1].stmts[0] =
                    assign(0, add(Operand::Move(LocalId(2)), Operand::Copy(LocalId(2))));
            },
            |k| {
                matches!(
                    k,
                    K::UseNotInitialized {
                        state: LocalState::Moved,
                        ..
                    }
                )
            },
        ),
        (
            "double drop",
            |m| {
                let drop = || Stmt {
                    kind: StmtKind::Drop(LocalId(2)),
                    provenance: src(),
                };
                m.funcs[0].blocks[1].stmts.extend([drop(), drop()]);
            },
            |k| {
                matches!(
                    k,
                    K::DropNotInitialized {
                        state: LocalState::Dropped,
                        ..
                    }
                )
            },
        ),
        (
            "return place maybe uninitialized",
            |m| m.funcs[0].blocks[1].stmts.clear(),
            |k| {
                matches!(
                    k,
                    K::ReturnPlaceNotInitialized {
                        state: LocalState::Maybe
                    }
                )
            },
        ),
    ]
}

#[test]
fn verifier_names_each_broken_rule_and_nothing_downstream_panics() {
    let clean = base();
    for (name, mutate, expected) in cases() {
        let mut m = clean.clone();
        mutate(&mut m);
        let findings = verify_module(&m);
        assert!(
            findings.iter().any(|e| expected(&e.kind)),
            "{name}: {findings:#?}\n{}",
            dump(&m)
        );
        for e in &findings {
            assert!(!e.to_string().is_empty());
        }
        // Dump and interpreter stay total on the broken module.
        let _ = dump(&m);
        for c in [true, false] {
            let _ = interp::run(
                &m,
                FuncId(0),
                &[Value::Bool(c), Value::Int(1)],
                &Limits::default(),
            );
        }
    }
}

/// A loop needs more than one sweep of the dataflow, and a local assigned
/// only inside the loop is not definitely initialized after it.
#[test]
fn initialization_analysis_handles_loops() {
    // bb0: _2 = 0; goto bb1
    // bb1: branch(copy _1) -> [bb2, bb3]
    // bb2: _3 = copy _2; _2 = _3; goto bb1        (back edge)
    // bb3: _0 = copy _2; return
    let mut m = base();
    m.funcs.truncate(1);
    let f = &mut m.funcs[0];
    f.locals.truncate(3);
    f.locals.push(LocalDecl {
        ty: TirType::I64,
        name: None,
        kind: LocalKind::Temp,
    });
    let term = |kind| Terminator {
        kind,
        provenance: src(),
    };
    f.blocks = vec![
        tessera_mir::BasicBlock {
            stmts: vec![assign(2, Rvalue::Use(Operand::Const(Const::Int(0))))],
            terminator: term(TermKind::Goto(BlockId(1))),
        },
        tessera_mir::BasicBlock {
            stmts: vec![],
            terminator: term(TermKind::Branch {
                cond: Operand::Copy(LocalId(1)),
                then_bb: BlockId(2),
                else_bb: BlockId(3),
            }),
        },
        tessera_mir::BasicBlock {
            stmts: vec![
                assign(3, Rvalue::Use(Operand::Copy(LocalId(2)))),
                assign(2, Rvalue::Use(Operand::Copy(LocalId(3)))),
            ],
            terminator: term(TermKind::Goto(BlockId(1))),
        },
        tessera_mir::BasicBlock {
            stmts: vec![assign(0, Rvalue::Use(Operand::Copy(LocalId(2))))],
            terminator: term(TermKind::Return),
        },
    ];
    assert_eq!(verify_module(&m), Vec::new(), "{}", dump(&m));
    let analysis = tessera_mir::analyze_init(&m.funcs[0]).expect("analyzable");
    assert!(analysis.block_visits > m.funcs[0].blocks.len());
    // Terminates only by fuel when the loop condition stays true.
    let limits = Limits {
        fuel: 1_000,
        max_call_depth: 8,
    };
    let args = [Value::Bool(false), Value::Int(5)];
    assert_eq!(
        interp::run(&m, FuncId(0), &args, &limits),
        Ok(Value::Int(0))
    );
    let args = [Value::Bool(true), Value::Int(5)];
    assert_eq!(
        interp::run(&m, FuncId(0), &args, &limits),
        Err(Halt::OutOfFuel)
    );

    // Reading the loop-only temporary after the loop is a finding.
    m.funcs[0].blocks[3].stmts[0] = assign(0, Rvalue::Use(Operand::Copy(LocalId(3))));
    let findings = verify_module(&m);
    assert!(
        findings.iter().any(|e| matches!(
            e.kind,
            MirErrorKind::UseNotInitialized {
                state: LocalState::Maybe,
                ..
            }
        )),
        "{findings:#?}"
    );
}

// ---------- verifier soundness ----------

fn mutate(rng: &mut Rng, m: &mut MirModule) {
    let fi = rng.below(m.funcs.len());
    let nfuncs = m.funcs.len();
    let f = &mut m.funcs[fi];
    let nlocals = f.locals.len() as u32;
    let nblocks = f.blocks.len() as u32;
    let local = |rng: &mut Rng| LocalId(rng.below(nlocals as usize + 1) as u32);
    let bi = rng.below(f.blocks.len());
    let block = &mut f.blocks[bi];
    match rng.below(9) {
        0 if !block.stmts.is_empty() => {
            let si = rng.below(block.stmts.len());
            block.stmts.remove(si);
        }
        1 => block.stmts.insert(
            rng.below(block.stmts.len() + 1),
            Stmt {
                kind: StmtKind::Drop(local(rng)),
                provenance: src(),
            },
        ),
        2 => {
            block.terminator.kind = match rng.below(3) {
                0 => TermKind::Goto(BlockId(rng.below(nblocks as usize + 1) as u32)),
                1 => TermKind::Return,
                _ => TermKind::Branch {
                    cond: Operand::Copy(local(rng)),
                    then_bb: BlockId(rng.below(nblocks as usize) as u32),
                    else_bb: BlockId(rng.below(nblocks as usize) as u32),
                },
            };
        }
        3 => {
            let li = rng.below(f.locals.len().max(1));
            if let Some(d) = f.locals.get_mut(li) {
                d.ty = if d.ty == TirType::I64 {
                    TirType::Bool
                } else {
                    TirType::I64
                };
            }
        }
        _ if !block.stmts.is_empty() => {
            let si = rng.below(block.stmts.len());
            if let StmtKind::Assign(dest, rvalue) = &mut block.stmts[si].kind {
                match rng.below(5) {
                    0 => *dest = local(rng),
                    1 => {
                        if let Rvalue::Binary { overflow, .. } = rvalue {
                            *overflow = if overflow.is_some() {
                                None
                            } else {
                                Some(OverflowMode::Wrapping)
                            };
                        }
                    }
                    2 => {
                        if let Rvalue::Call { callee, .. } = rvalue {
                            *callee = FuncId(rng.below(nfuncs + 1) as u32);
                        }
                    }
                    _ => {
                        let op = match rng.below(3) {
                            0 => Operand::Copy(local(rng)),
                            1 => Operand::Move(local(rng)),
                            _ => Operand::Const(if rng.chance(2) {
                                Const::Int(rng.int())
                            } else {
                                Const::Bool(true)
                            }),
                        };
                        *rvalue = match rng.below(3) {
                            0 => Rvalue::Use(op),
                            1 => add(op, Operand::Copy(local(rng))),
                            _ => Rvalue::Unary {
                                op: tessera_mir::UnOp::Not,
                                operand: op,
                            },
                        };
                    }
                }
            }
        }
        _ => {}
    }
}

/// If the verifier accepts a module, the interpreter never meets a dynamic
/// violation (`Malformed`) running it: every check the interpreter makes at
/// runtime is one the verifier discharges statically. Randomly broken MIR must
/// also never panic the verifier, the dump or the interpreter.
#[test]
fn verified_mir_never_trips_the_interpreters_dynamic_checks() {
    let mut rng = Rng::new(0x5EED_0F5A_FE00);
    let limits = Limits {
        fuel: 20_000,
        max_call_depth: 32,
    };
    let (mut accepted, mut rejected) = (0, 0);
    for _ in 0..3000 {
        let tir = tir_module(&mut rng);
        let mut mir = lower(&tir, OverflowMode::Trapping);
        for _ in 0..1 + rng.below(3) {
            mutate(&mut rng, &mut mir);
        }
        let findings = verify_module(&mir);
        let _ = dump(&mir);
        for (i, func) in mir.funcs.iter().enumerate() {
            // Arguments follow the *MIR* signature: a mutation may retype a
            // parameter, and a mistyped entry call is the caller's error.
            let args: Vec<Value> = func
                .params
                .iter()
                .map(|p| func.local(*p).map_or(TirType::I64, |d| d.ty))
                .map(|ty| rng.value(ty))
                .collect();
            let result = interp::run(&mir, FuncId(i as u32), &args, &limits);
            if findings.is_empty() {
                assert!(
                    !matches!(result, Err(Halt::Malformed(_))),
                    "verifier accepted MIR the interpreter rejects: {result:?}\n{}",
                    dump(&mir)
                );
            }
        }
        if findings.is_empty() {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    // Both sides of the property are exercised.
    assert!(accepted > 300 && rejected > 300, "{accepted} {rejected}");
}
