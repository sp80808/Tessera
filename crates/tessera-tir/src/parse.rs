//! TIR text parser (`.tir` files).
//!
//! Grammar (whitespace-separated S-expressions; `;` starts a line comment,
//! accepted on input and never emitted):
//!
//! ```text
//! module := func+
//! func   := (func NAME param* (return TY) (body EXPR))
//! param  := (param NAME TY)
//! EXPR   := (int INT TY) | (bool true|false) | (var NAME TY)
//!         | (add TY EXPR EXPR) | (eq EXPR EXPR) | (and EXPR EXPR) | (not EXPR)
//!         | (let NAME TY EXPR EXPR) | (if TY EXPR EXPR EXPR)
//!         | (call NAME TY EXPR*)
//! ```
//!
//! The parser is strict about spelling so that `to_text(parse(text))` equals
//! `text` modulo whitespace/comments: integers are canonical decimal
//! (`-?[0-9]+`, no `+`, no leading zeros, no `-0`). It checks *shape* only;
//! typing and scoping belong to the verifier.
//!
//! **Total and non-recursive.** The reader builds a flat arena of lists, the
//! shape pass walks it with an explicit stack, and trees are assembled
//! bottom-up in a loop. No recursion depends on the input, so no input can
//! overflow the stack while parsing, however deeply nested. The only bound is
//! [`MAX_TIR_DEPTH`], which protects the *recursive consumers* (printing,
//! verification, clone, compare, drop) that run on an accepted tree.

use std::fmt;

use tessera_phases::{FileId, Provenance, ProvenanceMap, Span};

use crate::{
    FunctionProvenance, ModuleProvenance, TirExpr, TirFunction, TirModule, TirNodeId, TirParam,
    TirType,
};

/// Bound on expression nesting accepted from text. Larger than the frontend's
/// `MAX_EXPR_DEPTH` (a bounded chain of that length is one node deeper than the
/// limit) with headroom for TIR-only nesting such as `let`/`if`. Every recursive
/// consumer of the deepest accepted tree must fit a default 2 MiB test-thread
/// stack in a debug build (`deepest_accepted_input_survives_a_default_test_stack`).
pub const MAX_TIR_DEPTH: usize = 1100;

/// A malformed `.tir` text. `at` is a byte offset into the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TirParseError {
    pub at: usize,
    pub message: String,
}

impl fmt::Display for TirParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "offset {}: {}", self.at, self.message)
    }
}

impl std::error::Error for TirParseError {}

type Result<T> = std::result::Result<T, TirParseError>;

fn error<T>(at: usize, message: impl Into<String>) -> Result<T> {
    Err(TirParseError {
        at,
        message: message.into(),
    })
}

/// `[A-Za-z_][A-Za-z0-9_]*`, matching the TC identifier alphabet.
pub(crate) fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `-?[0-9]+` with no leading zeros and no negative zero: exactly the set
/// `i64::to_string` can produce.
fn is_canonical_int(atom: &str) -> bool {
    let digits = atom.strip_prefix('-').unwrap_or(atom);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && ((digits == "0" && atom == "0") || !digits.starts_with('0'))
}

// ---------- reader: text -> flat arena of lists (no recursion) ----------

#[derive(Clone, Copy)]
enum Elem {
    Atom(usize, usize),
    /// Index into [`Sexps::nodes`].
    List(usize),
}

struct Node {
    /// Offset of `(` (0 for the virtual root).
    start: usize,
    /// Offset of the matching `)` (input length for the virtual root).
    end: usize,
    elems: Vec<Elem>,
}

/// Every parenthesized list in source order; index 0 is a virtual root whose
/// elements are the top-level forms. Parents always precede their children.
struct Sexps<'a> {
    text: &'a str,
    nodes: Vec<Node>,
}

