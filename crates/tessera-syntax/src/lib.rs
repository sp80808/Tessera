//! Minimal TC surface frontend (v0 subset, issue #2).
//!
//! Supported subset — exactly what `examples/bootstrap.tes` establishes:
//! `f name(a:i64,b:i64)>i64=EXPR`, where `EXPR := INT | VAR | EXPR+EXPR`
//! (left-associative, parentheses allowed) and types are `i64` only.
//!
//! Expansion laws (surface -> TIR):
//! - whitespace and `//` line comments are trivia: skipped, never canonical;
//! - an integer literal expands to `(int VALUE i64)` (literals default to `i64`);
//! - a variable expands to `(var NAME T)` where `T` is the declared param type;
//! - `L+R` expands to `(add T L' R')` with `T` the unified operand type;
//! - one canonical spelling per construct: `f NAME(P:TY,…)>RET=BODY`, no spaces.
//!
//! Round-trip contract: `lower_to_tc(to_tir(parse(src))) == fmt(src)`
//! byte-exact, and `fmt` is idempotent.

use std::fmt;

use tessera_tir::{TirExpr, TirFunction, TirParam, TirType};

// ---------- tokens ----------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Func,
    Ident(String),
    Int(i64),
    Colon,
    Comma,
    LParen,
    RParen,
    Gt,
    Eq,
    Plus,
}

impl Token {
    const fn kind(&self) -> &'static str {
        match self {
            Self::Func => "`f`",
            Self::Ident(_) => "identifier",
            Self::Int(_) => "integer",
            Self::Colon => "`:`",
            Self::Comma => "`,`",
            Self::LParen => "`(`",
            Self::RParen => "`)`",
            Self::Gt => "`>`",
            Self::Eq => "`=`",
            Self::Plus => "`+`",
        }
    }
}

fn lex(src: &str) -> Result<Vec<(Token, usize)>, SyntaxError> {
    let bytes = src.as_bytes();
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < bytes.len() {
        let c = bytes[pos];
        if c.is_ascii_whitespace() {
            pos += 1;
        } else if c == b'/' && bytes.get(pos + 1) == Some(&b'/') {
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = pos;
            while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
            }
            let word = &src[start..pos];
            out.push((
                if word == "f" {
                    Token::Func
                } else {
                    Token::Ident(word.to_owned())
                },
                start,
            ));
        } else if c.is_ascii_digit() {
            let start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            let value: i64 = src[start..pos]
                .parse()
                .map_err(|_| SyntaxError::IntOutOfRange { at: start })?;
            out.push((Token::Int(value), start));
        } else {
            let token = match c {
                b':' => Token::Colon,
                b',' => Token::Comma,
                b'(' => Token::LParen,
                b')' => Token::RParen,
                b'>' => Token::Gt,
                b'=' => Token::Eq,
                b'+' => Token::Plus,
                _ => {
                    return Err(SyntaxError::Unexpected {
                        at: pos,
                        want: "TC token",
                        got: format!("byte {c:#04X}"),
                    });
                }
            };
            out.push((token, pos));
            pos += 1;
        }
    }
    Ok(out)
}

// ---------- AST (TC surface) ----------

