//! Shared helpers: a deterministic PRNG and a generator of random *well-typed*
//! TIR modules covering every node kind (shadowing `let`s, `if`, short-circuit
//! `and`, calls including recursion) with integer literals biased towards the
//! overflow boundary.

#![allow(dead_code)]

use tessera_mir::{LowerOptions, MirModule, OverflowMode, lower_module};
use tessera_phases::FileId;
use tessera_tir::eval::Value;
use tessera_tir::{TirExpr, TirFunction, TirModule, TirParam, TirType};

/// xorshift64*: tiny, deterministic, good enough for test-case generation.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }

    pub fn ty(&mut self) -> TirType {
        if self.chance(2) {
            TirType::I64
        } else {
            TirType::Bool
        }
    }

    /// Mostly boundary values, so trapping and wrapping `add` disagree often.
    pub fn int(&mut self) -> i64 {
        match self.below(8) {
            0 => i64::MAX,
            1 => i64::MIN,
            2 => 0,
            3 => 1,
            4 => -1,
            5 => i64::MAX - self.below(4) as i64,
            6 => (self.next() >> 1) as i64,
            _ => self.below(200) as i64 - 100,
        }
    }

    pub fn value(&mut self, ty: TirType) -> Value {
        match ty {
            TirType::I64 => Value::Int(self.int()),
            TirType::Bool => Value::Bool(self.chance(2)),
        }
    }
}

struct Sig {
    name: String,
    params: Vec<TirType>,
    ret: TirType,
}

struct Gen<'a> {
    rng: &'a mut Rng,
    sigs: &'a [Sig],
    caller: usize,
    scope: Vec<(String, TirType)>,
}

const NAMES: &[&str] = &["x", "y", "p0", "p1"];

impl Gen<'_> {
    /// Names whose innermost binding has type `ty`.
    fn visible(&self, ty: TirType) -> Vec<String> {
        let mut seen: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for (name, t) in self.scope.iter().rev() {
            if seen.contains(&name.as_str()) {
                continue;
            }
            seen.push(name);
            if *t == ty {
                out.push(name.clone());
            }
        }
        out
    }

    /// Callees returning `ty`: later functions (so most programs terminate),
    /// and with a small chance any function, including the caller itself.
    fn callees(&mut self, ty: TirType) -> Vec<usize> {
        let any = self.rng.chance(8);
        (0..self.sigs.len())
            .filter(|&j| self.sigs[j].ret == ty && (any || j > self.caller))
            .collect()
    }

    fn leaf(&mut self, ty: TirType) -> TirExpr {
        let vars = self.visible(ty);
        if !vars.is_empty() && self.rng.chance(2) {
            let name = vars[self.rng.below(vars.len())].clone();
            return TirExpr::Var { name, ty };
        }
        match ty {
            TirType::I64 => TirExpr::Int {
                value: self.rng.int(),
                ty,
            },
            TirType::Bool => TirExpr::Bool {
                value: self.rng.chance(2),
            },
        }
    }

    fn expr(&mut self, ty: TirType, budget: usize) -> TirExpr {
        if budget == 0 || self.rng.chance(4) {
            return self.leaf(ty);
        }
        let b = budget - 1;
        let bx = |e: TirExpr| Box::new(e);
        match (ty, self.rng.below(5)) {
            (_, 0) => {
                let name = NAMES[self.rng.below(NAMES.len())].to_owned();
                let init_ty = self.rng.ty();
                let init = self.expr(init_ty, b);
                self.scope.push((name.clone(), init_ty));
                let body = self.expr(ty, b);
                self.scope.pop();
                TirExpr::Let {
                    name,
                    ty: init_ty,
                    init: bx(init),
                    body: bx(body),
                }
            }
            (_, 1) => TirExpr::If {
                cond: bx(self.expr(TirType::Bool, b)),
                then_branch: bx(self.expr(ty, b)),
                else_branch: bx(self.expr(ty, b)),
                ty,
            },
            (_, 2) => {
                let callees = self.callees(ty);
                if callees.is_empty() {
                    return self.leaf(ty);
                }
                let j = callees[self.rng.below(callees.len())];
                let params = self.sigs[j].params.clone();
                let args = params.iter().map(|&p| self.expr(p, b.min(2))).collect();
                TirExpr::Call {
                    callee: self.sigs[j].name.clone(),
                    args,
                    ty,
                }
            }
            (TirType::I64, _) => TirExpr::Add {
                lhs: bx(self.expr(TirType::I64, b)),
                rhs: bx(self.expr(TirType::I64, b)),
                ty,
            },
            (TirType::Bool, 3) => {
                let operand_ty = self.rng.ty();
                TirExpr::Eq {
                    lhs: bx(self.expr(operand_ty, b)),
                    rhs: bx(self.expr(operand_ty, b)),
                }
            }
            (TirType::Bool, _) => {
                if self.rng.chance(2) {
                    TirExpr::And {
                        lhs: bx(self.expr(TirType::Bool, b)),
                        rhs: bx(self.expr(TirType::Bool, b)),
                    }
                } else {
                    TirExpr::Not {
                        expr: bx(self.expr(TirType::Bool, b)),
                    }
                }
            }
        }
    }
}

/// A random module that passes `tessera_tir::verify_module`.
pub fn tir_module(rng: &mut Rng) -> TirModule {
    let n = 1 + rng.below(4);
    let sigs: Vec<Sig> = (0..n)
        .map(|i| Sig {
            name: format!("f{i}"),
            params: (0..rng.below(4)).map(|_| rng.ty()).collect(),
            ret: rng.ty(),
        })
        .collect();
    let budget = 2 + rng.below(5);
    let funcs = (0..n)
        .map(|i| {
            let scope: Vec<(String, TirType)> = sigs[i]
                .params
                .iter()
                .enumerate()
                .map(|(k, &t)| (format!("p{k}"), t))
                .collect();
            let params = scope
                .iter()
                .map(|(name, ty)| TirParam {
                    name: name.clone(),
                    ty: *ty,
                })
                .collect();
            let body = Gen {
                rng: &mut *rng,
                sigs: &sigs,
                caller: i,
                scope,
            }
            .expr(sigs[i].ret, budget);
            TirFunction {
                name: sigs[i].name.clone(),
                params,
                ret: sigs[i].ret,
                body,
            }
        })
        .collect();
    TirModule { funcs }
}

/// Random arguments for `func`.
pub fn args(rng: &mut Rng, func: &TirFunction) -> Vec<Value> {
    func.params.iter().map(|p| rng.value(p.ty)).collect()
}

/// Lower through the text form, so provenance is real and the TIR reader is
/// exercised on every generated module.
pub fn lower(tir: &TirModule, overflow: OverflowMode) -> MirModule {
    let text = tir.to_text();
    let (parsed, prov) =
        TirModule::parse_with_provenance(FileId(0), &text).expect("generated TIR reparses");
    assert_eq!(&parsed, tir, "TIR text round trip");
    let out = lower_module(&parsed, &prov, &LowerOptions { overflow });
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    out.value
}
