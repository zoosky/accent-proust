//! The tokenizer reads a masked copy of the document; the tree never does.
//!
//! The segmenter overwrites every tag's body with `x` before the CommonMark
//! parse, all but its spaces and tabs, and a tag inside a container gets `<?`
//! and `?>` for delimiters as well. Each test here is a shape where that filler used to reach the tree --
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
fn an_indented_tag_line_over_an_underline_splits_the_paragraph() {
    // The underline is undone before the parse, so this is a paragraph like
    // any other, split at the tag as `DIVERGENCES.md` entry 13 has it.
    let source = "foo\n    {% x /%}\n---\n\nafter\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"foo\"
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
    let source = "[{% a %}]({%/%}{%b%}2)\n";
    let document = parse(source);
    let link = document
        .walk()
        .find(|node| node.node_type == NodeType::Link)
        .expect("a link");
    assert_eq!(attribute(link, "href"), "\"{%/%}{%b%}2\"");
}

#[test]
fn a_title_gets_back_its_own_tag_when_two_mask_alike() {
    let source = "[![a](s{%$b%} \"p {% $d %}\")](h{%$c%} \"t {% $e %}\")\n";
    let document = parse(source);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(link, "href"), "\"h{%$c%}\"");
    assert_eq!(attribute(link, "title"), "\"t {% $e %}\"");
    let image = at(&document, &[0, 0, 0, 0]);
    assert_eq!(attribute(image, "src"), "\"s{%$b%}\"");
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

