//! Frozen copy of the original fail-fast bootstrap parser, kept ONLY as a
//! differential-test oracle for `cst` and `ast` (test builds only). It is not
//! part of the compiler; delete it when the CST parser has its own
//! exhaustive corpus.

use crate::lexer::{self, TokenKind};
use crate::{AstExpr, AstFunction, AstParam, MAX_NESTING, SyntaxError, TirType};

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

/// Lex for the parser: trivia dropped, integers parsed, unknown input rejected.
/// The lossless token stream lives in [`lexer`]; this is the bootstrap parser's
/// view of it (the parser fails at the first bad token; recovery is #18 work).
fn lex(src: &str) -> Result<Vec<(Token, usize)>, SyntaxError> {
    let mut out = Vec::new();
    for tok in lexer::lex(src) {
        let text = tok.text(src);
        let token = match tok.kind {
            TokenKind::Whitespace | TokenKind::LineComment => continue,
            TokenKind::Ident if text == "f" => Token::Func,
            TokenKind::Ident => Token::Ident(text.to_owned()),
            TokenKind::Int => Token::Int(
                text.parse()
                    .map_err(|_| SyntaxError::IntOutOfRange { at: tok.start })?,
            ),
            TokenKind::Colon => Token::Colon,
            TokenKind::Comma => Token::Comma,
            TokenKind::LParen => Token::LParen,
            TokenKind::RParen => Token::RParen,
            TokenKind::Gt => Token::Gt,
            TokenKind::Eq => Token::Eq,
            TokenKind::Plus => Token::Plus,
            TokenKind::Error
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Lt
            | TokenKind::Bang
            | TokenKind::Amp
            | TokenKind::Dot
            | TokenKind::Semi
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::LBracket
            | TokenKind::RBracket => {
                return Err(SyntaxError::Unexpected {
                    at: tok.start,
                    want: "TC token".to_owned(),
                    got: format!("byte {:#04X}", text.as_bytes()[0]),
                });
            }
        };
        out.push((token, tok.start));
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    depth: usize,
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
                want: want_str.to_owned(),
                got: token.kind().to_owned(),
            }),
            None => Err(SyntaxError::Unexpected {
                at: self.tokens.last().map_or(0, |(_, at)| at + 1),
                want: want_str.to_owned(),
                got: "end of input".to_owned(),
            }),
        }
    }

    fn expect_ident(&mut self, want: &'static str) -> Result<(String, usize), SyntaxError> {
        match self.next() {
            Some((Token::Ident(name), at)) => Ok((name, at)),
            Some((token, at)) => Err(SyntaxError::Unexpected {
                at,
                want: want.to_owned(),
                got: token.kind().to_owned(),
            }),
            None => Err(SyntaxError::Unexpected {
                at: self.tokens.last().map_or(0, |(_, at)| at + 1),
                want: want.to_owned(),
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
            Some((Token::LParen, at)) => {
                if self.depth >= MAX_NESTING {
                    return Err(SyntaxError::NestingTooDeep { at });
                }
                self.depth += 1;
                let inner = self.parse_add()?;
                self.depth -= 1;
                self.expect(Token::RParen, "`)`")?;
                Ok(inner)
            }
            Some((token, at)) => Err(SyntaxError::Unexpected {
                at,
                want: "expression".to_owned(),
                got: token.kind().to_owned(),
            }),
            None => Err(SyntaxError::Unexpected {
                at: self.tokens.last().map_or(0, |(_, at)| at + 1),
                want: "expression".to_owned(),
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
    Parser {
        tokens,
        pos: 0,
        depth: 0,
    }
    .parse_function()
}
