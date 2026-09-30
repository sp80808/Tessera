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

use ast::AstSpans;
use tessera_tir::{TirExpr, TirFunction, TirParam};

pub use tessera_tir::TirType;

pub mod ast;
pub mod cst;
pub mod lexer;

#[cfg(test)]
mod legacy;

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
        want: String,
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
    /// Parenthesis nesting exceeded [`MAX_NESTING`]; bounded so hostile input
    /// yields a diagnostic instead of overflowing the stack.
    NestingTooDeep {
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
            Self::NestingTooDeep { at } => {
                write!(
                    f,
                    "offset {at}: expression too deeply nested (limits: {MAX_NESTING} parenthesis levels, {MAX_EXPR_DEPTH} expression depth)"
                )
            }
            Self::EmptyProgram => write!(f, "empty program: expected one `f` function"),
        }
    }
}

impl std::error::Error for SyntaxError {}

// ---------- parser (deterministic recursive descent, no backtracking) ----------

/// Provisional recursion bound for parenthesized expressions.
pub const MAX_NESTING: usize = 128;

/// Provisional bound on expression *tree* depth: the number of `+` and
/// parenthesis levels on the deepest root-to-leaf path (a leaf counts 0), so
/// `a+b+c` has depth 2 and `(a+b)+c` depth 3. It is measured over the whole
/// path, not per parenthesis level: chains nested inside the left operand of
/// other chains add up. The bootstrap AST/TIR and their consumers (formatter,
/// lowering, `Drop`) are recursive, so unbounded depth would overflow the
/// stack; exceeding it is a diagnostic. Revisit when #19 moves to arena-based
/// HIR.
pub const MAX_EXPR_DEPTH: usize = 1000;

// ---------- lowering (AST -> TIR: every inferred type made explicit) ----------

fn lower_expr(
    expr: &AstExpr,
    params: &[(AstParam, TirType)],
    spans: &AstSpans,
    next: &mut usize,
) -> Result<TirExpr, SyntaxError> {
    // pre-order index of this node in `spans.exprs`
    let at = spans.expr_start(*next);
    *next += 1;
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
            let lhs = lower_expr(lhs, params, spans, next)?;
            let rhs = lower_expr(rhs, params, spans, next)?;
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
/// Errors carry the source offset from `spans` (PROV-2); pass
/// `AstSpans::empty` for ASTs not parsed from source (offsets are then 0).
pub fn to_tir(func: &AstFunction, spans: &AstSpans) -> Result<TirFunction, SyntaxError> {
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
        body: lower_expr(&func.body, &func.params, spans, &mut 0)?,
    })
}

/// Parse one TC function with its span table. Whitespace and `//` comments
/// are trivia. Thin wrapper over the tolerant CST parser: the first syntax
/// diagnostic becomes the error.
pub fn parse_with_spans(src: &str) -> Result<(AstFunction, AstSpans), SyntaxError> {
    let out = cst::parse_file(tessera_phases::FileId(0), src);
    ast::lower(&out.value, src, &out.diagnostics)
}

pub fn parse(src: &str) -> Result<AstFunction, SyntaxError> {
    parse_with_spans(src).map(|(func, _)| func)
}

// ---------- canonical formatting (exactly one spelling) ----------

/// Canonical expression spelling. `+` is left-associative, so a right operand
/// that is itself a sum keeps its parentheses: dropping them (`a+(b+c)` ->
/// `a+b+c`) would re-parse to a different tree. Redundant parentheses (around
/// left operands, atoms, or the whole body) are the only ones removed.
#[must_use]
pub fn format_expr(expr: &AstExpr) -> String {
    match expr {
        AstExpr::Int(value) => value.to_string(),
        AstExpr::Var(name) => name.clone(),
        AstExpr::Add(lhs, rhs) => match **rhs {
            AstExpr::Add(..) => format!("{}+({})", format_expr(lhs), format_expr(rhs)),
            _ => format!("{}+{}", format_expr(lhs), format_expr(rhs)),
        },
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

/// A TIR construct the current TC grammar cannot spell. Lowering says so
/// instead of guessing a spelling (a wrong spelling would silently change the
/// program; the round-trip claim only covers what TC can express).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotSpellable {
    /// What has no TC form, e.g. `` `eq` node `` or `` type `bool` ``.
    pub what: String,
}

impl fmt::Display for NotSpellable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} has no spelling in the current TC grammar (v0 subset: i64 and `+` only)",
            self.what
        )
    }
}

