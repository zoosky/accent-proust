//! Upstream's `src/tokenizer/plugins/comments.test.ts`, ported.
//!
//! `allowComments` is an ordinary feature, and it is the option upstream's
//! conformance runner switches on alongside the one this crate cannot reach
//! (`spec/marktest/index.ts:21-24`). So it is ported in full, and the harness
//! sets it.
//!
//! # How it is implemented, and why that is not a divergence
//!
//! Upstream adds a block rule and an inline rule to markdown-it that scan for
//! `<!--` and `-->` themselves. This port does not: Markdoc runs markdown-it
//! with `html: false`, so raw HTML is literal text there, and pulldown-cmark
//! recognises an HTML comment for us. A comment therefore arrives as an HTML
//! event and becomes a `comment` node when the option is on, or a text node
//! carrying the markup verbatim when it is off -- which is what upstream
//! produces in both cases.
//!
//! The last of upstream's tests is the one that proves the two agree: a comment
//! whose `-->` is on the far side of a blank line is *not* a comment, in either
//! implementation, because a blank line ends the paragraph before the closer is
//! reached.

mod support;

use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse, parse_with};
use support::{dedent, outline};

fn parse_comments(source: &str) -> accent_proust::ast::Node<'_> {
    parse_with(
        source,
        &PulldownTokenizer::new(),
        &ParseOptions::new().allow_comments(true),
    )
}

// ---- inline comments ----------------------------------------------------

const INLINE: &str = "\
document
  paragraph
    inline
      text content=\"this is a test \"
      comment content=\"example comment\"
      text content=\" foo\"
";

#[test]
fn simple_inline_comment() {
    let source = dedent("\nthis is a test <!-- example comment --> foo\n");
    assert_eq!(outline(&parse_comments(&source)), INLINE);
}

#[test]
fn inline_comment_with_a_newline() {
    let source = dedent("\nthis is a test <!-- \nexample comment\n--> foo\n");
    assert_eq!(outline(&parse_comments(&source)), INLINE);
}

// ---- block comments -----------------------------------------------------

const BLOCK: &str = "\
document
  paragraph
    inline
      text content=\"this is a test\"
  comment content=\"example comment\"
  paragraph
    inline
      text content=\"foo\"
";

#[test]
fn simple_block_comment_after_a_paragraph() {
    let source = dedent("\nthis is a test\n\n<!--\nexample comment\n-->\n\nfoo\n");
    assert_eq!(outline(&parse_comments(&source)), BLOCK);
}

#[test]
fn block_comment_ending_on_the_same_line_as_content() {
    let source = dedent("\nthis is a test\n\n<!--\nexample comment -->\n\nfoo\n");
    assert_eq!(outline(&parse_comments(&source)), BLOCK);
}

#[test]
fn block_comment_on_one_line() {
    let source = dedent("\nthis is a test\n\n<!-- example comment -->\n\nfoo\n");
    assert_eq!(outline(&parse_comments(&source)), BLOCK);
}

/// A blank line inside a comment ends the paragraph before the closer arrives,
/// so neither implementation sees a comment at all.
#[test]
fn block_comment_across_multiple_lines_with_blank_lines() {
    let source = "foo <!-- example\n\ncomment --> bar\n";
    assert_eq!(
        outline(&parse_comments(source)),
        "\
document
  paragraph
    inline
      text content=\"foo <!-- example\"
  paragraph
    inline
      text content=\"comment --> bar\"
"
    );
}

/// The option is off by default, as upstream has it, and off means literal.
///
/// One text node, not three: markdown-it with `html: false` produces the `<` as
/// text and its `fragments_join` rule folds the run back together, which is what
/// this port's own text merging reproduces.
#[test]
fn comments_are_text_when_the_option_is_off() {
    let source = dedent("\nthis is a test <!-- example comment --> foo\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"this is a test <!-- example comment --> foo\"
"
    );
}

// ---- block comments in containers (#103) ----------------------------------

