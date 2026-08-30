//! Upstream's `src/parser.test.ts`, ported.
//!
//! 1,046 lines of TypeScript, and the real specification for this stage: the
//! conformance corpus grades a *renderable* tree, so it says almost nothing
//! about the AST that produces one. These assertions are what fix the AST's
//! shape.
//!
//! # What is ported, and what is not
//!
//! Two of upstream's blocks are deliberately absent, each for a reason that
//! is already written down:
//!
//! - **`handling frontmatter`.** `DIVERGENCES.md` entry 7: frontmatter is the
//!   host's, removed before this crate sees a document. There is no
//!   `document.attributes.frontmatter` to assert.
//! - **The `location` option's line numbers.** Upstream's location is a line
//!   map, so its heading ends on line 1. This crate's is a byte range with the
//!   text borrowed, so the same heading ends where its last byte is. The
//!   assertion is ported to that shape rather than dropped, because what it is
//!   really testing -- that `file` propagates and that `location: false` removes
//!   the field -- is unchanged.
//!
//! Everything else is here, in upstream's order, with upstream's names.

mod support;

use proust::ast::{NodeType, Value};
use proust::parse::{parse, parse_with, ParseOptions, PulldownTokenizer};
use support::{all_error_ids, at, attribute, dedent, error_ids, outline};

/// Upstream's `convert(example, options)`, which every test calls.
fn convert<'s>(source: &'s str, options: &ParseOptions<'s>) -> proust::ast::Node<'s> {
    parse_with(source, &PulldownTokenizer::new(), options)
}

const FENCE: &str = "```";

// ---- handling options ---------------------------------------------------

#[test]
fn no_args() {
    let source = dedent("# This is a test");
    let document = parse(&source);
    let heading = at(&document, &[0]);
    assert_eq!(heading.node_type, NodeType::Heading);
    assert_eq!(heading.location.expect("a location").file, None);
}

#[test]
fn filename_as_property() {
    let source = dedent("# This is a test");
    let document = convert(&source, &ParseOptions::new().file("foo.md"));
    let location = at(&document, &[0]).location.expect("a location");
    assert_eq!(location.file, Some("foo.md"));
}

#[test]
fn location_off() {
    let source = dedent("# This is a test");
    let document = convert(&source, &ParseOptions::new().file("foo.md").location(false));
    assert!(at(&document, &[0]).location.is_none());
}

#[test]
fn location_on() {
    let source = dedent("# This is a test");
    let document = convert(&source, &ParseOptions::new().file("foo.md"));
    let location = at(&document, &[0]).location.expect("a location");
    assert_eq!(location.file, Some("foo.md"));
    assert_eq!(location.start.line, 0);
    assert_eq!(location.text, "# This is a test");
    // Upstream asserts `end: {line: 1}` because its location is a half-open line
    // map. Here the same fact lives in `lines`, and `location.end` is where the
    // heading's last byte is.
    assert_eq!(at(&document, &[0]).lines, vec![0, 1]);
}

// ---- handling attributes ------------------------------------------------

#[test]
fn attributes_for_emphasis() {
    let markers = |example: &str, index: usize| {
        let source = dedent(example);
        let document = parse(&source);
        attribute(at(&document, &[0, 0, index]), "marker")
    };
    assert_eq!(markers("a*b*c", 1), "\"*\"");
    assert_eq!(markers("a**b**c", 1), "\"**\"");
    assert_eq!(markers("_foo_ bar", 0), "\"_\"");
    assert_eq!(markers("__foo__ bar", 0), "\"__\"");
    assert_eq!(markers("foo *bar* baz", 1), "\"*\"");
    assert_eq!(markers("foo **bar** baz", 1), "\"**\"");
}

#[test]
fn attributes_for_heading() {
    let source = dedent("# Sample Heading");
    let document = parse(&source);
    assert_eq!(attribute(at(&document, &[0]), "level"), "1");
}

#[test]
fn attributes_for_list() {
    let unordered = dedent("\n* Example 1\n* Example 2\n* Example 3\n");
    let ordered = dedent("\n1. Example 1\n2. Example 2\n3. Example 3\n");
    let unordered = parse(&unordered);
    let ordered = parse(&ordered);
    assert_eq!(attribute(at(&unordered, &[0]), "ordered"), "false");
    assert_eq!(attribute(at(&ordered, &[0]), "ordered"), "true");
}

