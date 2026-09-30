//! Lossless CST and tolerant parser for the provisional TC subset (#18).
//!
//! Layering (rust-analyzer style, contract B1 in
//! `docs/architecture/compiler-phases.md`):
//!
//! ```text
//! source --lexer--> tokens (incl. trivia)
//!        --parser--> Vec<Event>          (no tree storage involved)
//!        --builder-> Cst                 (arena; replaceable storage)
//! ```
//!
//! - The parser only emits [`Event`]s over non-trivia tokens; trivia placement
//!   is the builder's deterministic job, so storage/attachment policy can
//!   change without touching the grammar.
//! - Parsing never fails: malformed input yields a tree with explicit
//!   [`NodeKind::Error`] (consumed unexpected tokens) and [`NodeKind::Missing`]
//!   (zero-width, expected-but-absent) nodes plus diagnostics.
//! - The CST carries no name/type meaning. `i64` is just an identifier here;
//!   whether it is a type is a later phase's decision.
//!
//! Node kinds describe the *provisional* grammar and are replaceable with it.

use std::fmt::Write as _;

use tessera_phases::{Diagnostic, DiagnosticSet, FileId, Phase, PhaseOutput, Provenance, Span};

use crate::lexer::{self, Token, TokenKind};
use crate::{MAX_EXPR_DEPTH, MAX_NESTING};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Fn,
    ParamList,
    Param,
    TypeRef,
    BinExpr,
    ParenExpr,
    PathExpr,
    LiteralExpr,
    /// Unexpected tokens consumed during recovery.
    Error,
    /// Zero-width marker where a required element was absent.
    Missing,
}

impl NodeKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Fn => "Fn",
            Self::ParamList => "ParamList",
            Self::Param => "Param",
            Self::TypeRef => "TypeRef",
            Self::BinExpr => "BinExpr",
            Self::ParenExpr => "ParenExpr",
            Self::PathExpr => "PathExpr",
            Self::LiteralExpr => "LiteralExpr",
            Self::Error => "Error",
            Self::Missing => "Missing",
        }
    }
}

/// Parser output, independent of how the tree is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Start(NodeKind),
    /// Consume the token at this index in the full (trivia-inclusive) stream.
    Token(usize),
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    Node(usize),
    /// Index into [`ParsedFile::tokens`].
    Token(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeData {
    kind: NodeKind,
    start: usize,
    end: usize,
    children: Vec<Child>,
}

/// Arena-stored concrete syntax tree. Storage is private so it can be
/// replaced (green/red, interned, ...) without touching consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cst {
    nodes: Vec<NodeData>,
    root: usize,
}

impl Cst {
    #[must_use]
    pub fn root(&self) -> usize {
        self.root
    }

    #[must_use]
    pub fn kind(&self, node: usize) -> NodeKind {
        self.nodes[node].kind
    }

    /// Byte range of the node (first to last child; leading/trailing trivia
    /// belongs to the parent).
    #[must_use]
    pub fn range(&self, node: usize) -> (usize, usize) {
        (self.nodes[node].start, self.nodes[node].end)
    }

    #[must_use]
    pub fn children(&self, node: usize) -> &[Child] {
        &self.nodes[node].children
    }

    /// Every node kind in the tree (pre-order); handy for assertions.
    #[must_use]
    pub fn kinds(&self) -> Vec<NodeKind> {
        let mut out = Vec::new();
        self.walk(self.root, &mut |n| out.push(self.nodes[n].kind));
        out
    }

    fn walk(&self, node: usize, f: &mut impl FnMut(usize)) {
        f(node);
        for child in &self.nodes[node].children {
            if let Child::Node(n) = child {
                self.walk(*n, f);
            }
        }
    }

