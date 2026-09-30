//! TIR -> MIR lowering (contract B5 -> B6, TIR-5: TIR only).
//!
//! # Shape of the algorithm
//!
//! Lowering is a small explicit-stack machine, not a recursion over the TIR
//! tree, so the deepest tree the TIR parser accepts (`MAX_TIR_DEPTH`) needs no
//! deep call stack. Work items ([`Task`]) sit on one stack; finished operands
//! sit on a second one (`vals`).
//!
//! Every expression is lowered towards a [`Goal`]:
//!
//! * `Into(dest)`: write the value into an existing local (`_0` for the
//!   function body, a binder, or the join local of an enclosing `if`);
//! * `Operand`: produce an [`Operand`]; leaves need no statement, anything else
//!   gets a fresh temporary;
//! * `Bind(name)`: like `Operand` but the local is the binder of a `let`.
//!
//! Destination locals are allocated at the moment they are first written, so
//! locals are numbered in first-use order; blocks are numbered in reverse
//! post-order of the finished CFG with `then` before `else` (contract §2.4).
//! Nothing iterates a hash map.

use tessera_phases::{Diagnostic, DiagnosticSet, FileId, Phase, PhaseOutput, Provenance, Span};
use tessera_tir::{
    FunctionProvenance, ModuleProvenance, TirError, TirExpr, TirFunction, TirModule, TirNodeId,
    TirType, verify_module as verify_tir,
};

use crate::ir::{
    BasicBlock, BinOp, BlockId, Const, FuncId, LocalDecl, LocalId, LocalKind, MirFunction,
    MirModule, Operand, OverflowMode, Rvalue, Stmt, StmtKind, TermKind, Terminator, UnOp,
};

/// Reason strings of compiler-made provenance (`Provenance::Synthesized`).
/// They are stable: golden snapshots and tooling may match on them.
pub mod why {
    /// The `return` that ends a function whose value was produced by the body.
    pub const IMPLICIT_RETURN: &str = "implicit-return";
    /// The jump from the end of a branch of `if` to its join block.
    pub const IF_JOIN: &str = "if-join";
    /// The branch on the left operand that makes `and` short-circuit.
    pub const AND_SHORT_CIRCUIT: &str = "and-short-circuit";
    /// The `false` produced when the left operand of `and` is `false`.
    pub const AND_FALSE: &str = "and-short-circuit-false";
    /// The jump from the end of a branch of `and` to its join block.
    pub const AND_JOIN: &str = "and-join";
    /// Stand-in for a TIR node that has no provenance (with a diagnostic).
    pub const MISSING: &str = "missing-provenance";
    /// A block left open by an internal error (with a diagnostic).
    pub const UNTERMINATED: &str = "unterminated-block";
}

/// Options for [`lower_module`].
///
/// There is deliberately **no `Default`**: integer overflow semantics (open
/// question O1) are not decided at the language level, so a caller must state
/// the mode it wants. The mode is applied to every `Add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerOptions {
    pub overflow: OverflowMode,
}

/// Lower a verified TIR module to MIR.
///
/// Precondition: [`tessera_tir::verify_module`] is empty. If it is not, no
/// function is lowered: one `E-mir-ill-formed-tir` diagnostic per TIR finding is
/// returned together with an empty (structurally valid) module.
///
/// A TIR node without an entry in `provenance` is an `E-mir-missing-provenance`
/// diagnostic; the MIR still lowers, with `Synthesized { why: "missing-provenance" }`
/// standing in, so the result stays total. Lowering never panics.
#[must_use]
pub fn lower_module(
    tir: &TirModule,
    provenance: &ModuleProvenance,
    opts: &LowerOptions,
) -> PhaseOutput<MirModule> {
    let mut diags = DiagnosticSet::new();
    let findings = verify_tir(tir);
    if !findings.is_empty() {
        for finding in &findings {
            diags.push(Diagnostic::error(
                Phase::Mir,
                "E-mir-ill-formed-tir",
                format!("TIR is ill-formed, not lowering: {finding}"),
                ill_formed_at(tir, provenance, finding),
            ));
        }
        return PhaseOutput::with(MirModule::default(), diags);
    }
    let funcs = tir
        .funcs
        .iter()
        .enumerate()
        .map(|(i, func)| lower_function(tir, func, provenance.funcs.get(i), *opts, &mut diags))
        .collect();
    PhaseOutput::with(MirModule { funcs }, diags)
}