#[test]
fn attributes_for_ordered_list_start() {
    let start = |example: &str| {
        let source = dedent(example);
        let document = parse(&source);
        attribute(at(&document, &[0]), "start")
    };
    assert_eq!(start("\n* Example 1\n* Example 2\n"), "<unset>");
    assert_eq!(start("\n3. Example 1\n4. Example 2\n"), "3");
    // markdown-it omits `start` for a list beginning at 1, whatever follows.
    assert_eq!(start("\n1. Example 1\n4. Example 2\n"), "<unset>");
}

#[test]
fn attributes_for_link() {
    let source = dedent("\n[foo](/bar)\n");
    let document = parse(&source);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(link.node_type, NodeType::Link);
    assert_eq!(attribute(link, "href"), "\"/bar\"");
    assert_eq!(attribute(link, "title"), "<unset>");
}

#[test]
fn attributes_for_link_with_a_title() {
    let source = dedent("\n[foo](/bar \"title\")\n");
    let document = parse(&source);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(link, "href"), "\"/bar\"");
    assert_eq!(attribute(link, "title"), "\"title\"");
}

#[test]
fn attributes_for_text() {
    let source = dedent("\nThis is a test\n");
    let document = parse(&source);
    assert_eq!(
        attribute(at(&document, &[0, 0, 0]), "content"),
        "\"This is a test\""
    );
}

#[test]
fn attributes_for_code_fence() {
    let simple = dedent(&format!("\n{FENCE}ruby\nThis is a test\n{FENCE}\n"));
    let complex = dedent(&format!(
        "\n{FENCE}ruby this is a test\nThis is a test\n{FENCE}\n"
    ));
    let empty = dedent(&format!("\n{FENCE}\nThis is a test\n{FENCE}\n"));

    let fence = |source: &str| {
        let document = parse(source);
        let node = at(&document, &[0]);
        (
            attribute(node, "language"),
            attribute(node, "content"),
            node.node_type,
        )
    };
    assert_eq!(
        fence(&simple),
        (
            "\"ruby\"".to_string(),
            "\"This is a test\\n\"".to_string(),
            NodeType::Fence
        )
    );
    assert_eq!(
        fence(&complex),
        (
            "\"ruby\"".to_string(),
            "\"This is a test\\n\"".to_string(),
            NodeType::Fence
        )
    );
    assert_eq!(
        fence(&empty),
        (
            "<unset>".to_string(),
            "\"This is a test\\n\"".to_string(),
            NodeType::Fence
        )
    );
}

#[test]
fn attributes_for_image_with_no_title() {
    let source = dedent("![foo](/url)");
    let document = parse(&source);
    let image = at(&document, &[0, 0, 0]);
    assert_eq!(image.node_type, NodeType::Image);
    assert_eq!(attribute(image, "title"), "<unset>");
    assert_eq!(attribute(image, "src"), "\"/url\"");
    assert_eq!(attribute(image, "alt"), "\"foo\"");
}

#[test]
fn attributes_for_image_with_a_title() {
    let source = dedent("![foo](/url \"title\")");
    let document = parse(&source);
    let image = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(image, "title"), "\"title\"");
    assert_eq!(attribute(image, "src"), "\"/url\"");
    assert_eq!(attribute(image, "alt"), "\"foo\"");
}

#[test]
fn attributes_for_table_with_alignments() {
    let source = dedent(
        "\n| Left | Center | Right |\n| :--- | :----: | ----: |\n| Left | Center | Right |\n",
    );
    let document = parse(&source);
    let table = at(&document, &[0]);
    let head_row = at(table, &[0, 0]);
    let body_row = at(table, &[1, 0]);
    for row in [head_row, body_row] {
        assert_eq!(attribute(at(row, &[0]), "align"), "\"left\"");
        assert_eq!(attribute(at(row, &[1]), "align"), "\"center\"");
        assert_eq!(attribute(at(row, &[2]), "align"), "\"right\"");
    }
}

// ---- structure ----------------------------------------------------------

#[test]
fn handling_a_header() {
    let source = dedent("\n# Sample Heading\n\nThis is a sample paragraph\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  heading level=1
    inline
      text content=\"Sample Heading\"
  paragraph
    inline
      text content=\"This is a sample paragraph\"
"
    );
}

