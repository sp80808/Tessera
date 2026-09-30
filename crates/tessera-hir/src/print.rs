//! Canonical TC printer over HIR: the single definition of "exactly one TC
//! spelling" (`tsr fmt`), independent of how the source was written.
//!
//! Because HIR has no trivia and no redundant grouping, `print_tc(lower(x))` is
//! the canonical form of `x`: whitespace, comments and redundant parentheses
//! are gone by construction. The printer must be **tree-preserving**: it
//! prints exactly the parentheses needed for the result to parse back to the
//! same tree. `+` is left-associative, so a right operand that is itself a sum
//! is parenthesized (`a+(b+c)`), while a left operand never is (`a+b+c` is
//! `(a+b)+c`).
//!
//! Only clean HIR can be printed: error/missing nodes have no canonical
//! spelling and are reported, never guessed.

use std::fmt;

use crate::{BinOp, Body, Expr, ExprId, FnItem, HirModule, Item, TypeRef};

/// Why a HIR item has no canonical TC spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintError {
    /// Dump path of the item, e.g. `fn/add`.
    pub item: String,
    pub what: &'static str,
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: cannot print canonical TC: {}", self.item, self.what)
    }
}

impl std::error::Error for PrintError {}

/// Canonical TC for every item, one per line, no trailing newline.
pub fn print_tc(module: &HirModule) -> Result<String, PrintError> {
    let mut out = String::new();
    for (i, item) in module.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let Item::Fn(func) = item;
        print_fn(func, &mut out)?;
    }
    Ok(out)
}

fn print_fn(func: &FnItem, out: &mut String) -> Result<(), PrintError> {
    let fail = |what| PrintError {
        item: func.id.path(),
        what,
    };
    out.push_str("f ");
    out.push_str(
        func.name
            .as_deref()
            .ok_or_else(|| fail("missing function name"))?,
    );
    out.push('(');
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let name = func
            .body
            .local(param.local)
            .name
            .as_deref()
            .ok_or_else(|| fail("missing parameter name"))?;
        out.push_str(name);
        out.push(':');
        print_type(&param.ty, out).map_err(fail)?;
    }
    out.push_str(")>");
    print_type(&func.ret, out).map_err(fail)?;
    out.push('=');
    print_expr(&func.body, func.body.root, false, out).map_err(fail)
}

fn print_type(ty: &TypeRef, out: &mut String) -> Result<(), &'static str> {
    match ty {
        TypeRef::Path(name) => {
            out.push_str(name);
            Ok(())
        }
        TypeRef::Missing => Err("missing type"),
        TypeRef::Error => Err("erroneous type"),
    }
}

