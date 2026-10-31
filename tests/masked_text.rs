//! The tokenizer reads a masked copy of the document; the tree never does.
//!
//! The segmenter overwrites every tag's body with `x` before the CommonMark
//! parse, and a tag inside a container gets `<?` and `?>` for delimiters as
//! well. Each test here is a shape where that filler used to reach the tree --
//! a tag across a line break, a tag in a quote, a code block or a link
//! destination -- or where a tag inside a container used to leave it.
//!
//! `parse_proptests.rs` states the same property over generated documents.

mod support;

use accent_proust::ast::NodeType;
use accent_proust::parse::parse;
use support::{at, attribute, error_ids, outline};

// ---- a tag across a line break -------------------------------------------

#[test]
fn an_inline_tag_across_a_line_break_is_a_tag() {
    let source = "foo {% test #foo\n  baz=1 %} qux {% /test %} end\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo \"
      tag[test] id=\"foo\" baz=1
        text content=\" qux \"
      text content=\" end\"
"
    );
}

#[test]
fn an_inline_tag_across_quoted_lines_reads_past_the_markers() {
    let source = "> foo {% test #foo\n> baz=1 %} qux {% /test %}\n";
    let document = parse(source);
    let tag = at(&document, &[0, 0, 0, 1]);
    assert_eq!(tag.tag.as_deref(), Some("test"));
    assert_eq!(attribute(tag, "baz"), "1");
}

#[test]
fn a_tag_across_a_paragraph_break_is_text() {
    let source = "foo {% test\n\nbar %} baz\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo {% test\"
  paragraph
    inline
      text content=\"bar %} baz\"
"
    );
}

#[test]
fn a_tag_that_runs_out_of_its_heading_is_text() {
    let source = "# foo {% test\nbar %} baz\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  heading level=1
    inline
      text content=\"foo {% test\"
  paragraph
    inline
      text content=\"bar %} baz\"
"
    );
}

// ---- block tags inside containers ----------------------------------------

#[test]
fn a_block_tag_in_a_quote_is_inside_the_quote() {
    let source = "> foo\n> {% test /%}\n> bar\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  blockquote
    paragraph
      inline
        text content=\"foo\"
    tag[test]
    paragraph
      inline
        text content=\"bar\"
"
    );
}

#[test]
fn a_block_tag_indented_under_an_item_is_inside_the_item() {
    let source = "* Some content\n\n  {% if $foo %}\n  Conditional block\n  {% /if %}\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  list ordered=false marker=\"*\"
    item
      paragraph
        inline
          text content=\"Some content\"
      tag[if] primary=$foo
        paragraph
          inline
            text content=\"Conditional block\"
"
    );
}

#[test]
fn a_block_tag_at_the_margin_ends_the_quote() {
    let source = "> foo\n{% test /%}\n";
    let document = parse(source);
    assert_eq!(at(&document, &[0]).node_type, NodeType::Blockquote);
    assert_eq!(at(&document, &[1]).tag.as_deref(), Some("test"));
}

#[test]
fn a_tag_left_open_closes_with_its_container() {
    let source = "> {% test %}\n> bar\n{% /test %}\n";
    let document = parse(source);
    let open = at(&document, &[0, 0]);
    assert_eq!(open.tag.as_deref(), Some("test"));
    assert_eq!(error_ids(open), ["missing-closing"]);
    let close = at(&document, &[1]);
    assert_eq!(error_ids(close), ["missing-opening"]);
}

#[test]
fn an_indented_tag_line_splits_its_paragraph() {
    let source = "foo\n    {% test /%}\nbar\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo\"
  tag[test]
  paragraph
    inline
      text content=\"bar\"
"
    );
}

#[test]
fn an_indented_tag_line_in_a_tight_item_stays_in_the_item() {
    let source = "- foo\n      {% x /%}\n  bar\n\nafter\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  list ordered=false marker=\"-\"
    item
      inline
        text content=\"foo\"
      tag[x]
      inline
        text content=\"bar\"
  paragraph
    inline
      text content=\"after\"
"
    );
}

#[test]
fn a_hard_break_does_not_end_a_split_paragraph() {
    for (source, before) in [
        ("foo  \n    {% x /%}\nbar\n", "foo"),
        ("foo\\\n    {% x /%}\nbar\n", "foo\\\\"),
    ] {
        assert_eq!(
            outline(&parse(source)),
            format!(
                "\
document
  paragraph
    inline
      text content=\"{before}\"
  tag[x]
  paragraph
    inline
      text content=\"bar\"
"
            ),
            "{source:?}"
        );
    }
}

#[test]
fn a_hard_break_after_a_split_tag_is_dropped() {
    let source = "foo\n    {% x /%}  \nbar\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo\"
  tag[x]
  paragraph
    inline
      text content=\"bar\"
"
    );
}

