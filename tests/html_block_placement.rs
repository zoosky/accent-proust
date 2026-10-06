//! A raw-HTML block lands under the node whose delimiters contain it.
//!
//! The parser holds an HTML block back until the next markdown event. A tag
//! delimiter is not one, so a block that ended right before a tag used to
//! attach to whichever node was open at the next event: the tag that follows
//! it, the parent after a closing tag, the branch after `{% else /%}`, a
//! sibling. Accent's expander cuts a tag's body from its children, so a block
//! attached to the following tag put the tag's own opening line in its body
//! and recursed until the stack overflowed.

use std::ops::Range;

use accent_proust::ast::{Node, NodeType};
use accent_proust::parse::parse;

/// The first tag named `name` in a tree, in document order.
fn find<'n, 'a>(node: &'n Node<'a>, name: &str) -> Option<&'n Node<'a>> {
    if node.node_type == NodeType::Tag && node.tag.as_deref() == Some(name) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, name))
}

/// The trimmed source text of a span, empty when the span is out of range.
fn text(source: &str, span: Range<usize>) -> String {
    source.get(span).unwrap_or_default().trim_end().to_string()
}

/// The source text of each direct child of `node` that carries a location.
fn texts(source: &str, node: &Node<'_>) -> Vec<String> {
    node.children
        .iter()
        .filter_map(|child| child.location)
        .map(|location| text(source, location.start.offset..location.end.offset))
        .collect()
}

/// The children of the first tag named `name`, or `None` when the source
/// holds no such tag. The helpers answer with an [`Option`] and leave the
/// unwrapping to each `#[test]`, where `clippy.toml` relaxes the
/// panic-freedom lints.
fn children_of(source: &str, name: &str) -> Option<Vec<String>> {
    let document = parse(source);
    find(&document, name).map(|tag| texts(source, tag))
}

#[test]
fn a_block_before_a_tag_stays_before_it() {
    let source = "<div></div>\n\n{% note %}\nx\n{% /note %}\n";
    assert_eq!(children_of(source, "note").expect("the tag"), ["x"]);
    assert_eq!(texts(source, &parse(source))[0], "<div></div>");
}

#[test]
fn a_block_before_a_closing_tag_stays_inside() {
    let source = "{% note %}\n<div>a</div>\n{% /note %}\n";
    assert_eq!(
        children_of(source, "note").expect("the tag"),
        ["<div>a</div>"]
    );
    assert_eq!(
        texts(source, &parse(source)).len(),
        1,
        "nothing after the tag"
    );
}

#[test]
fn a_block_before_else_stays_in_its_branch() {
    let source = "{% if $x %}\n<div>yes</div>\n{% else /%}\nno\n{% /if %}\n";
    assert_eq!(
        children_of(source, "if").expect("the tag"),
        ["<div>yes</div>", "{% else /%}", "no"],
        "the block precedes the marker, in source order"
    );
}

#[test]
fn a_block_ending_one_tag_does_not_open_the_next() {
    let source = "{% tab %}\n<iframe src=\"v\"></iframe>\n{% /tab %}\n{% tab %}\nb\n{% /tab %}\n";
    assert_eq!(
        children_of(source, "tab").expect("the tag"),
        ["<iframe src=\"v\"></iframe>"]
    );
}
