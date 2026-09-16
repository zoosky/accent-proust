//! Attribute locations in document coordinates.
//!
//! The grammar's own tests cover spans relative to a tag body; these cover the
//! translation into the document, which is the half a consumer actually holds.
//! The last test is the one the feature exists for: rewriting a single
//! attribute's value leaves every other byte of the document alone.

use accent_proust::ast::{AttributeLocation, Node, NodeType};
use accent_proust::grammar::Attribute;
use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse, parse_with};

/// Every node of a document, parents before children.
///
/// `Node::descendants_in_order` is private, and a consumer walks the tree
/// itself, so the test does too.
fn nodes<'n, 'a>(root: &'n Node<'a>) -> Vec<&'n Node<'a>> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        found.push(node);
        // Slots are pushed first so they pop last, which keeps children in
        // document order ahead of them rather than behind them.
        stack.extend(node.slots.values().rev());
        stack.extend(node.children.iter().rev());
    }
    found
}

/// The first tag node with this name, if the document has one.
///
/// The helpers here answer with an [`Option`] and leave the unwrapping to each
/// `#[test]`: `clippy.toml` relaxes the panic-freedom lints inside `#[test]`
/// functions and nowhere else, so helper code stays under the same rules as the
/// library.
fn tag<'n, 'a>(root: &'n Node<'a>, name: &str) -> Option<&'n Node<'a>> {
    nodes(root)
        .into_iter()
        .find(|node| node.tag.as_deref() == Some(name))
}

/// The location of one named attribute, found the way a consumer finds it:
/// by matching the annotation and taking the entry beside it.
fn located<'a>(node: &Node<'a>, name: &str) -> Option<AttributeLocation<'a>> {
    // The field's contract is "empty, or one entry per annotation", so an empty
    // list is the contract being honoured rather than broken: a fence annotated
    // through its info string, and any parse with locations off, reach here
    // legitimately and answer `None` below.
    if !node.annotation_locations.is_empty() {
        assert_eq!(
            node.annotations.len(),
            node.annotation_locations.len(),
            "locations must be parallel to annotations"
        );
    }
    let index = node.annotations.iter().position(|annotation| {
        matches!(annotation, Attribute::Attribute { name: written, .. } if written == name)
    })?;
    node.annotation_locations.get(index).copied()
}