fn no_provenance() -> Provenance {
    Provenance::Synthesized {
        origin: Span::new(FileId(0), 0, 0),
        why: why::MISSING,
    }
}

/// Best-effort location of a TIR finding: its node, else its function, else
/// nothing (a stand-in). For a duplicated function name, the first function of
/// that name.
fn ill_formed_at(tir: &TirModule, provenance: &ModuleProvenance, finding: &TirError) -> Provenance {
    let fp = tir
        .funcs
        .iter()
        .position(|f| f.name == finding.func)
        .and_then(|i| provenance.funcs.get(i));
    let Some(fp) = fp else {
        return no_provenance();
    };
    finding
        .node
        .and_then(|node| fp.nodes.get(node))
        .unwrap_or(fp.func)
}

fn synth(at: Provenance, why: &'static str) -> Provenance {
    Provenance::Synthesized {
        origin: at.primary_span(),
        why,
    }
}

fn block_id(index: usize) -> BlockId {
    BlockId(u32::try_from(index).unwrap_or(u32::MAX))
}

fn ix(id: u32) -> usize {
    id as usize
}

/// Where the value of an expression must end up.
#[derive(Debug, Clone, Copy)]
enum Goal<'a> {
    /// Produce an operand (a fresh temporary unless the node is a leaf).
    Operand,
    /// Produce the binder of a `let`: a fresh named `Var` local.
    Bind(&'a str),
    /// Write into this existing local.
    Into(LocalId),
}