/// Upstream's block comment rule takes everything up to the first `-->`,
/// whatever container the `<!--` is in, and that container goes on after the
/// line holding the `-->` -- however the lines between are indented or
/// quoted. Upstream 0.5.12's trees, `allowComments`.
#[test]
fn a_block_comment_runs_past_the_end_of_its_container() {
    for (source, expected) in [
        // The container ends after the line holding the `-->`, and what follows it there is dropped.
        (
            "1. <!-- a\nb\n--> tail\nafter\n",
            "document\n  list ordered=true marker=\".\"\n    item\n      comment content=\"a\\nb\"\n  paragraph\n    inline\n      text content=\"after\"\n",
        ),
        // The list goes on with the next item.
        (
            "- <!-- a\nb -->\n- second\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      comment content=\"a\\nb\"\n    item\n      inline\n        text content=\"second\"\n",
        ),
        // The quote goes on after it.
        (
            "> <!-- a\nb\n-->\n> c\n",
            "document\n  blockquote\n    comment content=\"a\\nb\"\n    paragraph\n      inline\n        text content=\"c\"\n",
        ),
        // In a nested item.
        (
            "- x\n  - <!-- a\nb -->\n  - y\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      inline\n        text content=\"x\"\n      list ordered=false marker=\"-\"\n        item\n          comment content=\"a\\nb\"\n        item\n          inline\n            text content=\"y\"\n",
        ),
        // Across a blank line.
        (
            "- <!-- a\n\nb -->\n\npara\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      comment content=\"a\\n\\nb\"\n  paragraph\n    inline\n      text content=\"para\"\n",
        ),
        // In an item inside a quote.
        (
            "> - <!-- a\nb -->\n> c\n",
            "document\n  blockquote\n    list ordered=false marker=\"-\"\n      item\n        comment content=\"a\\nb\"\n    paragraph\n      inline\n        text content=\"c\"\n",
        ),
        // The item goes on after it.
        (
            "- <!-- a\nb -->\n  c\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      comment content=\"a\\nb\"\n      inline\n        text content=\"c\"\n",
        ),
        // A tag after the `-->` is dropped with the rest of its line.
        (
            "- <!-- a\nb --> {% x %}\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      comment content=\"a\\nb\"\n",
        ),
        // In a nested quote.
        (
            "> > <!-- a\nb -->\n> c\n",
            "document\n  blockquote\n    blockquote\n      comment content=\"a\\nb\"\n    paragraph\n      inline\n        text content=\"c\"\n",
        ),
        // One per item.
        (
            "- <!-- a\nb -->\n- <!-- c\nd -->\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      comment content=\"a\\nb\"\n    item\n      comment content=\"c\\nd\"\n",
        ),
        // A list marker inside it is comment text.
        (
            "* <!-- a\n  - b\n-->\n* c\n",
            "document\n  list ordered=false marker=\"*\"\n    item\n      comment content=\"a\\n  - b\"\n    item\n      inline\n        text content=\"c\"\n",
        ),
        // Inside a block tag.
        (
            "{% if $x %}\n- <!-- a\nb -->\n{% /if %}\n",
            "document\n  tag[if] primary=$x\n    list ordered=false marker=\"-\"\n      item\n        comment content=\"a\\nb\"\n",
        ),
    ] {
        assert_eq!(outline(&parse_comments(source)), expected, "{source:?}");
    }
}

/// A comment in a code block is code, line breaks and all, though the
/// segmenter cannot see a fence that opens behind container markers. The
/// fence's content is upstream's; upstream also reads tags in it, which this
/// crate does not by default (`DIVERGENCES.md` entry 1).
#[test]
fn a_comment_in_a_fence_inside_a_container_is_code() {
    for (source, expected) in [
        (
            "> ```\n> <!-- a\n> b -->\n> ```\n",
            "document\n  blockquote\n    fence content=\"<!-- a\\nb -->\\n\"\n",
        ),
        (
            "- ```\n  <!-- a\n  {% if $x %}\n  -->\n  ```\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      fence content=\"<!-- a\\n{% if $x %}\\n-->\\n\"\n",
        ),
        // The fence closes before the second `<!--`, which opens a comment.
        (
            "> ```\n> <!-- a\n> ```\n> <!-- b\n{% if $x %}\n> -->\n",
            "document\n  blockquote\n    fence content=\"<!-- a\\n\"\n    comment content=\"b\\n{% if $x %}\\n>\"\n",
        ),
        // However many `<!--` the fence holds: refusing them goes no further
        // than the fence.
        (
            "> ```\n> <!-- a\n> <!-- a2\n> ```\n> <!-- b\n{% if $x %}\n> -->\n",
            "document\n  blockquote\n    fence content=\"<!-- a\\n<!-- a2\\n\"\n    comment content=\"b\\n{% if $x %}\\n>\"\n",
        ),
    ] {
        assert_eq!(outline(&parse_comments(source)), expected, "{source:?}");
    }
}

