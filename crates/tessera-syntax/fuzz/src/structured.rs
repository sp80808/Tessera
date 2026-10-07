//! Structure-aware input for the `frontend_structured` target.
//!
//! Raw bytes almost never spell a complete, well-typed TC program, so the deep
//! branches (expansion, TIR verification, the TC -> TIR -> TC round trip) are
//! reached rarely. This module turns fuzz bytes into programs that are valid
//! by construction, optionally damaged, and adds an oracle the library cannot
//! influence: for an *undamaged* program the generator knows the expected
//! tree, canonical spelling and outcome without consulting the parser, so a
//! parser that is consistently wrong (say, right-associative `+`) fails here
//! even though every round-trip check in `frontend_inv` would still hold.
//!
//! Shapes that matter for the documented limits are produced by repetition
//! (`Tail`, `Nest`) so a few input bytes reach `MAX_NESTING` and
//! `MAX_EXPR_DEPTH` and both sides of them.

use std::fmt::Write as _;

use arbitrary::{Arbitrary, Result, Unstructured};
use tessera_syntax::{
    AstExpr, AstFunction, AstParam, MAX_EXPR_DEPTH, MAX_NESTING, SyntaxError, TirType, expand, fmt,
    parse,
};

use crate::stats::{Counter, hit};
use crate::{cst_inv, frontend_inv, show};

/// Total repetitions (chain links + nesting levels) one program may use.
const REPEAT_BUDGET: usize = 5_000;

/// Types other than `i64`: accepted by the CST, rejected by the AST facade.
const OTHER_TYPES: [&str; 6] = ["bool", "u8", "I64", "i32", "f", "_"];

/// Identifiers that are interesting to the parser or the lexer.
const NAMES: [&str; 12] = [
    "a", "b", "c", "x", "y", "f", "i64", "_", "_x", "a1", "add", "bool",
];

/// Tokens (and stray characters) spliced into damaged programs.
const LEXICON: [&str; 32] = [
    "f",
    "(",
    ")",
    ",",
    ":",
    ">",
    "=",
    "+",
    "i64",
    "a",
    "b",
    "1",
    "-",
    "*",
    "<",
    "!",
    "&",
    ".",
    ";",
    "{",
    "}",
    "[",
    "]",
    "//",
    "bool",
    "99999999999999999999",
    "\u{1d465}",
    "\u{0}",
    "\u{e9}",
    "/",
    "_",
    "0",
];

#[derive(Debug, Clone, Copy)]
enum TypeName {
    I64,
    Other(u8),
}

impl TypeName {
    fn text(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::Other(i) => OTHER_TYPES[usize::from(i) % OTHER_TYPES.len()],
        }
    }
}

#[derive(Debug)]
struct IntLit {
    text: String,
    /// `None` when the literal does not fit `i64`.
    value: Option<i64>,
}

#[derive(Debug)]
enum Expr {
    Int(IntLit),
    /// Index into the program's names (see [`Names::var`]).
    Var(u8),
    /// `lhs + rhs`; a right operand that is itself a chain gets parentheses
    /// so the tree is the one the generator intends.
    Add(Box<Expr>, Box<Expr>),
    /// Redundant parentheses.
    Paren(Box<Expr>),
    /// `head` (parenthesized if `wrap`) followed by `links` times `+ leaf`:
    /// long left-leaning chains, and, nested, the `left_nested` hostile shape.
    Tail {
        head: Box<Expr>,
        links: usize,
        leaf: Box<Expr>,
        wrap: bool,
    },
    /// `parens` opening parentheses, `inner`, `parens` closing ones.
    Nest {
        inner: Box<Expr>,
        parens: usize,
    },
}

impl Expr {
    /// Does it render as an unparenthesized `a+b+...` chain?
    fn is_chain(&self) -> bool {
        match self {
            Self::Add(..) => true,
            Self::Tail {
                head, links, wrap, ..
            } => *links > 0 || (!*wrap && head.is_chain()),
            Self::Int(_) | Self::Var(_) | Self::Paren(_) | Self::Nest { .. } => false,
        }
    }
}

#[derive(Debug, Arbitrary)]
enum Mutation {
    Delete(u16),
    Duplicate(u16),
    Insert(u16, u8),
    Replace(u16, u8),
    Swap(u16),
    Truncate(u16),
}