fn read(text: &str) -> Result<Sexps<'_>> {
    let bytes = text.as_bytes();
    let mut nodes = vec![Node {
        start: 0,
        end: text.len(),
        elems: Vec::new(),
    }];
    let mut open = vec![0_usize];
    let mut i = 0;
    while i < bytes.len() {
        let parent = open[open.len() - 1];
        match bytes[i] {
            b'(' => {
                let idx = nodes.len();
                nodes.push(Node {
                    start: i,
                    end: i,
                    elems: Vec::new(),
                });
                nodes[parent].elems.push(Elem::List(idx));
                open.push(idx);
                i += 1;
            }
            b')' => {
                if open.len() == 1 {
                    return error(i, "unexpected `)`");
                }
                nodes[parent].end = i;
                open.pop();
                i += 1;
            }
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                // Delimiters are ASCII, so `start..i` lies on char boundaries.
                while i < bytes.len()
                    && !matches!(bytes[i], b'(' | b')' | b';')
                    && !bytes[i].is_ascii_whitespace()
                {
                    i += 1;
                }
                if parent == 0 {
                    return error(start, format!("expected `(`, found `{}`", &text[start..i]));
                }
                nodes[parent].elems.push(Elem::Atom(start, i));
            }
        }
    }
    if open.len() > 1 {
        return error(text.len(), "expected `)`, found end of input");
    }
    Ok(Sexps { text, nodes })
}

// ---------- shape pass: explicit stack, validates atoms and arities ----------

/// The operation of one expression node with its atoms already validated.
enum Shape {
    Int(i64, TirType),
    Bool(bool),
    Var(String, TirType),
    Add(TirType),
    Eq,
    And,
    Not,
    Let(String, TirType),
    If(TirType),
    Call(String, TirType),
}

struct Planned {
    shape: Shape,
    /// Arena indices of the child expressions, in source order.
    children: Vec<usize>,
}

