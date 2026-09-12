//! JSON, written by hand and iteratively.
//!
//! The library carries no serde, and the shapes a Markdoc consumer expects
//! cannot come from a derive anyway: a tag is an object with `$$mdtype`, an
//! annotation is `{ type, name, value }`, a location's `file` is present only
//! when set, and a node's `tag` is absent rather than `null` when it has
//! none. So the encoders are written out, in the order upstream's classes
//! declare their fields, and they produce what `JSON.stringify` produces
//! over upstream's objects: numbers in ECMAScript's spelling, through the
//! library's own coercion, and strings with JSON's escapes.
//!
//! Positions are the WebAssembly bindings' shape exactly -- `line`,
//! `character` and `offset` in UTF-16 code units, `byteOffset` in the
//! library's bytes -- computed from the source the way the bindings compute
//! them, so a consumer written against either host reads the other.
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

use accent_proust::ast::{Location, Node, PathSegment, Position, ValidationError, Value};
use accent_proust::grammar::Attribute;
use accent_proust::render::attribute_value;
use accent_proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};
use accent_proust::validate::ValidateError;

/// Byte offsets into a source, as UTF-16 code-unit offsets.
///
/// Built once per document: one entry per byte, so a position converts by
/// one lookup rather than by counting from the start of the source for every
/// node in it.
pub struct Utf16Index {
    at: Vec<usize>,
}

impl Utf16Index {
    /// Index `source`.
    #[must_use]
    pub fn new(source: &str) -> Utf16Index {
        let mut at = Vec::with_capacity(source.len() + 1);
        let mut units = 0;
        for ch in source.chars() {
            for _ in 0..ch.len_utf8() {
                at.push(units);
            }
            units += ch.len_utf16();
        }
        at.push(units);
        Utf16Index { at }
    }

    /// The code-unit offset of a byte offset. Past the end clamps to the end.
    fn at(&self, byte: usize) -> usize {
        self.at.get(byte).or(self.at.last()).copied().unwrap_or(0)
    }
}