    /// Build a tree from an event stream over `tokens` (the full,
    /// trivia-inclusive stream), placing trivia exactly as the parser's own
    /// builder does. This is how a front-end other than the built-in parser, or
    /// a test that wants tree shapes the parser never emits, produces a
    /// [`Cst`] without touching private storage.
    ///
    /// Returns `None` unless the stream is well-formed: exactly one root node,
    /// balanced `Start`/`Finish`, and `Token` indices in range and strictly
    /// increasing. Every event stream the parser emits satisfies this.
    #[must_use]
    pub fn from_events(tokens: &[Token], events: &[Event]) -> Option<Cst> {
        let (mut depth, mut roots, mut next_tok) = (0_usize, 0_usize, 0_usize);
        for event in events {
            match *event {
                Event::Start(_) => {
                    if depth == 0 {
                        roots += 1;
                    }
                    depth += 1;
                }
                Event::Token(i) => {
                    if depth == 0 || i >= tokens.len() || i < next_tok {
                        return None;
                    }
                    next_tok = i + 1;
                }
                Event::Finish => depth = depth.checked_sub(1)?,
            }
        }
        (depth == 0 && roots == 1).then(|| build(tokens, events))
    }
}

/// The result of parsing one file: token stream plus tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    pub file: FileId,
    pub tokens: Vec<Token>,
    pub cst: Cst,
}

impl ParsedFile {
    /// Concatenate every token in tree order. Equals the source iff the tree
    /// is lossless (CST-1).
    #[must_use]
    pub fn reconstruct(&self, src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        self.collect(self.cst.root, src, &mut out);
        out
    }

    fn collect(&self, node: usize, src: &str, out: &mut String) {
        for child in self.cst.children(node) {
            match child {
                Child::Node(n) => self.collect(*n, src, out),
                Child::Token(t) => out.push_str(self.tokens[*t].text(src)),
            }
        }
    }

    /// Deterministic snapshot format: indented nodes with byte ranges, tokens
    /// as `Kind start..end "text"`.
    #[must_use]
    pub fn dump(&self, src: &str) -> String {
        let mut out = String::new();
        self.dump_node(self.cst.root, 0, src, &mut out);
        out
    }

    fn dump_node(&self, node: usize, depth: usize, src: &str, out: &mut String) {
        let (start, end) = self.cst.range(node);
        let _ = writeln!(
            out,
            "{:indent$}{} {start}..{end}",
            "",
            self.cst.kind(node).name(),
            indent = depth * 2
        );
        for child in self.cst.children(node) {
            match child {
                Child::Node(n) => self.dump_node(*n, depth + 1, src, out),
                Child::Token(t) => {
                    let tok = &self.tokens[*t];
                    let _ = writeln!(
                        out,
                        "{:indent$}{} {}..{} {:?}",
                        "",
                        tok.kind.name(),
                        tok.start,
                        tok.end,
                        tok.text(src),
                        indent = (depth + 1) * 2
                    );
                }
            }
        }
    }
}

fn to_span(file: FileId, start: usize, end: usize) -> Span {
    let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    Span::new(file, clamp(start), clamp(end))
}

// ---------- parser (emits events only) ----------

struct Parser<'a> {
    file: FileId,
    src: &'a str,
    tokens: &'a [Token],
    /// Indices of non-trivia tokens.
    sig: Vec<usize>,
    pos: usize,
    events: Vec<Event>,
    diagnostics: DiagnosticSet,
    depth: usize,
    /// After a nesting-limit error, suppress cascading diagnostics.
    poisoned: bool,
}

/// Tokens that end recovery: a required element is reported `Missing` instead
/// of swallowing one of these.
const fn is_sync(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RParen | TokenKind::Eq | TokenKind::Gt | TokenKind::Comma
    )
}

impl<'a> Parser<'a> {
    fn cur(&self) -> Option<TokenKind> {
        self.sig.get(self.pos).map(|&i| self.tokens[i].kind)
    }

