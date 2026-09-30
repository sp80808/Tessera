//! Shared invariant checks and input generators for the `tessera-syntax`
//! fuzz targets (issue #18, gap G9).
//!
//! The `fuzz_targets/*.rs` binaries are one-liners that decode libFuzzer's
//! bytes and call into this crate, so the same checks also run under plain
//! `cargo test` (`tests/replay.rs` replays `seeds/` and `regressions/`).
//!
//! Every check panics with `INVARIANT <ID> VIOLATED: ...` on failure; libFuzzer
//! turns that into a crash artifact. IDs are stable and documented in
//! `README.md`.

/// Assert an invariant; the failure message carries a stable ID.
macro_rules! invariant {
    ($id:literal, $cond:expr, $($detail:tt)+) => {
        if !$cond {
            $crate::fail($id, &format!($($detail)+));
        }
    };
}

pub mod cst_inv;
pub mod frontend_inv;
pub mod gen;
pub mod lexer_inv;
pub mod stats;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Environment variable listing invariant IDs to waive (comma separated).
///
/// A development aid for continuing to fuzz past a finding that is already
/// reported and reproduced under `regressions/`; nothing is waived by
/// default, and CI-style runs must not set it.
pub const WAIVE_ENV: &str = "TESSERA_FUZZ_WAIVE";

fn waived(id: &str) -> bool {
    static WAIVED: OnceLock<Vec<String>> = OnceLock::new();
    WAIVED
        .get_or_init(|| {
            std::env::var(WAIVE_ENV)
                .map(|list| list.split(',').map(|s| s.trim().to_owned()).collect())
                .unwrap_or_default()
        })
        .iter()
        .any(|w| w == id)
}

/// Report a broken invariant: panic with its stable ID unless it is waived.
#[cold]
#[inline(never)]
pub fn fail(id: &str, detail: &str) {
    if !waived(id) {
        violation(id, detail);
    }
}

/// Fail with a stable invariant ID (not waivable).
#[cold]
#[inline(never)]
pub fn violation(id: &str, detail: &str) -> ! {
    panic!("INVARIANT {id} VIOLATED: {detail}")
}

/// Wall-clock budget for one input: compiler code must return promptly on any
/// input (hostile nesting included), never hang or go quadratic.
pub const PROMPT: Duration = Duration::from_secs(10);

/// Fail if handling `src` took longer than [`PROMPT`].
pub fn assert_prompt(what: &str, started: Instant, src: &str) {
    let took = started.elapsed();
    invariant!(
        "TIME-prompt",
        took <= PROMPT,
        "{what} took {took:?} (budget {PROMPT:?}) for {}",
        show(src)
    );
}

/// Bounded, readable rendering of an input for failure messages.
#[must_use]
pub fn show(src: &str) -> String {
    const HEAD: usize = 120;
    const TAIL: usize = 60;
    if src.chars().count() <= HEAD + TAIL {
        return format!("{src:?}");
    }
    let head: String = src.chars().take(HEAD).collect();
    let mut tail: Vec<char> = src.chars().rev().take(TAIL).collect();
    tail.reverse();
    let tail: String = tail.into_iter().collect();
    format!("{head:?} ... {tail:?} ({} bytes)", src.len())
}

/// Decode raw fuzz bytes the way the driver does: invalid UTF-8 is rejected
/// *before* the parser (compiler-phases.md, B0), so raw-byte targets simply
/// return for it. Only the lexer target also checks the lossy decoding.
#[must_use]
pub fn decode(data: &[u8]) -> Option<&str> {
    std::str::from_utf8(data).ok()
}