impl<'a> Sexps<'a> {
    fn atom_text(&self, start: usize, end: usize) -> &'a str {
        &self.text[start..end]
    }

    /// What stands at `node.elems[idx]`, for "found ..." messages, and where.
    fn found(&self, node: usize, idx: usize) -> (usize, String) {
        match self.nodes[node].elems.get(idx) {
            Some(Elem::Atom(s, e)) => (*s, format!("`{}`", self.atom_text(*s, *e))),
            Some(Elem::List(n)) => (self.nodes[*n].start, "`(`".to_owned()),
            None => (self.nodes[node].end, "`)`".to_owned()),
        }
    }

    fn atom(&self, node: usize, idx: usize, what: &str) -> Result<(&'a str, usize)> {
        match self.nodes[node].elems.get(idx) {
            Some(Elem::Atom(s, e)) => Ok((self.atom_text(*s, *e), *s)),
            _ => {
                let (at, found) = self.found(node, idx);
                error(at, format!("expected {what}, found {found}"))
            }
        }
    }

    fn keyword(&self, node: usize, idx: usize, kw: &str) -> Result<()> {
        let (got, at) = self.atom(node, idx, &format!("`{kw}`"))?;
        if got == kw {
            Ok(())
        } else {
            error(at, format!("expected `{kw}`, found `{got}`"))
        }
    }

    fn name(&self, node: usize, idx: usize, what: &str) -> Result<String> {
        let (atom, at) = self.atom(node, idx, what)?;
        if is_valid_name(atom) {
            Ok(atom.to_owned())
        } else {
            error(
                at,
                format!("invalid {what} `{atom}` (expected [A-Za-z_][A-Za-z0-9_]*)"),
            )
        }
    }

    fn ty(&self, node: usize, idx: usize) -> Result<TirType> {
        let (atom, at) = self.atom(node, idx, "a type")?;
        TirType::parse(atom).map_or_else(|| error(at, format!("unknown type `{atom}`")), Ok)
    }

    fn list(&self, node: usize, idx: usize) -> Result<usize> {
        match self.nodes[node].elems.get(idx) {
            Some(Elem::List(n)) => Ok(*n),
            _ => {
                let (at, found) = self.found(node, idx);
                error(at, format!("expected `(`, found {found}"))
            }
        }
    }

    /// The list must have no elements from `idx` on.
    fn end(&self, node: usize, idx: usize) -> Result<()> {
        if idx < self.nodes[node].elems.len() {
            let (at, found) = self.found(node, idx);
            error(at, format!("expected `)`, found {found}"))
        } else {
            Ok(())
        }
    }

    /// Head atom of a list, if its first element is an atom.
    fn head(&self, node: usize) -> Option<(&'a str, usize)> {
        match self.nodes[node].elems.first() {
            Some(Elem::Atom(s, e)) => Some((self.atom_text(*s, *e), *s)),
            _ => None,
        }
    }

    fn int_literal(&self, node: usize) -> Result<Shape> {
        let (atom, at) = self.atom(node, 1, "an integer")?;
        if !is_canonical_int(atom) {
            return error(at, format!("`{atom}` is not a canonical decimal integer"));
        }
        let Ok(value) = atom.parse::<i64>() else {
            return error(at, format!("integer `{atom}` out of i64 range"));
        };
        let ty = self.ty(node, 2)?;
        self.end(node, 3)?;
        Ok(Shape::Int(value, ty))
    }

    fn bool_literal(&self, node: usize) -> Result<Shape> {
        let (atom, at) = self.atom(node, 1, "`true` or `false`")?;
        let value = match atom {
            "true" => true,
            "false" => false,
            other => {
                return error(at, format!("expected `true` or `false`, found `{other}`"));
            }
        };
        self.end(node, 2)?;
        Ok(Shape::Bool(value))
    }

    /// Validate one expression node's atoms and arity; report its child lists.
    fn shape_of(&self, node: usize) -> Result<Planned> {
        let Some((head, head_at)) = self.head(node) else {
            let (at, found) = self.found(node, 0);
            return error(at, format!("expected an operation name, found {found}"));
        };
        let mut children = Vec::new();
        let shape = match head {
            "int" => self.int_literal(node)?,
            "bool" => self.bool_literal(node)?,
            "var" => {
                let name = self.name(node, 1, "variable name")?;
                let ty = self.ty(node, 2)?;
                self.end(node, 3)?;
                Shape::Var(name, ty)
            }
            "add" => {
                let ty = self.ty(node, 1)?;
                children = vec![self.list(node, 2)?, self.list(node, 3)?];
                self.end(node, 4)?;
                Shape::Add(ty)
            }
            "eq" | "and" => {
                children = vec![self.list(node, 1)?, self.list(node, 2)?];
                self.end(node, 3)?;
                if head == "eq" { Shape::Eq } else { Shape::And }
            }
            "not" => {
                children = vec![self.list(node, 1)?];
                self.end(node, 2)?;
                Shape::Not
            }
            "let" => {
                let name = self.name(node, 1, "binding name")?;
                let ty = self.ty(node, 2)?;
                children = vec![self.list(node, 3)?, self.list(node, 4)?];
                self.end(node, 5)?;
                Shape::Let(name, ty)
            }
            "if" => {
                let ty = self.ty(node, 1)?;
                children = vec![
                    self.list(node, 2)?,
                    self.list(node, 3)?,
                    self.list(node, 4)?,
                ];
                self.end(node, 5)?;
                Shape::If(ty)
            }
            "call" => {
                let callee = self.name(node, 1, "callee name")?;
                let ty = self.ty(node, 2)?;
                for idx in 3..self.nodes[node].elems.len() {
                    children.push(self.list(node, idx)?);
                }
                Shape::Call(callee, ty)
            }
            other => return error(head_at, format!("unknown operation `{other}`")),
        };
        Ok(Planned { shape, children })
    }

    /// Plan the expression rooted at `root`: every node in source (pre-)order
    /// with its shape. Iterative; enforces [`MAX_TIR_DEPTH`].
    fn plan(&self, root: usize) -> Result<Vec<(usize, Planned)>> {
        let mut order = Vec::new();
        let mut stack = vec![(root, 0_usize)];
        while let Some((node, depth)) = stack.pop() {
            if depth >= MAX_TIR_DEPTH {
                return error(
                    self.nodes[node].start,
                    format!("expression nested deeper than the limit ({MAX_TIR_DEPTH})"),
                );
            }
            let planned = self.shape_of(node)?;
            for &child in planned.children.iter().rev() {
                stack.push((child, depth + 1));
            }
            order.push((node, planned));
        }
        Ok(order)
    }

    /// Assemble the planned tree bottom-up: children have larger arena indices
    /// than their parent, so a reverse pass sees every child before its parent.
    fn assemble(&self, root: usize, order: Vec<(usize, Planned)>) -> Result<TirExpr> {
        let mut slots: Vec<Option<TirExpr>> = Vec::new();
        slots.resize_with(self.nodes.len(), || None);
        for (node, planned) in order.into_iter().rev() {
            let at = self.nodes[node].start;
            let kids: Option<Vec<TirExpr>> =
                planned.children.iter().map(|&c| slots[c].take()).collect();
            let built = kids.and_then(|kids| build(planned.shape, kids));
            slots[node] = Some(built.ok_or_else(|| TirParseError {
                at,
                message: "internal error: inconsistent expression plan".to_owned(),
            })?);
        }
        slots[root].take().map_or_else(
            || {
                error(
                    self.nodes[root].start,
                    "internal error: root expression missing",
                )
            },
            Ok,
        )
    }

    /// Source span of arena list `node`, from its `(` through its `)`.
    fn span(&self, file: FileId, node: usize) -> Span {
        let c = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let n = &self.nodes[node];
        Span::new(file, c(n.start), c(n.end.saturating_add(1)))
    }

    /// The expression at `root` and the span of each node, indexed by
    /// [`TirNodeId`] (the plan is in pre-order, which is exactly that order).
    fn expr(&self, file: FileId, root: usize) -> Result<(TirExpr, ProvenanceMap<TirNodeId>)> {
        let order = self.plan(root)?;
        let mut nodes = ProvenanceMap::new();
        for (i, (node, _)) in order.iter().enumerate() {
            let id = TirNodeId(u32::try_from(i).unwrap_or(u32::MAX));
            nodes.insert(id, Provenance::Source(self.span(file, *node)));
        }
        Ok((self.assemble(root, order)?, nodes))
    }

    fn func(&self, file: FileId, node: usize) -> Result<(TirFunction, FunctionProvenance)> {
        self.keyword(node, 0, "func")?;
        let name = self.name(node, 1, "function name")?;
        let mut idx = 2;
        let mut params = Vec::new();
        let mut param_prov = Vec::new();
        while let Some(Elem::List(p)) = self.nodes[node].elems.get(idx) {
            if self.head(*p).map(|(h, _)| h) != Some("param") {
                break;
            }
            let pname = self.name(*p, 1, "parameter name")?;
            let ty = self.ty(*p, 2)?;
            self.end(*p, 3)?;
            params.push(TirParam { name: pname, ty });
            param_prov.push(Provenance::Source(self.span(file, *p)));
            idx += 1;
        }
        let ret_node = self.list(node, idx)?;
        self.keyword(ret_node, 0, "return")?;
        let ret = self.ty(ret_node, 1)?;
        self.end(ret_node, 2)?;
        let body_node = self.list(node, idx + 1)?;
        self.keyword(body_node, 0, "body")?;
        let body_expr = self.list(body_node, 1)?;
        self.end(body_node, 2)?;
        self.end(node, idx + 2)?;
        let (body, nodes) = self.expr(file, body_expr)?;
        let provenance = FunctionProvenance {
            func: Provenance::Source(self.span(file, node)),
            params: param_prov,
            nodes,
        };
        Ok((
            TirFunction {
                name,
                params,
                ret,
                body,
            },
            provenance,
        ))
    }

    fn module(&self, file: FileId) -> Result<(TirModule, ModuleProvenance)> {
        let mut funcs = Vec::new();
        let mut provenance = ModuleProvenance::default();
        for idx in 0..self.nodes[0].elems.len() {
            let (func, prov) = self.func(file, self.list(0, idx)?)?;
            funcs.push(func);
            provenance.funcs.push(prov);
        }
        if funcs.is_empty() {
            return error(0, "empty module: expected `(func ...)`");
        }
        Ok((TirModule { funcs }, provenance))
    }
}

