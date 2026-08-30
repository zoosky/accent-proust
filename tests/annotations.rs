//! Upstream's `src/tokenizer/plugins/annotations.test.ts`, ported.
//!
//! # Why these assertions look different
//!
//! Upstream's version asserts on markdown-it *tokens*: `{type: 'tag_open',
//! nesting: 1, meta: {tag: 'test', attributes: null}}`, `block: true`,
//! `map: [0, 2]`. None of those exist here. markdown-it's token stream is the
//! thing this port replaced, and asserting on a shape the crate does not have
//! would mean building one to be asserted on.
//!
//! So each test asserts the same *fact* one layer up, where it is observable:
//! a `tag_open` with `nesting: 1` is a `tag` node with children; `block: true`
//! is `inline == false`; `meta.attributes` is the node's attributes; and
//! `token.map` is `Node::lines`, which this port keeps in upstream's half-open
//! form for exactly this reason.
//!
//! The one thing not translated is `token.info`, the raw text of the tag.
//! Nothing above the tokenizer reads it upstream either -- the formatter
//! reprints from the parsed attributes -- and here the equivalent is
//! `Node::location`, whose text is the same bytes.

mod support;

use proust::ast::NodeType;
use proust::parse::parse;
use support::{at, attribute, dedent, outline};

// ---- parsing containers in content --------------------------------------

#[test]
fn with_a_simple_container() {
    let source = dedent("\n{% test %}\nThis is a test\n{% /test %}\n");
    let document = parse(&source);
    assert_eq!(
        outline(&document),
        "\
document
  tag[test]
    paragraph
      inline
        text content=\"This is a test\"
"
    );

    let tag = at(&document, &[0]);
    assert!(!tag.inline, "a block-level tag is not inline");
    // Upstream asserts `map: [0, 1]` on the open token and `[2, 3]` on the
    // close. Both pairs land on the node, in that order, because a tag knows
    // where it opened and where it closed.
    assert_eq!(tag.lines, vec![0, 1, 2, 3]);
}

#[test]
fn with_an_id_and_class() {
    let source = dedent("\n{% test #foo .bar %}\nThis is a test\n{% /test %}");
    let document = parse(&source);
    let tag = at(&document, &[0]);
    assert_eq!(attribute(tag, "id"), "\"foo\"");
    assert_eq!(attribute(tag, "class"), "{bar: true}");
    // Upstream asserts the annotation list verbatim, in authored order, because
    // the formatter reprints it rather than the attributes.
    assert_eq!(tag.annotations.len(), 2);
}

#[test]
fn with_a_self_closing_container() {
    let source = dedent("This is a test\n{% test /%}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"This is a test\"
  tag[test]
"
    );

    let alone = dedent("{% test /%}");
    assert_eq!(
        outline(&parse(&alone)),
        "\
document
  tag[test]
"
    );
}

#[test]
fn with_a_self_closing_container_with_annotations() {
    let source = dedent("\nThis is a test\n{% test #foo .bar baz=1 /%}\nThis is another test\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"This is a test\"
  tag[test] id=\"foo\" class={bar: true} baz=1
  paragraph
    inline
      text content=\"This is another test\"
"
    );
}

// ---- multiline ----------------------------------------------------------

#[test]
fn multiline_basic() {
    let source = dedent("\n{% test #foo .bar\nbaz=1 %}\nThis is a test\n{% /test %}\n");
    let document = parse(&source);
    let tag = at(&document, &[0]);
    assert_eq!(attribute(tag, "id"), "\"foo\"");
    assert_eq!(attribute(tag, "class"), "{bar: true}");
    assert_eq!(attribute(tag, "baz"), "1");
    // Upstream: `example[0].map` is `[0, 2]` and `example[4].map` is `[3, 4]`.
    assert_eq!(tag.lines, vec![0, 2, 3, 4]);
    assert_eq!(at(tag, &[0]).lines, vec![2, 3]);
    assert_eq!(tag.children.len(), 1);
}

#[test]
fn multiline_with_symbols_on_separate_lines() {
    let source = dedent("\n{%\ntest #foo .bar\nbaz=1\n%}\nThis is a test\n{% /test %}\n");
    let document = parse(&source);
    let tag = at(&document, &[0]);
    assert_eq!(attribute(tag, "id"), "\"foo\"");
    assert_eq!(attribute(tag, "baz"), "1");
    // Upstream: `[0, 4]` for the open and `[5, 6]` for the close.
    assert_eq!(tag.lines, vec![0, 4, 5, 6]);
    assert_eq!(tag.children.len(), 1);
}