    fn cur_text(&self) -> &'a str {
        self.sig
            .get(self.pos)
            .map_or("", |&i| self.tokens[i].text(self.src))
    }

    /// Where a diagnostic about the current position should point.
    fn here(&self) -> Span {
        match self.sig.get(self.pos) {
            Some(&i) => to_span(self.file, self.tokens[i].start, self.tokens[i].end),
            None => {
                let end = self.tokens.last().map_or(0, |t| t.end);
                to_span(self.file, end, end)
            }
        }
    }

    fn diag(&mut self, code: &'static str, message: String) {
        if !self.poisoned {
            let at = Provenance::Source(self.here());
            self.diagnostics
                .push(Diagnostic::error(Phase::Syntax, code, message, at));
        }
    }

    fn found(&self) -> &'static str {
        self.cur().map_or("end of input", TokenKind::describe)
    }

    fn bump(&mut self) {
        if let Some(&i) = self.sig.get(self.pos) {
            self.events.push(Event::Token(i));
            self.pos += 1;
        }
    }

    fn start(&mut self, kind: NodeKind) {
        self.events.push(Event::Start(kind));
    }

    fn finish(&mut self) {
        self.events.push(Event::Finish);
    }

    fn missing(&mut self) {
        self.start(NodeKind::Missing);
        self.finish();
    }

    /// Report `what` as absent. Sync tokens and end of input become a
    /// zero-width `Missing`; anything else is consumed inside an `Error`.
    /// Always terminates: either it consumes a token or the caller moves on.
    fn recover(&mut self, what: &str) {
        let found = self.found();
        self.diag(
            "E-syntax-expected",
            format!("expected {what}, found {found}"),
        );
        match self.cur() {
            None => self.missing(),
            Some(kind) if is_sync(kind) => self.missing(),
            Some(_) => {
                self.start(NodeKind::Error);
                self.bump();
                self.finish();
            }
        }
    }

    fn expect(&mut self, kind: TokenKind) {
        if self.cur() == Some(kind) {
            self.bump();
        } else {
            self.recover(kind.describe());
        }
    }

    fn expect_ident(&mut self, what: &str) {
        if self.cur() == Some(TokenKind::Ident) {
            self.bump();
        } else {
            self.recover(what);
        }
    }

    fn parse_file(&mut self) {
        self.start(NodeKind::File);
        if self.sig.is_empty() {
            self.diag(
                "E-syntax-empty-program",
                "empty program: expected one `f` function".to_owned(),
            );
        } else {
            self.parse_fn();
            if self.cur().is_some() {
                self.diag(
                    "E-syntax-trailing-input",
                    "trailing input after function".to_owned(),
                );
                self.start(NodeKind::Error);
                while self.cur().is_some() {
                    self.bump();
                }
                self.finish();
            }
        }
        self.finish();
    }

    fn parse_fn(&mut self) {
        self.start(NodeKind::Fn);
        // `f` is a parser-level keyword. A different identifier means the
        // keyword was omitted: report and keep going as if it were present.
        if self.cur() == Some(TokenKind::Ident) && self.cur_text() == "f" {
            self.bump();
        } else if self.cur() == Some(TokenKind::Ident) {
            self.diag(
                "E-syntax-expected",
                format!("expected `f`, found identifier `{}`", self.cur_text()),
            );
            self.missing();
        } else {
            self.recover("`f`");
        }
        self.expect_ident("function name");
        self.parse_param_list();
        self.expect(TokenKind::Gt);
        self.parse_type();
        self.expect(TokenKind::Eq);
        self.parse_expr();
        self.finish();
    }

    fn parse_param_list(&mut self) {
        self.start(NodeKind::ParamList);
        self.expect(TokenKind::LParen);
        if !matches!(self.cur(), None | Some(TokenKind::RParen)) {
            loop {
                self.parse_param();
                if self.cur() == Some(TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen);
        self.finish();
    }

    fn parse_param(&mut self) {
        self.start(NodeKind::Param);
        self.expect_ident("parameter name");
        self.expect(TokenKind::Colon);
        self.parse_type();
        self.finish();
    }

    fn parse_type(&mut self) {
        self.start(NodeKind::TypeRef);
        self.expect_ident("type");
        self.finish();
    }

    /// Parse a `+` chain; return its expression depth (see [`MAX_EXPR_DEPTH`]).
    fn parse_expr(&mut self) -> usize {
        let start = self.events.len();
        let mut depth = self.parse_primary();
        // Left-associative: every link of `a+b+c` starts at the same event as
        // its lhs, so all the `Start(BinExpr)` events are inserted in ONE
        // splice after the chain is parsed (per-link insertion is quadratic).
        let mut links = 0;
        while self.cur() == Some(TokenKind::Plus) {
            if depth >= MAX_EXPR_DEPTH {
                self.too_deep();
                break;
            }
            self.bump();
            let rhs = self.parse_primary();
            self.finish();
            links += 1;
            // The new link sits above both operands, so it is one deeper than
            // the deeper of them; the budget is the whole path, not this level.
            depth = depth.max(rhs) + 1;
            if depth > MAX_EXPR_DEPTH {
                self.too_deep();
                break;
            }
        }
        if links > 0 {
            self.events.splice(
                start..start,
                std::iter::repeat_n(Event::Start(NodeKind::BinExpr), links),
            );
        }
        depth
    }

    /// Report the depth limit once and swallow the rest of the input into one
    /// `Error` node so every enclosing rule unwinds without more diagnostics.
    fn too_deep(&mut self) {
        if self.poisoned {
            return;
        }
        self.diag(
            "E-syntax-nesting-too-deep",
            format!(
                "expression nested deeper than the limits ({MAX_NESTING} parenthesis levels, {MAX_EXPR_DEPTH} expression depth)"
            ),
        );
        self.poisoned = true;
        self.start(NodeKind::Error);
        while self.cur().is_some() {
            self.bump();
        }
        self.finish();
    }

    /// Parse one operand; return its expression depth (a leaf is 0, a
    /// parenthesized expression one more than its contents).
    fn parse_primary(&mut self) -> usize {
        match self.cur() {
            Some(TokenKind::Int) => {
                self.start(NodeKind::LiteralExpr);
                self.bump();
                self.finish();
                0
            }
            Some(TokenKind::Ident) => {
                self.start(NodeKind::PathExpr);
                self.bump();
                self.finish();
                0
            }
            Some(TokenKind::LParen) => {
                if self.depth >= MAX_NESTING {
                    self.too_deep();
                    return 0;
                }
                self.depth += 1;
                self.start(NodeKind::ParenExpr);
                self.bump();
                let depth = self.parse_expr() + 1;
                self.expect(TokenKind::RParen);
                self.finish();
                self.depth -= 1;
                if depth > MAX_EXPR_DEPTH {
                    self.too_deep();
                }
                depth
            }
            _ => {
                self.recover("expression");
                0
            }
        }
    }
}

// ---------- builder: events -> tree (owns trivia placement) ----------

struct Frame {
    kind: NodeKind,
    children: Vec<Child>,
}

fn build(tokens: &[Token], events: &[Event]) -> Cst {
    let mut nodes: Vec<NodeData> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut next_tok = 0;
    let mut last_end = 0;
    let mut root = 0;

    // Trivia between the last emitted token and the next significant one is
    // attached to whatever node is open when it is flushed: before a `Start`
    // it goes to the parent; before a `Token`, to the current node.
    // `last_end` tracks the end of the last emitted child (trivia included) so
    // zero-width nodes sit exactly where they appear in child order.
    let flush_trivia =
        |stack: &mut Vec<Frame>, next_tok: &mut usize, last_end: &mut usize, upto: usize| {
            while *next_tok < upto && tokens[*next_tok].kind.is_trivia() {
                if let Some(top) = stack.last_mut() {
                    top.children.push(Child::Token(*next_tok));
                }
                *last_end = tokens[*next_tok].end;
                *next_tok += 1;
            }
        };

    for event in events {
        match *event {
            Event::Start(kind) => {
                let frame = Frame {
                    kind,
                    children: Vec::new(),
                };
                if stack.is_empty() {
                    // root: open first so leading trivia has a home
                    stack.push(frame);
                    flush_trivia(&mut stack, &mut next_tok, &mut last_end, tokens.len());
                } else {
                    flush_trivia(&mut stack, &mut next_tok, &mut last_end, tokens.len());
                    stack.push(frame);
                }
            }
            Event::Token(i) => {
                flush_trivia(&mut stack, &mut next_tok, &mut last_end, i);
                if let Some(top) = stack.last_mut() {
                    top.children.push(Child::Token(i));
                }
                next_tok = i + 1;
                last_end = tokens[i].end;
            }
            Event::Finish => {
                let frame = stack.pop().expect("balanced events");
                let start = frame.children.first().map_or(last_end, |c| match c {
                    Child::Node(n) => nodes[*n].start,
                    Child::Token(t) => tokens[*t].start,
                });
                let end = frame.children.last().map_or(last_end, |c| match c {
                    Child::Node(n) => nodes[*n].end,
                    Child::Token(t) => tokens[*t].end,
                });
                let id = nodes.len();
                nodes.push(NodeData {
                    kind: frame.kind,
                    start,
                    end,
                    children: frame.children,
                });
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Child::Node(id)),
                    None => root = id,
                }
            }
        }
    }
    // Trailing trivia belongs to the root. The root is already finished, so
    // append to it directly and widen nothing: the root spans the whole file.
    while next_tok < tokens.len() {
        nodes[root].children.push(Child::Token(next_tok));
        next_tok += 1;
    }
    let end = tokens.last().map_or(0, |t| t.end);
    nodes[root].start = 0;
    nodes[root].end = end;
    Cst { nodes, root }
}

