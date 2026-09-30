//! Typed AST facade over the lossless CST (#18/#19 bridge).
//!
//! `lower` reads a *clean* CST (no `Error`/`Missing` nodes) and produces the
//! bootstrap [`AstFunction`] together with an [`AstSpans`] side table. Spans
//! are deliberately **not** stored in AST nodes (ADR 0002 decision 2): two
//! spellings of the same function yield equal `AstFunction`s and different
//! `AstSpans`.
//!
//! If the CST has errors, the first diagnostic (canonical order) is converted
//! to the legacy [`SyntaxError`] so existing callers keep working; tolerant
//! callers should use [`crate::cst::parse_file`] directly.
//!
//! Meaning is checked here only where the bootstrap AST demands it (`i64` is
//! the sole type, integers must fit `i64`): the CST itself stays meaning-free.

use tessera_phases::{Diagnostic, FileId, Span};

use crate::cst::{Child, NodeKind, ParsedFile};
use crate::lexer::TokenKind;
use crate::{AstExpr, AstFunction, AstParam, SyntaxError, TirType};

/// Source spans for one parsed function, keyed by position in the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstSpans {
    /// The whole `Fn` node (excludes leading/trailing trivia).
    pub func: Span,
    pub name: Span,
    pub params: Vec<ParamSpans>,
    pub ret: Span,
    pub body: Span,
    /// One span per `AstExpr` node in **pre-order** (node, then lhs, then rhs).
    /// Redundant parentheses are not AST nodes, so they have no entry.
    pub exprs: Vec<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpans {
    pub name: Span,
    pub ty: Span,
}

impl AstSpans {
    /// Start byte of the `index`-th pre-order expression, or 0 if this table
    /// does not describe the AST (e.g. a hand-built one).
    #[must_use]
    pub fn expr_start(&self, index: usize) -> usize {
        self.exprs.get(index).map_or(0, |s| s.start as usize)
    }

    /// A table for ASTs that were not parsed from source.
    #[must_use]
    pub fn empty(file: FileId) -> Self {
        let z = Span::new(file, 0, 0);
        Self {
            func: z,
            name: z,
            params: Vec::new(),
            ret: z,
            body: z,
            exprs: Vec::new(),
        }
    }
}

/// Convert the first diagnostic into the legacy error type.
fn to_syntax_error(d: &Diagnostic) -> SyntaxError {
    let at = d.at.primary_span().start as usize;
    match d.code {
        "E-syntax-trailing-input" => SyntaxError::TrailingInput { at },
        "E-syntax-empty-program" => SyntaxError::EmptyProgram,
        "E-syntax-nesting-too-deep" => SyntaxError::NestingTooDeep { at },
        _ => {
            let rest = d.message.strip_prefix("expected ").unwrap_or(&d.message);
            let (want, got) = rest.split_once(", found ").unwrap_or((rest, "?"));
            SyntaxError::Unexpected {
                at,
                want: want.to_owned(),
                got: got.to_owned(),
            }
        }
    }
}

struct Lowerer<'a> {
    parsed: &'a ParsedFile,
    src: &'a str,
    exprs: Vec<Span>,
}

