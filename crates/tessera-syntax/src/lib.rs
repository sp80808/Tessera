//! Provisional lossless-ish syntax layer for the Tessera bootstrap.
//!
//! The grammar in this crate is deliberately tiny and experimental. It exists
//! to validate source spans, recovery, canonical formatting and TC -> TIR
//! boundaries before Tessera syntax is frozen by the tokenizer experiments.

use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn join(self, other: Self) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxError {
    pub message: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Param {
    pub name: String,
    pub name_span: Span,
    pub ty: TypeRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expr {
    Int { value: i64, span: Span },
    Var { name: String, span: Span },
    Add {
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub const fn span(&self) -> Span {
        match self {
            Self::Int { span, .. } | Self::Var { span, .. } | Self::Add { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
    pub errors: Vec<SyntaxError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TokenKind {
    Fn,
    Ident(String),
    Int(i64),
    LParen,
    RParen,
    Colon,
    Comma,
    Greater,
    Eq,
    Plus,
    Newline,
    Unknown(char),
    Eof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Token {
    kind: TokenKind,
    span: Span,
}

pub fn parse(source: &str) -> Program {
    Parser::new(lex(source)).parse_program()
}

pub fn canonical_format(program: &Program) -> String {
    let mut out = String::new();
    for function in &program.functions {
        let _ = write!(out, "f {}(", function.name);
        for (index, param) in function.params.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}:{}", param.name, param.ty.name);
        }
        out.push(')');
        if let Some(return_type) = &function.return_type {
            let _ = write!(out, ">{}", return_type.name);
        }
        out.push('=');
        format_expr(&function.body, &mut out);
        out.push('\n');
    }
    out
}

fn format_expr(expr: &Expr, out: &mut String) {
    match expr {
        Expr::Int { value, .. } => {
            let _ = write!(out, "{value}");
        }
        Expr::Var { name, .. } => out.push_str(name),
        Expr::Add { left, right, .. } => {
            format_expr(left, out);
            out.push('+');
            format_expr(right, out);
        }
    }
}

fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let ch = bytes[index] as char;
        match ch {
            ' ' | '\t' | '\r' => index += 1,
            '\n' => {
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    span: Span::new(index, index + 1),
                });
                index += 1;
            }
            '(' => push_one(&mut tokens, TokenKind::LParen, index, &mut index),
            ')' => push_one(&mut tokens, TokenKind::RParen, index, &mut index),
            ':' => push_one(&mut tokens, TokenKind::Colon, index, &mut index),
            ',' => push_one(&mut tokens, TokenKind::Comma, index, &mut index),
            '>' => push_one(&mut tokens, TokenKind::Greater, index, &mut index),
            '=' => push_one(&mut tokens, TokenKind::Eq, index, &mut index),
            '+' => push_one(&mut tokens, TokenKind::Plus, index, &mut index),
            '0'..='9' => {
                let start = index;
                while index < bytes.len() && (bytes[index] as char).is_ascii_digit() {
                    index += 1;
                }
                let text = &source[start..index];
                let value = text.parse::<i64>().unwrap_or(i64::MAX);
                tokens.push(Token {
                    kind: TokenKind::Int(value),
                    span: Span::new(start, index),
                });
            }
            _ if is_ident_start(ch) => {
                let start = index;
                index += 1;
                while index < bytes.len() && is_ident_continue(bytes[index] as char) {
                    index += 1;
                }
                let text = &source[start..index];
                tokens.push(Token {
                    kind: if text == "f" {
                        TokenKind::Fn
                    } else {
                        TokenKind::Ident(text.to_owned())
                    },
                    span: Span::new(start, index),
                });
            }
            _ => {
                tokens.push(Token {
                    kind: TokenKind::Unknown(ch),
                    span: Span::new(index, index + 1),
                });
                index += 1;
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(source.len(), source.len()),
    });
    tokens
}

fn push_one(tokens: &mut Vec<Token>, kind: TokenKind, start: usize, index: &mut usize) {
    *index += 1;
    tokens.push(Token {
        kind,
        span: Span::new(start, *index),
    });
}

const fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

const fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, cursor: 0 }
    }

    fn parse_program(mut self) -> Program {
        let mut program = Program::default();

        while !matches!(self.peek().kind, TokenKind::Eof) {
            if matches!(self.peek().kind, TokenKind::Newline) {
                self.bump();
                continue;
            }

            match self.parse_function() {
                Ok(function) => program.functions.push(function),
                Err(error) => {
                    program.errors.push(error);
                    self.recover_line();
                }
            }

            if matches!(self.peek().kind, TokenKind::Newline) {
                self.bump();
            }
        }

        program
    }

    fn parse_function(&mut self) -> Result<Function, SyntaxError> {
        let start = self.expect_simple(TokenKind::Fn, "expected 'f' to start a function")?.span;
        let (name, name_span) = self.expect_ident("expected function name")?;
        self.expect_simple(TokenKind::LParen, "expected '(' after function name")?;

        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let (param_name, name_span) = self.expect_ident("expected parameter name")?;
                self.expect_simple(TokenKind::Colon, "expected ':' after parameter name")?;
                let (type_name, type_span) = self.expect_ident("expected parameter type")?;
                params.push(Param {
                    name: param_name,
                    name_span,
                    ty: TypeRef {
                        name: type_name,
                        span: type_span,
                    },
                });

                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }

        self.expect_simple(TokenKind::RParen, "expected ')' after parameters")?;

        let return_type = if matches!(self.peek().kind, TokenKind::Greater) {
            self.bump();
            let (name, span) = self.expect_ident("expected return type after '>'")?;
            Some(TypeRef { name, span })
        } else {
            None
        };

        self.expect_simple(TokenKind::Eq, "expected '=' before function body")?;
        let body = self.parse_expr()?;

        if !matches!(self.peek().kind, TokenKind::Newline | TokenKind::Eof) {
            return Err(self.error_here("unexpected token after function body"));
        }

        Ok(Function {
            name,
            name_span,
            params,
            return_type,
            span: start.join(body.span()),
            body,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, SyntaxError> {
        let mut expr = self.parse_term()?;

        while matches!(self.peek().kind, TokenKind::Plus) {
            self.bump();
            let right = self.parse_term()?;
            let span = expr.span().join(right.span());
            expr = Expr::Add {
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, SyntaxError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(Expr::Int {
                value,
                span: token.span,
            }),
            TokenKind::Ident(name) => Ok(Expr::Var {
                name,
                span: token.span,
            }),
            TokenKind::Unknown(ch) => Err(SyntaxError {
                message: format!("unknown character '{ch}'"),
                span: token.span,
            }),
            _ => Err(SyntaxError {
                message: "expected integer or variable expression".to_owned(),
                span: token.span,
            }),
        }
    }

    fn expect_ident(&mut self, message: &str) -> Result<(String, Span), SyntaxError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(name) => Ok((name, token.span)),
            _ => Err(SyntaxError {
                message: message.to_owned(),
                span: token.span,
            }),
        }
    }

    fn expect_simple(
        &mut self,
        expected: TokenKind,
        message: &str,
    ) -> Result<Token, SyntaxError> {
        let token = self.bump().clone();
        if std::mem::discriminant(&token.kind) == std::mem::discriminant(&expected) {
            Ok(token)
        } else {
            Err(SyntaxError {
                message: message.to_owned(),
                span: token.span,
            })
        }
    }

    fn recover_line(&mut self) {
        while !matches!(self.peek().kind, TokenKind::Newline | TokenKind::Eof) {
            self.bump();
        }
    }

    fn error_here(&self, message: &str) -> SyntaxError {
        SyntaxError {
            message: message.to_owned(),
            span: self.peek().span,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn bump(&mut self) -> &Token {
        let index = self.cursor;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_format_is_idempotent() {
        let source = " f add ( a : i64 , b:i64 ) > i64 = a + b \n";
        let first = parse(source);
        assert!(first.errors.is_empty(), "{:?}", first.errors);

        let canonical = canonical_format(&first);
        assert_eq!(canonical, "f add(a:i64,b:i64)>i64=a+b\n");

        let second = parse(&canonical);
        assert!(second.errors.is_empty(), "{:?}", second.errors);
        assert_eq!(canonical_format(&second), canonical);
    }

    #[test]
    fn malformed_line_recovers_to_following_function() {
        let program = parse("f good()=1\nwat\nf later()=2\n");
        assert_eq!(program.functions.len(), 2);
        assert_eq!(program.errors.len(), 1);
        assert_eq!(program.functions[1].name, "later");
    }

    #[test]
    fn function_span_maps_back_to_source() {
        let source = "f add(a:i64,b:i64)>i64=a+b\n";
        let program = parse(source);
        let function = &program.functions[0];
        assert_eq!(&source[function.span.start..function.span.end], "f add(a:i64,b:i64)>i64=a+b");
    }
}
