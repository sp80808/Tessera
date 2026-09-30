//! Name resolution (contract B3).
//!
//! Binds every expression path to a binder of its function and every type
//! annotation to a primitive type. Results are keyed by HIR ids and carry no
//! spans; diagnostics point at HIR provenance.
//!
//! Deterministic by construction: functions in module order, parameters in
//! declaration order, expressions in id order, `BTreeMap` for the results.

use std::collections::BTreeMap;

use tessera_hir::{Expr, ExprId, FnItem, HirOutput, Item, ItemId, LocalId, TypePos, TypeRef};
use tessera_phases::{Diagnostic, DiagnosticSet, FileId, Phase, PhaseOutput};

use crate::Ty;
use crate::input::{Site, check};

/// The v0 primitive type table: the type names TC programs may use. Only
/// `i64`, matching the documented subset (`tessera_syntax` rejects anything
/// else, and `lower_to_tc` cannot spell `bool`).
pub const V0_PRIMS: &[(&str, Ty)] = &[("i64", Ty::I64)];

/// What a name occurrence refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Res {
    /// A binder of the same function (a parameter, in the current grammar).
    Local(LocalId),
    /// Nothing of that name is in scope. Reported once, at the occurrence.
    Unresolved,
}

/// Resolution facts for one function.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedFn {
    pub item: ItemId,
    /// `false` if the HIR broke sema's input requirements (`E-hir-malformed`);
    /// nothing else here is meaningful then, and later phases skip it.
    pub well_formed: bool,
    /// The item and all parameters have names, and none is a duplicate, so
    /// TIR can name them. (Missing names were reported by the parser.)
    pub declarations_ok: bool,
    /// Declared parameter types in order; [`Ty::Error`] where an annotation is
    /// missing, erroneous or unknown.
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// The [`Res`] of every [`Expr::Path`] of the body.
    pub names: BTreeMap<ExprId, Res>,
}

/// Resolution facts for one file, parallel to `HirModule::items`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedModule {
    pub file: FileId,
    pub funcs: Vec<ResolvedFn>,
}

/// [`resolve_with`] the v0 primitive table.
#[must_use]
pub fn resolve(hir: &HirOutput) -> PhaseOutput<ResolvedModule> {
    resolve_with(hir, V0_PRIMS)
}

/// Resolve names and type annotations of every function, with `prims` as the
/// primitive type table.
#[must_use]
pub fn resolve_with(hir: &HirOutput, prims: &[(&str, Ty)]) -> PhaseOutput<ResolvedModule> {
    let file = hir.module.file;
    let mut diagnostics = DiagnosticSet::new();
    let funcs = hir
        .module
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let Item::Fn(func) = item;
            let site = Site::new(file, hir.provenance.items.get(i));
            resolve_fn(func, &site, prims, &mut diagnostics)
        })
        .collect();
    PhaseOutput::with(ResolvedModule { file, funcs }, diagnostics)
}

fn error(code: &'static str, message: String, at: tessera_phases::Provenance) -> Diagnostic {
    Diagnostic::error(Phase::Resolve, code, message, at)
}

fn resolve_fn(
    func: &FnItem,
    site: &Site<'_>,
    prims: &[(&str, Ty)],
    diagnostics: &mut DiagnosticSet,
) -> ResolvedFn {
    let path = func.id.path();
    if let Some(problem) = check(func, site.table()) {
        diagnostics.push(error(
            "E-hir-malformed",
            format!("malformed HIR for `{path}`, not analysed: {problem}"),
            site.item(),
        ));
        return ResolvedFn {
            item: func.id.clone(),
            well_formed: false,
            declarations_ok: false,
            params: Vec::new(),
            ret: Ty::Error,
            names: BTreeMap::new(),
        };
    }

    let mut declarations_ok = func.name.is_some();
    if func.name.is_some() && func.id.disambiguator > 0 {
        declarations_ok = false;
        diagnostics.push(error(
            "E-resolve-duplicate-item",
            format!("`{}` is already defined in this file", func.id.name),
            site.name(),
        ));
    }

    // Parameters are the only binders: the first of each name is in scope.
    // A map, not a list: files may declare tens of thousands of parameters.
    let mut scope: BTreeMap<&str, LocalId> = BTreeMap::new();
    for param in &func.params {
        match &func.body.local(param.local).name {
            None => declarations_ok = false,
            Some(name) if scope.contains_key(name.as_str()) => {
                declarations_ok = false;
                diagnostics.push(error(
                    "E-resolve-duplicate-param",
                    format!("duplicate parameter `{name}` (uses refer to the first one)"),
                    site.local(param.local),
                ));
            }
            Some(name) => {
                scope.insert(name, param.local);
            }
        }
    }

    let mut annotation = |ty: &TypeRef, pos: TypePos| match ty {
        TypeRef::Path(name) => {
            if let Some((_, ty)) = prims.iter().find(|(n, _)| n == name) {
                *ty
            } else {
                let known: Vec<&str> = prims.iter().map(|(n, _)| *n).collect();
                diagnostics.push(error(
                    "E-resolve-unknown-type",
                    format!("unknown type `{name}` (known: {})", known.join(", ")),
                    site.ty(pos),
                ));
                Ty::Error
            }
        }
        // The parser already reported these.
        TypeRef::Missing | TypeRef::Error => Ty::Error,
    };
    let params = func
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| annotation(&p.ty, TypePos::Param(crate::input::id32(i))))
        .collect();
    let ret = annotation(&func.ret, TypePos::Ret);

    let mut names = BTreeMap::new();
    for (i, expr) in func.body.exprs.iter().enumerate() {
        let Expr::Path(name) = expr else { continue };
        let id = ExprId(crate::input::id32(i));
        let res = if let Some(local) = scope.get(name.as_str()) {
            Res::Local(*local)
        } else {
            diagnostics.push(error(
                "E-resolve-unbound-name",
                format!(
                    "unbound variable `{name}` (not a parameter of `{}`)",
                    func.id.name
                ),
                site.expr(id),
            ));
            Res::Unresolved
        };
        names.insert(id, res);
    }

    ResolvedFn {
        item: func.id.clone(),
        well_formed: true,
        declarations_ok,
        params,
        ret,
        names,
    }
}