/// A comment block may not open on the line after a table, which upstream
/// reads as a row; a line with a `|` in a paragraph is no table, and a comment
/// interrupts the paragraph there, in a container too.
#[test]
fn a_comment_after_a_pipe_opens_unless_a_table_is_there() {
    for (source, expected) in [
        (
            "a | b\n<!-- c\n{% if $x %}\nd -->\n",
            "document\n  paragraph\n    inline\n      text content=\"a | b\"\n  comment content=\"c\\n{% if $x %}\\nd\"\n",
        ),
        (
            "- a | b\n- <!-- c\n{% if $x %}\nd -->\n- e\n",
            "document\n  list ordered=false marker=\"-\"\n    item\n      inline\n        text content=\"a | b\"\n    item\n      comment content=\"c\\n{% if $x %}\\nd\"\n    item\n      inline\n        text content=\"e\"\n",
        ),
        (
            "> a | b\n> <!-- c\nd -->\n> e\n",
            "document\n  blockquote\n    paragraph\n      inline\n        text content=\"a | b\"\n    comment content=\"c\\nd\"\n    paragraph\n      inline\n        text content=\"e\"\n",
        ),
    ] {
        assert_eq!(outline(&parse_comments(source)), expected, "{source:?}");
    }
    // A `|` line and a dash line are a table only with as many cells in each,
    // and not in code.
    for (source, comment) in [
        ("a | b | c\n--|--\n<!-- a\nb -->\n", "a\\nb"),
        ("```\na | b\n--|--\n```\n<!-- x\ny -->\n", "x\\ny"),
        ("    a | b\n    --|--\n\n<!-- x\ny -->\n", "x\\ny"),
    ] {
        let tree = outline(&parse_comments(source));
        assert!(
            tree.contains(&format!("\n  comment content=\"{comment}\"\n")),
            "{source:?}\n{tree}"
        );
    }
    // After a table, the `<!--` line is a row, and the tag is a tag.
    let table = outline(&parse_comments(
        "| a | b |\n|---|---|\n<!-- c\n{% if $x %}\nd -->\n",
    ));
    assert!(table.contains("text content=\"<!-- c\""), "{table}");
    assert!(table.contains("tag[if] primary=$x"), "{table}");
}

/// The line after a table is a row, whatever it holds: upstream has no HTML
/// block to end the table with, and its comment rule ends none either. A
/// comment there is the cell's, as inline. Upstream 0.5.12's trees.
#[test]
fn the_line_after_a_table_is_a_row() {
    let row = |cells: &[&str]| {
        cells.iter().fold(String::new(), |out, cell| {
            out + "      tr\n        td\n          inline\n" + cell
        })
    };
    let table = |rows: String| {
        format!(
            "document\n  table\n    thead\n      tr\n        th\n          inline\n            text content=\"a\"\n    tbody\n{rows}"
        )
    };
    for (source, expected) in [
        (
            "| a |\n|---|\n<!-- c\nd -->\n",
            table(row(&[
                "            text content=\"<!-- c\"\n",
                "            text content=\"d -->\"\n",
            ])),
        ),
        (
            "| a |\n|---|\n<!-- c --> *x* y\n",
            table(row(&[
                "            comment content=\"c\"\n            text content=\" \"\n            em marker=\"*\"\n              text content=\"x\"\n            text content=\" y\"\n",
            ])),
        ),
        // Markup inside the comment is the comment's, and what follows it is
        // the cell's text, a tag included.
        (
            "| a |\n|---|\n<!-- *c* `d` --> z {% $x %}\n",
            table(row(&[
                "            comment content=\"*c* `d`\"\n            text content=\" z \"\n            text content=$x\n",
            ])),
        ),
        // A `|` still splits the cell, and the comment then does not close in it.
        (
            "| a |\n|---|\n<!-- a | b -->\n",
            table(row(&["            text content=\"<!-- a\"\n"])),
        ),
        (
            "| a |\n|---|\n<div>x</div>\n",
            table(row(&["            text content=\"<div>x</div>\"\n"])),
        ),
    ] {
        assert_eq!(outline(&parse_comments(source)), expected, "{source:?}");
    }
    // In a quote too, and only right after the table.
    assert_eq!(
        outline(&parse_comments("> | a |\n> |---|\n> <!-- c\n> d -->\n")),
        "document\n  blockquote\n    table\n      thead\n        tr\n          th\n            inline\n              text content=\"a\"\n      tbody\n        tr\n          td\n            inline\n              text content=\"<!-- c\"\n        tr\n          td\n            inline\n              text content=\"d -->\"\n"
    );
    assert!(
        outline(&parse_comments("> | a |\n> |---|\n>\n> <!-- c\n> d -->\n"))
            .ends_with("    comment content=\"c\\n> d\"\n")
    );
}

