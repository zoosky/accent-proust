//! The built-in schema for every node type the parser produces.
//!
//! Ported from upstream `src/schema.ts`, one exported constant per node type.
//! This is schema *content* -- what a `heading` renders as, which attributes a
//! `fence` has -- as distinct from the schema *shape*, which the rest of
//! [`crate::validate`] owns. The two are separated because a host replaces
//! content freely (`config.nodes_mut().insert(Fence, ...)`) and never replaces
//! shape.
//!
//! # Why these are built in at all, when nothing else is
//!
//! Markdoc's node schemas are not a default theme. They are the definition of
//! what a parsed document *means*: without them a `paragraph` renders as its
//! children and a `heading` renders as nothing, because the transform stage
//! only knows what a schema tells it. A host that wants different markup
//! overrides a key; a host that wants none starts from
//! [`Config::new`](crate::validate::Config::new), which is empty, rather than
//! from [`builtins::config`](crate::builtins::config).
//!
//! # A replacement is total, not a patch
//!
//! Registering a `fence` schema replaces the whole thing, transform hook
//! included. That is upstream's behaviour and the corpus depends on it: "Using a
//! backtick in a fenced code block string attribute" supplies a `fence` schema
//! with no `transform` and expects the generic path -- attributes, then children
//! -- rather than the built-in hook.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::ast::{NodeType, Value};
use crate::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};
use crate::transform;
use crate::validate::{RenderPolicy, Schema, SchemaAttribute, ValidationType};

/// Every built-in node schema, keyed by node type.
///
/// Upstream builds this map with `import * as nodes from './schema'` and spreads
/// it under the caller's; here it is a function, so the caller's copy is its
/// own.
#[must_use]
pub fn builtin() -> IndexMap<NodeType, Schema> {
    let mut nodes = IndexMap::new();
    nodes.insert(NodeType::Document, document());
    nodes.insert(NodeType::Heading, heading());
    nodes.insert(NodeType::Paragraph, element("p", &[NodeType::Inline]));
    nodes.insert(NodeType::Image, image());
    nodes.insert(NodeType::Fence, fence());
    nodes.insert(NodeType::Blockquote, element("blockquote", NESTED_BLOCKS));
    nodes.insert(NodeType::Item, element("li", CELL_CHILDREN));
    nodes.insert(NodeType::List, list());
    nodes.insert(NodeType::Hr, Schema::new().render("hr"));
    nodes.insert(NodeType::Table, Schema::new().render("table"));
    nodes.insert(NodeType::Td, td());
    nodes.insert(NodeType::Th, th());
    nodes.insert(NodeType::Tr, element("tr", &[NodeType::Th, NodeType::Td]));
    nodes.insert(
        NodeType::Tbody,
        element("tbody", &[NodeType::Tr, NodeType::Tag]),
    );
    nodes.insert(NodeType::Thead, element("thead", &[NodeType::Tr]));
    nodes.insert(
        NodeType::Strong,
        marked(
            "strong",
            &[
                NodeType::Em,
                NodeType::S,
                NodeType::Link,
                NodeType::Code,
                NodeType::Text,
                NodeType::Tag,
            ],
        ),
    );
    nodes.insert(
        NodeType::Em,
        marked(
            "em",
            &[
                NodeType::Strong,
                NodeType::S,
                NodeType::Link,
                NodeType::Code,
                NodeType::Text,
                NodeType::Tag,
            ],
        ),
    );
    nodes.insert(
        NodeType::S,
        element(
            "s",
            &[
                NodeType::Strong,
                NodeType::Em,
                NodeType::Link,
                NodeType::Code,
                NodeType::Text,
                NodeType::Tag,
            ],
        ),
    );
    nodes.insert(NodeType::Inline, inline());
    nodes.insert(NodeType::Link, link());
    nodes.insert(NodeType::Code, code());
    nodes.insert(NodeType::Text, text());
    nodes.insert(NodeType::Hardbreak, Schema::new().render("br"));
    nodes.insert(NodeType::Softbreak, softbreak());
    nodes.insert(NodeType::Comment, comment());
    nodes.insert(NodeType::Error, Schema::new());
    nodes.insert(NodeType::Node, Schema::new());
    nodes
}