impl std::error::Error for NotSpellable {}

fn spellable_type(ty: TirType) -> Result<TirType, NotSpellable> {
    match ty {
        TirType::I64 => Ok(ty),
        other => Err(NotSpellable {
            what: format!("type `{other}`"),
        }),
    }
}

/// Lower TIR back to canonical TC. Fallible: TIR is richer than the v0 TC
/// grammar, and anything TC cannot spell (`bool` types and literals, `eq`,
/// `and`, `not`, `let`, `if`, `call`) is reported, never approximated.
pub fn lower_to_tc(func: &TirFunction) -> Result<String, NotSpellable> {
    let params = func
        .params
        .iter()
        .map(|param| {
            Ok((
                AstParam {
                    name: param.name.clone(),
                },
                spellable_type(param.ty)?,
            ))
        })
        .collect::<Result<Vec<_>, NotSpellable>>()?;
    Ok(format_tc(&AstFunction {
        name: func.name.clone(),
        params,
        ret: spellable_type(func.ret)?,
        body: strip_types(&func.body)?,
    }))
}

fn strip_types(expr: &TirExpr) -> Result<AstExpr, NotSpellable> {
    match expr {
        TirExpr::Int { value, ty } => {
            spellable_type(*ty)?;
            Ok(AstExpr::Int(*value))
        }
        TirExpr::Var { name, ty } => {
            spellable_type(*ty)?;
            Ok(AstExpr::Var(name.clone()))
        }
        TirExpr::Add { lhs, rhs, ty } => {
            spellable_type(*ty)?;
            Ok(AstExpr::Add(
                Box::new(strip_types(lhs)?),
                Box::new(strip_types(rhs)?),
            ))
        }
        other => Err(NotSpellable {
            what: format!("`{}` node", other.op_name()),
        }),
    }
}