/// One generated program: a valid TC function unless `mutations`, `tail` or a
/// non-`i64` type damage it.
#[derive(Debug)]
pub struct Program {
    name: String,
    params: Vec<(String, TypeName)>,
    ret: TypeName,
    body: Expr,
    /// Names a `Var` may refer to that are (usually) not parameters.
    unbound: Vec<String>,
    /// Seeds where trivia goes between tokens.
    seed: u64,
    mutations: Vec<Mutation>,
    tail: Option<String>,
}

// ---------- generation ----------

struct Gen<'a, 'b> {
    u: &'b mut Unstructured<'a>,
    budget: usize,
    has_params: bool,
}

impl Gen<'_, '_> {
    fn ident(&mut self) -> Result<String> {
        Ok(match self.u.int_in_range(0..=5u8)? {
            0..=3 => (*self.u.choose(&NAMES)?).to_owned(),
            4 => {
                let mut s = String::new();
                s.push(char::from(*self.u.choose(b"abcxyz_ABC")?));
                for _ in 0..self.u.int_in_range(0..=5usize)? {
                    s.push(char::from(*self.u.choose(b"abcxyz_ABC019")?));
                }
                s
            }
            _ => "n".repeat(self.u.int_in_range(1..=300usize)?),
        })
    }

    fn type_name(&mut self) -> Result<TypeName> {
        Ok(if self.u.ratio(1, 16)? {
            TypeName::Other(self.u.arbitrary()?)
        } else {
            TypeName::I64
        })
    }

    /// Mostly tiny; sometimes right at a documented limit (+-2).
    fn count(&mut self) -> Result<usize> {
        let raw = match self.u.int_in_range(0..=9u8)? {
            0..=4 => self.u.int_in_range(0..=4usize)?,
            5 => self.u.int_in_range(5..=40usize)?,
            6 => MAX_NESTING - 2 + self.u.int_in_range(0..=4usize)?,
            7 | 8 => MAX_EXPR_DEPTH - 3 + self.u.int_in_range(0..=6usize)?,
            _ => self.u.int_in_range(0..=2_000usize)?,
        };
        let n = raw.min(self.budget);
        self.budget -= n;
        Ok(n)
    }

    fn int_lit(&mut self) -> Result<IntLit> {
        let text = match self.u.int_in_range(0..=9u8)? {
            0..=5 => self.u.int_in_range(0..=99u32)?.to_string(),
            6 => "9223372036854775807".to_owned(),
            7 => "9223372036854775808".to_owned(),
            8 => format!("00{}", self.u.int_in_range(0..=9u8)?),
            _ => "9".repeat(self.u.int_in_range(19..=40usize)?),
        };
        let value = text.parse::<i64>().ok();
        Ok(IntLit { text, value })
    }

    fn leaf(&mut self) -> Result<Expr> {
        if self.has_params && self.u.ratio(3, 5)? {
            Ok(Expr::Var(self.u.arbitrary()?))
        } else {
            Ok(Expr::Int(self.int_lit()?))
        }
    }

    fn expr(&mut self, fuel: u32) -> Result<Expr> {
        if fuel == 0 {
            return self.leaf();
        }
        Ok(match self.u.int_in_range(0..=9u8)? {
            0..=2 => self.leaf()?,
            3..=5 => Expr::Add(
                Box::new(self.expr(fuel - 1)?),
                Box::new(self.expr(fuel - 1)?),
            ),
            6 => Expr::Paren(Box::new(self.expr(fuel - 1)?)),
            7 | 8 => {
                let head = Box::new(self.expr(fuel - 1)?);
                let links = self.count()?;
                Expr::Tail {
                    head,
                    links,
                    leaf: Box::new(self.leaf()?),
                    wrap: self.u.arbitrary()?,
                }
            }
            _ => {
                let inner = Box::new(self.expr(fuel - 1)?);
                Expr::Nest {
                    inner,
                    parens: self.count()?,
                }
            }
        })
    }
}

impl<'a> Arbitrary<'a> for Program {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let mut g = Gen {
            u,
            budget: REPEAT_BUDGET,
            has_params: false,
        };
        let name = g.ident()?;
        let nparams = if g.u.ratio(1, 16)? {
            g.u.int_in_range(6..=40usize)?
        } else {
            g.u.int_in_range(0..=4usize)?
        };
        let mut params = Vec::with_capacity(nparams);
        for _ in 0..nparams {
            params.push((g.ident()?, g.type_name()?));
        }
        g.has_params = !params.is_empty();
        let ret = g.type_name()?;
        let body = g.expr(6)?;
        let mut unbound = vec![g.ident()?];
        if g.u.ratio(1, 4)? {
            unbound.push(g.ident()?);
        }
        let seed = g.u.arbitrary()?;
        let mut mutations = Vec::new();
        if g.u.ratio(1, 2)? {
            for _ in 0..g.u.int_in_range(1..=3usize)? {
                mutations.push(Mutation::arbitrary(g.u)?);
            }
        }
        let tail = if g.u.ratio(1, 8)? {
            Some(String::arbitrary(g.u)?)
        } else {
            None
        };
        Ok(Self {
            name,
            params,
            ret,
            body,
            unbound,
            seed,
            mutations,
            tail,
        })
    }
}

