//! JSON, written by hand and iteratively.
//!
//! The library carries no serde, and the shapes a Markdoc consumer expects
//! cannot come from a derive anyway: a tag is an object with `$$mdtype`, an
//! annotation is `{ type, name, value }`, a location's `file` is present only
//! when set. So the encoders are written out, in the field order upstream
//! declares, and match what the WebAssembly bindings return for the same
//! tree -- columns and offsets in bytes rather than UTF-16 units, because a
//! terminal is not JavaScript.
//!
//! # Why the walks are iterative
//!
//! A tree comes from a document, and a document's nesting is its author's.
//! The library promises panic-freedom for every traversal of one, and a
//! stack overflow aborts rather than panics, so the walks below are worklists
//! for the same reason the library's own `Drop`, `Clone` and `Debug` are.
//! Output is sequential, so a container schedules its closing bracket first,
//! its contents in reverse with commas between, and its opening bracket last,
//! and everything pops in writing order.

use std::fmt::Write as _;

use accent_proust::ast::{Location, Node, PathSegment, ValidationError, Value};
use accent_proust::grammar::Attribute;
use accent_proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};
use accent_proust::validate::ValidateError;

/// One unit of writing.
enum Step<'a> {
    /// Literal text: punctuation, a key that is a constant.
    Text(&'static str),
    /// A string, escaped.
    Str(&'a str),
    /// A number, or `null` when it is not finite.
    Num(f64),
    /// A count: a line number, a byte offset.
    Count(usize),
    /// `true` or `false`.
    Bool(bool),
    /// A value from a syntax tree.
    Value(&'a Value),
    /// A scalar from a renderable tree.
    Scalar(&'a Scalar),
    /// A syntax-tree node.
    Node(&'a Node<'a>),
    /// A renderable-tree node.
    Renderable(&'a RenderableTreeNode),
    /// An attribute's subtree in a renderable tree: one node or a list.
    Renderables(&'a RenderableTreeNodes),
    /// A location, flat, written in one go.
    Location(&'a Location<'a>),
    /// A validation error, flat but for its location, written in one go.
    Error(&'a ValidationError<'a>),
}

/// Write a syntax tree.
pub fn node(out: &mut String, root: &Node<'_>) {
    run(out, vec![Step::Node(root)]);
}

/// Write a renderable tree's nodes as one array, which is what the
/// WebAssembly `transform` returns for the same document.
pub fn renderable(out: &mut String, nodes: &[RenderableTreeNode]) {
    let mut steps = Vec::new();
    push_seq(
        &mut steps,
        "[",
        "]",
        nodes
            .iter()
            .map(|node| vec![Step::Renderable(node)])
            .collect(),
    );
    run(out, steps);
}

/// Write one input's validation errors: `{"file": label, "errors": [...]}`.
pub fn diagnostics(out: &mut String, label: &str, errors: &[ValidateError<'_>]) {
    out.push_str("{\"file\":");
    string(out, label);
    out.push_str(",\"errors\":[");
    for (index, found) in errors.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        validate_error(out, found);
    }
    out.push_str("]}");
}

/// Drain the worklist into `out`.
///
/// The worklist is a stack in pop order: what is pushed last is written
/// first, which is how every container schedules itself.
fn run(out: &mut String, mut steps: Vec<Step<'_>>) {
    while let Some(step) = steps.pop() {
        match step {
            Step::Text(text) => out.push_str(text),
            Step::Str(text) => string(out, text),
            Step::Num(number) => num(out, number),
            Step::Count(count) => {
                let _ = write!(out, "{count}");
            }
            Step::Bool(flag) => out.push_str(if flag { "true" } else { "false" }),
            Step::Value(value) => push_value(&mut steps, value),
            Step::Scalar(scalar) => push_scalar(&mut steps, scalar),
            Step::Node(node) => push_node(&mut steps, node),
            Step::Renderable(node) => push_renderable(&mut steps, node),
            Step::Renderables(nodes) => match nodes {
                RenderableTreeNodes::One(node) => steps.push(Step::Renderable(node)),
                RenderableTreeNodes::Many(list) => push_seq(
                    &mut steps,
                    "[",
                    "]",
                    list.iter()
                        .map(|node| vec![Step::Renderable(node)])
                        .collect(),
                ),
                _ => steps.push(Step::Text("null")),
            },
            Step::Location(spot) => location(out, spot),
            Step::Error(found) => validation_error(out, found),
        }
    }
}

/// Schedule `open`, then `items` with commas between, then `close`.
///
/// `items` is in writing order and each item is in writing order; both are
/// pushed reversed, so they pop as written.
fn push_seq<'a>(
    steps: &mut Vec<Step<'a>>,
    open: &'static str,
    close: &'static str,
    items: Vec<Vec<Step<'a>>>,
) {
    steps.push(Step::Text(close));
    for (index, item) in items.into_iter().enumerate().rev() {
        for step in item.into_iter().rev() {
            steps.push(step);
        }
        // Pushed under the item's steps, so it pops before them: the comma
        // that separates this item from the one written before it.
        if index > 0 {
            steps.push(Step::Text(","));
        }
    }
    steps.push(Step::Text(open));
}

/// Schedule a syntax-tree value.
#[allow(
    clippy::match_same_arms,
    reason = "`Null` and the wildcard both write `null` and say different things: one is the document's null, the other is a variant added to a `#[non_exhaustive]` enum that this crate has not been taught"
)]
fn push_value<'a>(steps: &mut Vec<Step<'a>>, value: &'a Value) {
    match value {
        Value::Null => steps.push(Step::Text("null")),
        Value::Boolean(flag) => steps.push(Step::Bool(*flag)),
        Value::Number(number) => steps.push(Step::Num(*number)),
        Value::String(text) => steps.push(Step::Str(text)),
        Value::Array(items) => push_seq(
            steps,
            "[",
            "]",
            items.iter().map(|item| vec![Step::Value(item)]).collect(),
        ),
        Value::Hash(map) => push_seq(
            steps,
            "{",
            "}",
            map.iter()
                .map(|(key, item)| vec![Step::Str(key), Step::Text(":"), Step::Value(item)])
                .collect(),
        ),
        // Upstream's own shapes for the two values that are not data.
        Value::Function(function) => {
            steps.push(Step::Text("}"));
            push_seq(
                steps,
                "{",
                "}",
                function
                    .parameters
                    .iter()
                    .map(|(key, item)| vec![Step::Str(key), Step::Text(":"), Step::Value(item)])
                    .collect(),
            );
            steps.push(Step::Text(",\"parameters\":"));
            steps.push(Step::Str(&function.name));
            steps.push(Step::Text("{\"$$mdtype\":\"Function\",\"name\":"));
        }
        Value::Variable(variable) => {
            steps.push(Step::Text("}"));
            push_seq(
                steps,
                "[",
                "]",
                variable
                    .path
                    .iter()
                    .map(|segment| match segment {
                        PathSegment::Key(key) => vec![Step::Str(key)],
                        PathSegment::Index(index) => vec![Step::Num(*index)],
                        _ => vec![Step::Text("null")],
                    })
                    .collect(),
            );
            steps.push(Step::Text("{\"$$mdtype\":\"Variable\",\"path\":"));
        }
        // A variant this crate has not been taught renders as `null`, which
        // is what upstream's renderers do with a value they cannot place.
        _ => steps.push(Step::Text("null")),
    }
}

/// Schedule a renderable-tree scalar.
#[allow(
    clippy::match_same_arms,
    reason = "as for `push_value`: the document's null and an untaught variant both write `null`"
)]
fn push_scalar<'a>(steps: &mut Vec<Step<'a>>, scalar: &'a Scalar) {
    match scalar {
        Scalar::Null => steps.push(Step::Text("null")),
        Scalar::Boolean(flag) => steps.push(Step::Bool(*flag)),
        Scalar::Number(number) => steps.push(Step::Num(*number)),
        Scalar::String(text) => steps.push(Step::Str(text)),
        Scalar::Array(items) => push_seq(
            steps,
            "[",
            "]",
            items.iter().map(|item| vec![Step::Scalar(item)]).collect(),
        ),
        Scalar::Object(map) => push_seq(
            steps,
            "{",
            "}",
            map.iter()
                .map(|(key, item)| vec![Step::Str(key), Step::Text(":"), Step::Scalar(item)])
                .collect(),
        ),
        _ => steps.push(Step::Text("null")),
    }
}