/// A reference link takes its destination and title from a definition
/// elsewhere in the document, and its tags come back from there (#79).
#[test]
fn a_reference_link_gets_the_tags_of_its_definition_back() {
    for (source, kind, url) in [
        (
            "[a][r]\n\n[r]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
        (
            "[r][]\n\n[r]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
        (
            "[r]\n\n[r]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
        (
            "![i][r]\n\n[r]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Image,
            "src",
        ),
        (
            "[r]: /x{%$y%}z\n  \"t{% $z %}\"\n\n[a][r]\n",
            NodeType::Link,
            "href",
        ),
        (
            "[a][R]\n\n[r]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
        (
            "[a][r  s]\n\n[r s]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
        (
            "> [a][r]\n>\n> [r]: /x{%$y%}z\n> \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
        (
            "[a](<\n\n[a]: /x{%$y%}z \"t{% $z %}\"\n",
            NodeType::Link,
            "href",
        ),
    ] {
        let document = parse(source);
        let node = document
            .walk()
            .find(|node| node.node_type == kind)
            .unwrap_or_else(|| panic!("{source:?}:\n{}", outline(&document)));
        assert_eq!(attribute(node, url), "\"/x{%$y%}z\"", "{source:?}");
        assert_eq!(attribute(node, "title"), "\"t{% $z %}\"", "{source:?}");
    }
}

#[test]
fn a_tag_in_a_definitions_label_is_not_its_destinations() {
    for (source, name, expected) in [
        ("[a{%$p%}]\n\n[a{%$p%}]: /y{%$q%}\n", "href", "\"/y{%$q%}\""),
        (
            "[a{%$p%}][]\n\n[a{%$p%}]: /y \"{%$q%}\"\n",
            "title",
            "\"{%$q%}\"",
        ),
        (
            "[a][b\\]{%$p%}]\n\n[b\\]{%$p%}]: /y{%$q%}\n",
            "href",
            "\"/y{%$q%}\"",
        ),
    ] {
        let document = parse(source);
        let link = document
            .walk()
            .find(|node| node.node_type == NodeType::Link)
            .unwrap_or_else(|| panic!("{source:?}:\n{}", outline(&document)));
        assert_eq!(attribute(link, name), expected, "{source:?}");
    }
}

#[test]
fn two_definitions_that_mask_alike_keep_their_own_tags() {
    let source = "[a][p] [b][q]\n\n[p]: /{%$a%}\n[q]: /{%$b%}\n";
    let document = parse(source);
    let links: Vec<_> = document
        .walk()
        .filter(|node| node.node_type == NodeType::Link)
        .map(|node| attribute(node, "href"))
        .collect();
    assert_eq!(links, ["\"/{%$a%}\"", "\"/{%$b%}\""]);
}

/// A destination cannot hold whitespace, and a tag's spaces count (#80).
#[test]
fn a_tag_with_spaces_in_a_destination_makes_no_link() {
    for source in [
        "[a](/x{% $y %}z)\n",
        "![a](/x{% $y %}z)\n",
        "<https://a.b/{% $y %}>\n",
        "[r]\n\n[r]: /x{% $y %}z\n",
        "[a](/x{%\t$y%}z)\n",
    ] {
        let document = parse(source);
        assert!(
            !document
                .walk()
                .any(|node| matches!(node.node_type, NodeType::Link | NodeType::Image)),
            "{source:?}:\n{}",
            outline(&document)
        );
        assert!(
            document.walk().any(|node| matches!(
                node.get("content"),
                Some(accent_proust::ast::Value::Variable(_))
            )),
            "{source:?}: the tag is a variable in the text:\n{}",
            outline(&document)
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

/// Upstream reads the autolink before it looks for tags, so the tag in it is
/// literal text, in its text as in its destination.
#[test]
fn an_autolinks_text_keeps_its_tags_as_text() {
    for (source, text) in [
        ("<https://a.b/{%$y%}>\n", "https://a.b/{%$y%}"),
        ("<{%$y%}@b.c>\n", "{%$y%}@b.c"),
    ] {
        let document = parse(source);
        let link = at(&document, &[0, 0, 0]);
        assert_eq!(
            link.children.len(),
            1,
            "{source:?}:\n{}",
            outline(&document)
        );
        assert_eq!(
            attribute(at(link, &[0]), "content"),
            format!("{text:?}"),
            "{source:?}"
        );
    }
}

/// An email autolink's destination is `mailto:` and the address (#85).
#[test]
fn an_email_autolinks_destination_is_a_mailto_url() {
    let document = parse("<{%$y%}@b.c>\n");
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(attribute(link, "href"), "\"mailto:{%$y%}@b.c\"");
}

// ---- HTML ----------------------------------------------------------------

/// Upstream runs markdown-it with `html: false`, so raw inline HTML is text
/// like any other (#89): a tag in it is a tag, an escape and an entity are
/// read, a line break is a soft break and a `*` can open emphasis. Every
/// expected tree is upstream 0.5.12's.
#[test]
fn inline_html_is_text_with_tags_in_it() {
    for (source, expected) in [
        (
            "<a href=\"{% $y %}\">x</a>",
            "inline\n  text content=\"<a href=\\\"\"\n  text content=$y\n  text content=\"\\\">x</a>\"\n",
        ),
        (
            "<a title=\"&amp;\\*\">x</a>",
            "inline\n  text content=\"<a title=\\\"&*\\\">x</a>\"\n",
        ),
        (
            "<a\nhref=\"{% $y %}\">x</a>",
            "inline\n  text content=\"<a\"\n  softbreak\n  text content=\"href=\\\"\"\n  text content=$y\n  text content=\"\\\">x</a>\"\n",
        ),
        (
            "<b title=\"*\">a*",
            "inline\n  text content=\"<b title=\\\"\"\n  em marker=\"*\"\n    text content=\"\\\">a\"\n",
        ),
    ] {
        let document = parse(source);
        assert_eq!(outline(at(&document, &[0, 0])), expected, "{source:?}");
    }
}

/// Emphasis next to inline HTML flanks as it does beside a `<`, and a
/// reference label holding markup still finds its definition. Upstream's
/// trees, checked by kind.
#[test]
fn inline_html_read_as_text_keeps_flanking_and_labels() {
    for (source, kinds) in [
        ("foo*<b>bar*", vec![NodeType::Text]),
        ("_(x)_<b>", vec![NodeType::Em, NodeType::Text]),
        ("[<b>]\n\n[<b>]: /u", vec![NodeType::Link]),
        ("[x][<b>]\n\n[<b>]: /u", vec![NodeType::Link]),
    ] {
        let document = parse(source);
        let found: Vec<NodeType> = at(&document, &[0, 0])
            .children
            .iter()
            .map(|child| child.node_type)
            .collect();
        assert_eq!(found, kinds, "{source:?}:\n{}", outline(&document));
    }
}

/// The character written over a `<` is never mistaken for one the author
/// wrote or a reference decoded to, and a run that holds every candidate
/// still reads its markup as text.
#[test]
fn the_sentinel_for_a_bracket_is_never_the_authors() {
    let document = parse("<b>x &#94; y ^");
    assert_eq!(
        attribute(at(&document, &[0, 0, 0]), "content"),
        "\"<b>x ^ y ^\""
    );
    let document = parse("a<b>c [d](&#94;)");
    assert_eq!(attribute(at(&document, &[0, 0, 1]), "href"), "\"^\"");
    let source = "<a href=\"{% $y %}\">x</a> ^ $ @ ' \" , : and `<i>`";
    let document = parse(source);
    let inline = at(&document, &[0, 0]);
    assert!(
        inline.children.iter().any(|child| matches!(
            child.get("content"),
            Some(accent_proust::ast::Value::Variable(_))
        )),
        "{}",
        outline(&document)
    );
    assert_eq!(
        attribute(inline.children.last().unwrap_or(inline), "content"),
        "\"<i>\""
    );
}

/// A comment kept as a node stays one inside a label that also holds markup.
#[test]
fn a_kept_comment_in_a_reference_label_stays_a_comment() {
    use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

    let source = "[<!-- c --> <b>]\n\n[<!-- c --> <b>]: /u";
    let options = ParseOptions::new().allow_comments(true);
    let document = parse_with(source, &PulldownTokenizer::new(), &options);
    let link = at(&document, &[0, 0, 0]);
    assert_eq!(link.node_type, NodeType::Link, "{}", outline(&document));
    assert_eq!(
        at(link, &[0]).node_type,
        NodeType::Comment,
        "{}",
        outline(&document)
    );
}

/// A whole comment inside other markup is a comment when comments are nodes,
/// as upstream's inline comment rule finds one anywhere.
#[test]
fn a_comment_inside_markup_is_a_comment_when_comments_are_nodes() {
    use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

    let options = ParseOptions::new().allow_comments(true);
    let document = parse_with(
        "<a title=\"<!-- c -->\"> x",
        &PulldownTokenizer::new(),
        &options,
    );
    let kinds: Vec<NodeType> = at(&document, &[0, 0])
        .children
        .iter()
        .map(|child| child.node_type)
        .collect();
    assert_eq!(
        kinds,
        [NodeType::Text, NodeType::Comment, NodeType::Text],
        "{}",
        outline(&document)
    );
}

/// Neither references nor kept comments make a run's reading quadratic.
#[test]
fn many_references_and_comments_are_read_in_linear_time() {
    use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

    let started = std::time::Instant::now();
    let _ = parse(&format!("<b> {}", "&amp;".repeat(40_000)));
    let options = ParseOptions::new().allow_comments(true);
    let source = format!(
        "x {}{}",
        "<!-- c --> ".repeat(10_000),
        "<b> ".repeat(10_000)
    );
    let _ = parse_with(&source, &PulldownTokenizer::new(), &options);
    // Comments that never close, inside one span of markup.
    let unclosed = format!("<a {}>", "<!-- ".repeat(10_000));
    let _ = parse_with(&unclosed, &PulldownTokenizer::new(), &options);
    assert!(
        started.elapsed().as_secs() < 5,
        "took {:?}",
        started.elapsed()
    );
}

/// A long comment block is gathered in one pass over its lines.
#[test]
fn a_long_comment_block_is_read_in_linear_time() {
    use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

    let options = ParseOptions::new().allow_comments(true);
    let source = format!("<!--\n{}-->\n", "x\n".repeat(20_000));
    let started = std::time::Instant::now();
    let document = parse_with(&source, &PulldownTokenizer::new(), &options);
    assert!(
        started.elapsed().as_secs() < 5,
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(at(&document, &[0]).node_type, NodeType::Comment);
}

/// Markup nested in markup is undone in one pass, not one per level.
#[test]
fn nested_inline_html_is_read_in_linear_time() {
    let source = format!("{}-->", "x <!-- ".repeat(20_000));
    let started = std::time::Instant::now();
    let document = parse(&source);
    assert!(
        started.elapsed().as_secs() < 5,
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(document.children.len(), 1);
}

/// A comment stays a comment when comments are nodes, and is text otherwise.
#[test]
fn an_inline_comment_is_a_node_only_when_comments_are() {
    use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

    let source = "x <!-- c --> y";
    let options = ParseOptions::new().allow_comments(true);
    let document = parse_with(source, &PulldownTokenizer::new(), &options);
    assert_eq!(at(&document, &[0, 0, 1]).node_type, NodeType::Comment);
    let document = parse(source);
    assert_eq!(
        outline(at(&document, &[0, 0])),
        "inline\n  text content=\"x <!-- c --> y\"\n"
    );
}

/// An HTML block is markdown, as markdown-it with `html: false` reads it
/// (#98): its links, entities, escapes, emphasis and tags all work. Every
/// expected tree is upstream 0.5.12's.
#[test]
fn an_html_block_is_read_as_markdown() {
    for (source, expected) in [
        (
            "<!-- c -->[l](/u)",
            "inline\n  text content=\"<!-- c -->\"\n  link href=\"/u\"\n    text content=\"l\"\n",
        ),
        (
            "<span>&amp; {% $y %}</span>",
            "inline\n  text content=\"<span>& \"\n  text content=$y\n  text content=\"</span>\"\n",
        ),
        (
            "<div>\n*a*",
            "inline\n  text content=\"<div>\"\n  softbreak\n  em marker=\"*\"\n    text content=\"a\"\n",
        ),
    ] {
        let document = parse(source);
        assert_eq!(outline(at(&document, &[0, 0])), expected, "{source:?}");
    }
    let document = parse("> <div>\n> [l](/u)");
    assert_eq!(
        at(&document, &[0, 0, 0, 2]).node_type,
        NodeType::Link,
        "{}",
        outline(&document)
    );
}

/// With comments as nodes, a comment block is upstream's: it starts at any
/// line opening with `<!--`, inside a container too, ends at the line with
/// its `-->`, drops what follows that on its line, and is a block of its own.
/// Every expected tree is upstream 0.5.12's, with `allowComments`.
#[test]
fn a_comment_block_is_upstreams_when_comments_are_nodes() {
    use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

    let options = ParseOptions::new().allow_comments(true);
    for (source, expected) in [
        ("<!-- c -->[l](/u)", "document\n  comment content=\"c\"\n"),
        (
            "<div>\n<!-- a\nb -->",
            "document\n  paragraph\n    inline\n      text content=\"<div>\"\n  comment content=\"a\\nb\"\n",
        ),
        (
            "> <!-- a\n> b -->",
            "document\n  blockquote\n    comment content=\"a\\n> b\"\n",
        ),
        (
            "<!-- a -->\n<!-- b -->",
            "document\n  comment content=\"a\"\n  comment content=\"b\"\n",
        ),
        (
            "<!-- a -->\npara",
            "document\n  comment content=\"a\"\n  paragraph\n    inline\n      text content=\"para\"\n",
        ),
        (
            "x\n<!-- a -->",
            "document\n  paragraph\n    inline\n      text content=\"x\"\n  comment content=\"a\"\n",
        ),
        (
            "<!-- a\n\n# H",
            "document\n  paragraph\n    inline\n      text content=\"<!-- a\"\n  heading level=1\n    inline\n      text content=\"H\"\n",
        ),
        (
            "<!-->\n<!-- b -->",
            "document\n  comment content=\"\"\n  comment content=\"b\"\n",
        ),
    ] {
        let document = parse_with(source, &PulldownTokenizer::new(), &options);
        assert_eq!(outline(&document), expected, "{source:?}");
    }
}

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
