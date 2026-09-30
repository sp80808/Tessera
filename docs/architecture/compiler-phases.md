# Compiler phase contracts and representation invariants

Status: architecture contract for issue #17. Phases marked *proposed* are not implemented; nothing here freezes TC surface syntax.

This document fixes **what each compiler representation is allowed to know, how it is identified, and how it maps back to TC source**, so #18–#24 can be built and tested independently. It defines boundaries, not grammar. Every TC spelling that appears below comes from the provisional bootstrap subset (`examples/bootstrap.tes`) and stays provisional until #1/#2 evidence promotes it.

Related: [ADR 0001](decisions/0001-representation-boundaries.md) (TC/TIR/TCap/TCG/TMT stay distinct), [ADR 0002](decisions/0002-compiler-phase-boundaries.md) (decisions taken here), [incremental query engine](incremental-query-engine.md) (#9).

Vocabulary shared by all phases (spans, provenance, diagnostics, `PhaseOutput`) is real code in [`crates/tessera-phases`](../../crates/tessera-phases/src/lib.rs); dependency direction and purity are enforced by [`crates/tessera-phases/tests/architecture.rs`](../../crates/tessera-phases/tests/architecture.rs).

## 1. Pipeline

```
SourceText
  │  lex + parse                          (tolerant: always returns a tree + diagnostics)
  ▼
lossless CST          syntax only, byte-exact, contains trivia
  │  normalize
  ▼
normalized HIR        semantic-facing structure, names still unresolved
  │  resolve
  ▼
resolved HIR          every name is an explicit stable identity
  │  infer types + effects
  ▼
typed/effect HIR      owns type and effect facts
  │  make explicit
  ▼
TIR                   explicit, explanatory, recoverable to TC. NOT an optimization IR
  │  build CFG
  ▼
MIR / CFG             boring, explicit, backend-neutral
  │  codegen          (only crate that names Cranelift)
  ▼
backend IR            Cranelift IR: private to the backend crate
  │  emit + link
  ▼
object / executable
```

| # | Representation | Crate home | Issue | State today |
|---|---|---|---|---|
| B0 | `SourceText` | `tessera-db` (Salsa input) | #9 | exists (`SourceFile`) |
| B1 | lossless CST | `tessera-syntax` | #18 | **lexer lossless (this change)**; tree/events *proposed* |
| B2 | normalized HIR | `tessera-hir` *(proposed)* | #19 | *proposed* (bootstrap `AstFunction` is a stand-in) |
| B3 | resolved HIR | `tessera-sema` *(proposed)* | #20 | *proposed* |
| B4 | typed/effect HIR | `tessera-sema` | #21 | *proposed* (bootstrap type checking lives inside `to_tir`) |
| B5 | TIR | `tessera-tir` | #2/#21 | provisional, exists (single function, `i64` + `+`) |
| B6 | MIR / CFG | `tessera-mir` *(proposed)* | #22 | *proposed* |
| B7 | backend IR | `tessera-codegen-cranelift` *(proposed)* | #4 | *proposed* |
| B8 | object / link | driver (`tessera-cli`) | #24 | *proposed* |

Crate names for proposed crates are defaults recorded in the `LAYERS` table of the architecture test; the issue that creates them may change the name but must update the table and this document.

## 2. Cross-cutting contracts

These apply to every boundary; §3 only states what is specific.

### 2.1 Purity and determinism (INV-PURE-1, INV-SALSA-1)

A phase is a pure function of explicit inputs:

- inputs: previous-phase outputs plus explicit driver inputs (`SourceFile`, `CompilerOptions`, `TargetSpec`, manifest/lockfile). Nothing else;
- no network, no model calls, no process environment reads, no wall clock, no `static mut`/`thread_local!`/lazily initialised mutable globals (checked mechanically);
- outputs are deterministic values: `Eq + Hash + Clone`, iteration order defined by content (`BTreeMap`, sorted `Vec`, arena order fixed by the lowering algorithm) and never by hash-map iteration, allocation address or pass scheduling;
- phase functions take `&` inputs and return owned outputs. There is no `CompilerState` and no `&mut` context threaded across phases. Interning goes through the query database (Salsa interned/tracked structs), not through a global table.

Model, web and research activity updates `Evidence`/`Task` inputs **outside** the deterministic graph (see incremental-query-engine.md). TCG/TMT/context code may read compiler outputs; it must never be a dependency of any compiler phase (enforced by the `LAYERS` dependency table).

### 2.2 Results, never exits (INV-DIAG-1)

Every phase returns `PhaseOutput<T> { value, diagnostics }`. `value` is always present. Erroneous input produces a structurally valid value containing explicit error nodes (`Error`/`Missing`/`Res::Unresolved`/`Ty::Error`), so later phases and editors keep working and cascading diagnostics can be suppressed deterministically. Compiler crates never call `process::exit`, never print diagnostics, and never panic on any input, including arbitrary bytes and adversarially nested input. Rendering and exit codes belong to the driver (#23/#24).

### 2.3 Provenance (INV-PROV-1..3)

Every semantic entity that can carry a diagnostic has a `Provenance`:

```rust
enum Provenance {
    Source(Span),                                   // written by the author
    Synthesized { origin: Span, why: &'static str } // introduced by the compiler
}
```

There is no "unknown" variant. A compiler-introduced node (desugaring, implicit drop, inferred type made explicit) names the source span that caused it and a stable reason string.

- PROV-1: for every ID a phase exposes, its `ProvenanceMap<Id>` is total. `ProvenanceMap::missing(ids)` must be empty at every lowering boundary.
- PROV-2: lowering `A → B` maps every `B` ID to provenance derived from the `A` provenance of its origin. Spans never reset to zero or to "whole file" to make a check pass. *(Violated today by the bootstrap `to_tir`; see §6 G2 and the `#[ignore]`d test `semantic_errors_keep_their_source_offset`.)*
- PROV-3: provenance lives in **side tables keyed by ID, not inside semantic nodes** from B2 onward (see 2.4 for why).

Spans are half-open byte ranges into the exact source text of one revision. They are not stable across edits; identity (2.4) is.

### 2.4 Identity and early cutoff (INV-ID-1..3)

Two different questions get two different mechanisms:

- **Identity** answers "is this the same entity after an edit?". It is *structural/path-based*, never offset-based.
- **Provenance** answers "where in the current text is it?". It is a span table that changes on every edit that moves text.

Keeping them apart is what lets an incremental engine cut off: reformatting a file changes only the provenance tables, so the semantic value of the HIR/typed HIR compares equal and dependents are not recomputed. Diagnostics are the join of semantic results with the *current* provenance tables at report time (#23), so spans stay correct without invalidating typing.

| ID | Definition | Stable across |
|---|---|---|
| `FileId` | interned by the driver from the source path | revision |
| `ItemId` | interned `(ModuleId, ItemKind, name, disambiguator)` where the disambiguator counts same-name items in the same module in source order | edits that do not add/remove/rename that item or reorder same-name siblings |
| `BodyId` | `ItemId` of the owning item | as `ItemId` |
| `ExprId`/`LocalId` | dense index into the body's arenas, assigned in pre-order by the deterministic lowering | edits that do not change that body's structure; **body-local**, only meaningful with the `BodyId` |
| `DefId` | `ItemId` or `(BodyId, LocalId)` | as components |
| `TyId` | hash-consed structural type | always (structural) |
| `TirFuncId` | `DefId` of the function | as `DefId` |
| `MirFuncId`, `BlockId`, `MirLocal` | function = `DefId`; blocks/locals dense in a fixed reverse-post-order/first-use order per function | MIR-internal; not a stability promise |

INV-ID-1: no ID is derived from a byte offset, pointer, hash of formatted text, or global counter.
INV-ID-2: whitespace/comment/redundant-parenthesis edits must not change any semantic value from B2 onward (tested for the bootstrap witness by `surface_variation_does_not_change_semantic_result`).
INV-ID-3: IDs never leak the surface syntax that produced them: a grammar replacement must be able to produce the same `ItemId`s.

### 2.5 Explicit versus inferred facts

A fact is **explicit** if it is spelled in the TC source and **inferred** if a phase derived it. Each phase owns a fixed set of inferred facts and must never *re-derive* a fact an earlier phase owns:

| Fact class | Derived by (owner) | Visible as explicit from |
|---|---|---|
| tokens/trivia, nesting | B1 | B1 |
| desugared forms (`a+b` → binary op, `(x)` → `x`) | B2 | B2 |
| which declaration a name refers to | B3 | B3 |
| expression types, literal defaulting, effect sets | B4 | B4, spelled on every TIR node in B5 |
| ownership decisions (move / borrow / drop points) | ownership analysis over B4 (#3) | B5 as explicit ops, B6 as explicit statements |
| control flow, temporaries, drop elaboration | B6 lowering | B6 |
| machine representation, ABI, instruction selection | B7 | B7 only |

TIR is the point where **all** inferred semantic facts are written out (ADR 0001, AGENTS.md rule "TIR must make hidden/inferred semantics explicit"). MIR lowers from TIR alone.

### 2.6 Debug/serialization expectations

`#[derive(Debug)]` is not a contract. Each representation has a canonical **text dump**, deterministic, free of pointers, hashes and process-local counters, suitable for golden snapshots and for humans/models to read:

| Repr | Dump | Status |
|---|---|---|
| CST | `Kind start..end "text"` per token; nested/indented for nodes | tokens implemented (`lexer::dump`), tree *proposed* |
| HIR / resolved / typed | S-expression, IDs printed as paths (`fn/add`, `local/a`), no spans inline; spans dumped in a separate `provenance:` section | *proposed* |
| TIR | S-expression, every node typed (`.tir`) | implemented (`TirFunction::to_text`) |
| MIR | textual CFG: blocks, statements, terminators; locals `_N` | *proposed* |
| Backend IR | not a compiler contract (backend-private) | n/a |

Only **TIR text** is a public, versioned artifact (it is what tools and models read). Everything else is internal and may change without notice; internal dumps are snapshot-tested and their format changes are ordinary diffs.

## 3. Boundary specifications

Each table answers the ten required questions in order: (1) responsibility, (2) owns, (3) forbidden, (4) source/trivia preservation, (5) identity, (6) provenance mapping, (7) explicit vs inferred, (8) allowed consumers, (9) serialization/debug, (10) lowering preconditions/invariants to the next stage.

### B0 — `SourceText`

| | |
|---|---|
| Responsibility | Hold the exact bytes the author wrote for one file, as a Salsa **input**. |
| Owns | Path, text, revision. Content hash for cutoff. |
| Forbidden | Any interpretation: no line-ending normalization, BOM stripping, tab expansion or trimming. |
| Trivia | Total: it *is* the source. |
| Identity | `FileId`. |
| Provenance | The origin of all provenance: `Span { file, start, end }` in bytes. Line/column are derived on demand by the driver from the same text, never stored in phases. |
| Explicit/inferred | Everything is explicit; nothing inferred. |
| Consumers | `parse`, driver, diagnostic renderer. |
| Debug | The raw text. |
| Lowering precondition | Text is valid UTF-8 (invalid input is rejected by the driver with a diagnostic before B1; lossy decoding is not allowed to silently change offsets). |

### B1 — lossless CST

| | |
|---|---|
| Responsibility | Represent the source's syntactic structure so that the exact text can be reproduced, including malformed input. |
| Owns | Tokens (with trivia), node kinds of the *current* grammar, error/missing nodes, syntax diagnostics. Parser events are a separate stream decoupled from tree storage (#18). |
| Forbidden | Any name resolution, type, effect, ownership or capability information. No dependency on TIR/HIR types. No interpretation of literal values beyond token kind. Keywords are parser decisions, not lexer decisions, so grammar experiments do not touch the lexer. |
| Trivia | **Fully preserved.** Every input byte is covered by exactly one token; whitespace and comments are explicit trivia tokens (LEX-1, LEX-2, enforced for tokens today). |
| Identity | None across revisions. Within a revision a node is addressable by `(FileId, kind, range)`; a *syntax pointer* of that shape lets later queries re-find a node without holding the tree alive. |
| Provenance | Trivially exact: node/token range **is** the provenance. |
| Explicit/inferred | Only explicit syntax. Error recovery inserts explicit `Error`/`Missing` nodes; it does not guess intent. |
| Consumers | HIR lowering (#19); formatter; editor tooling; parser tests. **Not** resolution, typing, TIR, MIR or the backend. |
| Debug | Token dump (implemented, `lexer::dump`); tree dump (proposed). |
| Lowering precondition | (CST-1) concatenating all tokens equals the source; (CST-2) the parse returned a tree even for malformed input, with `diagnostics.has_errors()` describing it; (CST-3) parsing never panics or overflows the stack on any input (bounded nesting; enforced for the bootstrap parser by `hostile_nesting_returns_a_diagnostic_not_a_crash`); (CST-4) tree storage is replaceable without changing any HIR-facing API. |

The surface grammar is *replaceable*: two different grammars must be able to lower to the same B2 output. Only `SyntaxKind` and the parser depend on the grammar. Candidate grammars in #1/#2 are fixtures that produce the same B2, which is exactly what the `surface_variation_does_not_change_semantic_result` style of test generalizes.

### B2 — normalized HIR

| | |
|---|---|
| Responsibility | Turn syntax into a compact, stable, semantic-facing tree: normalize sugar, drop trivia and redundant grouping, allocate per-body arenas, assign IDs. |
| Owns | Items, bodies, expressions, patterns, type *syntax* (as paths not yet resolved), the item tree per module, IDs, and `ProvenanceMap`s. |
| Forbidden | Trivia, token/`SyntaxKind` types, byte offsets inside nodes (provenance is a side table), name resolution results, types, effects, ownership facts. |
| Trivia | Not preserved (lives in B1). What survives is the *provenance map*, which can locate the original text and therefore the trivia. |
| Identity | `ItemId`, `BodyId`, body-local `ExprId`/`LocalId` (§2.4). Assigned by deterministic pre-order lowering. |
| Provenance | `ProvenanceMap<ItemId|ExprId|LocalId>` total over every ID (PROV-1). Desugared nodes are `Synthesized { origin, why }`. |
| Explicit/inferred | Sugar becomes explicit structure (`a+b` → `Binary(Add, a, b)`; `(e)` → `e`). Nothing semantic is inferred: a path is still a path. |
| Consumers | Name resolution (#20), diagnostics, formatter-facing tooling that needs structure. **Not** the backend. |
| Debug | S-expression with IDs as paths + separate `provenance:` section. |
| Lowering precondition | (HIR-1) every node reachable from a module has an ID and provenance; (HIR-2) error/missing CST nodes appear as explicit `Error` nodes, not dropped; (HIR-3) no CST/syntax type appears in any public HIR type; (HIR-4) lowering is a pure function of the CST (plus module tree) — same CST ⇒ equal HIR. |

### B3 — resolved HIR

| | |
|---|---|
| Responsibility | Bind every name occurrence to an explicit stable identity via scopes, module index and imports. |
| Owns | `Res` per path occurrence (`Local(LocalId)`, `Def(DefId)`, `Prim(..)`, `Unresolved(name)`), module index, scope tree, visibility results, resolution diagnostics. |
| Forbidden | Types, effects, ownership; any resolution that consults the file system or environment ad hoc (module discovery is an explicit input). |
| Trivia | No. |
| Identity | `DefId`; resolution results are keyed by `(BodyId, ExprId)`. |
| Provenance | Reuses B2 provenance; new diagnostics (unresolved/ambiguous name) point at the occurrence's B2 provenance. |
| Explicit/inferred | Inferred: which declaration a name means. Written explicitly as `Res`. Unresolved names stay `Res::Unresolved` — no silent fallback to a "similar" name. |
| Consumers | Type/effect inference (#21); TCG/context tooling that wants a symbol graph. |
| Debug | B2 dump with `Res` annotations (`(path a -> local/a)`). |
| Lowering precondition | (RES-1) every path occurrence has a `Res`; (RES-2) no `Res::Def`/`Local` points at a non-existent ID; (RES-3) resolution is deterministic (same inputs ⇒ same `Res`, independent of hash order); (RES-4) name lookup dependencies are recorded as query reads so a changed *unrelated* item does not invalidate this body's resolution. |

### B4 — typed/effect HIR

| | |
|---|---|
| Responsibility | Assign a type to every expression and pattern, unify, default literals, and compute effect summaries. This is the **single home of type/effect facts**. |
| Owns | `Ty` per `(BodyId, ExprId)`, hash-consed `TyId`, function signatures, effect sets/summaries per function, type/effect diagnostics, an `OwnershipFacts` side table (see §5) once #3 lands. |
| Forbidden | Inference variables escaping the phase (all resolved or `Ty::Error`), CFG, machine layout/ABI, backend types. |
| Trivia | No. |
| Identity | `TyId` structural; facts keyed by existing B2/B3 IDs — B4 adds no new node identities. |
| Provenance | Reuses B2 provenance; cross-references (e.g. "expected `i64` because of return type at …") are additional `Provenance` values stored in the diagnostic. |
| Explicit/inferred | The inferred facts *are* this phase's output. Anything B4 infers must be spelled on the TIR node in B5. Effects are inferred bottom-up per function and summarized so a body-only change that keeps the summary equal does not invalidate callers. |
| Consumers | TIR lowering (B5), ownership analysis (#3), TCap (#12) via TIR, IDE queries. |
| Debug | B3 dump with `: Ty` and `! effects` annotations. |
| Lowering precondition | (TYP-1) every expression has a `Ty`; (TYP-2) no unresolved inference variable; (TYP-3) if `!diagnostics.has_errors()`, all operations have fully specified semantics (e.g. integer overflow behavior is decided, see §7 open question O1) — B5 is only built from clean typed HIR, or marks the function `Error`; (TYP-4) effect summary is deterministic and monotone (adding a call never removes effects). |

### B5 — TIR

| | |
|---|---|
| Responsibility | Be the **explanatory, explicit, recoverable** projection of a fully typed function: every type, coercion, ownership operation and effect is a written node, so a human or model can read what the compiler decided. It is the executable-semantic contract that MIR lowers from. |
| Owns | Functions, explicit-typed expression tree, explicit ownership ops (`move`/`borrow`/`drop`) once #3 lands, effect annotations, an `origin` link from every node to a B4 `(BodyId, ExprId)` or a `Synthesized` marker. |
| Forbidden | Optimization: TIR is **never** transformed by optimization passes and never becomes the optimization IR. No CFG, no basic blocks, no backend/target detail. No information that is not derivable from B4. |
| Trivia | No. Recoverability means: `lower_to_tc(TIR)` yields canonical TC (round-trip for the supported subset), not that formatting is preserved. |
| Identity | `TirFuncId = DefId`; a node is identified by `(TirFuncId, origin)`. |
| Provenance | Every TIR node has `Provenance`, from its B4 origin; nodes the compiler introduced (explicit drop, defaulted literal type made visible) are `Synthesized` with the causing source span. |
| Explicit/inferred | **Nothing inferred remains.** This is the boundary where inference ends. |
| Consumers | Humans and models (public artifact), `tsr tir`, TCap derivation (#12), tests, MIR lowering (#22), TMT/TCG projections. **Not** the backend and not optimization passes. |
| Debug | S-expression `.tir` text — the only public, versioned dump. Deterministic. |
| Lowering precondition | (TIR-1) every node carries an explicit type; (TIR-2) TIR is well-formed under a standalone verifier that does not consult B1–B4 (so a hand-written `.tir` file can be checked and lowered); (TIR-3) round-trip `lower_to_tc(TIR)` is canonical TC for the supported subset (enforced for the bootstrap subset today, see §6 G5 for its limits); (TIR-4) all nodes have provenance (PROV-1); (TIR-5) MIR lowering reads TIR only. |

### B6 — MIR / CFG

| | |
|---|---|
| Responsibility | Be a deliberately **boring**, explicit, backend-neutral control-flow representation on which flow-sensitive checks and optimizations run. |
| Owns | Functions as CFGs of basic blocks; typed locals/temporaries; statements (assign, explicit move/drop after drop elaboration, call); terminators (goto, branch, return); per-statement provenance. |
| Forbidden | Source syntax, trivia, names beyond debug labels, effect *inference*, inference variables, surface types unresolved to machine-neutral forms, target/ABI/instruction details, any Cranelift/LLVM type. |
| Trivia | No. |
| Identity | `MirFuncId = DefId`; blocks and locals are dense, numbered by a fixed deterministic order (MIR-internal; no stability promise). |
| Provenance | Every statement and terminator carries `Provenance`: lowered TIR nodes inherit theirs, compiler-made ones (implicit return, drop, temporary) are `Synthesized` with the causing TIR/source span. |
| Explicit/inferred | Everything explicit: control flow, temporaries, drops, evaluation order. Overflow/undefined-behavior modes must be explicit per operation (no "default"). |
| Consumers | MIR verifier, MIR passes (borrow/liveness refinements such as non-lexical extents, optimizations), backend crate. **Not** the CST/HIR/TIR consumers above. |
| Debug | Textual CFG dump; snapshot-tested. Internal, may change. |
| Lowering precondition | (MIR-1) verifier passes: every block reachable from entry ends in exactly one terminator, every local is assigned before use on all paths, types agree; (MIR-2) provenance total (PROV-1); (MIR-3) no dependence on target beyond an explicit `TargetSpec` value; (MIR-4) the backend crate's public API accepts `&MirModule` and `&TargetSpec` and nothing older (enforced by dependency direction: `tessera-codegen-cranelift` may depend only on `tessera-phases` and `tessera-mir`). |

### B7 — backend IR (Cranelift first)

| | |
|---|---|
| Responsibility | Select instructions and emit machine code for one target from verified MIR. |
| Owns | Cranelift function/context objects, ABI decisions, symbol mangling for the object file, code emission. |
| Forbidden | Escaping into other crates: no Cranelift type in any public signature outside `tessera-codegen-cranelift` (enforced: `INV-BACKEND-1`). No re-deriving semantics; no reading CST/HIR/TIR. |
| Trivia | No. |
| Identity | Symbol names are a pure function of `DefId` (mangling scheme decided in #4). Backend IDs are private. |
| Provenance | Emits a mapping *symbol/instruction range → MIR provenance* for debug info and for backend diagnostics; backend errors are `Diagnostic`s whose provenance is the MIR statement's. |
| Explicit/inferred | Backend inferences (register allocation, instruction choice) never flow back into the compiler's semantic model. |
| Consumers | Object emission only. |
| Debug | Not a contract (Cranelift's own printer may be used privately for debugging). |
| Lowering precondition | (BE-1) input passed the MIR verifier; (BE-2) failures return `Vec<Diagnostic>`/`Result`, never panic or exit; (BE-3) same MIR + same `TargetSpec` ⇒ byte-identical object (reproducibility). |

### B8 — object / link

| | |
|---|---|
| Responsibility | Turn emitted object code into a linkable artifact / executable. |
| Owns | Artifact bytes, link command construction. |
| Forbidden | Being a tracked semantic query. Linking runs an external tool and is a driver effect; only *artifact bytes for one crate* are a query result. |
| Trivia | No. |
| Identity | Artifact path from explicit inputs (target, package, profile). |
| Provenance | Link errors are reported as tool diagnostics; source provenance exists only through the symbol→MIR mapping from B7. |
| Explicit/inferred | Explicit `TargetSpec` and linker choice, never sniffed at compile time. |
| Consumers | Driver (#24), users. |
| Debug | Command line + artifact hash. |
| Lowering precondition | n/a (terminal). |

## 4. Rust-facing handoffs

Real today, in `tessera-phases`: `FileId`, `Span`, `Provenance`, `ProvenanceMap<Id>`, `Phase`, `Severity`, `Diagnostic`, `DiagnosticSet` (canonical order independent of insertion order), `PhaseOutput<T>`.

Proposed handoff signatures (pseudocode; each is a Salsa tracked query in #9, but is a plain pure function first, testable without a database):

```rust
// Input
struct SourceFile { id: FileId, text: Arc<str> }                 // tessera-db, Salsa input

// B0 -> B1   (tolerant)
fn parse(file: &SourceFile) -> PhaseOutput<ParsedFile>;          // ParsedFile = CST + parser events

// B1 -> B2
fn lower(parsed: &ParsedFile, tree: &ModuleTree) -> PhaseOutput<HirModule>; // + ProvenanceMap per ID kind

// B2 -> B3
fn resolve(hir: &HirModule, index: &ModuleIndex) -> PhaseOutput<ResolvedModule>;

// B3 -> B4
fn typeck(res: &ResolvedModule, sigs: &SignatureIndex) -> PhaseOutput<TypedModule>; // types + effects

// B4 -> B5
fn to_tir(typed: &TypedModule) -> PhaseOutput<TirModule>;

// B5 -> B6
fn build_mir(tir: &TirModule) -> PhaseOutput<MirModule>;         // TIR only (TIR-5)

// B6 -> B7 -> B8
fn codegen(mir: &MirModule, target: &TargetSpec) -> Result<ObjectArtifact, DiagnosticSet>;
```

Rules for these signatures: inputs by shared reference; outputs owned and `Eq + Hash`; no `&mut`; no phase takes an earlier phase's output than the one immediately before it *except* through an explicit index/signature query (e.g. `typeck` reads other functions' signatures, never their bodies — this is the change firewall from the incremental-query-engine doc). `MirModule` is built from TIR alone.

Deliberate non-goals: no `Phase` trait, no generic pipeline runner, no `CompilerState`. The driver composes plain functions/queries; abstraction is added only when two implementations exist.

## 5. Where ownership and effects live (before #3)

- **Effects:** B4 owns effect sets/summaries (facts), computed with types by #21, printed as annotations in TIR.
- **Ownership declarations** (parameter modes, borrow syntax) are ordinary B2 structure. Their **analysis results** — move points, borrows and their extents, drop points — are an `OwnershipFacts` *side table keyed by B4 `(BodyId, ExprId)`*, produced by a query over B4 (#3). Keeping it a keyed side table (not fields on typed nodes) means the analysis can later move to MIR for non-lexical shortening (ROADMAP Milestone 2, step 8) without changing B4's shape.
- **Explicit form:** TIR spells the results as explicit `move`/`borrow`/`drop` nodes with `Synthesized` provenance where the source did not write them. MIR carries them as explicit statements after drop elaboration.
- **TCap (#12)** is a projection **derived from TIR/ownership facts**; it never feeds facts back into B4. (Note: the current TCap crate on the #16 branch derives directly from TIR ahead of #3; that is acceptable as a prototype but the dependency direction `tessera-tcap → tessera-tir` is the only edge it may have.)

## 6. Boundary invariants checked today vs. planned

| ID | Invariant | Enforcement |
|---|---|---|
| INV-DEP-1 | Crate dependency direction follows the table (semantic crates never depend on backend/context/db) | **test** `architecture.rs::workspace_dependency_direction_follows_the_contract`, `layer_table_is_acyclic`, `every_crate_is_registered_in_the_layer_table` |
| INV-BACKEND-1 | Only `tessera-codegen-cranelift` may depend on or name Cranelift/LLVM | **test** `only_the_backend_crate_may_name_backend_libraries` |
| INV-PURE-1 | No network/model clients; no environment reads in compiler crates | **test** `no_crate_depends_on_network_or_model_clients`, `compiler_sources_are_pure_and_do_not_exit` |
| INV-DIAG-1 | No `process::exit` in any crate | **test** `compiler_sources_are_pure_and_do_not_exit` |
| INV-SALSA-1 | No `static mut` / `thread_local!` / lazy mutable globals / wall clock | **test** (same) |
| LEX-1..3 | Lexer covers every byte, trivia explicit, never fails or panics | **test** `lexer::tests::*` (edge cases + 25k random inputs) |
| CST-3 | Parser cannot overflow the stack on hostile nesting | **test** `hostile_nesting_returns_a_diagnostic_not_a_crash`, `frontend_never_panics_on_arbitrary_text` |
| INV-ID-2 | Surface variation does not change the semantic result | **test** `surface_variation_does_not_change_semantic_result` (bootstrap subset) |
| DET-1 | Expansion and diagnostic order are deterministic | **test** `expansion_is_deterministic`; `DiagnosticSet` order test |
| TIR-3 | TC→TIR→TC round trip is exact on the bootstrap subset | **test** `tc_tir_tc_round_trip_is_byte_exact`, golden fixtures |
| PROV-1 | Provenance total per phase | mechanism **tested** (`ProvenanceMap::missing`); applied per phase by #19–#22 |
| PROV-2 | Spans survive lowering | **violated today**, executable gap: `#[ignore]`d test `semantic_errors_keep_their_source_offset` |
| CST-1/2/4, HIR-*, RES-*, TYP-*, TIR-1/2/4/5, MIR-*, BE-* | as specified in §3 | planned with the owning issue (#18–#22, #4) |

### Known gaps between current code and this contract

| Gap | Where | Owner |
|---|---|---|
| G1 | `parse` is fail-fast `Result<AstFunction, SyntaxError>`; no CST, no recovery, first error only | #18 |
| G2 | AST/TIR carry no spans; `to_tir` reports offset 0 for semantic errors (PROV-2) | #19 / #23 |
| G3 | `tessera-syntax` depends on `tessera-tir` (bootstrap AST→TIR + `lower_to_tc`); CST crate must not know TIR. Recorded as `TEMPORARY(#19)` in `LAYERS` | #19 |
| G4 | Bootstrap type checking and name resolution are fused inside `to_tir`; B3/B4 do not exist as phases | #20, #21 |
| G5 | `lower_to_tc` maps TIR `Bool`/`Eq`/`And`/`Not` onto `Int`/`Add` (`strip_types`); those TIR nodes are unreachable from the grammar. Round-trip claim holds only for the `i64` + `+` subset | #21 / #2 |
| G6 | `SyntaxError { at: usize }` is not a `phases::Diagnostic` | #23 |
| G7 | `tessera-db` exposes only stats queries (`byte_len`, `line_count`, `source_units`), no `parse` query | #9 |
| G8 | `tsr` has no `check` command; CLI calls the frontend directly | #24 |
| G9 | Syntax fuzz crate has several `fuzz_target!`s in a `[lib]`; not runnable with `cargo fuzz run`. Property tests in-tree cover panic-freedom on stable meanwhile | #18 |

## 7. Worked witness

Input: `examples/bootstrap.tes`, exactly `f add(a:i64,b:i64)>i64=a+b` plus a trailing newline (27 bytes, `FileId(0)`). Provisional syntax; the spelling is not a language commitment. Blocks are labelled **[real]** if produced by current code and **[proposed]** if they show the intended debug form of an unimplemented phase.

**B0 — SourceText [real]**

```
FileId(0)  27 bytes: "f add(a:i64,b:i64)>i64=a+b\n"
```

**B1 — CST.** Tokens **[real]** (verified by `lexer::tests::bootstrap_golden_token_stream`); node layer **[proposed]**, kind names provisional with the grammar:

```
Ident 0..1 "f"   Whitespace 1..2 " "   Ident 2..5 "add"   LParen 5..6 "("
Ident 6..7 "a"   Colon 7..8 ":"   Ident 8..11 "i64"   Comma 11..12 ","
Ident 12..13 "b" Colon 13..14 ":"  Ident 14..17 "i64"  RParen 17..18 ")"
Gt 18..19 ">"     Ident 19..22 "i64"   Eq 22..23 "="
Ident 23..24 "a" Plus 24..25 "+"   Ident 25..26 "b"   Whitespace 26..27 "\n"

File 0..27
  Fn 0..26  ── children: f, ws, name "add", ParamList 5..18, Gt, RetTy 19..22, Eq, BinExpr 23..26
    ParamList 5..18: Param 6..11 (a : i64), Comma, Param 12..17 (b : i64)
    BinExpr 23..26: PathExpr 23..24, Plus 24..25, PathExpr 25..26
  trailing Whitespace 26..27
```

Every byte is covered; the CST holds no meaning for `add`, `i64`, `a` or `+`.

**B2 — normalized HIR [proposed]**

```
module 0 (file 0)
  fn/add          ItemId(mod0, Fn, "add", #0)
    param local/a : path(i64)
    param local/b : path(i64)
    ret   : path(i64)
    body  BodyId(fn/add):
      e0 = path(a)            e1 = path(b)
      e2 = binary(add, e0, e1)          ; `a+b` normalized to a binary op
provenance:
  fn/add [0..26) Source   local/a [6..7) Source   local/b [12..13) Source
  e0 [23..24) Source      e1 [25..26) Source      e2 [23..26) Source
```

Paths are still names; no meaning is attached. Trivia and the newline are gone from the tree but recoverable through provenance into B0/B1.

**B3 — resolved HIR [proposed]**

```
e0 = path(a) -> local/a          e1 = path(b) -> local/b
path(i64) at ret/param types -> prim(i64)
```

Adds `Res` only. An unknown name would stay `-> unresolved("b")` with a resolve diagnostic pointing at that occurrence's provenance.

**B4 — typed/effect HIR [proposed]**

```
e0 : i64   e1 : i64   e2 : i64   ; inferred: operands unified, result type
fn/add : (i64, i64) -> i64  !{}  ; effect summary: pure
e2.overflow = UNSPECIFIED        ; open question O1: must be decided before B5/B6
```

All inferred facts appear here; provenance is unchanged.

**B5 — TIR [real]** (output of `tsr tir examples/bootstrap.tes`; provenance links are proposed, not printed today):

```
(func add (param a i64) (param b i64) (return i64) (body (add i64 (var a i64) (var b i64))))
```

Nothing inferred remains: the literal-free body still spells the type on every node. Intended origin links: `(add …)` → e2 `[23..26)`, `(var a …)` → e0 `[23..24)`, `(var b …)` → e1 `[25..26)`.

**B6 — MIR / CFG [proposed]**

```
fn add(_1: i64, _2: i64) -> i64 {          ; provenance: fn/add [0..26) Source
  bb0:
    _0 = Add(_1, _2)     ; overflow=<decided by O1>   [23..26) Source (e2)
    return               ; [23..26) Synthesized{ origin: e2, why: "implicit-return" }
}
```

Boring on purpose: one block, one assignment, one terminator; the implicit return is made explicit and points back at the expression that caused it.

**B7 — backend boundary [proposed]**

```
codegen(&MirModule, &TargetSpec) -> Result<ObjectArtifact, DiagnosticSet>
```

Inside the backend crate only: an `(i64, i64) -> i64` Cranelift function, ABI and instruction selection. Cranelift types do not appear in any signature outside `tessera-codegen-cranelift`. Symbol name is a pure function of `fn/add`'s `DefId` (scheme decided in #4).

**B8** — object bytes → linker invocation by the driver.

What the witness shows: every stage lowers from the previous one alone (`e2` is the same key at B2/B3/B4, and reappears as the `origin` of the TIR `add` node and of MIR's assignment); reformatting `f add( a:i64 , b:i64 ) > i64 = a + b` changes B1 tokens and provenance offsets but leaves B2–B5 equal (tested: `surface_variation_does_not_change_semantic_result`).

## 8. Salsa mapping

| Query (proposed) | Input reads | Output | Cutoff point |
|---|---|---|---|
| `parse(file)` | `SourceFile` | `PhaseOutput<ParsedFile>` | text equal |
| `lower(file)` | `parse` | `HirModule` (+ provenance maps as **separate** queries) | HIR value equal despite different provenance ⇒ dependents stop |
| `resolve(body)` | `lower`, `module_index` | `ResolvedBody` | per-body results equal |
| `typeck(body)` | `resolve(body)`, callee **signatures** | `TypedBody` | body-only change with equal signature/effects does not invalidate callers |
| `tir(fn)` | `typeck(fn)`, ownership facts | `TirFunction` | equal TIR text ⇒ TCap/MIR skip |
| `mir(fn)` | `tir(fn)` | `MirFunction` | equal MIR ⇒ codegen skip |
| `codegen(fn, target)` | `mir(fn)`, `TargetSpec` | object bytes | equal MIR |
| `diagnostics(file)` | phase outputs + **current** provenance maps | ordered `DiagnosticSet` | — |

Requirements this document places on #9: every output above is a pure value with `Eq + Hash`; provenance maps are separate queries so span-only changes stop at the HIR boundary; no query performs I/O; evidence/model inputs are separate Salsa inputs that no compiler query reads.

## 9. Open questions (not decided here)

- **O1 — integer overflow semantics.** `a+b` on `i64` has no specified overflow behavior yet. B4 marks it `UNSPECIFIED` and B6 requires it explicit. Decide in the semantics/spec before #22 lowers arithmetic. This is a *language* decision, not a phase-contract one.
- **O2 — where non-lexical borrow shortening runs.** If it needs a CFG, ownership analysis for that step moves from B4 to B6; the `OwnershipFacts` side-table design keeps both options open (§5).
- **O3 — is TIR complete enough to lower loops/branches without re-reading B4?** The decision to lower MIR from TIR alone (TIR-5) is what guarantees "what you read is what runs". If TIR cannot express control flow without becoming a CFG itself, the reversal is: MIR lowers from B4 and TIR becomes a *verified projection*. Decide with evidence when #22 hits the first `if`/loop (requires grammar work in #2).
- **O4 — CST storage** (rowan-style green/red vs. custom vs. arena) is #18's benchmark decision; the CST contract above holds for any of them.
- **O5 — crate split.** Whether B2 and B3/B4 live in one or two crates is #19/#20's call as long as `LAYERS` stays acyclic and B1 never depends on them.

## 10. Prior art consulted

Repository docs/issues cite these; this contract takes from them only what is stated:

- rustc separates AST/HIR/THIR/MIR and uses MIR as a simplified CFG for flow-sensitive analysis and codegen — motivates B2/B4/B6 and "MIR is boring". <https://rustc-dev-guide.rust-lang.org/overview.html>
- rust-analyzer keeps the lossless syntax tree semantically empty, separates parser events from tree storage, and requires the parser to return a tree plus errors — motivates B1 and the tolerance rule. <https://rust-analyzer.github.io/book/contributing/syntax.html>
- Salsa's early-cutoff on equal query values — motivates identity/provenance separation (§2.4).
- Mun separates HIR, driver, diagnostics and codegen crates — motivates the crate layout in `LAYERS`. <https://github.com/mun-lang/mun/tree/main/crates>

Tessera-specific departures: TIR is a public explanatory artifact *between* typed HIR and MIR (rustc has no equivalent), and MIR lowers from it alone; TCG/TMT are explicitly not compiler phases.