#[test]
fn a_hard_break_can_still_open_a_paragraph() {
    let source = "{% x /%}\n\\\nbar\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  tag[x]
  paragraph
    inline
      hardbreak
      text content=\"bar\"
"
    );
}

#[test]
fn a_marker_read_as_text_keeps_the_tag_inline() {
    // Four columns in, `>` cannot open a quote, so it is text before the tag.
    let source = "foo\n    > {% x /%}\nbar\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo\"
      softbreak
      text content=\"> \"
      tag[x]
      softbreak
      text content=\"bar\"
"
    );
}

#[test]
fn an_emphasis_across_the_tag_line_keeps_the_tag_inline() {
    let source = "*foo\n    {% x /%}\nbar*\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      em marker=\"*\"
        text content=\"foo\"
        softbreak
        tag[x]
        softbreak
        text content=\"bar\"
"
    );
}

#[test]
fn an_indented_tag_line_in_a_setext_heading_stays_inline() {
    // The tokenizer ends a setext heading, not a paragraph, so there is no
    // paragraph end to resume from after a split.
    let source = "foo\n    {% x /%}\n---\n\nafter\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo\"
      softbreak
      tag[x]
  hr
  paragraph
    inline
      text content=\"after\"
"
    );
}

#[test]
fn a_tag_line_in_indented_code_splits_the_code() {
    let source = "    one\n    {% test /%}\n    two\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  fence content=\"one\\n\"
  tag[test]
  fence content=\"two\\n\"
"
    );
}

// ---- code ----------------------------------------------------------------

#[test]
fn a_fence_in_a_container_keeps_its_content() {
    for source in [
        "> ```\n> {% test %}\n> ```\n",
        "- ```\n  {% test %}\n  ```\n",
    ] {
        let document = parse(source);
        let fence = document
            .walk()
            .find(|node| node.node_type == NodeType::Fence)
            .expect("a fence");
        assert_eq!(
            attribute(fence, "content"),
            "\"{% test %}\\n\"",
            "{source:?}"
        );
    }
}

#[test]
fn a_fence_annotation_in_a_quote_is_read() {
    let source = "> ```js {% #id %}\n> x\n> ```\n";
    let document = parse(source);
    let fence = at(&document, &[0, 0]);
    assert_eq!(attribute(fence, "language"), "\"js\"");
    assert_eq!(attribute(fence, "id"), "\"id\"");
}

// ---- links ---------------------------------------------------------------

#[test]
fn a_link_keeps_the_tags_in_its_destination_and_title() {
    let source = "[a](/x{%$y%}z \"t{% $y %}\") ![b](/i{%$y%}.png)\n";
    let document = parse(source);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(link, "href"), "\"/x{%$y%}z\"");
    assert_eq!(attribute(link, "title"), "\"t{% $y %}\"");
    let image = at(&document, &[0, 0, 2]);
    assert_eq!(attribute(image, "src"), "\"/i{%$y%}.png\"");
}

#[test]
fn a_tag_in_link_text_is_not_taken_for_one_in_the_destination() {
    let source = "[{% a %}]({%/%}{% b %}2)\n";
    let document = parse(source);
    let link = document
        .walk()
        .find(|node| node.node_type == NodeType::Link)
        .expect("a link");
    assert_eq!(attribute(link, "href"), "\"{%/%}{% b %}2\"");
}

#[test]
fn a_title_gets_back_its_own_tag_when_two_mask_alike() {
    let source = "[![a](s{% $b %} \"p {% $d %}\")](h{% $c %} \"t {% $e %}\")\n";
    let document = parse(source);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(link, "href"), "\"h{% $c %}\"");
    assert_eq!(attribute(link, "title"), "\"t {% $e %}\"");
    let image = at(&document, &[0, 0, 0, 0]);
    assert_eq!(attribute(image, "src"), "\"s{% $b %}\"");
    assert_eq!(attribute(image, "title"), "\"p {% $d %}\"");
}

#[test]
fn a_title_across_container_lines_gets_its_tag_back() {
    for source in [
        "[a](u \"t {% foo\n   b=1 /%}\")\n",
        "> [a](u \"t {% foo\n> b=1 /%}\")\n",
        "- [a](u \"t {% foo\n  b=1 /%}\")\n",
    ] {
        let document = parse(source);
        let link = document
            .walk()
            .find(|node| node.node_type == NodeType::Link)
            .expect("a link");
        assert_eq!(
            attribute(link, "title"),
            "\"t {% foo\\nb=1 /%}\"",
            "{source:?}"
        );
    }
}

#[test]
fn an_autolink_keeps_its_tags() {
    let source = "<https://a.b/{%$y%}>\n";
    let document = parse(source);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(link, "href"), "\"https://a.b/{%$y%}\"");
}

// ---- HTML ----------------------------------------------------------------

#[test]
fn an_indented_html_block_leaves_no_stray_text() {
    let source = "  <div>\n\nfoo\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"<div>\"
  paragraph
    inline
      text content=\"foo\"
"
    );
}