#[test]
fn handling_an_image() {
    let source = dedent("![Alt](/logo.png)");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      image alt=\"Alt\" src=\"/logo.png\"
"
    );
}

#[test]
fn handling_lists_with_bullets() {
    let source = dedent("\n* foo\n* bar\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  list ordered=false marker=\"*\"
    item
      inline
        text content=\"foo\"
    item
      inline
        text content=\"bar\"
"
    );
}

#[test]
fn handling_lists_with_numbers() {
    let source = dedent("\n1. foo\n2. bar\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  list ordered=true marker=\".\"
    item
      inline
        text content=\"foo\"
    item
      inline
        text content=\"bar\"
"
    );
}

#[test]
fn handling_fenced_code_with_a_language() {
    let source = dedent(&format!("\n{FENCE}ruby\nputs \"foo\"\n{FENCE}\n"));
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  fence content=\"puts \\\"foo\\\"\\n\" language=\"ruby\"
"
    );
}

#[test]
fn handling_fenced_code_with_an_annotation() {
    let source = dedent(&format!("\n{FENCE}ruby {{% #foo .bar %}}\ntest\n{FENCE}\n"));
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  fence content=\"test\\n\" language=\"ruby\" id=\"foo\" class={bar: true}
"
    );
}

// ---- tags ---------------------------------------------------------------

#[test]
fn tags_at_block_level_with_a_class() {
    let source =
        dedent("\n{% callout .foo .bar %}\n### Heading\n\nThis is a paragraph\n{% /callout %}\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  tag[callout] class={foo: true, bar: true}
    heading level=3
      inline
        text content=\"Heading\"
    paragraph
      inline
        text content=\"This is a paragraph\"
"
    );
}

#[test]
fn tags_at_block_level_with_nesting() {
    let source =
        dedent("\n{% callout %}\n{% callout %}\nThis is a test\n{% /callout %}\n{% /callout %}\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  tag[callout]
    tag[callout]
      paragraph
        inline
          text content=\"This is a test\"
"
    );
}

// ---- annotations --------------------------------------------------------

#[test]
fn annotations_in_a_header_with_an_id() {
    let source = dedent("# Sample Heading {% #foo-bar %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  heading level=1 id=\"foo-bar\"
    inline
      text content=\"Sample Heading \"
"
    );
}

#[test]
fn annotations_in_a_header_with_a_class() {
    let source = dedent("# Sample Heading {% .foo-bar .test %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  heading level=1 class={foo-bar: true, test: true}
    inline
      text content=\"Sample Heading \"
"
    );
}

#[test]
fn annotations_in_a_header_with_complex_values() {
    let source = dedent("# Sample Heading {% #asdf .foo-bar .test foo=\"bar\" %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  heading level=1 id=\"asdf\" class={foo-bar: true, test: true} foo=\"bar\"
    inline
      text content=\"Sample Heading \"
"
    );
}

// ---- variables ----------------------------------------------------------

#[test]
fn variables_by_itself_on_a_line() {
    let source = dedent("{% $test %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=$test
"
    );
}

#[test]
fn variables_in_an_inline_text_node() {
    let source = dedent("This is a test: {% $test %}");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"This is a test: \"
      text content=$test
"
    );
}

/// Ported together with upstream's `ast/variable.test.ts`, which asserts the
/// same path shape by constructing a `Variable` directly.
///
/// Its third case, resolution against a config, is not ported here: `resolve`
/// reads a `Config`, which belongs to the transform stage, and Goal A left it
/// out of the value lattice for that reason.
#[test]
fn variables_with_nested_property_access() {
    let source = dedent("{% $bar.baz[1].test %}");
    let document = parse(&source);
    let text = at(&document, &[0, 0, 0]);
    let Some(Value::Variable(variable)) = text.get("content") else {
        panic!("expected a variable, got {:?}", text.get("content"));
    };
    assert_eq!(variable.path.len(), 4);
    assert_eq!(attribute(text, "content"), "$bar.baz[1].test");
}

#[test]
fn a_variable_with_a_string_index() {
    let source = dedent("{% $foo[\"this is a test\"] %}");
    let document = parse(&source);
    assert_eq!(
        attribute(at(&document, &[0, 0, 0]), "content"),
        "$foo.this is a test"
    );
}

// ---- robustness ---------------------------------------------------------