// ---------- rendering, with the generator's own expectations ----------

struct Names<'p> {
    params: &'p [(String, TypeName)],
    unbound: &'p [String],
}

impl Names<'_> {
    /// Mostly a parameter; one index in 64 names a (usually) unbound variable.
    fn var(&self, index: u8) -> &str {
        let i = usize::from(index);
        if self.params.is_empty() || i % 64 == 63 {
            &self.unbound[i % self.unbound.len()]
        } else {
            &self.params[i % self.params.len()].0
        }
    }

    fn is_bound(&self, name: &str) -> bool {
        self.params.iter().any(|(p, _)| p == name)
    }
}

/// Tokens as ranges into one buffer, so mutation and trivia insertion work on
/// tokens without re-lexing.
#[derive(Default)]
struct Out {
    buf: String,
    toks: Vec<(usize, usize)>,
}

impl Out {
    fn push(&mut self, text: &str) {
        let start = self.buf.len();
        self.buf.push_str(text);
        self.toks.push((start, self.buf.len()));
    }
}

/// What rendering an expression tells the oracle.
struct Info {
    /// `BinExpr` + `ParenExpr` nodes on the deepest path (leaf = 0).
    depth: usize,
    /// `ParenExpr` nodes on the deepest path.
    parens: usize,
    /// The intended tree, only while it is within `MAX_EXPR_DEPTH`.
    ast: Option<AstExpr>,
    overflow: bool,
    unbound: bool,
}

impl Info {
    /// Wrap in `by` parenthesis levels.
    fn wrapped(mut self, by: usize) -> Self {
        self.depth += by;
        self.parens += by;
        if self.depth > MAX_EXPR_DEPTH {
            self.ast = None;
        }
        self
    }
}

impl Expr {
    fn render(&self, out: &mut Out, names: &Names<'_>) -> Info {
        match self {
            Self::Int(lit) => {
                out.push(&lit.text);
                Info {
                    depth: 0,
                    parens: 0,
                    ast: Some(AstExpr::Int(lit.value.unwrap_or(0))),
                    overflow: lit.value.is_none(),
                    unbound: false,
                }
            }
            Self::Var(index) => {
                let name = names.var(*index);
                out.push(name);
                Info {
                    depth: 0,
                    parens: 0,
                    ast: Some(AstExpr::Var(name.to_owned())),
                    overflow: false,
                    unbound: !names.is_bound(name),
                }
            }
            Self::Paren(inner) => {
                out.push("(");
                let info = inner.render(out, names);
                out.push(")");
                info.wrapped(1)
            }
            Self::Nest { inner, parens } => {
                for _ in 0..*parens {
                    out.push("(");
                }
                let info = inner.render(out, names);
                for _ in 0..*parens {
                    out.push(")");
                }
                info.wrapped(*parens)
            }
            Self::Add(lhs, rhs) => {
                let lhs = lhs.render(out, names);
                out.push("+");
                let wrap = rhs.is_chain();
                if wrap {
                    out.push("(");
                }
                let mut rhs_info = rhs.render(out, names);
                if wrap {
                    out.push(")");
                    rhs_info = rhs_info.wrapped(1);
                }
                let depth = lhs.depth.max(rhs_info.depth) + 1;
                let ast = match (lhs.ast, rhs_info.ast) {
                    (Some(l), Some(r)) if depth <= MAX_EXPR_DEPTH => {
                        Some(AstExpr::Add(Box::new(l), Box::new(r)))
                    }
                    _ => None,
                };
                Info {
                    depth,
                    parens: lhs.parens.max(rhs_info.parens),
                    ast,
                    overflow: lhs.overflow || rhs_info.overflow,
                    unbound: lhs.unbound || rhs_info.unbound,
                }
            }
            Self::Tail {
                head,
                links,
                leaf,
                wrap,
            } => {
                if *wrap {
                    out.push("(");
                }
                let mut info = head.render(out, names);
                if *wrap {
                    out.push(")");
                    info = info.wrapped(1);
                }
                for _ in 0..*links {
                    out.push("+");
                    let leaf_info = leaf.render(out, names);
                    info.depth = info.depth.max(leaf_info.depth) + 1;
                    info.ast = match (info.ast.take(), leaf_info.ast) {
                        (Some(l), Some(r)) if info.depth <= MAX_EXPR_DEPTH => {
                            Some(AstExpr::Add(Box::new(l), Box::new(r)))
                        }
                        _ => None,
                    };
                    info.overflow |= leaf_info.overflow;
                    info.unbound |= leaf_info.unbound;
                }
                info
            }
        }
    }
}