/// Build a node from its shape and already-built children (source order).
/// `None` only if the plan and the shape disagree on arity (never, by
/// construction, but the parser must stay panic-free even so).
fn build(shape: Shape, kids: Vec<TirExpr>) -> Option<TirExpr> {
    let mut it = kids.into_iter();
    let mut next = || it.next().map(Box::new);
    Some(match shape {
        Shape::Int(value, ty) => TirExpr::Int { value, ty },
        Shape::Bool(value) => TirExpr::Bool { value },
        Shape::Var(name, ty) => TirExpr::Var { name, ty },
        Shape::Add(ty) => TirExpr::Add {
            lhs: next()?,
            rhs: next()?,
            ty,
        },
        Shape::Eq => TirExpr::Eq {
            lhs: next()?,
            rhs: next()?,
        },
        Shape::And => TirExpr::And {
            lhs: next()?,
            rhs: next()?,
        },
        Shape::Not => TirExpr::Not { expr: next()? },
        Shape::Let(name, ty) => TirExpr::Let {
            name,
            ty,
            init: next()?,
            body: next()?,
        },
        Shape::If(ty) => TirExpr::If {
            cond: next()?,
            then_branch: next()?,
            else_branch: next()?,
            ty,
        },
        Shape::Call(callee, ty) => TirExpr::Call {
            callee,
            args: std::iter::from_fn(next).map(|b| *b).collect(),
            ty,
        },
    })
}