/// Parse `src` into a lossless CST. Never fails; malformed input yields
/// `Error`/`Missing` nodes and diagnostics.
#[must_use]
pub fn parse_file(file: FileId, src: &str) -> PhaseOutput<ParsedFile> {
    let tokens = lexer::lex(src);
    let sig = (0..tokens.len())
        .filter(|&i| !tokens[i].kind.is_trivia())
        .collect();
    let mut parser = Parser {
        file,
        src,
        tokens: &tokens,
        sig,
        pos: 0,
        events: Vec::new(),
        diagnostics: DiagnosticSet::new(),
        depth: 0,
        poisoned: false,
    };
    parser.parse_file();
    let Parser {
        events,
        diagnostics,
        ..
    } = parser;
    let cst = build(&tokens, &events);
    PhaseOutput::with(ParsedFile { file, tokens, cst }, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: FileId = FileId(0);

    fn check(src: &str) -> (ParsedFile, DiagnosticSet) {
        let out = parse_file(F, src);
        assert_eq!(out.value.reconstruct(src), src, "lossless: {src:?}");
        (out.value, out.diagnostics)
    }

    #[test]
    fn bootstrap_golden_tree() {
        let src = "f add(a:i64,b:i64)>i64=a+b\n";
        let (parsed, diags) = check(src);
        assert!(diags.is_empty());
        assert_eq!(
            parsed.dump(src),
            "\
File 0..27
  Fn 0..26
    Ident 0..1 \"f\"
    Whitespace 1..2 \" \"
    Ident 2..5 \"add\"
    ParamList 5..18
      LParen 5..6 \"(\"
      Param 6..11
        Ident 6..7 \"a\"
        Colon 7..8 \":\"
        TypeRef 8..11
          Ident 8..11 \"i64\"
      Comma 11..12 \",\"
      Param 12..17
        Ident 12..13 \"b\"
        Colon 13..14 \":\"
        TypeRef 14..17
          Ident 14..17 \"i64\"
      RParen 17..18 \")\"
    Gt 18..19 \">\"
    TypeRef 19..22
      Ident 19..22 \"i64\"
    Eq 22..23 \"=\"
    BinExpr 23..26
      PathExpr 23..24
        Ident 23..24 \"a\"
      Plus 24..25 \"+\"
      PathExpr 25..26
        Ident 25..26 \"b\"
  Whitespace 26..27 \"\\n\"
"
        );
    }

    #[test]
    fn plus_is_left_associative() {
        let src = "f s()>i64=1+2+3";
        let (parsed, _) = check(src);
        // ((1+2)+3): outer BinExpr's first child is the inner BinExpr
        let kinds = parsed.cst.kinds();
        assert_eq!(kinds.iter().filter(|k| **k == NodeKind::BinExpr).count(), 2);
        let dump = parsed.dump(src);
        let inner = dump
            .find("\n      BinExpr")
            .expect("nested BinExpr is deeper");
        let outer = dump.find("\n    BinExpr").expect("outer BinExpr");
        assert!(outer < inner);
    }

    #[test]
    fn trivia_is_kept_and_placed_deterministically() {
        let src = "// lead\nf x()>i64=1 // tail\n";
        let (parsed, diags) = check(src);
        assert!(diags.is_empty());
        let root = parsed.cst.root();
        // leading comment and whitespace are children of File, before Fn
        let first = parsed.cst.children(root)[0];
        assert!(
            matches!(first, Child::Token(t) if parsed.tokens[t].kind == TokenKind::LineComment)
        );
        // Fn starts at `f`, not at the comment
        let fn_node = parsed
            .cst
            .children(root)
            .iter()
            .find_map(|c| match c {
                Child::Node(n) => Some(*n),
                Child::Token(_) => None,
            })
            .expect("Fn node");
        assert_eq!(parsed.cst.range(fn_node).0, 8);
    }

    #[test]
    fn empty_and_trivia_only_input() {
        for src in ["", "   ", "// only a comment"] {
            let (parsed, diags) = check(src);
            assert!(diags.iter().any(|d| d.code == "E-syntax-empty-program"));
            assert_eq!(parsed.cst.kind(parsed.cst.root()), NodeKind::File);
        }
    }

    #[test]
    fn missing_pieces_become_zero_width_nodes() {
        // body missing after `=`
        let src = "f x()>i64=";
        let (parsed, diags) = check(src);
        assert!(parsed.cst.kinds().contains(&NodeKind::Missing));
        let d: Vec<_> = diags.iter().collect();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, "E-syntax-expected");
        assert_eq!(d[0].message, "expected expression, found end of input");
        assert!(d[0].at.primary_span().is_empty());
        assert_eq!(d[0].at.primary_span().start, 10);
    }

    #[test]
    fn unexpected_tokens_are_wrapped_not_dropped() {
        // `*` where an expression is expected, and junk after the function
        let src = "f x()>i64=* 1";
        let (parsed, diags) = check(src);
        assert!(diags.has_errors());
        assert!(parsed.cst.kinds().contains(&NodeKind::Error));

        let src = "f x()>i64=1 2 3";
        let (parsed, diags) = check(src);
        assert!(diags.iter().any(|d| d.code == "E-syntax-trailing-input"));
        assert!(parsed.cst.kinds().contains(&NodeKind::Error));
    }

    #[test]
    fn omitted_keyword_recovers_and_parses_the_rest() {
        let src = "add(a:i64)>i64=a";
        let (parsed, diags) = check(src);
        assert_eq!(diags.len(), 1);
        assert!(parsed.cst.kinds().contains(&NodeKind::Missing));
        // the rest of the function still has structure
        assert!(parsed.cst.kinds().contains(&NodeKind::ParamList));
        assert!(parsed.cst.kinds().contains(&NodeKind::PathExpr));
    }

    #[test]
    fn hostile_nesting_is_bounded_and_reported_once() {
        let n = 200_000;
        let src = format!("f x()>i64={}1{}", "(".repeat(n), ")".repeat(n));
        let (_, diags) = check(&src);
        let deep: Vec<_> = diags
            .iter()
            .filter(|d| d.code == "E-syntax-nesting-too-deep")
            .collect();
        assert_eq!(deep.len(), 1);
        assert_eq!(diags.len(), 1, "cascade must be suppressed");
    }

    #[test]
    fn long_addition_chains_are_bounded_and_linear() {
        let n = 1_000_000;
        let src = format!("f c(a:i64)>i64={}", vec!["a"; n].join("+"));
        let (parsed, diags) = check(&src);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags.iter().next().unwrap().code,
            "E-syntax-nesting-too-deep"
        );
        // depth of the resulting CST is bounded by the limit
        assert!(
            parsed
                .cst
                .kinds()
                .iter()
                .filter(|k| **k == NodeKind::BinExpr)
                .count()
                <= MAX_EXPR_DEPTH
        );
        // exactly at the limit is fine
        let ok = format!("f c(a:i64)>i64={}", vec!["a"; MAX_EXPR_DEPTH].join("+"));
        assert!(check(&ok).1.is_empty());
    }

    #[test]
    fn parse_is_deterministic() {
        let src = "f x(a:i64,)>i64=(a+";
        assert_eq!(parse_file(F, src), parse_file(F, src));
    }

    fn assert_well_formed(parsed: &ParsedFile, src: &str) {
        let cst = &parsed.cst;
        assert_eq!(cst.range(cst.root()), (0, src.len()));
        let mut stack = vec![cst.root()];
        while let Some(n) = stack.pop() {
            let (s, e) = cst.range(n);
            assert!(s <= e && e <= src.len());
            let mut cursor = s;
            for child in cst.children(n) {
                let (cs, ce) = match child {
                    Child::Node(c) => {
                        stack.push(*c);
                        cst.range(*c)
                    }
                    Child::Token(t) => (parsed.tokens[*t].start, parsed.tokens[*t].end),
                };
                // children are ordered, non-overlapping and inside the parent
                assert!(cs >= cursor, "order in {n}");
                assert!(cs >= s && ce <= e, "child outside parent");
                cursor = ce;
            }
        }
    }

    /// Differential check against the frozen fail-fast oracle (`legacy`): they must
    /// agree on syntactic acceptance. (Semantic rejections — unknown type,
    /// integer range — are not syntax and are allowed to differ.)
    fn agree(src: &str) {
        let out = parse_file(F, src);
        assert_eq!(out.value.reconstruct(src), src);
        assert_well_formed(&out.value, src);
        match crate::legacy::parse(src) {
            Ok(_) => assert!(out.is_clean(), "legacy accepts, cst rejects: {src:?}"),
            Err(
                crate::SyntaxError::Unexpected { .. }
                | crate::SyntaxError::TrailingInput { .. }
                | crate::SyntaxError::EmptyProgram
                | crate::SyntaxError::NestingTooDeep { .. },
            ) => assert!(
                out.diagnostics.has_errors(),
                "legacy rejects syntax, cst clean: {src:?}"
            ),
            Err(_) => {}
        }
    }

    #[test]
    fn agrees_with_bootstrap_parser_on_corpus_and_random_text() {
        for src in [
            "f add(a:i64,b:i64)>i64=a+b",
            "f add( a:i64 , b:i64 ) > i64 = a + b",
            "f zero()>i64=42",
            "f n(a:i64,b:i64)>i64=(a+b)+1",
            "f a(x:i64,)>i64=x",
            "f a(x:i64)>i64=",
            "f a(x:i64)>i64=1 extra",
            "f 1(a:i64)>i64=a",
            "f a(x:i64>i64=x",
            "",
        ] {
            agree(src);
        }
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
            "-",
            "*",
            "{",
            "i64",
            "a",
            "b",
            "1",
            " ",
            "\\n",
            "//",
            "𝑥",
            "99999999999999999999999",
            "\\u{0}",
            "bool",
        ];
        let mut state = 0xC0FF_EE12_3456_789A_u64;
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
            agree(&src);
        }
    }
}