/// The node types that may appear at the top level of a document.
const TOP_LEVEL_BLOCKS: &[NodeType] = &[
    NodeType::Heading,
    NodeType::Paragraph,
    NodeType::Image,
    NodeType::Table,
    NodeType::Tag,
    NodeType::Fence,
    NodeType::Blockquote,
    NodeType::Comment,
    NodeType::List,
    NodeType::Hr,
];

/// The same, minus `comment`, which upstream omits inside a blockquote.
const NESTED_BLOCKS: &[NodeType] = &[
    NodeType::Heading,
    NodeType::Paragraph,
    NodeType::Image,
    NodeType::Table,
    NodeType::Tag,
    NodeType::Fence,
    NodeType::Blockquote,
    NodeType::List,
    NodeType::Hr,
];

/// What a list item or a table cell may contain: block content plus `inline`.
const CELL_CHILDREN: &[NodeType] = &[
    NodeType::Inline,
    NodeType::Heading,
    NodeType::Paragraph,
    NodeType::Image,
    NodeType::Table,
    NodeType::Tag,
    NodeType::Fence,
    NodeType::Blockquote,
    NodeType::List,
    NodeType::Hr,
];

/// A schema that renders one element and permits a fixed set of children.
fn element(render: &str, children: &[NodeType]) -> Schema {
    let mut schema = Schema::new().render(render);
    schema.children = Some(children.to_vec());
    schema
}

/// An emphasis element, which carries the marker it was written with.
///
/// `marker` is parse output rather than render input -- `*` versus `_` -- so it
/// is declared and hidden. The formatter reads it; the renderer must not.
fn marked(render: &str, children: &[NodeType]) -> Schema {
    element(render, children).attribute("marker", hidden(ValidationType::String))
}

/// A declared attribute of the given type, rendered under its own name.
fn typed(kind: ValidationType) -> SchemaAttribute {
    SchemaAttribute {
        attribute_type: Some(kind),
        ..SchemaAttribute::default()
    }
}

/// A declared attribute the transformer reads and the renderer never sees.
fn hidden(kind: ValidationType) -> SchemaAttribute {
    SchemaAttribute {
        attribute_type: Some(kind),
        render: RenderPolicy::Hidden,
        ..SchemaAttribute::default()
    }
}

/// A declared attribute rendered under a different name.
fn renamed(kind: ValidationType, name: &str) -> SchemaAttribute {
    SchemaAttribute {
        attribute_type: Some(kind),
        render: RenderPolicy::Renamed(name.to_string()),
        ..SchemaAttribute::default()
    }
}

/// Mark an attribute required.
fn required(mut attribute: SchemaAttribute) -> SchemaAttribute {
    attribute.required = true;
    attribute
}

/// `document`, the root. Renders an `<article>`.
///
/// `frontmatter` is declared and hidden so that a host which puts one on the
/// document does not have it appear as an attribute. This crate never sets it
/// (`DIVERGENCES.md` entry 7); the declaration is kept anyway, because dropping
/// it would let a host's frontmatter leak into the output.
fn document() -> Schema {
    let mut schema = element("article", TOP_LEVEL_BLOCKS);
    schema.attributes.insert(
        "frontmatter".to_string(),
        SchemaAttribute {
            render: RenderPolicy::Hidden,
            ..SchemaAttribute::default()
        },
    );
    schema
}