/// One unit of writing.
enum Step<'a> {
    /// Literal text: punctuation, a key that is a constant.
    Text(&'static str),
    /// A string, escaped.
    Str(&'a str),
    /// A number, in ECMAScript's spelling, or `null` when it is not finite.
    Num(f64),
    /// A count: a line number.
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

/// Write a syntax tree parsed from `source`, whose positions are converted
/// through it.
pub fn node(out: &mut String, source: &str, root: &Node<'_>) {
    run(out, &Utf16Index::new(source), vec![Step::Node(root)]);
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
    // A renderable tree carries no locations, so nothing here consults the
    // index; it exists because the walker has one signature.
    run(out, &Utf16Index::new(""), steps);
}

/// Write one input's validation errors: `{"file": label, "errors": [...]}`.
pub fn diagnostics(out: &mut String, label: &str, source: &str, errors: &[ValidateError<'_>]) {
    let index = Utf16Index::new(source);
    out.push_str("{\"file\":");
    string(out, label);
    out.push_str(",\"errors\":[");
    for (position, found) in errors.iter().enumerate() {
        if position > 0 {
            out.push(',');
        }
        validate_error(out, &index, found);
    }
    out.push_str("]}");
}

/// Drain the worklist into `out`.
///
/// The worklist is a stack in pop order: what is pushed last is written
/// first, which is how every container schedules itself.
fn run(out: &mut String, index: &Utf16Index, mut steps: Vec<Step<'_>>) {
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
            Step::Location(spot) => location(out, index, spot),
            Step::Error(found) => validation_error(out, index, found),
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
        // Upstream's own shapes for the two values that are not data, in
        // their classes' field order.
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

/// Schedule a syntax-tree node, every field, in the order upstream's `Node`
/// class declares them (`ast/node.ts`): `$$mdtype`, `attributes`, `slots`,
/// `children`, `errors`, `lines`, `type`, `tag`, `annotations`, `inline`,
/// `location`. `tag` and `location` are optional there, and `JSON.stringify`
/// omits an undefined property, so both are omitted rather than `null`.
fn push_node<'a>(steps: &mut Vec<Step<'a>>, node: &'a Node<'a>) {
    steps.push(Step::Text("}"));

    if let Some(spot) = &node.location {
        steps.push(Step::Location(spot));
        steps.push(Step::Text(",\"location\":"));
    }

    steps.push(Step::Bool(node.inline));
    steps.push(Step::Text(",\"inline\":"));

    push_seq(
        steps,
        "[",
        "]",
        node.annotations.iter().map(annotation).collect(),
    );
    steps.push(Step::Text(",\"annotations\":"));

    if let Some(tag) = &node.tag {
        steps.push(Step::Str(tag));
        steps.push(Step::Text(",\"tag\":"));
    }

    steps.push(Step::Str(node.node_type.as_str()));
    steps.push(Step::Text(",\"type\":"));

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
        node.slots
            .iter()
            .map(|(key, slot)| vec![Step::Str(key), Step::Text(":"), Step::Node(slot)])
            .collect(),
    );
    steps.push(Step::Text(",\"slots\":"));

    push_seq(
        steps,
        "{",
        "}",
        node.attributes
            .iter()
            .map(|(key, value)| vec![Step::Str(key), Step::Text(":"), Step::Value(value)])
            .collect(),
    );
    steps.push(Step::Text("{\"$$mdtype\":\"Node\",\"attributes\":"));
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
fn validate_error(out: &mut String, index: &Utf16Index, found: &ValidateError<'_>) {
    out.push_str("{\"type\":");
    string(out, found.node_type.as_str());
    out.push_str(",\"lines\":[");
    for (position, line) in found.lines.iter().enumerate() {
        if position > 0 {
            out.push(',');
        }
        let _ = write!(out, "{line}");
    }
    out.push(']');
    if let Some(spot) = &found.location {
        out.push_str(",\"location\":");
        location(out, index, spot);
    }
    out.push_str(",\"error\":");
    validation_error(out, index, &found.error);
    out.push('}');
}

/// The inner error: `id`, `level`, `message`, and `location` when known.
fn validation_error(out: &mut String, index: &Utf16Index, found: &ValidationError<'_>) {
    out.push_str("{\"id\":");
    string(out, found.id);
    out.push_str(",\"level\":");
    string(out, found.level.as_str());
    out.push_str(",\"message\":");
    string(out, &found.message);
    if let Some(spot) = &found.location {
        out.push_str(",\"location\":");
        location(out, index, spot);
    }
    out.push('}');
}

/// A location: `file` only when the caller set one, then `start` and `end`.
fn location(out: &mut String, index: &Utf16Index, spot: &Location<'_>) {
    out.push('{');
    if let Some(file) = spot.file {
        out.push_str("\"file\":");
        string(out, file);
        out.push(',');
    }
    out.push_str("\"start\":");
    position(out, index, &spot.start);
    out.push_str(",\"end\":");
    position(out, index, &spot.end);
    out.push('}');
}

/// One edge of a location, as the bindings write it: `line` zero-based,
/// `character` and `offset` in UTF-16 code units, `byteOffset` in bytes.
///
/// `column` is a byte count from the start of the line, so the line's own
/// start is converted too: the difference of two absolute code-unit offsets
/// is the column in code units, and subtracting the byte column from the byte
/// offset is how the line start is found.
fn position(out: &mut String, index: &Utf16Index, edge: &Position) {
    let offset = index.at(edge.offset);
    let line_start = index.at(edge.offset.saturating_sub(edge.column));
    let _ = write!(
        out,
        "{{\"line\":{},\"character\":{},\"offset\":{offset},\"byteOffset\":{}}}",
        edge.line,
        offset.saturating_sub(line_start),
        edge.offset
    );
}

/// A JSON string with `JSON.stringify`'s escapes: the two characters JSON
/// requires, the five controls it names, `\u00xx` for the rest, and nothing
/// else -- UTF-8 is JSON as it stands.
fn string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
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

/// A JSON number, spelled as ECMAScript spells it: `1e21` is `1e+21`, `-0`
/// is `0`, and a value JSON cannot hold -- an infinity, `NaN` -- is `null`,
/// as `JSON.stringify` has it. The spelling is the library's own coercion,
/// so this and upstream agree on every value.
fn num(out: &mut String, number: f64) {
    if number.is_finite() {
        let value = RenderableTreeNodes::One(RenderableTreeNode::Scalar(Scalar::Number(number)));
        out.push_str(&attribute_value(&value));
    } else {
        out.push_str("null");
    }
}