/// What the generator knows about an undamaged program.
#[derive(Debug)]
enum Expected {
    /// Damaged (mutation, tail, non-`i64` type): only the invariants apply.
    Unchecked,
    /// Parses; `bound` says whether every variable is a parameter.
    Ok { ast: AstFunction, bound: bool },
    /// Exceeds `MAX_NESTING` or `MAX_EXPR_DEPTH`.
    NestingTooDeep,
    /// Parses but a literal does not fit `i64`.
    IntOutOfRange,
}

struct Rendered {
    src: String,
    expected: Expected,
}

fn trivia(state: &mut u64) -> &'static str {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    match *state % 16 {
        0..=9 => "",
        10 => " ",
        11 => "\n",
        12 => "\t",
        13 => "  \r\n",
        14 => "//c\n",
        _ => "// f x()>i64=1\n",
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl Program {
    fn render(&self) -> Rendered {
        let names = Names {
            params: &self.params,
            unbound: &self.unbound,
        };
        let mut out = Out::default();
        out.push("f");
        out.push(&self.name);
        out.push("(");
        for (i, (name, ty)) in self.params.iter().enumerate() {
            if i > 0 {
                out.push(",");
            }
            out.push(name);
            out.push(":");
            out.push(ty.text());
        }
        out.push(")");
        out.push(">");
        out.push(self.ret.text());
        out.push("=");
        let info = self.body.render(&mut out, &names);

        let pristine = self.mutations.is_empty()
            && self.tail.is_none()
            && self.ret.text() == "i64"
            && self.params.iter().all(|(_, ty)| ty.text() == "i64");
        let expected = if !pristine {
            Expected::Unchecked
        } else if info.parens > MAX_NESTING || info.depth > MAX_EXPR_DEPTH {
            Expected::NestingTooDeep
        } else if info.overflow {
            Expected::IntOutOfRange
        } else {
            Expected::Ok {
                ast: AstFunction {
                    name: self.name.clone(),
                    params: self
                        .params
                        .iter()
                        .map(|(p, _)| (AstParam { name: p.clone() }, TirType::I64))
                        .collect(),
                    ret: TirType::I64,
                    body: info.ast.expect("depth is within the limit"),
                },
                bound: !info.unbound,
            }
        };

        let mut toks = std::mem::take(&mut out.toks);
        for mutation in &self.mutations {
            apply(mutation, &mut toks, &mut out.buf);
        }
        let mut state = self.seed | 1;
        let mut src = String::with_capacity(out.buf.len() * 2);
        src.push_str(trivia(&mut state));
        let mut prev_last = None;
        for &(start, end) in &toks {
            let text = &out.buf[start..end];
            let gap = trivia(&mut state);
            let glued = prev_last.is_some_and(is_word_byte)
                && text.bytes().next().is_some_and(is_word_byte);
            src.push_str(if gap.is_empty() && glued { " " } else { gap });
            src.push_str(text);
            prev_last = text.bytes().next_back();
        }
        src.push_str(trivia(&mut state));
        if let Some(tail) = &self.tail {
            src.push_str(tail);
        }
        Rendered { src, expected }
    }
}

fn apply(mutation: &Mutation, toks: &mut Vec<(usize, usize)>, buf: &mut String) {
    let lexeme = |buf: &mut String, which: u8| {
        let text = LEXICON[usize::from(which) % LEXICON.len()];
        let start = buf.len();
        buf.push_str(text);
        (start, buf.len())
    };
    let at = |index: u16, len: usize| usize::from(index) % len.max(1);
    match *mutation {
        Mutation::Delete(i) if !toks.is_empty() => {
            toks.remove(at(i, toks.len()));
        }
        Mutation::Duplicate(i) if !toks.is_empty() => {
            let i = at(i, toks.len());
            toks.insert(i, toks[i]);
        }
        Mutation::Insert(i, which) => {
            let new = lexeme(buf, which);
            toks.insert(at(i, toks.len() + 1), new);
        }
        Mutation::Replace(i, which) if !toks.is_empty() => {
            let new = lexeme(buf, which);
            let i = at(i, toks.len());
            toks[i] = new;
        }
        Mutation::Swap(i) if toks.len() >= 2 => {
            let i = at(i, toks.len() - 1);
            toks.swap(i, i + 1);
        }
        Mutation::Truncate(i) if !toks.is_empty() => {
            toks.truncate(at(i, toks.len()));
        }
        _ => {}
    }
}

