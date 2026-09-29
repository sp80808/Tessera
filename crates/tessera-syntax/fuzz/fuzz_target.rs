//! Fuzzing harness for tessera-syntax parser and formatter.
//!
//! Tests:
//! - parse() doesn't panic on arbitrary input
//! - fmt() doesn't panic on arbitrary input
//! - Round-trip property: fmt(parse(src)) == fmt(src) for valid inputs
//! - Error paths don't crash

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use tessera_syntax::{fmt, parse, format_tc, format_expr, expand, AstExpr, AstFunction, AstParam, TirType};

/// Arbitrary TC-like source generator using structured approach
#[derive(Debug, Clone)]
struct TcSource {
    source: String,
}

impl Arbitrary<'_> for TcSource {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        // Generate using structured approach: either valid-ish or completely random
        let strategy: u8 = u.arbitrary()?;
        let source = match strategy % 4 {
            0 => generate_valid_function(u)?,
            1 => generate_invalid_function(u)?,
            2 => generate_random_string(u)?,
            _ => generate_edge_case(u)?,
        };
        Ok(TcSource { source })
    }
}

fn generate_valid_function(u: &mut Unstructured<'_>) -> arbitrary::Result<String> {
    // Generate a valid-ish function: f name(params...)>ret=body
    let name = generate_ident(u)?;
    let param_count = (u.arbitrary::<u8>()? % 4) + 1; // 1-4 params
    let mut params = Vec::new();
    for i in 0..param_count {
        let param_name = format!("p{}", i);
        params.push(format!("{}:i64", param_name));
    }
    let ret = "i64";
    let body = generate_expr(u, &params.iter().map(|s| s.split(':').next().unwrap().to_string()).collect::<Vec<_>>())?;
    Ok(format!("f {}({}>{})={}", name, params.join(","), ret, body))
}

fn generate_invalid_function(u: &mut Unstructured<'_>) -> arbitrary::Result<String> {
    // Generate various invalid patterns
    let kind: u8 = u.arbitrary()?;
    match kind % 8 {
        0 => Ok("".to_string()), // empty
        1 => Ok("f".to_string()), // incomplete
        2 => Ok("f foo()".to_string()), // missing return type and body
        3 => Ok("f foo(a:i64)>i64=".to_string()), // missing body
        4 => Ok("f foo(a:i64)>i64=1+".to_string()), // incomplete expr
        5 => Ok("f foo(a:bool)>bool=1".to_string()), // unknown type
        6 => Ok("f foo(a:i64)>i64=unknown_var".to_string()), // unbound var
        7 => Ok("f 123(a:i64)>i64=1".to_string()), // invalid name
        _ => generate_random_string(u),
    }
}

fn generate_random_string(u: &mut Unstructured<'_>) -> arbitrary::Result<String> {
    let len = (u.arbitrary::<u16>()? % 200) as usize;
    let bytes: Vec<u8> = (0..len).map(|_| u.arbitrary::<u8>().unwrap_or(0)).collect();
    // Ensure it's valid UTF-8
    String::from_utf8(bytes).map_err(|_| arbitrary::Error::IncorrectFormat)
}

fn generate_edge_case(u: &mut Unstructured<'_>) -> arbitrary::Result<String> {
    let kind: u8 = u.arbitrary()?;
    match kind % 12 {
        0 => Ok("f add(a:i64,b:i64)>i64=a+b".to_string()), // bootstrap
        1 => Ok("f id(x:i64)>i64=x".to_string()),
        2 => Ok("f zero()>i64=42".to_string()),
        3 => Ok("f sum(a:i64,b:i64,c:i64)>i64=a+b+c".to_string()),
        4 => Ok("f nested(a:i64,b:i64)>i64=(a+b)+1".to_string()),
        5 => Ok("f add( a:i64 , b:i64 ) > i64 = a + b".to_string()), // with spaces
        6 => Ok("// comment\nf add(a:i64,b:i64)>i64=a+b // trailing".to_string()),
        7 => Ok("f large(a:i64)>i64=1+2+3+4+5+6+7+8+9+10".to_string()),
        8 => Ok("f deep(a:i64)>i64=((((1))))".to_string()), // deep parens
        9 => Ok("f max_i64(x:i64)>i64=9223372036854775807".to_string()), // i64::MAX
        10 => Ok("f min_i64(x:i64)>i64=-9223372036854775808".to_string()), // i64::MIN
        11 => Ok("f unicode(𝑥:i64)>i64=𝑥".to_string()), // unicode ident (will fail)
        _ => generate_random_string(u),
    }
}

fn generate_expr(u: &mut Unstructured<'_>, params: &[String]) -> arbitrary::Result<String> {
    let depth: u8 = u.arbitrary()?;
    generate_expr_with_depth(u, params, depth % 4)
}

