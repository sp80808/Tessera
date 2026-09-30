//! Optional branch counters, compiled in only with `--features stats`.
//!
//! Purpose: show that a target really reaches its deep branches (for example
//! the TC -> TIR -> TC round trip), not merely that libFuzzer reports coverage.
//! Totals are printed once, to stderr, when the process exits.

macro_rules! counters {
    ($($name:ident),* $(,)?) => {
        /// A branch of a check that is worth counting.
        #[derive(Clone, Copy, Debug)]
        #[repr(usize)]
        pub enum Counter { $($name),* }

        #[cfg(feature = "stats")]
        const NAMES: &[&str] = &[$(stringify!($name)),*];
    };
}

counters! {
    // lexer
    LexRuns,
    LexUtf8Valid,
    LexUtf8Invalid,
    LexLossyChecked,
    LexNonEmpty,
    // parse_cst
    CstRuns,
    CstClean,
    CstWithErrors,
    CstEmptyProgram,
    CstNestingTooDeep,
    CstTrailingInput,
    CstMissingNode,
    // frontend (raw and structured)
    FeRuns,
    FeParseOk,
    FeParseErr,
    FeFmtIdempotenceChecked,
    FeExpandOk,
    FeExpandErrUnbound,
    FeRoundTripChecked,
    FeSpansChecked,
    FeRespaceChecked,
}

#[cfg(feature = "stats")]
mod imp {
    use super::{Counter, NAMES};
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTS: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];
    static INSTALL: Once = Once::new();

    unsafe extern "C" {
        fn atexit(callback: extern "C" fn()) -> core::ffi::c_int;
    }

    extern "C" fn report() {
        let get = |name: &str| {
            NAMES
                .iter()
                .position(|n| *n == name)
                .map_or(0, |i| COUNTS[i].load(Ordering::Relaxed))
        };
        eprintln!("[stats] branch counters (only non-zero shown):");
        for (i, name) in NAMES.iter().enumerate() {
            let n = COUNTS[i].load(Ordering::Relaxed);
            if n > 0 {
                eprintln!("[stats]   {name:<26} {n}");
            }
        }
        let pct = |part: u64, whole: u64| {
            if whole == 0 {
                0.0
            } else {
                part as f64 * 100.0 / whole as f64
            }
        };
        let fe = get("FeRuns");
        if fe > 0 {
            eprintln!(
                "[stats]   frontend: parse ok {:.1}% | expand ok {:.1}% | round trip checked {:.1}% of runs",
                pct(get("FeParseOk"), fe),
                pct(get("FeExpandOk"), fe),
                pct(get("FeRoundTripChecked"), fe),
            );
        }
    }

    pub fn hit(counter: Counter) {
        INSTALL.call_once(|| {
            // SAFETY: `report` is an `extern "C" fn()` with no captured state.
            unsafe {
                atexit(report);
            }
        });
        COUNTS[counter as usize].fetch_add(1, Ordering::Relaxed);
    }
}

/// Count one occurrence of `counter` (no-op unless built with `stats`).
#[cfg(feature = "stats")]
pub fn hit(counter: Counter) {
    imp::hit(counter);
}

/// Count one occurrence of `counter` (no-op unless built with `stats`).
#[cfg(not(feature = "stats"))]
#[inline(always)]
pub fn hit(_counter: Counter) {}
