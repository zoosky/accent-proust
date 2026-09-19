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
/// text and its `text_collapse` rule folds the run back together, which is what
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
