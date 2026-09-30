//! Deterministic textual CFG (contract §2.6: "MIR: textual CFG: blocks,
//! statements, terminators; locals `_N`").
//!
//! Internal format, snapshot-tested; it may change with ordinary diffs. It is
//! total: MIR that fails the verifier still prints (dangling ids print with a
//! `?`), because a dump is most needed exactly when something is wrong.
//!
//! ```text
//! fn add(_1: i64, _2: i64) -> i64 {  // #0 src 0:0..26
//!     let _0: i64;  // return
//!     let _1: i64;  // param a
//!     let _2: i64;  // param b
//!     bb0: {
//!         _0 = Add.trapping(copy _1, copy _2);  // src 0:23..26
//!         return;  // synth(implicit-return) 0:23..26
//!     }
//! }
//! ```
//!
//! Operands: `copy _N`, `move _N`, `5_i64`, `true`. Every statement and
//! terminator ends with its provenance: `src FILE:START..END` or
//! `synth(REASON) FILE:START..END`.

use std::fmt::Write as _;

use tessera_phases::Provenance;

use crate::ir::{
    BasicBlock, BinOp, Const, FuncId, LocalKind, MirFunction, MirModule, Operand, Rvalue, StmtKind,
    TermKind, UnOp,
};

/// The whole module, functions in [`FuncId`] order separated by a blank line.
#[must_use]
pub fn dump(module: &MirModule) -> String {
    let mut out = String::new();
    for i in 0..module.funcs.len() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&dump_function(
            module,
            FuncId(u32::try_from(i).unwrap_or(u32::MAX)),
        ));
    }
    out
}

/// One function (empty if `id` is out of range). `module` resolves callee
/// names.
#[must_use]
pub fn dump_function(module: &MirModule, id: FuncId) -> String {
    let Some(f) = module.func(id) else {
        return String::new();
    };
    let mut out = String::new();
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{p}: {}", ty_of(f, p.0)))
        .collect();
    let _ = writeln!(
        out,
        "fn {}({}) -> {} {{  // {id} {}",
        f.name,
        params.join(", "),
        f.ret,
        prov(f.provenance)
    );
    for (i, decl) in f.locals.iter().enumerate() {
        let what = match (decl.kind, &decl.name) {
            (LocalKind::Return, _) => "return".to_owned(),
            (kind, Some(name)) => format!("{} {name}", kind.as_str()),
            (kind, None) => kind.as_str().to_owned(),
        };
        let _ = writeln!(out, "    let _{i}: {};  // {what}", decl.ty);
    }
    for (i, block) in f.blocks.iter().enumerate() {
        write_block(&mut out, module, i, block);
    }
    out.push_str("}\n");
    out
}

fn write_block(out: &mut String, module: &MirModule, index: usize, block: &BasicBlock) {
    let _ = writeln!(out, "    bb{index}: {{");
    for stmt in &block.stmts {
        let text = match &stmt.kind {
            StmtKind::Assign(dest, rvalue) => format!("{dest} = {}", rvalue_text(module, rvalue)),
            StmtKind::Drop(local) => format!("drop({local})"),
        };
        let _ = writeln!(out, "        {text};  // {}", prov(stmt.provenance));
    }
    let term = &block.terminator;
    let text = match &term.kind {
        TermKind::Goto(target) => format!("goto -> {target}"),
        TermKind::Branch {
            cond,
            then_bb,
            else_bb,
        } => format!(
            "branch({}) -> [then: {then_bb}, else: {else_bb}]",
            operand(cond)
        ),
        TermKind::Return => "return".to_owned(),
    };
    let _ = writeln!(out, "        {text};  // {}", prov(term.provenance));
    out.push_str("    }\n");
}

fn ty_of(f: &MirFunction, local: u32) -> String {
    f.locals
        .get(local as usize)
        .map_or_else(|| "?".to_owned(), |d| d.ty.to_string())
}

fn prov(p: Provenance) -> String {
    match p {
        Provenance::Source(span) => format!("src {span}"),
        Provenance::Synthesized { origin, why } => format!("synth({why}) {origin}"),
    }
}

fn operand(op: &Operand) -> String {
    match op {
        Operand::Copy(l) => format!("copy {l}"),
        Operand::Move(l) => format!("move {l}"),
        Operand::Const(Const::Int(v)) => format!("{v}_i64"),
        Operand::Const(Const::Bool(v)) => v.to_string(),
    }
}

fn rvalue_text(module: &MirModule, rvalue: &Rvalue) -> String {
    match rvalue {
        Rvalue::Use(op) => operand(op),
        Rvalue::Unary {
            op: UnOp::Not,
            operand: op,
        } => format!("Not({})", operand(op)),
        Rvalue::Binary {
            op,
            overflow,
            lhs,
            rhs,
        } => {
            let name = match op {
                BinOp::Add => "Add",
                BinOp::Eq => "Eq",
            };
            let mode = overflow.map_or_else(String::new, |m| format!(".{}", m.as_str()));
            format!("{name}{mode}({}, {})", operand(lhs), operand(rhs))
        }
        Rvalue::Call { callee, args } => {
            let name = module.func(*callee).map_or("?", |f| f.name.as_str());
            let args: Vec<String> = args.iter().map(operand).collect();
            format!("call {name}{callee}({})", args.join(", "))
        }
    }
}