/// Expand TC source to TIR text (`tsr tir`); the compiler performs expansion.
pub fn expand(src: &str) -> Result<String, SyntaxError> {
    let (func, spans) = parse_with_spans(src)?;
    to_tir(&func, &spans).map(|tir| tir.to_text())
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
            let tir = parse_with_spans(input)
                .and_then(|(func, spans)| to_tir(&func, &spans))
                .expect("lowers");
            assert_eq!(
                lower_to_tc(&tir).as_deref(),
                Ok(*canonical),
                "input: {input}"
            );
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
        assert_eq!(
            fmt("f add(a:i64)>i64=b").as_deref(),
            Ok("f add(a:i64)>i64=b")
        );
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

    /// Regression: `fmt` used to print `a+(b+c)` as `a+b+c`, which parses to a
    /// different tree ((a+b)+c). Formatting must never change the semantic result.
    #[test]
    fn fmt_keeps_parentheses_that_change_the_tree() {
        assert_eq!(
            fmt("f x(a:i64,b:i64,c:i64)>i64=a+(b+c)").as_deref(),
            Ok("f x(a:i64,b:i64,c:i64)>i64=a+(b+c)")
        );
        assert_eq!(
            fmt("f x(a:i64,b:i64,c:i64)>i64=(a+b)+c").as_deref(),
            Ok("f x(a:i64,b:i64,c:i64)>i64=a+b+c")
        );
        assert_ne!(
            expand("f x(a:i64,b:i64,c:i64)>i64=a+(b+c)"),
            expand("f x(a:i64,b:i64,c:i64)>i64=a+b+c"),
            "the two spellings are different trees"
        );
    }

    /// Property: for random parenthesized sums, `fmt` is idempotent and never
    /// changes the TIR (`expand(fmt(x)) == expand(x)`), and `tir -> tc` returns
    /// exactly `fmt(x)`.
    #[test]
    fn fmt_preserves_the_semantic_result_on_random_sums() {
        fn gen_expr(next: &mut impl FnMut() -> u64, fuel: u32) -> String {
            if fuel == 0 || next() % 3 == 0 {
                return match next() % 3 {
                    0 => "a".to_owned(),
                    1 => "b".to_owned(),
                    _ => (next() % 100).to_string(),
                };
            }
            let l = gen_expr(next, fuel - 1);
            let r = gen_expr(next, fuel - 1);
            let sum = format!(
                "{l}{}+{}{r}",
                ["", " "][(next() % 2) as usize],
                ["", " "][(next() % 2) as usize]
            );
            if next() % 2 == 0 {
                format!("({sum})")
            } else {
                sum
            }
        }
        let mut state = 0x1234_5678_9ABC_DEF1_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..3_000 {
            let src = format!("f g(a:i64,b:i64)>i64={}", gen_expr(&mut next, 6));
            let canonical = fmt(&src).expect("generated sources parse");
            assert_eq!(
                fmt(&canonical).as_deref(),
                Ok(canonical.as_str()),
                "idempotent: {src}"
            );
            assert_eq!(
                expand(&canonical),
                expand(&src),
                "fmt changed the tree: {src}"
            );
            let (func, spans) = parse_with_spans(&src).expect("parses");
            let tir = to_tir(&func, &spans).expect("lowers");
            assert_eq!(
                lower_to_tc(&tir).as_deref(),
                Ok(canonical.as_str()),
                "{src}"
            );
        }
    }

    /// Regression for G5: `lower_to_tc` used to map `Bool`/`Eq`/`And`/`Not` onto
    /// `Int`/`Add` and print a *different program* as if the round trip held.
    /// Anything TC cannot spell must be an explicit error.
    #[test]
    fn lower_to_tc_refuses_what_tc_cannot_spell() {
        use tessera_tir::TirModule;
        for (tir, what) in [
            ("(func f (return bool) (body (bool true)))", "type `bool`"),
            (
                "(func f (param a bool) (return i64) (body (int 1 i64)))",
                "type `bool`",
            ),
            (
                "(func f (return i64) (body (if i64 (bool true) (int 1 i64) (int 2 i64))))",
                "`if` node",
            ),
            (
                "(func f (param a i64) (return i64) (body (let x i64 (var a i64) (var x i64))))",
                "`let` node",
            ),
            ("(func f (return i64) (body (call f i64)))", "`call` node"),
        ] {
            let module = TirModule::parse(tir).expect("fixture parses");
            let err = lower_to_tc(&module.funcs[0]).expect_err(tir);
            assert!(err.what.contains(what), "{tir}: {err}");
        }
        // a nested unspellable node is found too
        let module = TirModule::parse(
            "(func f (param a i64) (return i64) (body (add i64 (var a i64) (if i64 (bool true) (int 1 i64) (int 2 i64)))))",
        )
        .unwrap();
        assert!(lower_to_tc(&module.funcs[0]).is_err());
        // and what TC *can* spell still round-trips exactly
        let module =
            TirModule::parse(&expand("f add(a:i64,b:i64)>i64=a+b").unwrap()).expect("tir parses");
        assert_eq!(
            lower_to_tc(&module.funcs[0]).as_deref(),
            Ok("f add(a:i64,b:i64)>i64=a+b")
        );
    }

    /// Formatting changes (whitespace, comments, redundant parens) must not
    /// change the semantic result: the same TIR comes out.
    #[test]
    fn surface_variation_does_not_change_semantic_result() {
        let canonical = expand("f add(a:i64,b:i64)>i64=a+b").expect("expands");
        for variant in [
            "f add( a:i64 , b:i64 ) > i64 = a + b",
            "// c\nf add(a:i64,b:i64)>i64=a+b // t\n",
            "f add(a:i64,b:i64)>i64=(a+b)",
            "\n\n  f   add(a:i64,b:i64)>i64=((a)+(b))",
        ] {
            assert_eq!(
                expand(variant).as_deref(),
                Ok(canonical.as_str()),
                "{variant:?}"
            );
        }
    }

    #[test]
    fn expansion_is_deterministic() {
        for (input, _) in CORPUS {
            assert_eq!(expand(input), expand(input));
        }
    }

    #[test]
    fn hostile_nesting_returns_a_diagnostic_not_a_crash() {
        let n = 200_000;
        let src = format!("f x()>i64={}1{}", "(".repeat(n), ")".repeat(n));
        assert!(matches!(
            parse(&src),
            Err(SyntaxError::NestingTooDeep { .. })
        ));
        let at_limit = format!(
            "f x()>i64={}1{}",
            "(".repeat(MAX_NESTING),
            ")".repeat(MAX_NESTING)
        );
        assert_eq!(fmt(&at_limit).as_deref(), Ok("f x()>i64=1"));
    }

    #[test]
    fn frontend_never_panics_on_arbitrary_text() {
        const PIECES: &[&str] = &[
            "f ",
            "add",
            "(",
            ")",
            ":",
            ",",
            ">",
            "=",
            "+",
            "i64",
            "a",
            "b",
            "1",
            " ",
            "\n",
            "//",
            "𝑥",
            "9999999999999999999999",
            "\u{0}",
            "bool",
        ];
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let len = (next() % 24) as usize;
            let src: String = (0..len)
                .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
                .collect();
            let _ = parse(&src);
            let _ = fmt(&src);
            let _ = expand(&src);
        }
    }

    /// PROV-2: semantic errors keep the source offset of the offending node.
    #[test]
    fn semantic_errors_keep_their_source_offset() {
        let src = "f add(a:i64)>i64=a+b";
        let want = src.rfind('b').expect("has b");
        match expand(src) {
            Err(SyntaxError::UnboundVar { at, .. }) => assert_eq!(at, want),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn ast_spans_are_exact_for_the_bootstrap_fixture() {
        let src = "f add(a:i64,b:i64)>i64=a+b\n";
        let (_, s) = parse_with_spans(src).expect("parses");
        let r = |sp: tessera_phases::Span| (sp.start, sp.end);
        assert_eq!(r(s.func), (0, 26));
        assert_eq!(r(s.name), (2, 5));
        assert_eq!(r(s.ret), (19, 22));
        assert_eq!(r(s.body), (23, 26));
        assert_eq!(
            s.params
                .iter()
                .map(|p| (r(p.name), r(p.ty)))
                .collect::<Vec<_>>(),
            [((6, 7), (8, 11)), ((12, 13), (14, 17))]
        );
        // pre-order: Add, lhs, rhs
        assert_eq!(
            s.exprs.iter().map(|&e| r(e)).collect::<Vec<_>>(),
            [(23, 26), (23, 24), (25, 26)]
        );
    }

    /// INV-ID-2 at the AST level: reformatting changes spans, never the AST.
    #[test]
    fn reformatting_changes_spans_but_not_the_ast() {
        let (a, sa) = parse_with_spans("f add(a:i64,b:i64)>i64=a+b").expect("a");
        let (b, sb) =
            parse_with_spans("// c\nf add( a:i64 , b:i64 ) > i64 = ( a + b )").expect("b");
        assert_eq!(a, b);
        assert_ne!(sa, sb);
    }

    #[test]
    fn parenthesized_and_nested_expression_spans_follow_the_ast_not_the_parens() {
        let src = "f n(a:i64,b:i64)>i64=(a+b)+1";
        let (_, s) = parse_with_spans(src).expect("parses");
        // Add(Add(a,b),1): 4 nodes, pre-order; inner Add spans `a+b` without parens
        let spans: Vec<_> = s
            .exprs
            .iter()
            .map(|e| &src[e.start as usize..e.end as usize])
            .collect();
        assert_eq!(spans, ["(a+b)+1", "a+b", "a", "b", "1"]);
    }

    #[test]
    fn errors_from_the_cst_path_keep_legacy_shape_and_offsets() {
        assert!(matches!(
            parse("f x()>i64=1 2"),
            Err(SyntaxError::TrailingInput { at: 12 })
        ));
        assert!(matches!(
            parse("f x(a:bool)>i64=1"),
            Err(SyntaxError::UnknownType { at: 6, .. })
        ));
        assert!(matches!(
            parse("f x()>i64=99999999999999999999"),
            Err(SyntaxError::IntOutOfRange { at: 10 })
        ));
        match parse("f x()>i64=") {
            Err(SyntaxError::Unexpected { want, got, .. }) => {
                assert_eq!(
                    (want.as_str(), got.as_str()),
                    ("expression", "end of input")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// The facade and the frozen oracle agree on every accepted program's AST.
    #[test]
    fn facade_matches_legacy_oracle_on_accepted_programs() {
        for (input, _) in CORPUS {
            assert_eq!(parse(input), legacy::parse(input), "{input:?}");
        }
    }

    /// Every recursive consumer (lowering, formatter, TIR text, Drop) must
    /// survive the deepest accepted input on a small test-thread stack, and
    /// anything beyond it must be a diagnostic, not an abort.
    #[test]
    fn deepest_accepted_and_hostile_chains_never_overflow_the_stack() {
        let at_limit = format!("f c(a:i64)>i64={}", vec!["a"; MAX_EXPR_DEPTH].join("+"));
        let canonical = fmt(&at_limit).expect("at the limit is accepted");
        assert_eq!(fmt(&canonical).as_deref(), Ok(canonical.as_str()));
        assert!(expand(&at_limit).is_ok());
        let parens = format!(
            "f c(a:i64)>i64={}a{}",
            "(".repeat(MAX_NESTING),
            ")".repeat(MAX_NESTING)
        );
        assert!(expand(&parens).is_ok());

        let hostile = format!("f c(a:i64)>i64={}", vec!["a"; 1_000_000].join("+"));
        assert!(matches!(
            parse(&hostile),
            Err(SyntaxError::NestingTooDeep { .. })
        ));
    }

    /// `a+a+…` of `links` links, as the left operand of an enclosing chain.
    fn left_nested(levels: &[usize]) -> String {
        let mut e = "a".to_owned();
        for (i, &links) in levels.iter().enumerate() {
            if i > 0 {
                e = format!("({e})");
            }
            e.push_str(&"+a".repeat(links));
        }
        format!("f c(a:i64)>i64={e}")
    }

    /// Regression: the depth budget used to reset at every parenthesis level
    /// (`paren depth + links of this chain`), so chains nested in the left
    /// operand of other chains built ASTs ~120x deeper than `MAX_EXPR_DEPTH`
    /// and `tsr fmt`/`tsr tir` aborted with a stack overflow on a 240 KB file
    /// while `tsr check` called it clean.
    #[test]
    fn left_nested_chains_count_against_one_depth_budget() {
        // 128 levels, each a near-maximal chain: ~120k deep before the fix.
        let levels: Vec<usize> = (0..MAX_NESTING).map(|d| MAX_EXPR_DEPTH - d - 1).collect();
        let hostile = left_nested(&levels);
        assert!(matches!(
            parse(&hostile),
            Err(SyntaxError::NestingTooDeep { .. })
        ));
        assert!(fmt(&hostile).is_err());
        assert!(expand(&hostile).is_err());
        let out = cst::parse_file(tessera_phases::FileId(0), &hostile);
        assert_eq!(out.diagnostics.len(), 1, "reported once, no cascade");

        // Exactly at the limit through the same shape: 499 + 1 (parens) + 500.
        let at_limit = left_nested(&[499, 500]);
        let canonical = fmt(&at_limit).expect("depth == MAX_EXPR_DEPTH is accepted");
        assert_eq!(fmt(&canonical).as_deref(), Ok(canonical.as_str()));
        let tir = expand(&at_limit).expect("expands");
        let module = tessera_tir::TirModule::parse(&tir).expect("TIR reader accepts it");
        assert_eq!(module.to_text(), tir);
        // One more link anywhere on the deepest path is over the limit.
        assert!(matches!(
            parse(&left_nested(&[499, 501])),
            Err(SyntaxError::NestingTooDeep { .. })
        ));
        assert!(matches!(
            parse(&left_nested(&[500, 500])),
            Err(SyntaxError::NestingTooDeep { .. })
        ));
    }
}
