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

use proust::parse::{parse, parse_with, ParseOptions, PulldownTokenizer};
use support::{dedent, outline};

fn parse_comments(source: &str) -> proust::ast::Node<'_> {
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