#[test]
fn a_location_covers_the_attribute_as_written_and_its_value() {
    let source = "Intro.\n\n{% callout type=\"note\" open=true %}\nBody.\n{% /callout %}\n";
    let document = parse(source);
    let callout = tag(&document, "callout").expect("the callout tag");

    let kind = located(callout, "type").expect("a located type attribute");
    assert_eq!(kind.all.text, r#"type="note""#);
    assert_eq!(kind.value.expect("a written value").text, r#""note""#);
    assert_eq!(&source[kind.all.span()], r#"type="note""#);

    let open = located(callout, "open").expect("a located open attribute");
    assert_eq!(open.all.text, "open=true");
    assert_eq!(open.value.expect("a written value").text, "true");
}

#[test]
fn a_location_reports_the_line_and_column_the_attribute_sits_on() {
    let source = "Line one.\n\nLine three.\n\n{% callout type=\"note\" %}\nBody.\n{% /callout %}\n";
    let document = parse(source);
    let callout = tag(&document, "callout").expect("the callout tag");
    let kind = located(callout, "type").expect("a located type attribute");
    // Zero-based, as every position in this crate is.
    assert_eq!(kind.all.start.line, 4);
    assert_eq!(kind.all.start.column, "{% callout ".len());
}

#[test]
fn multibyte_text_earlier_in_the_document_does_not_shift_a_location() {
    let source = "Über größer — ünicode.\n\n{% callout type=\"note\" %}\nBody.\n{% /callout %}\n";
    let document = parse(source);
    let callout = tag(&document, "callout").expect("the callout tag");
    let kind = located(callout, "type").expect("a located type attribute");
    assert_eq!(kind.all.text, r#"type="note""#);
    assert_eq!(&source[kind.all.span()], r#"type="note""#);
}

#[test]
fn an_indented_tag_inside_a_list_is_located_where_it_sits() {
    let source = "- item\n\n  {% callout type=\"note\" %}\n  Body.\n  {% /callout %}\n";
    let document = parse(source);
    let callout = tag(&document, "callout").expect("the callout tag");
    let kind = located(callout, "type").expect("a located type attribute");
    assert_eq!(kind.all.text, r#"type="note""#);
    assert_eq!(&source[kind.all.span()], r#"type="note""#);
}

#[test]
fn an_inline_annotation_locates_onto_the_block_it_annotates() {
    let source = "# Title {% #intro .lead %}\n";
    let document = parse(source);
    let heading = nodes(&document)
        .into_iter()
        .find(|node| node.node_type == NodeType::Heading)
        .expect("a heading");

    assert_eq!(heading.annotations.len(), 2);
    assert_eq!(heading.annotation_locations.len(), 2);
    let first = heading.annotation_locations.first().expect("a location");
    let second = heading.annotation_locations.get(1).expect("a location");
    assert_eq!(first.all.text, "#intro");
    assert_eq!(second.all.text, ".lead");
    // The shortcuts imply their values rather than spelling them.
    assert!(first.value.is_none());
    assert!(second.value.is_none());
}

#[test]
fn a_fence_annotated_through_its_info_string_reports_no_locations() {
    // The tokenizer hands over the info string's text but not where it sits, so
    // there is no honest offset. The annotation still applies.
    let source = "```js {% #snippet %}\ncode\n```\n";
    let document = parse(source);
    let fence = nodes(&document)
        .into_iter()
        .find(|node| node.node_type == NodeType::Fence)
        .expect("a fence");

    assert!(
        !fence.annotations.is_empty(),
        "the annotation still applies"
    );
    assert!(fence.annotation_locations.is_empty());
}

#[test]
fn a_tag_inside_a_processed_fence_is_located_where_it_sits() {
    // A fence that opts into processing parses its content, so a tag in there
    // reaches the same translation as any other. That is the opposite of the
    // fence's own info-string annotation, which has no offset to translate
    // against -- the asymmetry is deliberate, so it gets a test.
    let source = "```html {% process=true %}\n{% callout type=\"note\" /%}\n```\n";
    let document = parse(source);
    let callout = tag(&document, "callout").expect("the callout tag");
    let kind = located(callout, "type").expect("a located type attribute");

    assert_eq!(kind.all.text, r#"type="note""#);
    assert_eq!(&source[kind.all.span()], r#"type="note""#);
}

#[test]
fn switching_locations_off_reports_none() {
    let source = "{% callout type=\"note\" %}\nBody.\n{% /callout %}\n";
    let mut options = ParseOptions::new();
    options.location = false;
    let document = parse_with(source, &PulldownTokenizer::new(), &options);
    let callout = tag(&document, "callout").expect("the callout tag");

    assert!(!callout.annotations.is_empty());
    assert!(callout.annotation_locations.is_empty());
}

#[test]
fn every_node_keeps_locations_parallel_to_annotations() {
    let source = concat!(
        "# Title {% #intro .lead %}\n\n",
        "```js {% #snippet %}\ncode\n```\n\n",
        "{% callout type=\"note\" title=\"Heads up\" %}\n",
        "Body with an {% tag a=1 /%} inside.\n",
        "{% /callout %}\n",
    );
    for node in nodes(&parse(source)) {
        assert!(
            node.annotation_locations.is_empty()
                || node.annotation_locations.len() == node.annotations.len(),
            "{:?} has {} annotations and {} locations",
            node.node_type,
            node.annotations.len(),
            node.annotation_locations.len(),
        );
    }
}

#[test]
fn replacing_a_value_leaves_every_other_byte_of_the_document_alone() {
    let source = concat!(
        "# Title\n\n",
        "Text with *emphasis* and `code`.\n\n",
        "{% callout type=\"note\"   title=\"Heads   up\" %}\n",
        "Body stays exactly as written.\n",
        "{% /callout %}\n",
    );
    let document = parse(source);
    let callout = tag(&document, "callout").expect("the callout tag");
    let value = located(callout, "type")
        .expect("a located type attribute")
        .value
        .expect("a written value");

    let mut rewritten = String::new();
    rewritten.push_str(&source[..value.start.offset]);
    rewritten.push_str(r#""warning""#);
    rewritten.push_str(&source[value.end.offset..]);

    // The odd spacing is deliberate: a rewrite that reformatted the tag, rather
    // than replacing one span, would tidy it and pass a weaker assertion.
    assert_eq!(
        rewritten,
        concat!(
            "# Title\n\n",
            "Text with *emphasis* and `code`.\n\n",
            "{% callout type=\"warning\"   title=\"Heads   up\" %}\n",
            "Body stays exactly as written.\n",
            "{% /callout %}\n",
        )
    );
}
