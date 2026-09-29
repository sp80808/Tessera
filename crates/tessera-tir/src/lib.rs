//! Tessera Intent IR (TIR): explicit, lossless semantic expansion of TC.
//!
//! Design rule: every fact the TC surface leaves implicit (operand types,
//! literal types, result types) is written out on every node. Nothing here is
//! canonical source; TIR is always derived from TC and must lower back to
//! byte-identical canonical TC (see `tessera_syntax::lower_to_tc`).
//!
//! Textual form is a boring S-expression (`.tir` files), e.g.
//! `(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))`.

use std::fmt;

/// Scalar type in the v0 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirType {
    I64,
    Bool,
}

impl TirType {
    /// Canonical spelling used in both TC and TIR text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::Bool => "bool",
        }
    }

    /// All known scalar types, in canonical declaration order.
    #[must_use]
    pub const fn all() -> &'static [TirType] {
        &[TirType::I64, TirType::Bool]
    }

    /// Parse a type from its canonical spelling, or `None` if unknown.
    #[must_use]
    pub fn parse(name: &str) -> Option<TirType> {
        match name {
            "i64" => Some(TirType::I64),
            "bool" => Some(TirType::Bool),
            _ => None,
        }
    }
}

/// Explicitly typed expression. The `ty` on every node is the inferred fact
/// TC omits; `Int` literals default to `I64` in the v0 subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TirExpr {
    Int {
        value: i64,
        ty: TirType,
    },
    Bool {
        value: bool,
    },
    Var {
        name: String,
        ty: TirType,
    },
    Add {
        lhs: Box<Self>,
        rhs: Box<Self>,
        ty: TirType,
    },
    Eq {
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    And {
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    Not {
        expr: Box<Self>,
    },
}

impl TirExpr {
    #[must_use]
    pub const fn ty(&self) -> TirType {
        match self {
            Self::Int { ty, .. } | Self::Var { ty, .. } | Self::Add { ty, .. } => *ty,
            Self::Bool { .. } | Self::Eq { .. } | Self::And { .. } => TirType::Bool,
            Self::Not { .. } => TirType::Bool,
        }
    }

    /// Boring explicit textual form; every node carries its type.
    #[must_use]
    pub fn to_text(&self) -> String {
        match self {
            Self::Int { value, ty } => format!("(int {value} {})", ty.as_str()),
            Self::Bool { value } => format!("(bool {value})"),
            Self::Var { name, ty } => format!("(var {name} {})", ty.as_str()),
            Self::Add { lhs, rhs, ty } => {
                format!("(add {} {} {})", ty.as_str(), lhs.to_text(), rhs.to_text())
            }
            Self::Eq { lhs, rhs } => format!("(eq {} {})", lhs.to_text(), rhs.to_text()),
            Self::And { lhs, rhs } => format!("(and {} {})", lhs.to_text(), rhs.to_text()),
            Self::Not { expr } => format!("(not {})", expr.to_text()),
        }
    }
}

/// A single function parameter with its explicit type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TirParam {
    pub name: String,
    pub ty: TirType,
}

/// Explicit function definition: the whole v0 TIR program is one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TirFunction {
    pub name: String,
    pub params: Vec<TirParam>,
    pub ret: TirType,
    pub body: TirExpr,
}

impl TirFunction {
    /// Boring explicit textual form (`.tir` golden files use this).
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = format!("(func {}", self.name);
        for param in &self.params {
            out.push_str(&format!(" (param {} {})", param.name, param.ty.as_str()));
        }
        out.push_str(&format!(
            " (return {}) (body {})",
            self.ret.as_str(),
            self.body.to_text()
        ));
        out.push(')');
        out
    }
}

/// Compact display for diagnostics and logs.
impl fmt::Display for TirType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tir_text_makes_every_type_explicit() {
        let func = TirFunction {
            name: "add".to_owned(),
            params: vec![
                TirParam {
                    name: "a".to_owned(),
                    ty: TirType::I64,
                },
                TirParam {
                    name: "b".to_owned(),
                    ty: TirType::I64,
                },
            ],
            ret: TirType::I64,
            body: TirExpr::Add {
                lhs: Box::new(TirExpr::Var {
                    name: "a".to_owned(),
                    ty: TirType::I64,
                }),
                rhs: Box::new(TirExpr::Var {
                    name: "b".to_owned(),
                    ty: TirType::I64,
                }),
                ty: TirType::I64,
            },
        };
        assert_eq!(
            func.to_text(),
            "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))"
        );
    }

    #[test]
    fn bool_literal_round_trips() {
        let expr = TirExpr::Bool { value: true };
        assert_eq!(expr.ty(), TirType::Bool);
        assert_eq!(expr.to_text(), "(bool true)");
    }

    #[test]
    fn type_parse_round_trip() {
        for ty in TirType::all() {
            assert_eq!(TirType::parse(ty.as_str()), Some(*ty));
        }
        assert_eq!(TirType::parse("i32"), None);
    }
}