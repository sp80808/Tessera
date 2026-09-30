//! `check`: the regression gate between a baseline and a fresh results file.
//!
//! Fails when
//! - the corpus hash differs (the corpus changed without the baseline being
//!   regenerated and reviewed);
//! - the measured tokenizer set or its provenance differs (counts would not be
//!   comparable);
//! - a `parsed` Tessera variant of the baseline is missing or no longer
//!   `parsed` in the results;
//! - a `parsed` variant's median or worst token count grew by more than the
//!   threshold percentage.
//!
//! Only `parsed` variants gate: `unparsed` candidate spellings are unvalidated
//! text and never fail CI.

use std::fs;
use std::path::Path;

use serde_json::Value;

pub const DEFAULT_MAX_REGRESSION_PCT: f64 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckReport {
    /// Number of `parsed` tessera variants compared.
    pub checked: usize,
    pub failures: Vec<String>,
}

impl CheckReport {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// # Errors
/// Unreadable or invalid JSON files, or a negative/non-finite threshold.
pub fn check_files(
    baseline: &Path,
    results: &Path,
    max_regression_pct: f64,
) -> Result<CheckReport, String> {
    let read = |path: &Path, what: &str| -> Result<Value, String> {
        let bytes =
            fs::read(path).map_err(|e| format!("cannot read {what} {}: {e}", path.display()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("{what} {} is not valid JSON: {e}", path.display()))
    };
    Ok(check(
        &read(baseline, "baseline")?,
        &read(results, "results")?,
        max_regression_pct,
    ))
}

fn measured_tokenizers(doc: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = doc["tokenizers"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|t| t["status"] == "measured")
        .map(|t| {
            (
                t["id"].as_str().unwrap_or("").to_owned(),
                t["provenance"].to_string(),
            )
        })
        .collect();
    out.sort();
    out
}

fn parsed_variants(doc: &Value) -> Vec<(String, String, &Value)> {
    let mut out = Vec::new();
    for program in doc["programs"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
    {
        for v in program["variants"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            if v["lang"] == "tessera" && v["status"] == "parsed" {
                out.push((
                    program["id"].as_str().unwrap_or("").to_owned(),
                    v["candidate"].as_str().unwrap_or("").to_owned(),
                    v,
                ));
            }
        }
    }
    out
}

fn find<'a>(doc: &'a Value, program: &str, candidate: &str) -> Option<&'a Value> {
    doc["programs"]
        .as_array()?
        .iter()
        .find(|p| p["id"] == program)?["variants"]
        .as_array()?
        .iter()
        .find(|v| v["lang"] == "tessera" && v["candidate"] == candidate)
}

/// Compare `results` against `baseline`; see the module docs for the rules.
#[must_use]
pub fn check(baseline: &Value, results: &Value, max_regression_pct: f64) -> CheckReport {
    let mut failures = Vec::new();
    if !max_regression_pct.is_finite() || max_regression_pct < 0.0 {
        failures.push(format!(
            "invalid threshold {max_regression_pct}: must be a non-negative number"
        ));
        return CheckReport {
            checked: 0,
            failures,
        };
    }
    if baseline["schema_version"] != results["schema_version"] {
        failures.push(format!(
            "results schema_version {} differs from baseline {}",
            results["schema_version"], baseline["schema_version"]
        ));
        return CheckReport {
            checked: 0,
            failures,
        };
    }
    if baseline["corpus_hash"] != results["corpus_hash"] {
        failures.push(format!(
            "corpus_hash changed (baseline {}, results {}): the corpus changed without the baseline being updated; regenerate and review the baseline",
            baseline["corpus_hash"], results["corpus_hash"]
        ));
        return CheckReport {
            checked: 0,
            failures,
        };
    }
    let (base_tok, new_tok) = (measured_tokenizers(baseline), measured_tokenizers(results));
    if base_tok != new_tok {
        let ids = |v: &[(String, String)]| {
            v.iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        failures.push(format!(
            "measured tokenizer set or provenance differs (baseline: [{}], results: [{}]); counts are not comparable, regenerate the baseline",
            ids(&base_tok),
            ids(&new_tok)
        ));
        return CheckReport {
            checked: 0,
            failures,
        };
    }
    let gated = parsed_variants(baseline);
    if gated.is_empty() {
        failures
            .push("baseline has no parsed tessera variants: the gate would be vacuous".to_owned());
    }
    for (program, candidate, base) in &gated {
        let name = format!("{program}/tessera-{candidate}");
        let Some(new) = find(results, program, candidate) else {
            failures.push(format!("{name}: missing from results"));
            continue;
        };
        if new["status"] != "parsed" {
            failures.push(format!(
                "{name}: no longer `parsed` in results (status {})",
                new["status"]
            ));
            continue;
        }
        for metric in ["median", "worst"] {
            let (Some(old), Some(cur)) = (
                base["stats"][metric].as_f64(),
                new["stats"][metric].as_f64(),
            ) else {
                failures.push(format!("{name}: stats.{metric} missing"));
                continue;
            };
            let grew = cur > old;
            let pct = if old > 0.0 {
                (cur - old) / old * 100.0
            } else if grew {
                f64::INFINITY
            } else {
                0.0
            };
            if grew && pct > max_regression_pct {
                failures.push(format!(
                    "{name}: {metric} regressed {old} -> {cur} ({pct:.1}% > {max_regression_pct}%)"
                ));
            }
        }
    }
    CheckReport {
        checked: gated.len(),
        failures,
    }
}
