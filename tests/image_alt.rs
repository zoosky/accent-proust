//! An image's `alt` is the source of its label, as upstream's is (#96).
//!
//! markdown-it's image rule sets `alt` from the token's `content`, the label
//! as written, and Markdoc keeps it. Every expected value is upstream's, from
//! `@markdoc/markdoc` 0.5.12.

use accent_proust::ast::{Node, NodeType, Value};
use accent_proust::format::format;
use accent_proust::parse::parse;

/// The first image's `alt`.
fn alt(document: &Node<'_>) -> Option<String> {
    let image = document
        .walk()
        .find(|node| node.node_type == NodeType::Image)?;
    match image.get("alt") {
        Some(Value::String(alt)) => Some(alt.clone()),
        _ => None,
    }
}

#[test]
fn the_alt_is_the_label_as_written() {
    for (source, expected) in [
        ("![<https://a.b>](/i.png)", "<https://a.b>"),
        ("![*a* `b` <c@d.e>](/i.png)", "*a* `b` <c@d.e>"),
        ("![a {% $y %} b](/i.png)", "a {% $y %} b"),
        ("![<https://a.b/{%$y%}>](/i.png)", "<https://a.b/{%$y%}>"),
        ("![a\\*b &amp; c](/i.png)", "a\\*b &amp; c"),
        ("![a\\]b](/i.png)", "a\\]b"),
        ("![`]`](/i.png)", "`]`"),
        ("![a ![b](/c) d](/i.png)", "a ![b](/c) d"),
        ("![a [b](/u) c](/i.png)", "a [b](/u) c"),
        ("![x][r]\n\n[r]: /i.png", "x"),
        ("x ![](/i.png)", ""),
    ] {
        assert_eq!(alt(&parse(source)).as_deref(), Some(expected), "{source:?}");
    }
}

/// A line of the label after the first is without its container prefix, and
/// keeps any indentation past it; line endings are `\n`.
#[test]
fn a_label_over_lines_loses_only_its_container_prefix() {
    for (source, expected) in [
        ("![a\nb](/i.png)", "a\nb"),
        ("![a\n   b](/i.png)", "a\n   b"),
        ("- ![a\n  b](/i.png)", "a\nb"),
        ("- ![a\n     b](/i.png)", "a\n   b"),
        ("> > ![a\n> > b](/i.png)", "a\nb"),
        ("![a  \nb](/i.png)", "a  \nb"),
        ("![a\r\nb](/i.png)", "a\nb"),
    ] {
        assert_eq!(alt(&parse(source)).as_deref(), Some(expected), "{source:?}");
    }
}

/// `fmt` writes the alt back as the label, which reads back as the same alt:
/// a tag kept as text in an autolink stays text (found in review of #95).
#[test]
fn the_alt_survives_formatting() {
    for source in [
        "![<https://a.b/{%$y%}>](/i.png)\n",
        "![*a* `b`](/i.png)\n",
        "![a {% $y %} b](/i.png)\n",
    ] {
        let once = format(&parse(source));
        assert_eq!(
            alt(&parse(&once)),
            alt(&parse(source)),
            "{source:?} -> {once:?}"
        );
        assert_eq!(format(&parse(&once)), once, "{source:?}");
    }
}