/// Explicit AST for the v0 subset; types live only on binders, never on exprs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstExpr {
    Int(i64),
    Var(String),
    Add(Box<Self>, Box<Self>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstParam {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstFunction {
    pub name: String,
    pub params: Vec<(AstParam, TirType)>,
    pub ret: TirType,
    pub body: AstExpr,
}

// ---------- errors ----------

/// All frontend failures; `at` is always a byte offset into the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    Unexpected {
        at: usize,
        want: &'static str,
        got: String,
    },
    UnknownType {
        at: usize,
        name: String,
    },
    UnboundVar {
        at: usize,
        name: String,
    },
    TypeMismatch {
        at: usize,
        want: TirType,
        got: TirType,
    },
    IntOutOfRange {
        at: usize,
    },
    TrailingInput {
        at: usize,
    },
    EmptyProgram,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unexpected { at, want, got } => {
                write!(f, "offset {at}: expected {want}, found {got}")
            }
            Self::UnknownType { at, name } => {
                write!(
                    f,
                    "offset {at}: unsupported type `{name}` (v0 subset: i64 only)"
                )
            }
            Self::UnboundVar { at, name } => {
                write!(
                    f,
                    "offset {at}: unbound variable `{name}` (not a parameter)"
                )
            }
            Self::TypeMismatch { at, want, got } => {
                write!(
                    f,
                    "offset {at}: type mismatch in `+`: expected {}, found {}",
                    want.as_str(),
                    got.as_str()
                )
            }
            Self::IntOutOfRange { at } => write!(f, "offset {at}: integer out of i64 range"),
            Self::TrailingInput { at } => write!(f, "offset {at}: trailing input after function"),
            Self::EmptyProgram => write!(f, "empty program: expected one `f` function"),
        }
    }
}

impl std::error::Error for SyntaxError {}