/// `as_right_operand`: the expression is the right operand of a binary
/// operator, so a nested binary needs parentheses to keep its tree.
fn print_expr(
    body: &Body,
    id: ExprId,
    as_right_operand: bool,
    out: &mut String,
) -> Result<(), &'static str> {
    match body.expr(id) {
        Expr::Int(value) => {
            out.push_str(&value.to_string());
            Ok(())
        }
        Expr::Path(name) => {
            out.push_str(name);
            Ok(())
        }
        Expr::Binary { op, lhs, rhs } => {
            if as_right_operand {
                out.push('(');
            }
            print_expr(body, *lhs, false, out)?;
            out.push_str(match op {
                BinOp::Add => "+",
            });
            print_expr(body, *rhs, true, out)?;
            if as_right_operand {
                out.push(')');
            }
            Ok(())
        }
        Expr::Error => Err("erroneous expression"),
        Expr::Missing => Err("missing expression"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ItemId, ItemKind, Local, LocalId, Param};
    use tessera_phases::FileId;

    /// Build `f NAME(params: i64)>i64=BODY` from a pre-order expression list.
    fn func(name: &str, params: &[&str], exprs: Vec<Expr>) -> HirModule {
        HirModule {
            file: FileId(0),
            items: vec![Item::Fn(FnItem {
                id: ItemId {
                    file: FileId(0),
                    kind: ItemKind::Fn,
                    name: name.to_owned(),
                    disambiguator: 0,
                },
                name: Some(name.to_owned()),
                params: (0..params.len() as u32)
                    .map(|i| Param {
                        local: LocalId(i),
                        ty: TypeRef::Path("i64".to_owned()),
                    })
                    .collect(),
                ret: TypeRef::Path("i64".to_owned()),
                body: Body {
                    locals: params
                        .iter()
                        .map(|p| Local {
                            name: Some((*p).to_owned()),
                        })
                        .collect(),
                    exprs,
                    root: ExprId(0),
                },
            })],
        }
    }

    fn path(n: &str) -> Expr {
        Expr::Path(n.to_owned())
    }

    fn add(lhs: u32, rhs: u32) -> Expr {
        Expr::Binary {
            op: BinOp::Add,
            lhs: ExprId(lhs),
            rhs: ExprId(rhs),
        }
    }

    #[test]
    fn prints_the_bootstrap_function_canonically() {
        let m = func("add", &["a", "b"], vec![add(1, 2), path("a"), path("b")]);
        assert_eq!(print_tc(&m).as_deref(), Ok("f add(a:i64,b:i64)>i64=a+b"));
    }

    #[test]
    fn left_nesting_needs_no_parentheses_but_right_nesting_does() {
        // (a+b)+c, pre-order: 0=Add(1,4) 1=Add(2,3) 2=a 3=b 4=c
        let left = func(
            "x",
            &["a", "b", "c"],
            vec![add(1, 4), add(2, 3), path("a"), path("b"), path("c")],
        );
        assert_eq!(
            print_tc(&left).as_deref(),
            Ok("f x(a:i64,b:i64,c:i64)>i64=a+b+c")
        );
        // a+(b+c), pre-order: 0=Add(1,2) 1=a 2=Add(3,4) 3=b 4=c
        let right = func(
            "x",
            &["a", "b", "c"],
            vec![add(1, 2), path("a"), add(3, 4), path("b"), path("c")],
        );
        assert_eq!(
            print_tc(&right).as_deref(),
            Ok("f x(a:i64,b:i64,c:i64)>i64=a+(b+c)")
        );
    }

    #[test]
    fn literals_and_zero_parameter_functions() {
        let m = func("zero", &[], vec![Expr::Int(42)]);
        assert_eq!(print_tc(&m).as_deref(), Ok("f zero()>i64=42"));
    }

    #[test]
    fn error_and_missing_nodes_have_no_canonical_spelling() {
        for (expr, what) in [
            (Expr::Error, "erroneous expression"),
            (Expr::Missing, "missing expression"),
        ] {
            let err = print_tc(&func("e", &[], vec![expr])).unwrap_err();
            assert_eq!((err.item.as_str(), err.what), ("fn/e", what));
        }
        let with = |edit: &dyn Fn(&mut FnItem)| {
            let mut m = func("e", &[], vec![Expr::Int(1)]);
            let Item::Fn(f) = &mut m.items[0];
            edit(f);
            print_tc(&m).unwrap_err().what
        };
        assert_eq!(with(&|f| f.ret = TypeRef::Missing), "missing type");
        assert_eq!(with(&|f| f.ret = TypeRef::Error), "erroneous type");
        assert_eq!(with(&|f| f.name = None), "missing function name");
    }

    /// The deepest chain the frontend accepts must print on a default test stack.
    #[test]
    fn deep_left_chains_print_without_overflowing_the_stack() {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let n = 1001_u32; // terms; frontend limit is 1000 `+` links
                // pre-order of a left chain: node i = Add(lhs = i+1, rhs = leaf_i);
                // leaves are appended after the spine in the arena.
                let mut exprs = Vec::new();
                let spine = n - 1; // Add nodes
                for i in 0..spine {
                    exprs.push(add(i + 1, spine + 1 + i));
                }
                exprs.push(path("a")); // deepest lhs leaf
                for _ in 0..spine {
                    exprs.push(path("a"));
                }
                // exprs[spine] is the deepest lhs; rhs leaves start at spine+1
                let m = func("c", &["a"], exprs);
                let text = print_tc(&m).unwrap();
                assert_eq!(text.matches('+').count(), spine as usize);
                assert!(text.starts_with("f c(a:i64)>i64=a+a+a"));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