impl Lowerer<'_> {
    fn span(&self, start: usize, end: usize) -> Span {
        let c = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Span::new(self.parsed.file, c(start), c(end))
    }

    fn node_span(&self, node: usize) -> Span {
        let (s, e) = self.parsed.cst.range(node);
        self.span(s, e)
    }

    /// Direct non-trivia token children as `(kind, start, end)`.
    fn tokens(&self, node: usize) -> Vec<(TokenKind, usize, usize)> {
        self.parsed
            .cst
            .children(node)
            .iter()
            .filter_map(|c| match c {
                Child::Token(t) => Some(&self.parsed.tokens[*t]),
                Child::Node(_) => None,
            })
            .filter(|t| !t.kind.is_trivia())
            .map(|t| (t.kind, t.start, t.end))
            .collect()
    }

    fn nodes(&self, node: usize) -> Vec<usize> {
        self.parsed
            .cst
            .children(node)
            .iter()
            .filter_map(|c| match c {
                Child::Node(n) => Some(*n),
                Child::Token(_) => None,
            })
            .collect()
    }

    fn type_of(&self, type_ref: usize) -> Result<(TirType, Span), SyntaxError> {
        let (_, start, end) = self.tokens(type_ref)[0];
        let name = &self.src[start..end];
        match name {
            "i64" => Ok((TirType::I64, self.span(start, end))),
            _ => Err(SyntaxError::UnknownType {
                at: start,
                name: name.to_owned(),
            }),
        }
    }

    fn expr(&mut self, node: usize) -> Result<AstExpr, SyntaxError> {
        match self.parsed.cst.kind(node) {
            NodeKind::ParenExpr => {
                let inner = self.nodes(node)[0];
                self.expr(inner)
            }
            NodeKind::LiteralExpr => {
                let (_, start, end) = self.tokens(node)[0];
                self.exprs.push(self.span(start, end));
                let value = self.src[start..end]
                    .parse()
                    .map_err(|_| SyntaxError::IntOutOfRange { at: start })?;
                Ok(AstExpr::Int(value))
            }
            NodeKind::PathExpr => {
                let (_, start, end) = self.tokens(node)[0];
                self.exprs.push(self.span(start, end));
                Ok(AstExpr::Var(self.src[start..end].to_owned()))
            }
            NodeKind::BinExpr => {
                self.exprs.push(self.node_span(node));
                let kids = self.nodes(node);
                let lhs = self.expr(kids[0])?;
                let rhs = self.expr(kids[1])?;
                Ok(AstExpr::Add(Box::new(lhs), Box::new(rhs)))
            }
            other => unreachable!("clean CST has no {other:?} in expression position"),
        }
    }

    fn function(&mut self, fn_node: usize) -> Result<(AstFunction, AstSpans), SyntaxError> {
        let toks = self.tokens(fn_node);
        // clean shape: [f, name, `>`, `=`] tokens; [ParamList, TypeRef, expr] nodes
        let (_, name_start, name_end) = toks[1];
        let kids = self.nodes(fn_node);
        let (list, ret_node, body_node) = (kids[0], kids[1], kids[2]);

        let mut params = Vec::new();
        let mut param_spans = Vec::new();
        for param in self.nodes(list) {
            let (_, ps, pe) = self.tokens(param)[0];
            let ty_node = self.nodes(param)[0];
            let (ty, ty_span) = self.type_of(ty_node)?;
            params.push((
                AstParam {
                    name: self.src[ps..pe].to_owned(),
                },
                ty,
            ));
            param_spans.push(ParamSpans {
                name: self.span(ps, pe),
                ty: ty_span,
            });
        }
        let (ret, ret_span) = self.type_of(ret_node)?;
        let body = self.expr(body_node)?;
        Ok((
            AstFunction {
                name: self.src[name_start..name_end].to_owned(),
                params,
                ret,
                body,
            },
            AstSpans {
                func: self.node_span(fn_node),
                name: self.span(name_start, name_end),
                params: param_spans,
                ret: ret_span,
                body: self.node_span(body_node),
                exprs: std::mem::take(&mut self.exprs),
            },
        ))
    }
}

/// Lower a parsed file. `diagnostics` are those returned with `parsed`.
pub fn lower(
    parsed: &ParsedFile,
    src: &str,
    diagnostics: &tessera_phases::DiagnosticSet,
) -> Result<(AstFunction, AstSpans), SyntaxError> {
    if let Some(first) = diagnostics.iter().next() {
        return Err(to_syntax_error(first));
    }
    let root = parsed.cst.root();
    let fn_node = parsed
        .cst
        .children(root)
        .iter()
        .find_map(|c| match c {
            Child::Node(n) if parsed.cst.kind(*n) == NodeKind::Fn => Some(*n),
            _ => None,
        })
        .ok_or(SyntaxError::EmptyProgram)?;
    Lowerer {
        parsed,
        src,
        exprs: Vec::new(),
    }
    .function(fn_node)
}