// ---------- parser (deterministic recursive descent, no backtracking) ----------

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&(Token, usize)> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<(Token, usize)> {
        let item = self.tokens.get(self.pos).cloned();
        if item.is_some() {
            self.pos += 1;
        }
        item
    }

    fn expect(&mut self, want: Token, want_str: &'static str) -> Result<usize, SyntaxError> {
        match self.next() {
            Some((token, at)) if token == want => Ok(at),
            Some((token, at)) => Err(SyntaxError::Unexpected {
                at,
                want: want_str,
                got: token.kind().to_owned(),
            }),
            None => Err(SyntaxError::Unexpected {
                at: self.tokens.last().map_or(0, |(_, at)| at + 1),
                want: want_str,
                got: "end of input".to_owned(),
            }),
        }
    }

    fn expect_ident(&mut self, want: &'static str) -> Result<(String, usize), SyntaxError> {
        match self.next() {
            Some((Token::Ident(name), at)) => Ok((name, at)),
            Some((token, at)) => Err(SyntaxError::Unexpected {
                at,
                want,
                got: token.kind().to_owned(),
            }),
            None => Err(SyntaxError::Unexpected {
                at: self.tokens.last().map_or(0, |(_, at)| at + 1),
                want,
                got: "end of input".to_owned(),
            }),
        }
    }

    fn parse_type(&mut self) -> Result<(TirType, usize), SyntaxError> {
        let (name, at) = self.expect_ident("type")?;
        match name.as_str() {
            "i64" => Ok((TirType::I64, at)),
            _ => Err(SyntaxError::UnknownType { at, name }),
        }
    }

    fn parse_function(&mut self) -> Result<AstFunction, SyntaxError> {
        self.expect(Token::Func, "`f`")?;
        let (name, _) = self.expect_ident("function name")?;
        self.expect(Token::LParen, "`(`")?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Some((Token::RParen, _))) {
            loop {
                let (param, _) = self.expect_ident("parameter name")?;
                self.expect(Token::Colon, "`:`")?;
                let (ty, _) = self.parse_type()?;
                params.push((AstParam { name: param }, ty));
                if matches!(self.peek(), Some((Token::Comma, _))) {
                    self.next();
                } else {
                    break;
                }
            }
        }
        self.expect(Token::RParen, "`)`")?;
        self.expect(Token::Gt, "`>`")?;
        let (ret, _) = self.parse_type()?;
        self.expect(Token::Eq, "`=`")?;
        let body = self.parse_add()?;
        if let Some((_, at)) = self.peek() {
            return Err(SyntaxError::TrailingInput { at: *at });
        }
        Ok(AstFunction {
            name,
            params,
            ret,
            body,
        })
    }

    fn parse_add(&mut self) -> Result<AstExpr, SyntaxError> {
        let mut lhs = self.parse_primary()?;
        while matches!(self.peek(), Some((Token::Plus, _))) {
            self.next();
            let rhs = self.parse_primary()?;
            lhs = AstExpr::Add(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_primary(&mut self) -> Result<AstExpr, SyntaxError> {
        match self.next() {
            Some((Token::Int(value), _)) => Ok(AstExpr::Int(value)),
            Some((Token::Ident(name), _)) => Ok(AstExpr::Var(name)),
            Some((Token::LParen, _)) => {
                let inner = self.parse_add()?;
                self.expect(Token::RParen, "`)`")?;
                Ok(inner)
            }
            Some((token, at)) => Err(SyntaxError::Unexpected {
                at,
                want: "expression",
                got: token.kind().to_owned(),
            }),
            None => Err(SyntaxError::Unexpected {
                at: self.tokens.last().map_or(0, |(_, at)| at + 1),
                want: "expression",
                got: "end of input".to_owned(),
            }),
        }
    }
}

/// Parse one TC function. Whitespace and `//` comments are trivia.
pub fn parse(src: &str) -> Result<AstFunction, SyntaxError> {
    let tokens = lex(src)?;
    if tokens.is_empty() {
        return Err(SyntaxError::EmptyProgram);
    }
    Parser { tokens, pos: 0 }.parse_function()
}

// ---------- lowering (AST -> TIR: every inferred type made explicit) ----------

fn lower_expr(
    expr: &AstExpr,
    params: &[(AstParam, TirType)],
    at: usize,
) -> Result<TirExpr, SyntaxError> {
    match expr {
        AstExpr::Int(value) => Ok(TirExpr::Int {
            value: *value,
            ty: TirType::I64,
        }),
        AstExpr::Var(name) => params
            .iter()
            .find(|(param, _)| &param.name == name)
            .map(|(_, ty)| TirExpr::Var {
                name: name.clone(),
                ty: *ty,
            })
            .ok_or_else(|| SyntaxError::UnboundVar {
                at,
                name: name.clone(),
            }),
        AstExpr::Add(lhs, rhs) => {
            let lhs = lower_expr(lhs, params, at)?;
            let rhs = lower_expr(rhs, params, at)?;
            if lhs.ty() != rhs.ty() {
                return Err(SyntaxError::TypeMismatch {
                    at,
                    want: lhs.ty(),
                    got: rhs.ty(),
                });
            }
            let ty = lhs.ty();
            Ok(TirExpr::Add {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty,
            })
        }
    }
}

/// Expand a parsed function to explicit TIR. Pure; no model/network involved.
pub fn to_tir(func: &AstFunction) -> Result<TirFunction, SyntaxError> {
    Ok(TirFunction {
        name: func.name.clone(),
        params: func
            .params
            .iter()
            .map(|(param, ty)| TirParam {
                name: param.name.clone(),
                ty: *ty,
            })
            .collect(),
        ret: func.ret,
        body: lower_expr(&func.body, &func.params, 0)?,
    })
}

// ---------- canonical formatting (exactly one spelling) ----------

fn format_expr(expr: &AstExpr) -> String {
    match expr {
        AstExpr::Int(value) => value.to_string(),
        AstExpr::Var(name) => name.clone(),
        AstExpr::Add(lhs, rhs) => format!("{}+{}", format_expr(lhs), format_expr(rhs)),
    }
}

/// The single canonical TC spelling: `f NAME(P:TY,…)>RET=BODY`, no spaces.
#[must_use]
pub fn format_tc(func: &AstFunction) -> String {
    let params = func
        .params
        .iter()
        .map(|(param, ty)| format!("{}:{}", param.name, ty.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "f {}({})>{}={}",
        func.name,
        params,
        func.ret.as_str(),
        format_expr(&func.body)
    )
}

/// Parse leniently, return canonical TC.
pub fn fmt(src: &str) -> Result<String, SyntaxError> {
    parse(src).map(|func| format_tc(&func))
}

/// Lower well-formed TIR back to canonical TC (infallible: TIR is explicit).
#[must_use]
pub fn lower_to_tc(func: &TirFunction) -> String {
    format_tc(&AstFunction {
        name: func.name.clone(),
        params: func
            .params
            .iter()
            .map(|param| {
                (
                    AstParam {
                        name: param.name.clone(),
                    },
                    param.ty,
                )
            })
            .collect(),
        ret: func.ret,
        body: strip_types(&func.body),
    })
}

fn strip_types(expr: &TirExpr) -> AstExpr {
    match expr {
        TirExpr::Int { value, .. } => AstExpr::Int(*value),
        TirExpr::Var { name, .. } => AstExpr::Var(name.clone()),
        TirExpr::Add { lhs, rhs, .. } => {
            AstExpr::Add(Box::new(strip_types(lhs)), Box::new(strip_types(rhs)))
        }
    }
}

/// Expand TC source to TIR text (`tsr tir`); the compiler performs expansion.
pub fn expand(src: &str) -> Result<String, SyntaxError> {
    parse(src)
        .and_then(|func| to_tir(&func))
        .map(|tir| tir.to_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORPUS: &[(&str, &str)] = &[
        ("f add(a:i64,b:i64)>i64=a+b", "f add(a:i64,b:i64)>i64=a+b"),
        (
            "f add( a:i64 , b:i64 ) > i64 = a + b",
            "f add(a:i64,b:i64)>i64=a+b",
        ),
        (
            "// leading comment\nf add(a:i64,b:i64)>i64=a+b // trailing",
            "f add(a:i64,b:i64)>i64=a+b",
        ),
        (
            "f sum(a:i64,b:i64,c:i64)>i64=a+b+c",
            "f sum(a:i64,b:i64,c:i64)>i64=a+b+c",
        ),
        (
            "f nested(a:i64,b:i64)>i64=(a+b)+1",
            "f nested(a:i64,b:i64)>i64=a+b+1",
        ),
        ("f zero()>i64=42", "f zero()>i64=42"),
        ("f id(x:i64)>i64=x", "f id(x:i64)>i64=x"),
    ];

    #[test]
    fn fmt_maps_corpus_to_canonical() {
        for (input, want) in CORPUS {
            assert_eq!(fmt(input).as_deref(), Ok(*want), "input: {input}");
        }
    }

    #[test]
    fn fmt_is_idempotent() {
        for (input, _) in CORPUS {
            let once = fmt(input).expect("parses");
            assert_eq!(fmt(&once).as_deref(), Ok(once.as_str()), "input: {input}");
        }
    }

    #[test]
    fn tc_tir_tc_round_trip_is_byte_exact() {
        for (input, canonical) in CORPUS {
            let tir = parse(input).and_then(|func| to_tir(&func)).expect("lowers");
            assert_eq!(lower_to_tc(&tir), **canonical, "input: {input}");
        }
    }

    #[test]
    fn bootstrap_golden_files_agree() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let tc = std::fs::read_to_string(format!("{manifest}/../../examples/bootstrap.tes"))
            .expect("reads fixture");
        let tir = std::fs::read_to_string(format!("{manifest}/../../examples/bootstrap.tir"))
            .expect("reads fixture");
        assert_eq!(fmt(&tc).as_deref(), Ok(tc.trim_end()));
        assert_eq!(expand(&tc).as_deref(), Ok(tir.trim_end()));
    }

    #[test]
    fn errors_name_offset_and_cause() {
        assert!(matches!(fmt(""), Err(SyntaxError::EmptyProgram)));
        // `fmt` is surface normalization only; name resolution happens on expand.
        assert_eq!(fmt("f add(a:i64)>i64=b").as_deref(), Ok("f add(a:i64)>i64=b"));
        assert!(matches!(
            expand("f add(a:i64)>i64=b"),
            Err(SyntaxError::UnboundVar { .. })
        ));
        assert!(matches!(
            fmt("f add(a:bool)>bool=a"),
            Err(SyntaxError::UnknownType { .. })
        ));
        assert!(matches!(
            fmt("f add(a:i64)>i64=a+b extra"),
            Err(SyntaxError::TrailingInput { .. })
        ));
        assert!(matches!(
            fmt("f 1(a:i64)>i64=a"),
            Err(SyntaxError::Unexpected { .. })
        ));
    }
}