/// Schedule a renderable-tree node: a tag in upstream's field order, or a
/// scalar as itself.
fn push_renderable<'a>(steps: &mut Vec<Step<'a>>, node: &'a RenderableTreeNode) {
    match node {
        RenderableTreeNode::Scalar(scalar) => steps.push(Step::Scalar(scalar)),
        RenderableTreeNode::Tag(tag) => push_tag(steps, tag),
        _ => steps.push(Step::Text("null")),
    }
}

/// `{"$$mdtype":"Tag","name":..,"attributes":{..},"children":[..]}`, which is
/// upstream's declaration order in `tag.ts`, so that this and `JSON.stringify`
/// over upstream's object produce the same bytes.
fn push_tag<'a>(steps: &mut Vec<Step<'a>>, tag: &'a Tag) {
    steps.push(Step::Text("}"));
    push_seq(
        steps,
        "[",
        "]",
        tag.children
            .iter()
            .map(|child| vec![Step::Renderable(child)])
            .collect(),
    );
    steps.push(Step::Text(",\"children\":"));
    push_seq(
        steps,
        "{",
        "}",
        tag.attributes
            .iter()
            .map(|(key, value)| vec![Step::Str(key), Step::Text(":"), Step::Renderables(value)])
            .collect(),
    );
    steps.push(Step::Text(",\"attributes\":"));
    steps.push(Step::Str(&tag.name));
    steps.push(Step::Text("{\"$$mdtype\":\"Tag\",\"name\":"));
}