#[test]
fn parsing_nested_tags_with_indentation_should_not_throw() {
    // Upstream writes this one without its `convert` dedent, because the
    // indentation is the point.
    let document =
        parse("{% tag1 %}\n      {% tag2 %}\n          Contents\n      {% /tag2 %}\n{% /tag1 %}\n");
    assert_eq!(document.node_type, NodeType::Document);
}

#[test]
fn parsing_comments() {
    let source = dedent("\nthis is a test\n\n<!-- foo -->\n");
    let document = convert(&source, &ParseOptions::new().allow_comments(true));
    assert_eq!(
        outline(&document),
        "\
document
  paragraph
    inline
      text content=\"this is a test\"
  comment content=\"foo\"
"
    );
}

// ---- attribute errors ---------------------------------------------------

#[test]
fn error_for_duplicate_attributes() {
    let source = dedent("{% foo bar=1 bar=2 bar=3 bar=4 /%}");
    let document = parse(&source);
    assert_eq!(
        error_ids(at(&document, &[0])),
        [
            "duplicate-attribute",
            "duplicate-attribute",
            "duplicate-attribute"
        ]
    );
}

#[test]
fn error_for_duplicate_ids() {
    let source = dedent("{% foo #bar #baz #qux /%}");
    let document = parse(&source);
    assert_eq!(
        error_ids(at(&document, &[0])),
        ["duplicate-attribute", "duplicate-attribute"]
    );
}

#[test]
fn error_with_annotation_values() {
    let source = dedent("testing {% foo=1 foo=2 %}");
    let document = parse(&source);
    let paragraph = at(&document, &[0]);
    assert_eq!(paragraph.node_type, NodeType::Paragraph);
    assert_eq!(error_ids(paragraph), ["duplicate-attribute"]);
}

#[test]
fn error_across_annotations_on_the_same_node() {
    let source = dedent("testing {% foo=1 %} another test {% foo=1 %}");
    let document = parse(&source);
    assert_eq!(error_ids(at(&document, &[0])), ["duplicate-attribute"]);
}

#[test]
fn no_error_for_multiple_classes() {
    let source = dedent("{% foo .bar .baz .qux /%}");
    let document = parse(&source);
    assert!(error_ids(at(&document, &[0])).is_empty());
}

/// Upstream writes this fence without an annotation, because its fences process
/// tags by default. `DIVERGENCES.md` entry 1 inverts that, so the port opts in
/// -- which is the same test of the same rule, with the opt-in made explicit.
#[test]
fn displays_error_for_annotations_in_a_fence() {
    let source = dedent("\n~~~ {% process=true %}\ntest\n{% #foo %}\ntest\n~~~\n");
    let document = parse(&source);
    let fence = at(&document, &[0]);
    assert_eq!(fence.node_type, NodeType::Fence);
    assert!(fence.annotations.len() == 1, "only `process` is annotated");
    assert_eq!(error_ids(fence), ["no-inline-annotations"]);
}

// ---- inline identification ----------------------------------------------

#[test]
fn correctly_identifies_inlines() {
    let source = dedent(
        "\n# This is a test\n\n{% foo %}\nAnother {% bar %}test{% /bar %} test\n{% /foo %}\n\n* bar\n",
    );
    let document = parse(&source);

    let flags: Vec<(String, bool)> = std::iter::once(&document)
        .chain(document.walk())
        .map(|node| (node.name().to_string(), node.inline))
        .collect();

    assert_eq!(
        flags,
        [
            ("document".to_string(), false),
            ("heading".to_string(), false),
            ("inline".to_string(), false),
            ("text".to_string(), true),
            ("foo".to_string(), false),
            ("paragraph".to_string(), false),
            ("inline".to_string(), false),
            ("text".to_string(), true),
            ("bar".to_string(), true),
            ("text".to_string(), true),
            ("text".to_string(), true),
            ("list".to_string(), false),
            ("item".to_string(), false),
            ("inline".to_string(), false),
            ("text".to_string(), true),
        ]
    );
}

// ---- structural errors --------------------------------------------------

#[test]
fn with_unmatched_closing_tag() {
    let source = dedent("\n{% foo %}\nTest\n{% /bar %}\n");
    let document = parse(&source);
    assert_eq!(error_ids(at(&document, &[0]))[0], "missing-closing");
}