/// `heading`, which renders `h1` through `h6` from its `level`.
fn heading() -> Schema {
    let mut schema = Schema::new().attribute("level", required(hidden(ValidationType::Number)));
    schema.children = Some(vec![NodeType::Inline]);
    schema.transform = Some(Arc::new(|node, config| {
        let level = match node.get("level") {
            Some(Value::Number(level)) => js_integer(*level),
            _ => "1".to_string(),
        };
        one(Tag::with(
            format!("h{level}"),
            transform::attributes(node, config),
            transform::children(node, config),
        ))
    }));
    schema
}

/// A number as JavaScript's template literal writes it.
///
/// `level` is an `f64` because markdown-it reports a number, and `` `h${1}` ``
/// is `h1` rather than `h1.0`.
fn js_integer(number: f64) -> String {
    if number.fract() == 0.0 && number.is_finite() {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "integral and finite, checked immediately above"
        )]
        return (number as i64).to_string();
    }
    number.to_string()
}

fn image() -> Schema {
    Schema::new()
        .render("img")
        .attribute("src", required(typed(ValidationType::String)))
        .attribute("alt", typed(ValidationType::String))
        .attribute("title", typed(ValidationType::String))
}

/// `fence`, a code block.
///
/// The hook exists for one reason: a fence whose tags were not processed has no
/// children, and its text lives in the `content` attribute, which is hidden.
/// Without the hook such a fence would render as an empty `<pre>`.
fn fence() -> Schema {
    let mut schema = Schema::new()
        .render("pre")
        .attribute("content", required(hidden(ValidationType::String)))
        .attribute("language", renamed(ValidationType::String, "data-language"))
        .attribute("process", {
            let mut process = hidden(ValidationType::Boolean);
            process.default = Some(Value::Boolean(true));
            process
        });
    schema.transform = Some(Arc::new(|node, config| {
        let attributes = transform::attributes(node, config);
        let children = if node.children.is_empty() {
            content_child(node, config)
        } else {
            transform::children(node, config)
        };
        one(Tag::with("pre", attributes, children))
    }));
    schema
}

/// The `content` attribute as a single rendered child.
///
/// Shared by `fence` and `code`, which both keep their text out of the tree so
/// that a schema can decide whether to render it.
fn content_child<'a>(
    node: &'a crate::ast::Node<'a>,
    config: &crate::validate::Config<'a>,
) -> Vec<RenderableTreeNode> {
    node.get("content")
        .and_then(|content| transform::resolve(content, config))
        .as_ref()
        .and_then(Scalar::from_value)
        .map(|content| vec![RenderableTreeNode::Scalar(content)])
        .unwrap_or_default()
}

/// `list`, which renders `ol` or `ul` depending on its `ordered` attribute.
fn list() -> Schema {
    let mut schema = Schema::new()
        .attribute("ordered", required(hidden(ValidationType::Boolean)))
        .attribute("start", typed(ValidationType::Number))
        .attribute("marker", hidden(ValidationType::String));
    schema.children = Some(vec![NodeType::Item]);
    schema.transform = Some(Arc::new(|node, config| {
        let ordered = matches!(node.get("ordered"), Some(Value::Boolean(true)));
        one(Tag::with(
            if ordered { "ol" } else { "ul" },
            transform::attributes(node, config),
            transform::children(node, config),
        ))
    }));
    schema
}

fn td() -> Schema {
    element("td", CELL_CHILDREN)
        .attribute("align", typed(ValidationType::String))
        .attribute("colspan", renamed(ValidationType::Number, "colSpan"))
        .attribute("rowspan", renamed(ValidationType::Number, "rowSpan"))
}

fn th() -> Schema {
    Schema::new()
        .render("th")
        .attribute("width", typed(ValidationType::String))
        .attribute("align", typed(ValidationType::String))
        .attribute("colspan", renamed(ValidationType::Number, "colSpan"))
        .attribute("rowspan", renamed(ValidationType::Number, "rowSpan"))
}

