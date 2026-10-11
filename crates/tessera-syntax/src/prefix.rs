//! Incremental prefix recognizer for grammar-constrained generation (issue
//! #11, first slice).
//!
//! The compiler side of constrained decoding: a byte-level, vocabulary-free
//! automaton that answers "can this prefix still become a TC program that
//! parses without a syntax diagnostic?". A decoder adapter maps a tokenizer's
//! byte strings onto it ([`Prefix::admits`], [`Prefix::mask`]); nothing here
//! knows about any model or vocabulary.
//!
//! Contract (tested against [`crate::cst::parse_file`] in this module):
//! - **sound**: every state reachable through [`Prefix::accept`] is viable;
//!   [`Prefix::completion`] returns bytes that finish it into a file the
//!   parser accepts with no syntax diagnostic. A masked-in token can never
//!   create an impossible prefix;
//! - **exact for whole files**: [`Prefix::is_accepting`] after feeding a file
//!   is true exactly when the parser reports no syntax diagnostic for it,
//!   including the nesting and expression-depth limits
//!   ([`MAX_NESTING`], [`MAX_EXPR_DEPTH`]);
//! - **atomic**: a rejected [`Prefix::accept`] leaves the state unchanged and
//!   names the first rejected byte.
//!
//! Scope is syntax only, the same as the parser: names and types are not
//! constrained (`f g(a:bool)>x=b` is a valid *prefix state* and a resolve
//! error later). Grammar validity says nothing about semantic correctness.
//!
//! The state is a plain value ([`Prefix::checkpoint`] / [`Prefix::restore`]
//! are clones), so an adapter can branch, backtrack and cache freely. Cache
//! keys must include [`fingerprint`], which changes with the grammar or its
//! limits.

use crate::{MAX_EXPR_DEPTH, MAX_NESTING};

/// Version of the grammar this recognizer implements. Bump it whenever the
/// accepted language changes; adapters key their caches on [`fingerprint`].
pub const GRAMMAR: &str = "tessera.tc-syntax/v0";

/// [`GRAMMAR`] plus the parser limits that shape the accepted language.
#[must_use]
pub fn fingerprint() -> String {
    format!("{GRAMMAR};nesting={MAX_NESTING};expr-depth={MAX_EXPR_DEPTH}")
}

/// A grammar terminal: what the next significant token may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Terminal {
    /// The `f` keyword (an identifier spelled exactly `f`).
    KwF,
    Ident,
    Int,
    LParen,
    RParen,
    Colon,
    Comma,
    Gt,
    Eq,
    Plus,
    /// End of input: the prefix is a complete file.
    End,
}

/// Where in `f NAME(P:T,...)>T=EXPR` the next significant token belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Syn {
    Kw,
    Name,
    LParen,
    ParamOrRParen,
    ParamName,
    Colon,
    ParamType,
    CommaOrRParen,
    Gt,
    RetType,
    Eq,
    Operand,
    AfterOperand,
}

/// Lexical state: what the last byte started or continued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lex {
    /// Between tokens (or in whitespace).
    Between,
    /// Inside an identifier; `kw` when it is the `f` keyword, which must not
    /// be extended.
    Ident {
        kw: bool,
    },
    Int,
    /// A lone `/`: only `//` (a comment) is valid.
    Slash,
    /// Inside a `//` comment. `need` continuation bytes of a UTF-8 sequence
    /// remain, the next one in `lo..=hi`.
    Comment {
        need: u8,
        lo: u8,
        hi: u8,
    },
}

/// One parenthesis level of the expression being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Level {
    /// Expression depth of the `+` chain so far (a leaf is 0).
    depth: usize,
    /// Largest depth this level may end with and still satisfy every
    /// enclosing level's limit.
    budget: usize,
    /// The operand being written is the right-hand side of a `+`.
    rhs: bool,
}

/// A byte that cannot extend the prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// Offset of the rejected byte within the `accept` argument.
    pub offset: usize,
    /// What the grammar allowed at that point.
    pub expected: Vec<Terminal>,
}