pub(crate) fn parse_module(file: FileId, text: &str) -> Result<(TirModule, ModuleProvenance)> {
    read(text)?.module(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_int_spelling_is_exactly_what_to_string_emits() {
        for ok in [
            "0",
            "7",
            "-7",
            "9223372036854775807",
            "-9223372036854775808",
        ] {
            assert!(is_canonical_int(ok), "{ok}");
        }
        for bad in ["", "-", "+1", "01", "-0", "00", "1_0", "0x1", "1.0", "--1"] {
            assert!(!is_canonical_int(bad), "{bad}");
        }
    }

    #[test]
    fn names_follow_the_tc_identifier_alphabet() {
        assert!(is_valid_name("a") && is_valid_name("_x9") && is_valid_name("Add"));
        for bad in ["", "9a", "a-b", "a b", "𝑥", "a.b"] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn errors_carry_byte_offsets_and_causes() {
        let e = TirModule::parse("(func f (return i64) (body (int 01 i64)))").unwrap_err();
        assert_eq!(e.at, 32, "{e}");
        assert!(e.message.contains("canonical"), "{e}");
        let e = TirModule::parse("").unwrap_err();
        assert!(e.message.contains("empty module"), "{e}");
        let e = TirModule::parse("(func f (return i32) (body (int 1 i64)))").unwrap_err();
        assert_eq!((e.at, e.message.as_str()), (16, "unknown type `i32`"));
        let e = TirModule::parse("(func f (return i64) (body (frob)))").unwrap_err();
        assert_eq!(e.at, 28);
        assert!(e.message.contains("unknown operation `frob`"), "{e}");
        let e = TirModule::parse("(func f (return i64) (body (int 1 i64))").unwrap_err();
        assert_eq!(e.at, 39);
        assert!(e.message.contains("end of input"), "{e}");
        let e = TirModule::parse("(func f (return i64) (body (int 1 i64))))").unwrap_err();
        assert_eq!((e.at, e.message.as_str()), (40, "unexpected `)`"));
        let e = TirModule::parse("junk").unwrap_err();
        assert_eq!(
            (e.at, e.message.as_str()),
            (0, "expected `(`, found `junk`")
        );
        let e = TirModule::parse("(func f (return i64) (body (not x)))").unwrap_err();
        assert_eq!((e.at, e.message.as_str()), (32, "expected `(`, found `x`"));
        let e = TirModule::parse("(func f (return i64) (body (int 1 i64 extra)))").unwrap_err();
        assert_eq!(
            (e.at, e.message.as_str()),
            (38, "expected `)`, found `extra`")
        );
    }

    #[test]
    fn comments_and_whitespace_are_input_only_trivia() {
        let m = TirModule::parse(
            "; leading\n(func  f\n  (return i64) ; ret\n  (body (int 1 i64)))\n; trailing",
        )
        .unwrap();
        assert_eq!(m.to_text(), "(func f (return i64) (body (int 1 i64)))");
    }

    fn wrap(depth: usize) -> String {
        format!(
            "(func f (return bool) (body {}(bool true){}))",
            "(not ".repeat(depth),
            ")".repeat(depth)
        )
    }

    /// Parsing is non-recursive, so even absurd nesting is a clean error and
    /// needs no big stack.
    #[test]
    fn hostile_nesting_is_an_error_not_a_crash() {
        let e = TirModule::parse(&wrap(300_000)).unwrap_err();
        assert!(e.message.contains("deeper than the limit"), "{e}");
        // ... even on a tiny stack: the parser must not recurse on input depth
        std::thread::Builder::new()
            .stack_size(128 << 10)
            .spawn(|| assert!(TirModule::parse(&wrap(300_000)).is_err()))
            .unwrap()
            .join()
            .unwrap();
    }

    /// Every recursive consumer of the deepest accepted tree must fit the default
    /// 2 MiB test-thread stack in a debug build (the worst case for frame size).
    #[test]
    fn deepest_accepted_input_survives_a_default_test_stack() {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let n = MAX_TIR_DEPTH - 1;
                let text = wrap(n);
                let m = TirModule::parse(&text).expect("at the limit is accepted");
                assert_eq!(m.to_text(), text);
                assert_eq!(crate::verify_module(&m), vec![]);
                let copy = m.clone();
                assert_eq!(copy, m);
                drop(copy);
                assert!(TirModule::parse(&wrap(n + 1)).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// Mutate valid seeds at the token level: most mutants are rejected, some
    /// (whitespace, renames, reordered args) are accepted. The parser must never
    /// panic, and whatever it accepts must print back to text that reparses to
    /// the same module (the normalization is a fixed point).
    #[test]
    fn mutated_seeds_never_panic_and_accepted_text_reaches_a_fixed_point() {
        const SEEDS: &[&str] = &[
            "(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))",
            "(func abs (param x i64) (return i64) (body (if i64 (eq (var x i64) (int 0 i64)) (int 0 i64) (var x i64))))",
            "(func f (param a i64) (return i64) (body (let x i64 (add i64 (var a i64) (int -1 i64)) (call g i64 (var x i64) (var a i64)))))",
            "(func t (return bool) (body (and (bool true) (not (bool false)))))",
        ];
        const JUNK: &[&str] = &[
            "(",
            ")",
            "int",
            "bool",
            "i64",
            "-0",
            "+1",
            "07",
            "true",
            "x",
            "𝑥",
            ";",
            "\n",
            " ",
            "add",
            "call",
            "let",
            "if",
            "\u{0}",
            "9223372036854775808",
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let (mut accepted, mut rejected) = (0_u32, 0_u32);
        for round in 0..40_000_u32 {
            let seed = SEEDS[(round as usize) % SEEDS.len()];
            let mut pieces: Vec<String> = seed
                .split_inclusive([' ', '(', ')'])
                .map(str::to_owned)
                .collect();
            for _ in 0..=(next() % 3) {
                if pieces.is_empty() {
                    break;
                }
                let at = (next() as usize) % pieces.len();
                match next() % 4 {
                    0 => {
                        pieces.remove(at);
                    }
                    1 => pieces.insert(at, JUNK[(next() as usize) % JUNK.len()].to_owned()),
                    2 => {
                        let dup = pieces[at].clone();
                        pieces.insert(at, dup);
                    }
                    _ => pieces[at] = JUNK[(next() as usize) % JUNK.len()].to_owned(),
                }
            }
            let text: String = pieces.concat();
            match TirModule::parse(&text) {
                Ok(module) => {
                    accepted += 1;
                    let printed = module.to_text();
                    let again = TirModule::parse(&printed).expect("printed text parses");
                    assert_eq!(again, module, "{text:?}");
                    assert_eq!(again.to_text(), printed);
                }
                Err(_) => rejected += 1,
            }
        }
        assert!(accepted > 1_000, "property barely exercised: {accepted}");
        assert!(rejected > 1_000, "mutations too tame: {rejected}");
    }
}