/// A comment's content has every line break as `\n`, as upstream reads the
/// source.
#[test]
fn a_comment_has_its_line_breaks_as_newlines() {
    for (source, content) in [
        ("<!-- a\r\nb -->\r\n", "a\nb"),
        ("<!-- a\rb -->\r", "a\nb"),
        ("> <!-- a\r\n> b -->\r\n", "a\n> b"),
        ("x <!-- a\r\nb --> y\r\n", "a\nb"),
        ("> x <!-- a\r\n> b --> y\r\n", "a\nb"),
    ] {
        let document = parse_comments(source);
        let comment = document
            .walk()
            .find(|node| node.node_type == accent_proust::ast::NodeType::Comment)
            .and_then(|node| node.get("content"));
        assert_eq!(
            comment,
            Some(&accent_proust::ast::Value::String(content.into())),
            "{source:?}"
        );
    }
}

/// An inline comment's later lines are without their container prefixes, as
/// upstream's inline rule reads a paragraph's lines; a comment block keeps
/// them, as upstream's block rule reads the source.
#[test]
fn an_inline_comment_is_without_its_lines_prefixes() {
    for (source, content) in [
        ("> x <!-- a\n> b --> y\n", "a\nb"),
        (">x <!-- a\n>b --> y\n", "a\nb"),
        ("> x <!-- a\n>    b --> y\n", "a\n   b"),
        ("> x <!-- a\nb --> y\n", "a\nb"),
        ("- x <!-- a\n  b --> y\n", "a\nb"),
        ("- x <!-- a\n     b --> y\n", "a\n   b"),
        ("> - x <!-- a\n>   b\n>   c --> y\n", "a\nb\nc"),
        ("> > x <!-- a\n> > b --> y\n", "a\nb"),
        (">\tx <!-- a\n>\tb --> y\n", "a\n\tb"),
        ("1. x <!-- a\n   b --> y\n", "a\nb"),
        ("x <!-- a\n   b --> y\n", "a\n   b"),
        ("> <!-- a\n> b -->\n", "a\n> b"),
        ("- <!-- a\n  b -->\n", "a\n  b"),
        // A tab is as wide as to the next multiple of four. Right after a
        // `>` it is kept whole, unless the prefix goes on past it.
        ("> a <!-- x\n>\ty --> z\n", "x\n\ty"),
        ("- a <!-- x\n\ty --> z\n", "x\n  y"),
        ("-\ta <!-- x\n\ty --> z\n", "x\ny"),
        ("> - a <!-- x\n>\ty --> z\n", "x\ny"),
        ("  > a <!-- x\n  >\ty --> z\n", "x\ny"),
        ("10. a <!-- x\n\t y --> z\n", "x\n y"),
        // In a cell, `\|` is the table's before it is the comment's.
        ("| a |\n|---|\n| <!-- x \\| y --> |\n", "x | y"),
        ("| a |\n|---|\n<!-- x \\| y --> z\n", "x | y"),
    ] {
        let document = parse_comments(source);
        let comment = document
            .walk()
            .find(|node| node.node_type == accent_proust::ast::NodeType::Comment)
            .and_then(|node| node.get("content"));
        assert_eq!(
            comment,
            Some(&accent_proust::ast::Value::String(content.into())),
            "{source:?}"
        );
    }
}

