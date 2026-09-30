//! Minimal deterministic JSON emitter.
//!
//! Results must be byte-identical across runs and machines, so output is not
//! delegated to a general serializer: objects are `BTreeMap`s (sorted keys by
//! construction, regardless of any feature another crate turns on), integers
//! print as integers, and floats print with a fixed four decimals.
//! Reading (for `check`) uses `serde_json`; only writing needs this.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    /// Build an object from `(key, value)` pairs (later duplicates win).
    pub fn obj<K: Into<String>, I: IntoIterator<Item = (K, Json)>>(pairs: I) -> Self {
        Self::Obj(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn str(s: impl Into<String>) -> Self {
        Self::Str(s.into())
    }

    pub fn int(n: usize) -> Self {
        Self::Int(i64::try_from(n).unwrap_or(i64::MAX))
    }

    pub fn opt_str(s: Option<&str>) -> Self {
        s.map_or(Self::Null, Self::str)
    }

    pub fn opt_int(n: Option<usize>) -> Self {
        n.map_or(Self::Null, Self::int)
    }

    pub fn strs<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> Self {
        Self::Arr(items.into_iter().map(|s| Self::str(s.as_ref())).collect())
    }

    /// Pretty-print with two-space indentation and a trailing newline.
    /// Arrays and objects made only of scalars stay on one line to keep files
    /// and diffs small (keys are still sorted).
    #[must_use]
    pub fn to_pretty_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn is_scalar(&self) -> bool {
        !matches!(self, Self::Arr(_) | Self::Obj(_))
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Self::Float(x) => out.push_str(&format_float(*x)),
            Self::Str(s) => write_string(out, s),
            Self::Arr(items) if items.is_empty() => out.push_str("[]"),
            Self::Arr(items) if items.iter().all(Self::is_scalar) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out, indent);
                }
                out.push(']');
            }
            Self::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write(out, indent + 1);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                push_indent(out, indent);
                out.push(']');
            }
            Self::Obj(map) if map.is_empty() => out.push_str("{}"),
            Self::Obj(map) if map.values().all(Self::is_scalar) => {
                out.push('{');
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent);
                }
                out.push('}');
            }
            Self::Obj(map) => {
                out.push_str("{\n");
                for (i, (key, value)) in map.iter().enumerate() {
                    push_indent(out, indent + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
                }
                push_indent(out, indent);
                out.push('}');
            }
        }
    }
}

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Fixed four decimals, trailing zeros trimmed but at least one decimal kept
/// (`12.0`, `12.5`, `0.1667`). Non-finite values become `null` upstream; here
/// they print as `0.0` so the emitter can never produce invalid JSON.
#[must_use]
pub fn format_float(x: f64) -> String {
    if !x.is_finite() {
        return "0.0".to_owned();
    }
    let mut s = format!("{x:.4}");
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    if s == "-0.0" {
        s = "0.0".to_owned();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_are_fixed_and_trimmed() {
        assert_eq!(format_float(12.0), "12.0");
        assert_eq!(format_float(12.5), "12.5");
        assert_eq!(format_float(1.0 / 6.0), "0.1667");
        assert_eq!(format_float(-0.00001), "0.0");
        assert_eq!(format_float(f64::NAN), "0.0");
    }

    #[test]
    fn keys_are_sorted_and_output_is_stable() {
        let v = Json::obj([
            ("b", Json::int(2)),
            ("a", Json::Arr(vec![Json::int(1), Json::Null])),
            (
                "c",
                Json::obj([("z", Json::Float(0.5)), ("y", Json::str("q\"\n"))]),
            ),
        ]);
        let text = v.to_pretty_string();
        assert_eq!(
            text,
            "{\n  \"a\": [1, null],\n  \"b\": 2,\n  \"c\": {\"y\": \"q\\\"\\n\", \"z\": 0.5}\n}\n"
        );
        assert_eq!(text, v.clone().to_pretty_string());
        let reparsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(reparsed["c"]["z"], 0.5);
    }
}
