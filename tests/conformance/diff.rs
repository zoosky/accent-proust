//! Saying what differs, in a way that points at the fix.
//!
//! Upstream prints a coloured character diff of two JSON blobs. That reads well
//! for a one-character difference and badly for a structural one, which is the
//! kind a port produces: a missing attribute in the third child of the second
//! node is a wall of green and red either way round.
//!
//! So this reports paths instead. `[0].children[1].attributes.id` names the
//! place, and the two values are printed next to it. The path is the thing an
//! implementer needs; the surrounding tree is what they already have on screen.

use crate::value::Value;

/// How many differences to print before summarising the rest.
const LIMIT: usize = 8;

/// Describe how `actual` differs from `expected`, most-specific path first.
///
/// An empty result means the two are equal.
pub fn describe(expected: &Value, actual: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk("", expected, actual, &mut out);
    if out.len() > LIMIT {
        let extra = out.len() - LIMIT;
        out.truncate(LIMIT);
        out.push(format!("... and {extra} more"));
    }
    out
}

/// Describe how two strings differ, for the HTML and validation-message grades.
///
/// Both are compared trimmed, as upstream does.
pub fn describe_text(expected: &str, actual: &str) -> Vec<String> {
    let (expected, actual) = (expected.trim(), actual.trim());
    if expected == actual {
        return Vec::new();
    }
    let common = expected
        .char_indices()
        .zip(actual.chars())
        .take_while(|((_, e), a)| e == a)
        .count();
    vec![
        format!("first difference at character {common}"),
        format!("expected: {}", elide(expected)),
        format!("actual:   {}", elide(actual)),
    ]
}

fn walk(path: &str, expected: &Value, actual: &Value, out: &mut Vec<String>) {
    let here = if path.is_empty() { "(root)" } else { path };
    match (expected, actual) {
        (Value::Seq(expected_items), Value::Seq(actual_items)) => {
            if expected_items.len() != actual_items.len() {
                out.push(format!(
                    "{here}: expected {} items, got {}",
                    expected_items.len(),
                    actual_items.len()
                ));
            }
            for (index, (expected_item, actual_item)) in
                expected_items.iter().zip(actual_items).enumerate()
            {
                walk(&format!("{path}[{index}]"), expected_item, actual_item, out);
            }
        }
        (Value::Map(expected_entries), Value::Map(_)) => {
            for (key, expected_value) in expected_entries {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match actual.get(key) {
                    Some(actual_value) => walk(&child, expected_value, actual_value, out),
                    None => out.push(format!(
                        "{child}: missing, expected {}",
                        elide(&expected_value.to_json())
                    )),
                }
            }
            if let Value::Map(actual_entries) = actual {
                for (key, actual_value) in actual_entries {
                    if expected.get(key).is_none() {
                        let child = if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{path}.{key}")
                        };
                        out.push(format!(
                            "{child}: unexpected, got {}",
                            elide(&actual_value.to_json())
                        ));
                    }
                }
            }
        }
        _ => {
            if expected != actual {
                out.push(format!(
                    "{here}: expected {}, got {}",
                    elide(&expected.to_json()),
                    elide(&actual.to_json())
                ));
            }
        }
    }
}

/// Keep a value short enough that a list of differences stays a list.
fn elide(value: &str) -> String {
    const MAX: usize = 120;
    let mut out = String::new();
    for (count, ch) in value.chars().enumerate() {
        if count == MAX {
            out.push_str("...");
            break;
        }
        match ch {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}