/// One unit of pending work. Node ids are TIR pre-order indices.
#[derive(Debug, Clone, Copy)]
enum Task<'a> {
    /// Lower node `id` towards `goal`.
    Expr { id: u32, goal: Goal<'a> },
    /// Operands of `add`/`eq` node `id` are on `vals`: emit the operation.
    FinishBinary { id: u32, goal: Goal<'a> },
    /// The operand of `not` node `id` is on `vals`.
    FinishNot { id: u32, goal: Goal<'a> },
    /// The arguments of `call` node `id` are on `vals`.
    FinishCall { id: u32, goal: Goal<'a> },
    /// The condition of `if` node `id` is on `vals`: open the diamond.
    IfBranch { id: u32, goal: Goal<'a> },
    /// The left operand of `and` node `id` is on `vals`: open the diamond.
    AndBranch { id: u32, goal: Goal<'a> },
    /// The binder of a `let` is on `vals`: bring `name` into scope.
    BindScope(&'a str),
    /// Leave the scope of the innermost `let`.
    PopScope,
    /// Continue emitting into block `block`.
    SwitchTo { block: usize },
    /// End the current block with a jump to `join`.
    GotoJoin {
        join: usize,
        origin: u32,
        why: &'static str,
    },
    /// The `false` arm of `and`.
    AssignFalse { dest: LocalId, origin: u32 },
    /// Publish the finished result of an `if`/`and` as an operand.
    PushOperand(Operand),
    /// End the function: implicit `return`.
    Return { origin: u32 },
}

/// A block under construction. `term` is `None` only while it is open; open
/// blocks never escape [`FnLower::finish`].
struct OpenBlock {
    stmts: Vec<Stmt>,
    term: Option<Terminator>,
}

struct FnLower<'a> {
    tir: &'a TirModule,
    opts: LowerOptions,
    /// TIR nodes in pre-order (index = `TirNodeId`).
    nodes: Vec<&'a TirExpr>,
    /// Child node ids of each node, in source order.
    kids: Vec<Vec<u32>>,
    /// Provenance of each node (total: gaps are filled with a stand-in).
    prov: Vec<Provenance>,
    func_prov: Provenance,
    locals: Vec<LocalDecl>,
    blocks: Vec<OpenBlock>,
    cur: usize,
    env: Vec<(&'a str, LocalId)>,
    vals: Vec<Operand>,
    tasks: Vec<Task<'a>>,
    /// Broken internal invariants; reported as `E-mir-internal`. Never expected.
    problems: Vec<String>,
}

/// Pre-order numbering without recursion: nodes and the children of each.
fn preorder(body: &TirExpr) -> (Vec<&TirExpr>, Vec<Vec<u32>>) {
    let mut nodes = Vec::new();
    let mut kids: Vec<Vec<u32>> = Vec::new();
    let mut stack: Vec<(&TirExpr, Option<u32>)> = vec![(body, None)];
    while let Some((expr, parent)) = stack.pop() {
        let id = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
        nodes.push(expr);
        kids.push(Vec::new());
        if let Some(list) = parent.and_then(|p| kids.get_mut(ix(p))) {
            list.push(id);
        }
        for child in expr.children().into_iter().rev() {
            stack.push((child, Some(id)));
        }
    }
    (nodes, kids)
}

fn lower_function(
    tir: &TirModule,
    func: &TirFunction,
    fprov: Option<&FunctionProvenance>,
    opts: LowerOptions,
    diags: &mut DiagnosticSet,
) -> MirFunction {
    let func_prov = fprov.map_or_else(no_provenance, |p| p.func);
    if fprov.is_none() {
        diags.push(Diagnostic::error(
            Phase::Mir,
            "E-mir-missing-provenance",
            format!("no provenance table for function `{}`", func.name),
            func_prov,
        ));
    }
    let (nodes, kids) = preorder(&func.body);
    let prov: Vec<Provenance> = nodes
        .iter()
        .enumerate()
        .map(|(i, expr)| {
            let id = TirNodeId(u32::try_from(i).unwrap_or(u32::MAX));
            match fprov.and_then(|p| p.nodes.get(id)) {
                Some(found) => found,
                None => {
                    if fprov.is_some() {
                        diags.push(Diagnostic::error(
                            Phase::Mir,
                            "E-mir-missing-provenance",
                            format!(
                                "function `{}`: TIR node {} (`{}`) has no provenance",
                                func.name,
                                id.0,
                                expr.op_name()
                            ),
                            func_prov,
                        ));
                    }
                    synth(func_prov, why::MISSING)
                }
            }
        })
        .collect();

    let mut locals = vec![LocalDecl {
        ty: func.ret,
        name: None,
        kind: LocalKind::Return,
    }];
    let mut env = Vec::new();
    let mut params = Vec::new();
    for (i, param) in func.params.iter().enumerate() {
        let id = LocalId(u32::try_from(i + 1).unwrap_or(u32::MAX));
        locals.push(LocalDecl {
            ty: param.ty,
            name: Some(param.name.clone()),
            kind: LocalKind::Param,
        });
        env.push((param.name.as_str(), id));
        params.push(id);
    }

    let mut lower = FnLower {
        tir,
        opts,
        nodes,
        kids,
        prov,
        func_prov,
        locals,
        blocks: vec![OpenBlock {
            stmts: Vec::new(),
            term: None,
        }],
        cur: 0,
        env,
        vals: Vec::new(),
        tasks: vec![
            Task::Return { origin: 0 },
            Task::Expr {
                id: 0,
                goal: Goal::Into(LocalId::RETURN),
            },
        ],
        problems: Vec::new(),
    };
    while let Some(task) = lower.tasks.pop() {
        lower.step(task);
    }
    let (locals, blocks, problems) = lower.finish();
    for problem in problems {
        diags.push(Diagnostic::error(
            Phase::Mir,
            "E-mir-internal",
            format!(
                "function `{}`: internal lowering error: {problem}",
                func.name
            ),
            func_prov,
        ));
    }
    MirFunction {
        name: func.name.clone(),
        params,
        ret: func.ret,
        locals,
        blocks,
        provenance: func_prov,
    }
}

impl<'a> FnLower<'a> {
    fn problem(&mut self, what: &str) {
        self.problems.push(what.to_owned());
    }

    fn pop(&mut self) -> Operand {
        if let Some(op) = self.vals.pop() {
            op
        } else {
            self.problem("operand stack underflow");
            Operand::Const(Const::Int(0))
        }
    }

    fn new_local(&mut self, ty: TirType, name: Option<String>, kind: LocalKind) -> LocalId {
        let id = LocalId(u32::try_from(self.locals.len()).unwrap_or(u32::MAX));
        self.locals.push(LocalDecl { ty, name, kind });
        id
    }

    fn new_block(&mut self) -> usize {
        self.blocks.push(OpenBlock {
            stmts: Vec::new(),
            term: None,
        });
        self.blocks.len() - 1
    }

    /// The local a result of type `ty` is written to, allocated now (so numbering
    /// follows first use), and whether the caller must publish it as an operand.
    fn dest(&mut self, goal: Goal<'a>, ty: TirType) -> (LocalId, bool) {
        match goal {
            Goal::Into(dest) => (dest, false),
            Goal::Operand => (self.new_local(ty, None, LocalKind::Temp), true),
            Goal::Bind(name) => (
                self.new_local(ty, Some(name.to_owned()), LocalKind::Var),
                true,
            ),
        }
    }

    fn assign(&mut self, dest: LocalId, rvalue: Rvalue, provenance: Provenance) {
        self.blocks[self.cur].stmts.push(Stmt {
            kind: StmtKind::Assign(dest, rvalue),
            provenance,
        });
    }

    fn terminate(&mut self, kind: TermKind, provenance: Provenance) {
        let cur = self.cur;
        if self.blocks[cur].term.is_some() {
            self.problem("block terminated twice");
            return;
        }
        self.blocks[cur].term = Some(Terminator { kind, provenance });
    }

    fn lookup(&mut self, name: &str) -> Operand {
        let found = self.env.iter().rev().find(|(n, _)| *n == name).map(|e| e.1);
        if let Some(local) = found {
            Operand::Copy(local)
        } else {
            self.problem(&format!("unbound variable `{name}` in verified TIR"));
            Operand::Const(Const::Int(0))
        }
    }

    fn step(&mut self, task: Task<'a>) {
        match task {
            Task::Expr { id, goal } => self.expr(id, goal),
            Task::FinishBinary { id, goal } => self.finish_binary(id, goal),
            Task::FinishNot { id, goal } => {
                let operand = self.pop();
                let (dest, publish) = self.dest(goal, TirType::Bool);
                let at = self.prov[ix(id)];
                self.assign(
                    dest,
                    Rvalue::Unary {
                        op: UnOp::Not,
                        operand,
                    },
                    at,
                );
                if publish {
                    self.vals.push(Operand::Copy(dest));
                }
            }
            Task::FinishCall { id, goal } => self.finish_call(id, goal),
            Task::IfBranch { id, goal } => self.if_branch(id, goal),
            Task::AndBranch { id, goal } => self.and_branch(id, goal),
            Task::BindScope(name) => match self.pop() {
                Operand::Copy(local) => self.env.push((name, local)),
                _ => self.problem("`let` initializer did not produce a local"),
            },
            Task::PopScope => {
                if self.env.pop().is_none() {
                    self.problem("scope underflow");
                }
            }
            Task::SwitchTo { block } => self.cur = block,
            Task::GotoJoin { join, origin, why } => {
                let at = synth(self.prov[ix(origin)], why);
                self.terminate(TermKind::Goto(block_id(join)), at);
            }
            Task::AssignFalse { dest, origin } => {
                let at = synth(self.prov[ix(origin)], why::AND_FALSE);
                self.assign(dest, Rvalue::Use(Operand::Const(Const::Bool(false))), at);
            }
            Task::PushOperand(op) => self.vals.push(op),
            Task::Return { origin } => {
                let at = synth(self.prov[ix(origin)], why::IMPLICIT_RETURN);
                self.terminate(TermKind::Return, at);
            }
        }
    }

    /// Schedule `Operand`-goal lowering of `kids`, first child first.
    fn push_operands(&mut self, kids: &[u32]) {
        for &kid in kids.iter().rev() {
            self.tasks.push(Task::Expr {
                id: kid,
                goal: Goal::Operand,
            });
        }
    }

    fn leaf(&mut self, id: u32, goal: Goal<'a>, value: Operand, ty: TirType) {
        match goal {
            Goal::Operand => self.vals.push(value),
            Goal::Into(dest) => {
                let at = self.prov[ix(id)];
                self.assign(dest, Rvalue::Use(value), at);
            }
            Goal::Bind(_) => {
                let (dest, _) = self.dest(goal, ty);
                let at = self.prov[ix(id)];
                self.assign(dest, Rvalue::Use(value), at);
                self.vals.push(Operand::Copy(dest));
            }
        }
    }

    fn expr(&mut self, id: u32, goal: Goal<'a>) {
        let expr: &'a TirExpr = self.nodes[ix(id)];
        let kids = self.kids[ix(id)].clone();
        match expr {
            TirExpr::Int { value, .. } => {
                self.leaf(id, goal, Operand::Const(Const::Int(*value)), TirType::I64);
            }
            TirExpr::Bool { value } => {
                self.leaf(id, goal, Operand::Const(Const::Bool(*value)), TirType::Bool);
            }
            TirExpr::Var { name, ty } => {
                let value = self.lookup(name);
                self.leaf(id, goal, value, *ty);
            }
            TirExpr::Add { .. } | TirExpr::Eq { .. } => {
                self.tasks.push(Task::FinishBinary { id, goal });
                self.push_operands(&kids);
            }
            TirExpr::Not { .. } => {
                self.tasks.push(Task::FinishNot { id, goal });
                self.push_operands(&kids);
            }
            TirExpr::Call { .. } => {
                self.tasks.push(Task::FinishCall { id, goal });
                self.push_operands(&kids);
            }
            TirExpr::And { .. } => {
                self.tasks.push(Task::AndBranch { id, goal });
                self.push_operands(&kids[..1]);
            }
            TirExpr::If { .. } => {
                self.tasks.push(Task::IfBranch { id, goal });
                self.push_operands(&kids[..1]);
            }
            TirExpr::Let { name, .. } => {
                // Runs in this order: init (into a fresh binder), bring the
                // binder into scope, body towards the caller's goal, leave scope.
                self.tasks.push(Task::PopScope);
                self.tasks.push(Task::Expr { id: kids[1], goal });
                self.tasks.push(Task::BindScope(name));
                self.tasks.push(Task::Expr {
                    id: kids[0],
                    goal: Goal::Bind(name),
                });
            }
        }
    }

    fn finish_binary(&mut self, id: u32, goal: Goal<'a>) {
        let expr: &'a TirExpr = self.nodes[ix(id)];
        let (op, ty, overflow) = match expr {
            TirExpr::Add { .. } => (BinOp::Add, TirType::I64, Some(self.opts.overflow)),
            _ => (BinOp::Eq, TirType::Bool, None),
        };
        let rhs = self.pop();
        let lhs = self.pop();
        let (dest, publish) = self.dest(goal, ty);
        let at = self.prov[ix(id)];
        self.assign(
            dest,
            Rvalue::Binary {
                op,
                overflow,
                lhs,
                rhs,
            },
            at,
        );
        if publish {
            self.vals.push(Operand::Copy(dest));
        }
    }

    fn finish_call(&mut self, id: u32, goal: Goal<'a>) {
        let expr: &'a TirExpr = self.nodes[ix(id)];
        let TirExpr::Call { callee, args, ty } = expr else {
            self.problem("`FinishCall` on a node that is not a call");
            return;
        };
        let split = self.vals.len().saturating_sub(args.len());
        let args = self.vals.split_off(split);
        let callee_id = self.tir.funcs.iter().position(|f| f.name == *callee);
        let callee_id = if let Some(i) = callee_id {
            FuncId(u32::try_from(i).unwrap_or(u32::MAX))
        } else {
            self.problem(&format!("unknown callee `{callee}` in verified TIR"));
            FuncId(u32::MAX)
        };
        let (dest, publish) = self.dest(goal, *ty);
        let at = self.prov[ix(id)];
        self.assign(
            dest,
            Rvalue::Call {
                callee: callee_id,
                args,
            },
            at,
        );
        if publish {
            self.vals.push(Operand::Copy(dest));
        }
    }

    fn if_branch(&mut self, id: u32, goal: Goal<'a>) {
        let expr: &'a TirExpr = self.nodes[ix(id)];
        let TirExpr::If { ty, .. } = expr else {
            self.problem("`IfBranch` on a node that is not an `if`");
            return;
        };
        let (then_id, else_id) = (self.kids[ix(id)][1], self.kids[ix(id)][2]);
        let cond = self.pop();
        let (dest, publish) = self.dest(goal, *ty);
        let (then_bb, else_bb, join) = (self.new_block(), self.new_block(), self.new_block());
        let at = self.prov[ix(id)];
        self.terminate(
            TermKind::Branch {
                cond,
                then_bb: block_id(then_bb),
                else_bb: block_id(else_bb),
            },
            at,
        );
        // Pushed in reverse: then-arm, else-arm, continue at the join.
        if publish {
            self.tasks.push(Task::PushOperand(Operand::Copy(dest)));
        }
        self.tasks.push(Task::SwitchTo { block: join });
        for (block, arm) in [(else_bb, else_id), (then_bb, then_id)] {
            self.tasks.push(Task::GotoJoin {
                join,
                origin: id,
                why: why::IF_JOIN,
            });
            self.tasks.push(Task::Expr {
                id: arm,
                goal: Goal::Into(dest),
            });
            self.tasks.push(Task::SwitchTo { block });
        }
    }

    fn and_branch(&mut self, id: u32, goal: Goal<'a>) {
        let rhs_id = self.kids[ix(id)][1];
        let cond = self.pop();
        let (dest, publish) = self.dest(goal, TirType::Bool);
        let (rhs_bb, false_bb, join) = (self.new_block(), self.new_block(), self.new_block());
        let at = synth(self.prov[ix(id)], why::AND_SHORT_CIRCUIT);
        self.terminate(
            TermKind::Branch {
                cond,
                then_bb: block_id(rhs_bb),
                else_bb: block_id(false_bb),
            },
            at,
        );
        if publish {
            self.tasks.push(Task::PushOperand(Operand::Copy(dest)));
        }
        self.tasks.push(Task::SwitchTo { block: join });
        // false arm (runs second)
        self.tasks.push(Task::GotoJoin {
            join,
            origin: id,
            why: why::AND_JOIN,
        });
        self.tasks.push(Task::AssignFalse { dest, origin: id });
        self.tasks.push(Task::SwitchTo { block: false_bb });
        // right operand arm (runs first)
        self.tasks.push(Task::GotoJoin {
            join,
            origin: id,
            why: why::AND_JOIN,
        });
        self.tasks.push(Task::Expr {
            id: rhs_id,
            goal: Goal::Into(dest),
        });
        self.tasks.push(Task::SwitchTo { block: rhs_bb });
    }

    /// Close the function: every block gets its terminator, blocks are
    /// renumbered in reverse post-order.
    fn finish(mut self) -> (Vec<LocalDecl>, Vec<BasicBlock>, Vec<String>) {
        if !self.tasks.is_empty() {
            self.problem("work left on the task stack");
        }
        if !self.vals.is_empty() {
            self.problem("operands left on the value stack");
        }
        let fallback = synth(self.func_prov, why::UNTERMINATED);
        let mut problems = std::mem::take(&mut self.problems);
        let closed: Vec<(Vec<Stmt>, Terminator)> = self
            .blocks
            .into_iter()
            .map(|b| match b.term {
                Some(term) => (b.stmts, term),
                None => {
                    problems.push("a block was left without a terminator".to_owned());
                    (
                        b.stmts,
                        Terminator {
                            kind: TermKind::Return,
                            provenance: fallback,
                        },
                    )
                }
            })
            .collect();
        (self.locals, renumber_rpo(closed), problems)
    }
}

/// Renumber blocks in reverse post-order from the entry, visiting `then`
/// before `else`, so the dump reads top to bottom and each join follows its
/// arms. Iterative. Blocks unreachable from the entry (never produced by
/// lowering) keep their relative order at the end so the verifier still sees
/// them.
fn renumber_rpo(blocks: Vec<(Vec<Stmt>, Terminator)>) -> Vec<BasicBlock> {
    let n = blocks.len();
    let succs = |b: usize| -> Vec<usize> {
        // Reverse so the DFS finishes `else` first and `then` lands earlier in RPO.
        let mut s: Vec<usize> = blocks[b]
            .1
            .kind
            .successors()
            .iter()
            .map(|t| ix(t.0))
            .filter(|&t| t < n)
            .collect();
        s.reverse();
        s
    };
    let mut visited = vec![false; n];
    let mut post = Vec::with_capacity(n);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    if n > 0 {
        visited[0] = true;
        stack.push((0, 0));
    }
    while let Some(top) = stack.last_mut() {
        let (block, next) = *top;
        let s = succs(block);
        if let Some(&target) = s.get(next) {
            top.1 += 1;
            if !visited[target] {
                visited[target] = true;
                stack.push((target, 0));
            }
        } else {
            post.push(block);
            stack.pop();
        }
    }
    let mut order: Vec<usize> = post.into_iter().rev().collect();
    order.extend((0..n).filter(|&b| !visited[b]));

    let mut new_of = vec![0_u32; n];
    for (new, &old) in order.iter().enumerate() {
        new_of[old] = u32::try_from(new).unwrap_or(u32::MAX);
    }
    let remap = |b: BlockId| BlockId(new_of.get(ix(b.0)).copied().unwrap_or(b.0));
    let mut slots: Vec<Option<(Vec<Stmt>, Terminator)>> = blocks.into_iter().map(Some).collect();
    order
        .iter()
        .filter_map(|&old| slots[old].take())
        .map(|(stmts, mut terminator)| {
            terminator.kind = match terminator.kind {
                TermKind::Goto(t) => TermKind::Goto(remap(t)),
                TermKind::Branch {
                    cond,
                    then_bb,
                    else_bb,
                } => TermKind::Branch {
                    cond,
                    then_bb: remap(then_bb),
                    else_bb: remap(else_bb),
                },
                TermKind::Return => TermKind::Return,
            };
            BasicBlock { stmts, terminator }
        })
        .collect()
}