/// An opaque saved state; see [`Prefix::restore`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Checkpoint(Prefix);

/// The recognizer state for one prefix. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Prefix {
    syn: Syn,
    lex: Lex,
    /// Expression levels; `levels[0]` is the function body. Empty until `=`.
    levels: Vec<Level>,
    /// Bytes accepted so far.
    len: usize,
}

impl Default for Prefix {
    fn default() -> Self {
        Self::start()
    }
}

const fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

const fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl Prefix {
    /// The empty prefix.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            syn: Syn::Kw,
            lex: Lex::Between,
            levels: Vec::new(),
            len: 0,
        }
    }

    /// Bytes accepted so far.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Extend the prefix by `bytes`, all or nothing.
    ///
    /// # Errors
    /// The first byte that makes the prefix non-viable; `self` is unchanged.
    pub fn accept(&mut self, bytes: &[u8]) -> Result<(), Rejected> {
        let mut next = self.clone();
        for (offset, &b) in bytes.iter().enumerate() {
            let expected = next.expected();
            if next.step(b).is_err() {
                return Err(Rejected { offset, expected });
            }
        }
        *self = next;
        Ok(())
    }

    /// Would `bytes` keep the prefix viable? (`accept` on a copy.)
    #[must_use]
    pub fn admits(&self, bytes: &[u8]) -> bool {
        self.clone().accept(bytes).is_ok()
    }

    /// `admits` for each entry of a vocabulary of byte strings. An empty
    /// entry is admitted (it changes nothing). Naive: an adapter should cache
    /// by state and token equivalence class under [`fingerprint`].
    #[must_use]
    pub fn mask<T: AsRef<[u8]>>(&self, vocab: &[T]) -> Vec<bool> {
        vocab.iter().map(|t| self.admits(t.as_ref())).collect()
    }

    /// Exactly the bytes that may come next.
    #[must_use]
    pub fn allowed_bytes(&self) -> [bool; 256] {
        let mut out = [false; 256];
        for (b, slot) in out.iter_mut().enumerate() {
            *slot = self.clone().step(b as u8).is_ok();
        }
        out
    }

    /// Is the prefix a complete file the parser accepts without a syntax
    /// diagnostic?
    #[must_use]
    pub fn is_accepting(&self) -> bool {
        let lex_done = match self.lex {
            Lex::Slash => false,
            Lex::Comment { need, .. } => need == 0,
            Lex::Between | Lex::Ident { .. } | Lex::Int => true,
        };
        lex_done && self.syn == Syn::AfterOperand && self.levels.len() == 1
    }

    /// The terminals the next significant token may be (plus [`Terminal::End`]
    /// when the prefix is already complete). This is the grammar-level view:
    /// whitespace, comments and extending the current identifier or integer
    /// are always lexical, never listed.
    #[must_use]
    pub fn expected(&self) -> Vec<Terminal> {
        use Terminal as T;
        match self.syn {
            Syn::Kw => vec![T::KwF],
            Syn::Name | Syn::ParamName | Syn::ParamType | Syn::RetType => vec![T::Ident],
            Syn::LParen => vec![T::LParen],
            Syn::ParamOrRParen => vec![T::Ident, T::RParen],
            Syn::Colon => vec![T::Colon],
            Syn::CommaOrRParen => vec![T::Comma, T::RParen],
            Syn::Gt => vec![T::Gt],
            Syn::Eq => vec![T::Eq],
            Syn::Operand => {
                let mut out = vec![T::Ident, T::Int];
                if self.paren_budget().is_some() {
                    out.push(T::LParen);
                }
                out
            }
            Syn::AfterOperand => {
                let mut out = Vec::new();
                if self.top().is_some_and(|l| l.depth < l.budget) {
                    out.push(T::Plus);
                }
                if self.levels.len() > 1 {
                    out.push(T::RParen);
                } else {
                    out.push(T::End);
                }
                out
            }
        }
    }

    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.clone())
    }

    pub fn restore(&mut self, checkpoint: Checkpoint) {
        *self = checkpoint.0;
    }

    /// The shortest-effort bytes that complete the prefix into a file the
    /// parser accepts (empty when it is already accepting). Its existence is
    /// the viability witness; the content is syntax-only filler
    /// (`f x(...)>i=0`-style), not a suggestion.
    #[must_use]
    pub fn completion(&self) -> Vec<u8> {
        let mut state = self.clone();
        let mut out = Vec::new();
        while !state.is_accepting() {
            let b = state.filler();
            state
                .step(b)
                .expect("filler bytes are always accepted: every state is viable");
            out.push(b);
        }
        out
    }

    /// One byte that moves toward an accepting state.
    fn filler(&self) -> u8 {
        match self.lex {
            Lex::Comment { need, lo, .. } if need > 0 => return lo,
            Lex::Comment { .. } => return b'\n',
            Lex::Slash => return b'/',
            Lex::Ident { kw: true } => return b' ',
            Lex::Between | Lex::Ident { kw: false } | Lex::Int => {}
        }
        match self.syn {
            Syn::Kw => b'f',
            Syn::Name | Syn::ParamName | Syn::ParamType | Syn::RetType => b'x',
            Syn::LParen => b'(',
            Syn::ParamOrRParen | Syn::CommaOrRParen => b')',
            Syn::Colon => b':',
            Syn::Gt => b'>',
            Syn::Eq => b'=',
            Syn::Operand => b'0',
            // Not accepting here means a parenthesis is still open.
            Syn::AfterOperand => b')',
        }
    }

    fn top(&self) -> Option<&Level> {
        self.levels.last()
    }

    /// The budget a parenthesized operand opened now would get, or `None`
    /// when `(` is not allowed (nesting limit or no depth left).
    fn paren_budget(&self) -> Option<usize> {
        let top = self.top()?;
        // Open parentheses so far are `levels.len() - 1`.
        if self.levels.len() > MAX_NESTING {
            return None;
        }
        // A first operand ends with depth `inner + 1 <= budget`; a `+`
        // right-hand side needs `max(depth, inner + 1) + 1 <= budget`.
        top.budget.checked_sub(if top.rhs { 2 } else { 1 })
    }

    /// Feed one byte. On `Err` the state may be partly updated; callers
    /// work on a copy.
    fn step(&mut self, b: u8) -> Result<(), ()> {
        match self.lex {
            Lex::Comment { need, lo, hi } => {
                self.lex = comment_byte(need, lo, hi, b)?;
                self.len += 1;
                return Ok(());
            }
            Lex::Slash => {
                if b != b'/' {
                    return Err(());
                }
                self.lex = Lex::Comment {
                    need: 0,
                    lo: 0,
                    hi: 0,
                };
                self.len += 1;
                return Ok(());
            }
            Lex::Ident { kw } if is_ident_continue(b) => {
                if kw {
                    return Err(());
                }
                self.len += 1;
                return Ok(());
            }
            Lex::Int if b.is_ascii_digit() => {
                self.len += 1;
                return Ok(());
            }
            Lex::Between | Lex::Ident { .. } | Lex::Int => {}
        }
        // The current token (if any) ends here; `b` starts a new one.
        self.lex = if b.is_ascii_whitespace() {
            Lex::Between
        } else if b == b'/' {
            Lex::Slash
        } else if is_ident_start(b) {
            let kw = self.syn == Syn::Kw;
            if kw && b != b'f' {
                return Err(());
            }
            self.token(Terminal::Ident)?;
            Lex::Ident { kw }
        } else if b.is_ascii_digit() {
            self.token(Terminal::Int)?;
            Lex::Int
        } else {
            let t = match b {
                b'(' => Terminal::LParen,
                b')' => Terminal::RParen,
                b':' => Terminal::Colon,
                b',' => Terminal::Comma,
                b'>' => Terminal::Gt,
                b'=' => Terminal::Eq,
                b'+' => Terminal::Plus,
                _ => return Err(()),
            };
            self.token(t)?;
            Lex::Between
        };
        self.len += 1;
        Ok(())
    }

    /// Advance the syntactic state over one significant token.
    fn token(&mut self, t: Terminal) -> Result<(), ()> {
        use Terminal as T;
        self.syn = match (self.syn, t) {
            (Syn::Kw, T::Ident) => Syn::Name,
            (Syn::Name, T::Ident) => Syn::LParen,
            (Syn::LParen, T::LParen) => Syn::ParamOrRParen,
            (Syn::ParamOrRParen | Syn::ParamName, T::Ident) => Syn::Colon,
            (Syn::Colon, T::Colon) => Syn::ParamType,
            (Syn::ParamType, T::Ident) => Syn::CommaOrRParen,
            (Syn::CommaOrRParen, T::Comma) => Syn::ParamName,
            (Syn::ParamOrRParen | Syn::CommaOrRParen, T::RParen) => Syn::Gt,
            (Syn::Gt, T::Gt) => Syn::RetType,
            (Syn::RetType, T::Ident) => Syn::Eq,
            (Syn::Eq, T::Eq) => {
                self.levels.push(Level {
                    depth: 0,
                    budget: MAX_EXPR_DEPTH,
                    rhs: false,
                });
                Syn::Operand
            }
            (Syn::Operand, T::Ident | T::Int) => {
                self.finish_operand(0);
                Syn::AfterOperand
            }
            (Syn::Operand, T::LParen) => {
                let budget = self.paren_budget().ok_or(())?;
                self.levels.push(Level {
                    depth: 0,
                    budget,
                    rhs: false,
                });
                Syn::Operand
            }
            (Syn::AfterOperand, T::Plus) => {
                let top = self.levels.last_mut().ok_or(())?;
                if top.depth >= top.budget {
                    return Err(());
                }
                top.rhs = true;
                Syn::Operand
            }
            (Syn::AfterOperand, T::RParen) if self.levels.len() > 1 => {
                let inner = self.levels.pop().ok_or(())?;
                self.finish_operand(inner.depth + 1);
                Syn::AfterOperand
            }
            _ => return Err(()),
        };
        Ok(())
    }

    /// An operand of depth `depth` is complete at the innermost level.
    fn finish_operand(&mut self, depth: usize) {
        if let Some(top) = self.levels.last_mut() {
            top.depth = if top.rhs {
                top.depth.max(depth) + 1
            } else {
                depth
            };
            top.rhs = false;
        }
    }
}