/// A comment in a container formats as its content has it: a quote adds no
/// `>` to its later lines, which already hold theirs, so the reprint parses
/// back to the same tree and formats to the same bytes.
#[test]
fn a_comment_in_a_container_formats_as_written() {
    for source in [
        "> <!-- a\n> b -->\n",
        "> - <!-- a\n>   b\n>   -->\n",
        "> > <!-- a\n> > b\nc -->\n> > d\n",
        "> <!-- a\nb -->\n> c\n",
        "- > <!-- a\n  > b -->\n",
        "> x <!-- a\n> b --> y\n",
        "- x <!-- a\n     b --> y\n",
        "> - x <!-- a\n>   b\n>   c --> y\n",
        "> x <!-- a\n> b --> y\n>\n> <!-- c\n> d -->\n",
        "{% note %}\n<!-- a\nb -->\n{% /note %}\n",
        "{% note %}\n<!-- a -->\n{% /note %}\n",
        "1. a\n\n   <!-- x\n   y -->\n   z\n",
        "> - a\n>\n>   <!-- x\n>   y -->\n",
        "- x\n\n  <!-- a -->\n  <!-- b -->\n",
        "- a\n  <!-- c -->\n",
        "| a | b |\n|---|---|\n<!-- x \\| y --> z\n",
        "| a | b |\n|---|---|\n| `c\\|d` | <!-- e \\| f --> |\n",
    ] {
        let document = parse_comments(source);
        let formatted = accent_proust::format::format(&document);
        let again = parse_comments(&formatted);
        assert_eq!(
            outline(&again),
            outline(&document),
            "{source:?} -> {formatted:?}"
        );
        assert_eq!(
            accent_proust::format::format(&again),
            formatted,
            "{source:?}"
        );
    }
    assert_eq!(
        accent_proust::format::format(&parse_comments("> <!-- a\n> b -->\n")),
        "> <!-- a\n> b -->\n"
    );
}

/// Many `<!--` before one `-->`, each in code, are refused in a bounded number
/// of passes, and each looks for its `-->` without searching the source again:
/// four times the lines take about four times as long, not sixteen.
#[test]
fn many_comments_in_code_stay_linear() {
    let documents = |lines: usize| {
        [
            format!("{}-->\n", "    <!-- a\n".repeat(lines)),
            format!("> ```\n{}> -->\n", "> <!-- a\n".repeat(lines)),
            "> ```\n> <!-- a\n> ```\n".repeat(lines / 4) + "-->\n",
        ]
    };
    // The fastest of a few runs, so that a busy machine does not count.
    let time = |lines: usize| {
        let documents = documents(lines);
        (0..3)
            .map(|_| {
                let started = std::time::Instant::now();
                for source in &documents {
                    let _ = parse_comments(source);
                }
                started.elapsed()
            })
            .min()
            .unwrap_or_default()
    };
    let (small, large) = (time(5_000), time(20_000));
    assert!(
        large < small * 10,
        "5,000 lines took {small:?} and 20,000 took {large:?}"
    );
}

// ---- block HTML with the option off ----------------------------------------
//
// markdown-it with `html: false` has no HTML block rule, so the lines of what
// pulldown-cmark calls an HTML block are an ordinary paragraph there: literal
// text, one text node per line, joined by soft breaks. These pin that shape.
// The port once attached the raw markup as a bare `text` node in the block
// position instead, which the `document` schema rejects.

/// A comment on its own line, with the option off, is a paragraph of text.
#[test]
fn a_block_comment_is_a_paragraph_when_the_option_is_off() {
    let source = dedent("\nthis is a test\n\n<!-- example comment -->\n\nfoo\n");
    assert_eq!(
        outline(&parse(&source)),
        "\
document
  paragraph
    inline
      text content=\"this is a test\"
  paragraph
    inline
      text content=\"<!-- example comment -->\"
  paragraph
    inline
      text content=\"foo\"
"
    );
}

/// Each line is its own text node, and the lines are joined by soft breaks.
#[test]
fn a_multi_line_block_comment_keeps_its_lines_when_the_option_is_off() {
    assert_eq!(
        outline(&parse("<!--\nexample comment\n-->\n")),
        "\
document
  paragraph
    inline
      text content=\"<!--\"
      softbreak
      text content=\"example comment\"
      softbreak
      text content=\"-->\"
"
    );
}

/// Raw HTML other than a comment is literal text too, whatever the option.
#[test]
fn a_raw_html_block_is_a_paragraph_of_text() {
    let expected = "\
document
  paragraph
    inline
      text content=\"<div>\"
      softbreak
      text content=\"foo\"
      softbreak
      text content=\"</div>\"
";
    assert_eq!(outline(&parse("<div>\nfoo\n</div>\n")), expected);
    assert_eq!(outline(&parse_comments("<div>\nfoo\n</div>\n")), expected);
}

/// pulldown-cmark keeps a comment open across a blank line; markdown-it ends
/// the paragraph there. With the option off, the blank line splits it.
#[test]
fn a_blank_line_splits_an_html_block_when_the_option_is_off() {
    assert_eq!(
        outline(&parse("<!--\n\nexample\n-->\n")),
        "\
document
  paragraph
    inline
      text content=\"<!--\"
  paragraph
    inline
      text content=\"example\"
      softbreak
      text content=\"-->\"
"
    );
}
