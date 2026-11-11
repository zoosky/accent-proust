//! A link's destination is encoded as upstream encodes it, in the renderable
//! tree and not in the syntax tree (#78, `DIVERGENCES.md` entry 17).
//!
//! Every expected value is upstream's, from `@markdoc/markdoc` 0.5.12.

mod support;

use accent_proust::ast::{Node, NodeType, Value};
use accent_proust::parse::parse;
use accent_proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar};
use accent_proust::{builtins, format, render, transform};

/// The source of every case, and the destination upstream renders for it.
const CASES: &[(&str, &str)] = &[
    ("[a](</a b>)", "/a%20b"),
    ("[a](/a{b})", "/a%7Bb%7D"),
    ("[a](/50%)", "/50%25"),
    ("[a](/%zz)", "/%25zz"),
    ("[a](/a%20b)", "/a%20b"),
    ("![i](/ä.png)", "/%C3%A4.png"),
    ("[a](/&auml;)", "/%C3%A4"),
    ("[a](https://bücher.de/)", "https://xn--bcher-kva.de/"),
    ("[a](mailto:max@bücher.de)", "mailto:max@xn--bcher-kva.de"),
    ("[a](ftp://bücher.de/x)", "ftp://b%C3%BCcher.de/x"),
    ("<https://bücher.de/ä>", "https://xn--bcher-kva.de/%C3%A4"),
    ("[a](/x{%$y%}z)", "/x%7B%25$y%25%7Dz"),
    ("[a][r]\n\n[r]: /ä\n", "/%C3%A4"),
];

/// The first link's `href` or image's `src` in a node.
fn destination(document: &Node<'_>) -> Option<String> {
    let node = document
        .walk()
        .find(|node| matches!(node.node_type, NodeType::Link | NodeType::Image))?;
    let key = if node.node_type == NodeType::Link {
        "href"
    } else {
        "src"
    };
    match node.get(key) {
        Some(Value::String(url)) => Some(url.clone()),
        _ => None,
    }
}

/// The first `href` or `src` in a renderable tree.
fn rendered_destination(nodes: &[RenderableTreeNode]) -> Option<String> {
    for node in nodes {
        let RenderableTreeNode::Tag(tag) = node else {
            continue;
        };
        for key in ["href", "src"] {
            if let Some(RenderableTreeNodes::One(RenderableTreeNode::Scalar(Scalar::String(url)))) =
                tag.attributes.get(key)
            {
                return Some(url.clone());
            }
        }
        if let Some(found) = rendered_destination(&tag.children) {
            return Some(found);
        }
    }
    None
}

#[test]
fn the_renderable_tree_holds_upstreams_destination() {
    for (source, encoded) in CASES {
        let document = parse(source);
        let tree = transform::transform(&document, &builtins::config()).into_vec();
        assert_eq!(
            rendered_destination(&tree).as_deref(),
            Some(*encoded),
            "{source:?}"
        );
        let html = render::render_all(&tree);
        assert!(
            html.contains(&format!("=\"{encoded}\"")),
            "{source:?}: {html}"
        );
    }
}

#[test]
fn the_syntax_tree_keeps_the_destination_as_written() {
    for (source, written) in [
        ("[a](</a b>)", "/a b"),
        ("![i](/ä.png)", "/ä.png"),
        ("[a](https://bücher.de/)", "https://bücher.de/"),
        ("<https://bücher.de/ä>", "https://bücher.de/ä"),
    ] {
        assert_eq!(
            destination(&parse(source)).as_deref(),
            Some(written),
            "{source:?}"
        );
    }
}

#[test]
fn formatting_does_not_rewrite_a_destination() {
    for source in [
        "![i](/ä.png)\n",
        "[a](https://bücher.de/)\n",
        "<https://bücher.de/ä>\n",
    ] {
        assert_eq!(format::format(&parse(source)), *source, "{source:?}");
    }
}

#[test]
fn an_autolink_keeps_its_text_readable() {
    let document = parse("<https://bücher.de/ä>\n");
    let html = render::render_all(&transform::transform(&document, &builtins::config()).into_vec());
    assert!(html.contains(">https://bücher.de/ä</a>"), "{html}");
}

/// A custom `href` type that records the value it is handed.
struct Seen;

impl accent_proust::validate::AttributeType for Seen {
    fn name(&self) -> &'static str {
        "Seen"
    }

    fn transform(
        &self,
        value: Option<&Value>,
        _config: &accent_proust::validate::Config<'_>,
    ) -> Option<Scalar> {
        match value {
            Some(Value::String(url)) => Some(Scalar::String(format!("{url}|"))),
            _ => None,
        }
    }
}

#[test]
fn a_custom_type_is_handed_the_encoded_destination() {
    use std::sync::Arc;

    use accent_proust::validate::{MapSchemaSource, Schema, SchemaAttribute, ValidationType};

    let mut schemas = MapSchemaSource::builtin();
    schemas.insert_node(
        NodeType::Link,
        Schema::new().render("a").attribute(
            "href",
            SchemaAttribute {
                attribute_type: Some(ValidationType::Custom(Arc::new(Seen))),
                ..SchemaAttribute::default()
            },
        ),
    );
    let config = builtins::config_with(Arc::new(schemas));
    let document = parse("[a](</ä b>)\n");
    let html = render::render_all(&transform::transform(&document, &config).into_vec());
    // Upstream's custom type sees what its tokenizer stored, the encoded form,
    // and what it returns is not encoded again: the `|` stays.
    assert!(html.contains("href=\"/%C3%A4%20b|\""), "{html}");
}
