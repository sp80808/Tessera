//! Tokenizer benchmark harness (issue #1).
//!
//! Measures how competing Tessera Compact (TC) syntax candidates tokenize
//! under real model tokenizers, next to equivalent Rust/C/Zig/Odin programs,
//! and reports cost, cross-tokenizer dispersion, grammar-boundary alignment
//! and rewrite stability. Dev tool, not a compiler phase: it depends only on
//! `tessera-phases` and `tessera-syntax`, never touches the network, and
//! produces byte-identical results for identical inputs.
//!
//! See `bench/README.md` for how to run it and `docs/research/tokenbench.md`
//! for the schema.

pub mod adapter;
pub mod analysis;
pub mod bench;
pub mod check;
pub mod cli;
pub mod corpus;
pub mod json;
pub mod metrics;
pub mod report_json;
pub mod report_md;
pub mod rewrite;