fn generate_expr_with_depth(u: &mut Unstructured<'_>, params: &[String], depth: u8) -> arbitrary::Result<String> {
    if depth == 0 || u.arbitrary::<bool>()? {
        // Leaf: int literal or variable
        if u.arbitrary::<bool>()? && !params.is_empty() {
            let idx = (u.arbitrary::<u8>()? as usize) % params.len();
            Ok(params[idx].clone())
        } else {
            let val: i64 = u.arbitrary()?;
            Ok(val.to_string())
        }
    } else {
        // Binary expression
        let lhs = generate_expr_with_depth(u, params, depth - 1)?;
        let rhs = generate_expr_with_depth(u, params, depth - 1)?;
        if u.arbitrary::<bool>()? {
            Ok(format!("({}+{})", lhs, rhs))
        } else {
            Ok(format!("{}+{}", lhs, rhs))
        }
    }
}

fn generate_ident(u: &mut Unstructured<'_>) -> arbitrary::Result<String> {
    let first = u.arbitrary::<char>()?;
    let rest_len = (u.arbitrary::<u8>()? % 10) as usize;
    let mut ident = String::new();
    if first.is_ascii_alphabetic() || first == '_' {
        ident.push(first);
    } else {
        ident.push('f');
    }
    for _ in 0..rest_len {
        let c = u.arbitrary::<char>()?;
        if c.is_ascii_alphanumeric() || c == '_' {
            ident.push(c);
        }
    }
    if ident.is_empty() {
        ident.push('x');
    }
    Ok(ident)
}

/// Test that parse() never panics
fuzz_target!(|data: TcSource| {
    let _ = parse(&data.source);
});

/// Test that fmt() never panics
fuzz_target!(|data: TcSource| {
    let _ = fmt(&data.source);
});

/// Test round-trip property: for valid inputs, fmt(parse(src)) == fmt(src)
fuzz_target!(|data: TcSource| {
    if let Ok(parsed) = parse(&data.source) {
        let formatted = format_tc(&parsed);
        // Re-parse the formatted output
        if let Ok(reparsed) = parse(&formatted) {
            let reformatted = format_tc(&reparsed);
            // fmt is idempotent
            assert_eq!(formatted, reformatted, "fmt not idempotent: {:?}", data.source);
        }
    }
});

/// Test parse -> to_tir -> lower_to_tc round-trip
fuzz_target!(|data: TcSource| {
    if let Ok(parsed) = parse(&data.source) {
        if let Ok(tir) = tessera_syntax::to_tir(&parsed) {
            let tc = tessera_syntax::lower_to_tc(&tir);
            // tc should be valid and parseable
            if let Ok(reparsed) = parse(&tc) {
                let reformatted = format_tc(&reparsed);
                assert_eq!(tc, reformatted);
            }
        }
    }
});

/// Test error paths don't crash
fuzz_target!(|data: TcSource| {
    // Just exercise all error paths
    let _ = parse(&data.source);
    let _ = fmt(&data.source);
    let _ = expand(&data.source);
});

/// Direct test of AstExpr generation and formatting
#[derive(Debug, Clone, Arbitrary)]
enum FuzzExpr {
    Int(i64),
    Var(String),
    Add(Box<FuzzExpr>, Box<FuzzExpr>),
}

impl FuzzExpr {
    fn to_ast(&self, params: &[String]) -> AstExpr {
        match self {
            FuzzExpr::Int(v) => AstExpr::Int(*v),
            FuzzExpr::Var(name) => AstExpr::Var(name.clone()),
            FuzzExpr::Add(l, r) => AstExpr::Add(
                Box::new(l.to_ast(params)),
                Box::new(r.to_ast(params)),
            ),
        }
    }
}

fuzz_target!(|expr: FuzzExpr| {
    let params = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let ast = expr.to_ast(&params);
    let formatted = format_expr(&ast);
    // Should not panic
    let _ = formatted;
});

/// Test with manually constructed AstFunction
#[derive(Debug, Clone, Arbitrary)]
struct FuzzFunction {
    name: String,
    params: Vec<String>,
    body: FuzzExpr,
}

impl FuzzFunction {
    fn to_ast(&self) -> Option<AstFunction> {
        if self.name.is_empty() || self.params.is_empty() {
            return None;
        }
        Some(AstFunction {
            name: self.name.clone(),
            params: self.params.iter().map(|p| (AstParam { name: p.clone() }, TirType::I64)).collect(),
            ret: TirType::I64,
            body: self.body.to_ast(&self.params),
        })
    }
}

fuzz_target!(|func: FuzzFunction| {
    if let Some(ast) = func.to_ast() {
        let formatted = format_tc(&ast);
        // Round-trip
        if let Ok(parsed) = parse(&formatted) {
            let reformatted = format_tc(&parsed);
            assert_eq!(formatted, reformatted);
        }
    }
});