/// Schedule a syntax-tree node, every field, in upstream's order.
fn push_node<'a>(steps: &mut Vec<Step<'a>>, node: &'a Node<'a>) {
    steps.push(Step::Text("}"));

    push_seq(
        steps,
        "[",
        "]",
        node.errors
            .iter()
            .map(|found| vec![Step::Error(found)])
            .collect(),
    );
    steps.push(Step::Text(",\"errors\":"));

    push_seq(
        steps,
        "[",
        "]",
        node.annotations.iter().map(annotation).collect(),
    );
    steps.push(Step::Text(",\"annotations\":"));

    steps.push(Step::Bool(node.inline));
    steps.push(Step::Text(",\"inline\":"));

    match &node.location {
        Some(spot) => steps.push(Step::Location(spot)),
        None => steps.push(Step::Text("null")),
    }
    steps.push(Step::Text(",\"location\":"));

    push_seq(
        steps,
        "[",
        "]",
        node.lines
            .iter()
            .map(|line| vec![Step::Count(*line)])
            .collect(),
    );
    steps.push(Step::Text(",\"lines\":"));

    push_seq(
        steps,
        "{",
        "}",
        node.slots
            .iter()
            .map(|(key, slot)| vec![Step::Str(key), Step::Text(":"), Step::Node(slot)])
            .collect(),
    );
    steps.push(Step::Text(",\"slots\":"));

    push_seq(
        steps,
        "[",
        "]",
        node.children
            .iter()
            .map(|child| vec![Step::Node(child)])
            .collect(),
    );
    steps.push(Step::Text(",\"children\":"));

    push_seq(
        steps,
        "{",
        "}",
        node.attributes
            .iter()
            .map(|(key, value)| vec![Step::Str(key), Step::Text(":"), Step::Value(value)])
            .collect(),
    );
    steps.push(Step::Text(",\"attributes\":"));

    match &node.tag {
        Some(tag) => steps.push(Step::Str(tag)),
        None => steps.push(Step::Text("null")),
    }
    steps.push(Step::Text(",\"tag\":"));

    steps.push(Step::Str(node.node_type.as_str()));
    steps.push(Step::Text("{\"$$mdtype\":\"Node\",\"type\":"));
}

/// An annotation in upstream's shape: `{ type, name, value }`, with the
/// `.class` shortcut carrying `value: true` as upstream does.
fn annotation(attribute: &Attribute) -> Vec<Step<'_>> {
    match attribute {
        Attribute::Attribute { name, value } => vec![
            Step::Text("{\"type\":\"attribute\",\"name\":"),
            Step::Str(name),
            Step::Text(",\"value\":"),
            Step::Value(value),
            Step::Text("}"),
        ],
        Attribute::Class { name } => vec![
            Step::Text("{\"type\":\"class\",\"name\":"),
            Step::Str(name),
            Step::Text(",\"value\":true}"),
        ],
        _ => vec![Step::Text("null")],
    }
}

/// A validate error in the bindings' shape: `type`, `lines`, `location` when
/// known, and the nested `error`.
fn validate_error(out: &mut String, found: &ValidateError<'_>) {
    out.push_str("{\"type\":");
    string(out, found.node_type.as_str());
    out.push_str(",\"lines\":[");
    for (index, line) in found.lines.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{line}");
    }
    out.push(']');
    if let Some(spot) = &found.location {
        out.push_str(",\"location\":");
        location(out, spot);
    }
    out.push_str(",\"error\":");
    validation_error(out, &found.error);
    out.push('}');
}

/// The inner error: `id`, `level`, `message`, and `location` when known.
fn validation_error(out: &mut String, found: &ValidationError<'_>) {
    out.push_str("{\"id\":");
    string(out, found.id);
    out.push_str(",\"level\":");
    string(out, found.level.as_str());
    out.push_str(",\"message\":");
    string(out, &found.message);
    if let Some(spot) = &found.location {
        out.push_str(",\"location\":");
        location(out, spot);
    }
    out.push('}');
}

/// A location: `file` only when the caller set one, then `start` and `end`
/// as `line`, `column` and `offset`, all in bytes and zero-based as the
/// library counts them.
fn location(out: &mut String, spot: &Location<'_>) {
    out.push('{');
    if let Some(file) = spot.file {
        out.push_str("\"file\":");
        string(out, file);
        out.push(',');
    }
    let _ = write!(
        out,
        "\"start\":{{\"line\":{},\"column\":{},\"offset\":{}}},\"end\":{{\"line\":{},\"column\":{},\"offset\":{}}}}}",
        spot.start.line,
        spot.start.column,
        spot.start.offset,
        spot.end.line,
        spot.end.column,
        spot.end.offset
    );
}

/// A JSON string: the two characters JSON requires escaped, the controls it
/// forbids, and nothing else -- UTF-8 is JSON as it stands.
fn string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// A JSON number. Rust's `Display` for `f64` is the shortest round-trip
/// decimal with no exponent, which is JSON; a value JSON cannot spell -- an
/// infinity, `NaN` -- is `null`, as `JSON.stringify` has it.
fn num(out: &mut String, number: f64) {
    if number.is_finite() {
        let _ = write!(out, "{number}");
    } else {
        out.push_str("null");
    }
}
