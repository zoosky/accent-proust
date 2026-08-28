//! The value model the corpus and the engine meet in.
//!
//! Upstream grades a case by JSON-diffing the expected tree against the
//! renderable tree the pipeline produced (`spec/marktest/index.ts`, `run`). Two
//! different shapes therefore have to become comparable: YAML read out of
//! `tests.yaml`, and whatever `proust` returns. [`Value`] is that meeting
//! point -- deliberately its own type rather than the YAML crate's, so that the
//! choice of YAML reader stays confined to [`crate::corpus`] and the engine
//! seam never depends on it.

use std::fmt::Write as _;

/// A JSON-shaped value: what a corpus expectation and a rendered tree both
/// reduce to.
#[derive(Clone, Debug)]
pub enum Value {
    /// YAML `null`, JSON `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// An integer. Kept apart from [`Value::Float`] so that `1` prints as `1`.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// A string.
    Str(String),
    /// An array.
    Seq(Vec<Value>),
    /// An object, in authored order.
    ///
    /// Order is preserved for printing, not for comparison: it is what makes a
    /// diff read like the source it came from. Equality ignores it, because
    /// upstream's `diffJson` canonicalises key order and an attribute map that
    /// differs only in ordering is not a conformance failure.
    Map(Vec<(String, Value)>),
}

impl Value {
    /// Look up a key, or [`None`] if this is not a map or has no such key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The name of this variant, for error messages that need to say what a
    /// value was when they expected something else.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::Float(_) => "number",
            Value::Str(_) => "string",
            Value::Seq(_) => "sequence",
            Value::Map(_) => "mapping",
        }
    }

    /// Render as compact JSON, in authored key order.
    ///
    /// Used by the diff, so it is a readable rendering rather than a strictly
    /// conformant JSON encoder: control characters other than the ones below
    /// are passed through, because corpus values are documentation text and
    /// escaping them further makes a diff harder to read, not safer.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            Value::Int(v) => {
                // `write!` to a String is infallible; the result is discarded
                // rather than unwrapped so no formatting path can panic.
                let _ = write!(out, "{v}");
            }
            Value::Float(v) => {
                let _ = write!(out, "{v}");
            }
            Value::Str(v) => write_json_string(v, out),
            Value::Seq(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_json(out);
                }
                out.push(']');
            }
            Value::Map(entries) => {
                out.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(key, out);
                    out.push(':');
                    value.write_json(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('"');
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Seq(a), Value::Seq(b)) => a == b,
            // A YAML `1` and a rendered `1.0` are the same number. The corpus
            // writes integers and a numeric attribute may arrive either way, so
            // comparing across the two variants is the honest reading of
            // upstream's JSON diff rather than a leniency.
            (Value::Int(a), Value::Int(b)) => a == b,
            #[allow(
                clippy::cast_precision_loss,
                reason = "comparing an i64 against an f64 is the point; \
                          a value large enough to lose precision here is not one \
                          the corpus contains"
            )]
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => *a as f64 == *b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(key, value)| {
                        b.iter()
                            .find(|(other_key, _)| other_key == key)
                            .is_some_and(|(_, found)| found == value)
                    })
            }
            _ => false,
        }
    }
}