/// `inline`, the seam between a block and its inline content.
///
/// Renders nothing of its own: its children become its parent's.
fn inline() -> Schema {
    let mut schema = Schema::new();
    schema.children = Some(vec![
        NodeType::Strong,
        NodeType::Em,
        NodeType::S,
        NodeType::Code,
        NodeType::Text,
        NodeType::Tag,
        NodeType::Link,
        NodeType::Image,
        NodeType::Hardbreak,
        NodeType::Softbreak,
        NodeType::Comment,
    ]);
    schema
}

fn link() -> Schema {
    element(
        "a",
        &[
            NodeType::Strong,
            NodeType::Em,
            NodeType::S,
            NodeType::Code,
            NodeType::Text,
            NodeType::Tag,
        ],
    )
    .attribute("href", required(typed(ValidationType::String)))
    .attribute("title", typed(ValidationType::String))
}

/// `code`, an inline code span.
///
/// Like [`fence`], its text is a hidden attribute rather than a child, so the
/// hook puts it back.
fn code() -> Schema {
    let mut schema = Schema::new()
        .render("code")
        .attribute("content", required(hidden(ValidationType::String)));
    schema.transform = Some(Arc::new(|node, config| {
        let attributes = transform::attributes(node, config);
        one(Tag::with("code", attributes, content_child(node, config)))
    }));
    schema
}

/// `text`, a run of literal text.
///
/// The one schema whose transform returns a bare value rather than an element.
/// Its `content` may be a `$variable`, which is where an inline variable's value
/// enters the tree.
fn text() -> Schema {
    let mut schema = Schema::new().attribute("content", required(typed(ValidationType::String)));
    schema.transform = Some(Arc::new(|node, config| {
        transform::scalar(
            node.get("content")
                .and_then(|content| transform::resolve(content, config))
                .as_ref()
                .and_then(Scalar::from_value),
        )
    }));
    schema
}

/// `softbreak`, a newline inside a block, which renders as one space.
fn softbreak() -> Schema {
    let mut schema = Schema::new();
    schema.transform = Some(Arc::new(|_node, _config| {
        RenderableTreeNodes::One(RenderableTreeNode::text(" "))
    }));
    schema
}

/// `comment`, which declares its content and renders nothing.
fn comment() -> Schema {
    Schema::new().attribute("content", required(typed(ValidationType::String)))
}

/// One tag, as renderable nodes.
fn one(tag: Tag) -> RenderableTreeNodes {
    RenderableTreeNodes::One(RenderableTreeNode::tag(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_type_the_parser_produces_has_a_schema() {
        // A node type with no schema renders as its children, silently. That is
        // right for `inline` and wrong for everything else, so the map is
        // checked rather than trusted.
        let nodes = builtin();
        for node_type in [
            NodeType::Blockquote,
            NodeType::Code,
            NodeType::Comment,
            NodeType::Document,
            NodeType::Em,
            NodeType::Error,
            NodeType::Fence,
            NodeType::Hardbreak,
            NodeType::Heading,
            NodeType::Hr,
            NodeType::Image,
            NodeType::Inline,
            NodeType::Item,
            NodeType::Link,
            NodeType::List,
            NodeType::Node,
            NodeType::Paragraph,
            NodeType::S,
            NodeType::Softbreak,
            NodeType::Strong,
            NodeType::Table,
            NodeType::Tbody,
            NodeType::Td,
            NodeType::Text,
            NodeType::Th,
            NodeType::Thead,
            NodeType::Tr,
        ] {
            assert!(nodes.contains_key(&node_type), "no schema for {node_type}");
        }
        // `tag` is deliberately absent: a tag is looked up by name in
        // `config.tags`, never by node type.
        assert!(!nodes.contains_key(&NodeType::Tag));
    }

    #[test]
    fn a_heading_level_prints_as_an_integer() {
        assert_eq!(js_integer(1.0), "1");
        assert_eq!(js_integer(6.0), "6");
    }
}
