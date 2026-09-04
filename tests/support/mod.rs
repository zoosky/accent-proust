//! Shared helpers for the ported upstream test suites.
//!
//! Upstream's parser tests are written against Jest's `toDeepEqualSubset`,
//! which lets an assertion name a few fields of a deeply nested object and
//! ignore the rest. Rust has no such matcher, and building one would mean
//! writing a matcher library to port a test suite.
//!
//! So the assertions are ported, not the idiom. A tree is rendered to an
//! **outline** -- one indented line per node, carrying its type, its tag and its
//! attributes -- and compared against a literal. That is stricter than
//! `toDeepEqualSubset`, because nothing is ignored, and it reads closer to the
//! `expect` blocks it came from than a chain of index lookups would.
//!
//! Where upstream really does assert one field of one node, so does the port:
//! [`at`] walks a path of child indices and the test reads that node directly.

#![allow(dead_code, reason = "each ported suite uses a different subset")]

use std::fmt::Write as _;

use accent_proust::ast::{Node, Value};

/// Upstream's `convert` preamble: strip the indentation a template literal adds.
///
/// `example.replace(/\n\s+/gm, '\n').trim()` in `parser.test.ts`. Every test
/// there writes its document indented inside the source file and relies on this
/// to make it column zero, so a port that skipped it would be feeding the parser
/// documents upstream never parsed -- and indentation is exactly what
/// `DIVERGENCES.md` entries 8 and 11 are about.
#[must_use]
pub fn dedent(example: &str) -> String {
    let mut out = String::with_capacity(example.len());
    let mut at_line_start = false;
    for character in example.chars() {
        if character == '\n' {
            out.push('\n');
            at_line_start = true;
            continue;
        }
        if at_line_start && character.is_whitespace() {
            continue;
        }
        at_line_start = false;
        out.push(character);
    }
    out.trim().to_string()
}

/// One indented line per node: type, tag, attributes, errors.
#[must_use]
pub fn outline(node: &Node<'_>) -> String {
    let mut out = String::new();
    write_node(node, 0, &mut out);
    out
}

fn write_node(node: &Node<'_>, depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(node.node_type.as_str());
    if let Some(tag) = &node.tag {
        let _ = write!(out, "[{tag}]");
    }
    for (name, value) in &node.attributes {
        let _ = write!(out, " {name}={}", show(value));
    }
    for error in &node.errors {
        let _ = write!(out, " !{}", error.id);
    }
    out.push('\n');
    for (name, slot) in &node.slots {
        for _ in 0..=depth {
            out.push_str("  ");
        }
        let _ = writeln!(out, "slot {name}:");
        write_node(slot, depth + 2, out);
    }
    for child in &node.children {
        write_node(child, depth + 1, out);
    }
}

/// A value, in the shortest spelling that stays unambiguous.
#[must_use]
pub fn show(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Number(number) => {
            if number.fract() == 0.0 && number.is_finite() {
                format!("{number:.0}")
            } else {
                number.to_string()
            }
        }
        Value::String(text) => format!("{text:?}"),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(show).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Hash(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{key}: {}", show(value)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        Value::Variable(variable) => {
            let mut out = "$".to_string();
            for (index, segment) in variable.path.iter().enumerate() {
                match segment {
                    accent_proust::ast::PathSegment::Key(key) if index == 0 => out.push_str(key),
                    accent_proust::ast::PathSegment::Key(key) => {
                        let _ = write!(out, ".{key}");
                    }
                    accent_proust::ast::PathSegment::Index(number) => {
                        let _ = write!(out, "[{number}]");
                    }
                    other => {
                        let _ = write!(out, ".{other:?}");
                    }
                }
            }
            out
        }
        Value::Function(function) => {
            let parameters: Vec<String> = function
                .parameters
                .iter()
                .map(|(key, value)| format!("{key}={}", show(value)))
                .collect();
            format!("{}({})", function.name, parameters.join(", "))
        }
        // `Value` is `#[non_exhaustive]`, which has no effect inside the crate
        // but does here: this is a test helper in a separate crate.
        _ => format!("{value:?}"),
    }
}

/// The node at a path of child indices, for the assertions that name one node.
///
/// Panics with the path when it does not exist, which is the failure a test
/// wants: "there is no `children[0].children[1]`" says more than an index
/// panic.
#[must_use]
#[allow(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "the panic is this helper's whole purpose -- it reports a missing \
              node better than an index panic would -- and the slice is bounded \
              by the enumerate it came from"
)]
pub fn at<'n, 'a>(node: &'n Node<'a>, path: &[usize]) -> &'n Node<'a> {
    let mut current = node;
    for (depth, index) in path.iter().enumerate() {
        current = current.children.get(*index).unwrap_or_else(|| {
            panic!(
                "no node at {:?}: {:?} has {} children",
                &path[..=depth],
                current.node_type,
                current.children.len()
            )
        });
    }
    current
}

/// A node's attribute, rendered by [`show`], or `"<unset>"`.
#[must_use]
pub fn attribute(node: &Node<'_>, name: &str) -> String {
    node.get(name).map_or("<unset>".to_string(), show)
}

/// Every error id on a node, in order.
#[must_use]
pub fn error_ids(node: &Node<'_>) -> Vec<&'static str> {
    node.errors.iter().map(|error| error.id).collect()
}

/// Every error id anywhere in a tree, in walk order.
#[must_use]
pub fn all_error_ids(node: &Node<'_>) -> Vec<&'static str> {
    let mut out = error_ids(node);
    for child in node.walk() {
        out.extend(error_ids(child));
    }
    out
}
