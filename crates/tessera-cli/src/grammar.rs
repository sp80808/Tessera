//! `tsr grammar`: the TC v0 surface the front end accepts, for prompts and
//! grammar-constrained decoding.
//!
//! Putting a BNF grammar in the prompt helps models emit an unfamiliar DSL
//! (Wang et al., "Grammar Prompting", 2023), and constrained decoders accept
//! one directly: GBNF for llama.cpp, Lark for vLLM's `guided_grammar` and
//! other CFG-constrained APIs. GBNF and Lark describe only the canonical
//! spelling (`tsr fmt` output), so a constrained model cannot produce
//! anything TC would reformat. The grammars are context free; names and
//! types are still `tsr check`'s job.

/// ISO 14977 EBNF, whitespace-tolerant like the parser.
pub const EBNF: &str = r#"(* Tessera Compact (TC) v0: what `tsr check` accepts today. *)
(* Tokens may be separated by whitespace and `//` line comments. *)
program  = function ;                       (* exactly one function per file *)
function = "f" , ident , "(" , [ param , { "," , param } ] , ")" , ">" , type , "=" , expr ;
param    = ident , ":" , type ;
type     = "i64" ;                          (* the only type *)
expr     = term , { "+" , term } ;          (* `+` is the only operator *)
term     = int | ident | "(" , expr , ")" ; (* an ident must be a parameter *)
ident    = ( letter | "_" ) , { letter | digit | "_" } ;
int      = digit , { digit } ;              (* no sign: there are no negative literals *)
(* Canonical spelling: no spaces except after `f`, e.g. f add(a:i64,b:i64)>i64=a+b *)
"#;

/// llama.cpp GBNF, canonical spelling only.
pub const GBNF: &str = r#"# Tessera Compact (TC) v0, canonical spelling (`tsr fmt` output).
root   ::= "f " ident "(" params? ")>i64=" expr "\n"?
params ::= param ("," param)*
param  ::= ident ":i64"
expr   ::= term ("+" term)*
term   ::= int | ident | "(" expr ")"
ident  ::= [A-Za-z_] [A-Za-z0-9_]*
int    ::= [0-9]+
"#;

/// Lark, canonical spelling only.
pub const LARK: &str = r#"// Tessera Compact (TC) v0, canonical spelling (`tsr fmt` output).
start: "f " IDENT "(" [param ("," param)*] ")>i64=" expr "\n"?
param: IDENT ":i64"
?expr: term ("+" term)*
?term: INT | IDENT | "(" expr ")"
IDENT: /[A-Za-z_][A-Za-z0-9_]*/
INT: /[0-9]+/
"#;

/// The grammar in `format`, or `None` for an unknown format.
#[must_use]
pub fn grammar(format: &str) -> Option<&'static str> {
    match format {
        "ebnf" => Some(EBNF),
        "gbnf" => Some(GBNF),
        "lark" => Some(LARK),
        _ => None,
    }
}