// ---- inline -------------------------------------------------------------

#[test]
fn inline_on_a_line_by_itself() {
    let source = dedent("{% foo %}bar{% /foo %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      tag[foo]
        text content=\"bar\"
"
    );
}

#[test]
fn inline_with_a_paragraph() {
    let source = dedent("Example {% foo %}bar{% /foo %} baz");
    let document = parse(&source);
    assert_eq!(
        outline(&document),
        "\
document
  paragraph
    inline
      text content=\"Example \"
      tag[foo]
        text content=\"bar\"
      text content=\" baz\"
"
    );
    assert!(at(&document, &[0, 0, 1]).inline, "block: false upstream");
}

#[test]
fn inline_with_two_in_succession() {
    let source = dedent("Example {% foo %}bar{% /foo %}{% test %}test{% /test %} baz");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"Example \"
      tag[foo]
        text content=\"bar\"
      tag[test]
        text content=\"test\"
      text content=\" baz\"
"
    );
}

/// The test that justifies masking.
///
/// markdown-it never sees inside a tag, because its inline rule consumes the
/// whole tag at the `{` before the emphasis rule reaches the `*`. Here the
/// tokenizer reads a masked buffer, so it cannot either -- and the emphasis
/// *outside* the tag still has to work, which is what this asserts.
#[test]
fn inline_with_markdown_inside() {
    let source = dedent("Example {% foo %}this is a *test*{% /foo %} baz");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"Example \"
      tag[foo]
        text content=\"this is a \"
        em marker=\"*\"
          text content=\"test\"
      text content=\" baz\"
"
    );
}

#[test]
fn markdown_inside_a_tag_body_is_not_markdown() {
    // Not upstream's, but the inverse of the test above and the property the
    // masking exists for: a `*` inside the tag is part of a string, not
    // emphasis.
    let source = dedent("Example {% foo bar=\"a*b*c\" /%} baz");
    let document = parse(&source);
    assert_eq!(attribute(at(&document, &[0, 0, 1]), "bar"), "\"a*b*c\"");
}

// ---- fence --------------------------------------------------------------

/// `DIVERGENCES.md` entry 1 inverts upstream's default here, so upstream's
/// three fence tests are ported with `process` stated. What they check --
/// that fence content is split into text and tags, and that an unclosed `{%`
/// does not crash -- is unchanged.
#[test]
fn fence_simple_with_no_tags() {
    let source = "```\nhello\n```";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  fence content=\"hello\\n\"
"
    );
}

#[test]
fn fence_simple_with_one_tag() {
    let source = "``` {% process=true %}\nhello {% foo %}bar{% /foo %}\n```";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  fence content=\"hello {% foo %}bar{% /foo %}\\n\" process=true
    text content=\"hello \"
    tag[foo]
      text content=\"bar\"
    text content=\"\\n\"
"
    );
}

#[test]
fn fence_unclosed_tag() {
    // Upstream's comment: unclosed tags should not result in crashes.
    let source = "``` {% process=true %}\nhello {%\n```";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  fence content=\"hello {%\\n\" process=true
    text content=\"hello {%\\n\"
"
    );
}

#[test]
fn a_fence_without_the_opt_in_keeps_its_content_literal() {
    // The other half of divergence 1, asserted rather than assumed.
    let source = "```\nhello {% foo %}bar{% /foo %}\n```";
    let document = parse(source);
    let fence = at(&document, &[0]);
    assert_eq!(fence.node_type, NodeType::Fence);
    assert!(fence.children.is_empty());
    assert_eq!(
        attribute(fence, "content"),
        "\"hello {% foo %}bar{% /foo %}\\n\""
    );
}

// ---- parsing inline annotations -----------------------------------------

#[test]
fn annotation_with_a_header() {
    let source = dedent("# This is a test {% #foo .bar .baz %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  heading level=1 id=\"foo\" class={bar: true, baz: true}
    inline
      text content=\"This is a test \"
"
    );
}

#[test]
fn annotation_with_a_header_and_keys() {
    let source = dedent("# This is a test {% #foo .bar .baz foo=2 %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  heading level=1 id=\"foo\" class={bar: true, baz: true} foo=2
    inline
      text content=\"This is a test \"
"
    );
}