// ---------- checks ----------

/// The canonical spelling the documentation promises, written without the
/// library's formatter: `f NAME(P:TY,...)>RET=BODY`, no spaces, and
/// parentheses only around a right operand that is itself a sum.
fn canonical(func: &AstFunction) -> String {
    fn expr(e: &AstExpr, out: &mut String) {
        match e {
            AstExpr::Int(v) => {
                let _ = write!(out, "{v}");
            }
            AstExpr::Var(name) => out.push_str(name),
            AstExpr::Add(lhs, rhs) => {
                expr(lhs, out);
                out.push('+');
                if matches!(**rhs, AstExpr::Add(..)) {
                    out.push('(');
                    expr(rhs, out);
                    out.push(')');
                } else {
                    expr(rhs, out);
                }
            }
        }
    }
    let params: Vec<String> = func
        .params
        .iter()
        .map(|(p, ty)| format!("{}:{}", p.name, ty.as_str()))
        .collect();
    let mut out = format!(
        "f {}({})>{}=",
        func.name,
        params.join(","),
        func.ret.as_str()
    );
    expr(&func.body, &mut out);
    out
}

/// Decode fuzz bytes exactly as `fuzz_target!(|p: Program| ..)` does
/// (`arbitrary_take_rest`); this is how replays reproduce a libFuzzer
/// artifact of the `frontend_structured` target.
pub fn check_bytes(data: &[u8]) {
    if let Ok(program) = Program::arbitrary_take_rest(Unstructured::new(data)) {
        check(&program);
    }
}

/// Render `program`, run the frontend and CST invariants on the text, and
/// compare an undamaged program against what the generator intended.
pub fn check(program: &Program) {
    hit(Counter::StructuredRuns);
    let Rendered { src, expected } = program.render();
    frontend_inv::check_text(&src);
    cst_inv::check_text(&src);

    match expected {
        Expected::Unchecked => {}
        Expected::Ok { ast, bound } => {
            hit(Counter::StructuredPristine);
            let parsed = parse(&src);
            invariant!(
                "GEN-parse-matches-generator",
                parsed.as_ref() == Ok(&ast),
                "generated valid program parsed to {parsed:?}, expected {ast:?}: {}",
                show(&src)
            );
            let want = canonical(&ast);
            invariant!(
                "GEN-canonical-spelling",
                fmt(&src).as_deref() == Ok(want.as_str()),
                "fmt gave {:?}, expected {want:?} for {}",
                fmt(&src),
                show(&src)
            );
            let expanded = expand(&src);
            invariant!(
                "GEN-expand-matches-bound",
                if bound {
                    expanded.is_ok()
                } else {
                    matches!(expanded, Err(SyntaxError::UnboundVar { .. }))
                },
                "bound={bound} but expand gave {:?} for {}",
                expanded.as_ref().map(|_| ()),
                show(&src)
            );
            hit(Counter::StructuredOracleChecked);
        }
        Expected::NestingTooDeep => {
            hit(Counter::StructuredPristine);
            hit(Counter::StructuredTooDeep);
            let parsed = parse(&src);
            invariant!(
                "GEN-parse-matches-generator",
                matches!(parsed, Err(SyntaxError::NestingTooDeep { .. })),
                "program over the nesting/depth limits parsed to {:?}: {}",
                parsed.as_ref().map(|_| ()),
                show(&src)
            );
            hit(Counter::StructuredOracleChecked);
        }
        Expected::IntOutOfRange => {
            hit(Counter::StructuredPristine);
            let parsed = parse(&src);
            invariant!(
                "GEN-parse-matches-generator",
                matches!(parsed, Err(SyntaxError::IntOutOfRange { .. })),
                "program with an out-of-range literal parsed to {:?}: {}",
                parsed.as_ref().map(|_| ()),
                show(&src)
            );
            hit(Counter::StructuredOracleChecked);
        }
    }
}