/// One byte inside a `//` comment: anything up to `\n` that keeps the text
/// valid UTF-8 (the parser reads `&str`).
fn comment_byte(need: u8, lo: u8, hi: u8, b: u8) -> Result<Lex, ()> {
    let cont = |need: u8| Lex::Comment {
        need,
        lo: 0x80,
        hi: 0xBF,
    };
    if need > 0 {
        return if (lo..=hi).contains(&b) {
            Ok(cont(need - 1))
        } else {
            Err(())
        };
    }
    Ok(match b {
        b'\n' => Lex::Between,
        0x00..=0x7F => cont(0),
        0xC2..=0xDF => cont(1),
        // Exclude overlong encodings and UTF-16 surrogates.
        0xE0 => Lex::Comment {
            need: 2,
            lo: 0xA0,
            hi: 0xBF,
        },
        0xED => Lex::Comment {
            need: 2,
            lo: 0x80,
            hi: 0x9F,
        },
        0xE1..=0xEC | 0xEE..=0xEF => cont(2),
        0xF0 => Lex::Comment {
            need: 3,
            lo: 0x90,
            hi: 0xBF,
        },
        0xF1..=0xF3 => cont(3),
        0xF4 => Lex::Comment {
            need: 3,
            lo: 0x80,
            hi: 0x8F,
        },
        _ => return Err(()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::parse_file;
    use tessera_phases::{FileId, Phase};

    /// Ground truth: the real parser reports no syntax diagnostic.
    fn parses(bytes: &[u8]) -> bool {
        let Ok(src) = std::str::from_utf8(bytes) else {
            return false;
        };
        !parse_file(FileId(0), src)
            .diagnostics
            .iter()
            .any(|d| d.phase == Phase::Syntax)
    }

    fn fed(src: &[u8]) -> Result<Prefix, Rejected> {
        let mut p = Prefix::start();
        p.accept(src)?;
        Ok(p)
    }

    /// Every prefix of `src` is accepted and completes to a parsing file;
    /// the whole of `src` is accepting.
    fn assert_valid(src: &str) {
        let mut p = Prefix::start();
        for (i, &b) in src.as_bytes().iter().enumerate() {
            let completed = [src.as_bytes()[..i].to_vec(), p.completion()].concat();
            assert!(
                parses(&completed),
                "completion of {:?} is {:?}",
                &src[..i],
                String::from_utf8_lossy(&completed)
            );
            p.accept(&[b])
                .unwrap_or_else(|e| panic!("{src:?} rejected at {i}: {e:?}"));
        }
        assert!(p.is_accepting(), "{src:?} not accepting");
        assert!(p.completion().is_empty());
    }

    #[test]
    fn canonical_and_spaced_programs_are_accepted_byte_by_byte() {
        for src in [
            "f add(a:i64,b:i64)>i64=a+b",
            "f add(a:i64,b:i64)>i64=a+b\n",
            "f k()>i64=42",
            "  f   g ( x : i64 , y:i64 ) > i64 = ( x + 1 ) + ( ( y ) )  \n",
            "// header comment \u{e9}\u{4e2d}\u{1f600}\nf h(a:i64)>i64=a // trailing",
            "f//c\nn()>i64=0",
            "f f(f:f)>f=f",
            "f x_1(_a9:i64)>i64=_a9+007",
        ] {
            assert_valid(src);
        }
    }

    #[test]
    fn rejections_are_atomic_and_name_the_offset() {
        let mut p = fed(b"f add(a:i64)>i64=a").expect("valid prefix");
        let before = p.clone();
        let err = p.accept(b"+-1").expect_err("`-` is not TC");
        assert_eq!(err.offset, 1);
        assert_eq!(
            err.expected,
            vec![Terminal::Ident, Terminal::Int, Terminal::LParen]
        );
        assert_eq!(p, before);
    }

    #[test]
    fn foreign_syntax_is_rejected_where_it_diverges() {
        for (src, offset) in [
            ("fn add(a: i64) -> i64 { a }", 1),
            ("def add(a):", 0),
            ("f add(a:i64)->i64=a", 12),
            ("f add(a:i64)>i64{a}", 16),
            ("f add(a:i64)>i64=return a", 24),
            ("f add(a:i64)>i64=a;", 18),
            ("f add(a:i64)>i64=a*2", 18),
            ("f add(a:i64)>i64=-1", 17),
            ("fadd()>i64=0", 1),
            ("f add(a:i64)>i64=1a", 18),
            ("f add(a:i64)>i64=a b", 19),
            ("f a()>i64=0 / 1", 13),
        ] {
            let err = fed(src.as_bytes()).expect_err(src);
            assert_eq!(err.offset, offset, "{src}");
            assert!(!parses(src.as_bytes()), "{src}");
        }
    }

    #[test]
    fn expected_terminals_follow_the_grammar() {
        use Terminal as T;
        let at = |src: &str| fed(src.as_bytes()).expect(src).expected();
        assert_eq!(at(""), [T::KwF]);
        assert_eq!(at("f "), [T::Ident]);
        assert_eq!(at("f g("), [T::Ident, T::RParen]);
        assert_eq!(at("f g(a:i64"), [T::Comma, T::RParen]);
        assert_eq!(at("f g()>i64="), [T::Ident, T::Int, T::LParen]);
        assert_eq!(at("f g()>i64=1"), [T::Plus, T::End]);
        assert_eq!(at("f g()>i64=(1"), [T::Plus, T::RParen]);
    }

    #[test]
    fn incomplete_lexical_states_are_explicit() {
        // A lone `/`, a split UTF-8 sequence in a comment, and the `f`
        // keyword are viable but not accepting; each completes.
        for src in [&b"f g()>i64=1 /"[..], b"f g()>i64=1 //\xe2\x82", b"f"] {
            let p = fed(src).expect("viable");
            assert!(!p.is_accepting());
            assert!(parses(&[src, &p.completion()].concat()));
        }
        // Invalid UTF-8 never enters, even in a comment.
        assert!(fed(b"// \xc0\xaf").is_err());
        assert!(fed(b"// \xed\xa0\x80").is_err());
        assert!(fed(b"// \xff").is_err());
        assert!(fed("f g()>i64=1 // é".as_bytes()).is_ok_and(|p| p.is_accepting()));
    }

    #[test]
    fn checkpoint_restore_branches() {
        let mut p = fed(b"f g(a:i64)>i64=").expect("prefix");
        let cp = p.checkpoint();
        p.accept(b"a+1").expect("branch 1");
        assert!(p.is_accepting());
        p.restore(cp);
        assert!(!p.is_accepting());
        p.accept(b"(a)").expect("branch 2");
        assert!(p.is_accepting());
    }

    #[test]
    fn mask_admits_only_viable_tokens() {
        let vocab = [
            "a", "+", "+a", "->", " ", "(", ")", "", ";", "12", "//", "fn",
        ];
        let p = fed(b"f g(a:i64)>i64=a").expect("prefix");
        let mask = p.mask(&vocab);
        let admitted: Vec<&str> = vocab
            .iter()
            .zip(&mask)
            .filter(|(_, m)| **m)
            .map(|(t, _)| *t)
            .collect();
        // `a`, `12` and `fn` extend the identifier `a` (`aa`, `a12`, `afn`).
        assert_eq!(admitted, ["a", "+", "+a", " ", "", "12", "//", "fn"]);
        let allowed = p.allowed_bytes();
        assert!(allowed[usize::from(b'+')] && !allowed[usize::from(b')')]);
    }

    /// The nesting and depth limits are part of the language: `(` is masked
    /// out exactly where the parser would report `E-syntax-nesting-too-deep`.
    #[test]
    fn limits_match_the_parser() {
        let head = "f g(a:i64)>i64=";
        // MAX_NESTING parentheses parse; one more does not.
        let ok = format!(
            "{head}{}a{}",
            "(".repeat(MAX_NESTING),
            ")".repeat(MAX_NESTING)
        );
        assert!(parses(ok.as_bytes()));
        assert_valid(&ok);
        let deep = format!("{head}{}", "(".repeat(MAX_NESTING));
        let p = fed(deep.as_bytes()).expect("at the limit");
        assert!(!p.expected().contains(&Terminal::LParen));
        assert!(!p.admits(b"("));
        let over = format!(
            "{head}{}a{}",
            "(".repeat(MAX_NESTING + 1),
            ")".repeat(MAX_NESTING + 1)
        );
        assert!(!parses(over.as_bytes()));

        // A `+` chain of exactly MAX_EXPR_DEPTH links parses; one more does not.
        let chain = |n: usize| format!("{head}{}", vec!["a"; n + 1].join("+"));
        assert!(parses(chain(MAX_EXPR_DEPTH).as_bytes()));
        let p = fed(chain(MAX_EXPR_DEPTH).as_bytes()).expect("at the limit");
        assert!(p.is_accepting());
        assert!(!p.admits(b"+"));
        assert!(!parses(chain(MAX_EXPR_DEPTH + 1).as_bytes()));

        // Depth through parentheses: `a+...+(b+...)` must fit the outer
        // budget; check each boundary against the parser.
        for inner in [MAX_EXPR_DEPTH - 3, MAX_EXPR_DEPTH - 2, MAX_EXPR_DEPTH - 1] {
            for outer in [0, 1, 2] {
                let src = format!(
                    "{head}{}({})",
                    "a+".repeat(outer),
                    vec!["b"; inner + 1].join("+")
                );
                let full = fed(src.as_bytes()).is_ok_and(|p| p.is_accepting());
                assert_eq!(full, parses(src.as_bytes()), "outer {outer} inner {inner}");
            }
        }
    }

    /// Deterministic xorshift so the property tests are reproducible.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[(self.next() % items.len() as u64) as usize]
        }
    }

    const PIECES: &[&str] = &[
        "f", " ", "\n", "a", "b", "i64", "0", "12", "(", ")", ":", ",", ">", "=", "+", "/",
        "//x\n", "-", ";", "{", "}", "fn", "->", "é", "\t", "_", "f ",
    ];

    const SEEDS: &[&str] = &[
        "f add(a:i64,b:i64)>i64=a+b",
        "f g(x:i64)>i64=(x+1)+((x))\n",
        " f  h ( ) > i64 = 7 // c\n",
    ];

    /// Soundness and exactness on random edits of valid programs: whenever
    /// the recognizer accepts a prefix its completion parses, and a whole
    /// input is accepting exactly when the parser accepts it.
    #[test]
    fn random_edits_agree_with_the_parser() {
        let mut rng = Rng(0x7e55_e7a5_0b0b_cafe);
        let (mut accepted, mut rejected) = (0, 0);
        for _ in 0..5000 {
            let mut src = rng.pick(SEEDS).as_bytes().to_vec();
            for _ in 0..=rng.next() % 3 {
                let at = (rng.next() % (src.len() as u64 + 1)) as usize;
                match rng.next() % 3 {
                    0 if at < src.len() => {
                        src.remove(at);
                    }
                    1 => {
                        let piece = rng.pick(PIECES).as_bytes();
                        src.splice(at..at, piece.iter().copied());
                    }
                    _ => src.truncate(at),
                }
            }
            let full = fed(&src).is_ok_and(|p| p.is_accepting());
            assert_eq!(full, parses(&src), "{:?}", String::from_utf8_lossy(&src));
            if full {
                accepted += 1;
            } else {
                rejected += 1;
            }
            // The longest viable prefix completes.
            let mut p = Prefix::start();
            let mut end = 0;
            for (i, &b) in src.iter().enumerate() {
                if p.accept(&[b]).is_err() {
                    break;
                }
                end = i + 1;
            }
            let completed = [&src[..end], &p.completion()].concat();
            assert!(parses(&completed), "{completed:?}");
        }
        assert!(accepted > 500 && rejected > 500, "{accepted} / {rejected}");
    }

    /// Constrained random generation: sampling only masked-in pieces never
    /// leaves a viable state, and stopping anywhere completes.
    #[test]
    fn masked_generation_is_always_viable() {
        let mut rng = Rng(0x0dd_ba11);
        let vocab: Vec<&[u8]> = PIECES.iter().map(|s| s.as_bytes()).collect();
        let mut finished = 0;
        for _ in 0..300 {
            let mut p = Prefix::start();
            let mut out = Vec::new();
            for _ in 0..40 {
                let mask = p.mask(&vocab);
                let allowed: Vec<&[u8]> = vocab
                    .iter()
                    .zip(&mask)
                    .filter(|(_, m)| **m)
                    .map(|(t, _)| *t)
                    .collect();
                assert!(!allowed.is_empty(), "dead end after {out:?}");
                let t = *rng.pick(&allowed);
                p.accept(t).expect("masked-in tokens are accepted");
                out.extend_from_slice(t);
                assert!(parses(&[&out[..], &p.completion()].concat()), "{out:?}");
            }
            finished += usize::from(p.is_accepting());
        }
        assert!(finished > 0);
    }

    #[test]
    fn fingerprint_names_grammar_and_limits() {
        assert_eq!(
            fingerprint(),
            format!("tessera.tc-syntax/v0;nesting={MAX_NESTING};expr-depth={MAX_EXPR_DEPTH}")
        );
    }
}