#[test]
fn missing_opening() {
    let source = dedent("\nThis a test\n{% /foo %}\n");
    let document = parse(&source);
    assert_eq!(error_ids(at(&document, &[1]))[0], "missing-opening");
}

#[test]
fn with_missing_closing_tag() {
    let source = dedent("\n{% foo %}\nTest\n");
    let document = parse(&source);
    assert_eq!(error_ids(at(&document, &[0]))[0], "missing-closing");
}

#[test]
fn a_tag_that_does_not_parse_becomes_an_error_node() {
    let source = dedent("{% test foo={,} /%}");
    let document = parse(&source);
    let node = at(&document, &[0]);
    assert_eq!(node.node_type, NodeType::Error);
    assert_eq!(error_ids(node), ["parse-error"]);
    assert_eq!(
        node.errors[0].message,
        "Expected \"}\", identifier, string, or whitespace but \",\" found."
    );
    // The grammar reports offsets into the tag body; the parser translates them
    // into the document, which is what makes an editor able to underline it.
    let location = node.errors[0].location.expect("a location");
    assert_eq!(
        location.start.offset, 13,
        "the offset of the `,` in the document"
    );
    assert_eq!(location.text, ",");
}

#[test]
fn an_unclosed_tag_is_ordinary_text() {
    let source = dedent("hello {% world");
    let document = parse(&source);
    assert!(all_error_ids(&document).is_empty());
    assert_eq!(
        attribute(at(&document, &[0, 0, 0]), "content"),
        "\"hello {% world\""
    );
}

// Upstream's `table parsing` block. It asserts which tags survive the table
// rewrite that runs at the end of the parse, so it belongs here rather than
// with the rewrite's own suite: what it is really testing is that
// `conditionalTags` reaches the pass from the parser's arguments.

/// Upstream's `setupTableDoc`.
fn table_document(rows: &[&str]) -> String {
    dedent(&format!(
        "{{% table %}}\n- column 1\n- column 2\n---\n{}\n{{% /table %}}\n",
        rows.join("\n")
    ))
}

/// Every tag name in a document, in walk order.
fn tag_names(document: &proust::ast::Node<'_>) -> Vec<String> {
    document
        .walk()
        .filter(|node| node.node_type == NodeType::Tag)
        .filter_map(|node| node.tag.clone())
        .collect()
}

#[test]
fn should_preserve_default_if_tag_during_table_parsing_without_extra_parser_args() {
    let source = table_document(&[
        "{% if $fakeCondition.condition1 %}\n- cell 1\n- cell 2\n{% else /%}\n- cell 3\n- cell 4\n{% /if %}",
    ]);
    let names = tag_names(&parse(&source));
    assert!(names.contains(&"if".to_string()), "{names:?}");
}

#[test]
fn should_not_preserve_unregistered_tags_during_table_parsing_without_extra_parser_args() {
    let source = table_document(&[
        "{% if-pref conditions=[{platform: \"web\"}] %}\n- cell 1\n- cell 2\n{% /if-pref %}\n\
         {% if-pref conditions=[{platform: \"ios\"}] %}\n- cell 1\n- cell 2\n{% /if-pref %}",
    ]);
    let names = tag_names(&parse(&source));
    assert!(!names.contains(&"if-pref".to_string()), "{names:?}");
}

#[test]
fn should_preserve_registered_tags_and_ignore_unregistered_ones_with_extra_parser_args() {
    let source = table_document(&[
        "{% if-pref conditions=[{platform: \"web\"}] %}\n- cell 1\n- cell 2\n{% /if-pref %}\n\
         {% if-pref conditions=[{platform: \"ios\"}] %}\n- cell 1\n- cell 2\n{% /if-pref %}",
        "{% if $fakeCondition.condition1 %}\n- cell 1\n- cell 2\n{% else /%}\n- cell 3\n- cell 4\n{% /if %}",
        "{% unregistered-if-tag $fakeCondition.condition2 %}\n- cell 1\n- cell 2\n{% /unregistered-if-tag %}",
    ]);
    let options =
        ParseOptions::new().conditional_tags(vec!["if".to_string(), "if-pref".to_string()]);
    let names = tag_names(&convert(&source, &options));
    assert!(names.contains(&"if".to_string()), "{names:?}");
    assert!(names.contains(&"if-pref".to_string()), "{names:?}");
    assert!(
        !names.contains(&"unregistered-if-tag".to_string()),
        "{names:?}"
    );
}
