//! Setext headings are undone, as upstream's disabled `lheading` rule has it.
//!
//! The parser tells a setext heading from an ATX heading by its span. These
//! cases pin the shapes where the obvious test, "does the span start with
//! `#`", gets it wrong: a `#` that does not open an ATX heading (#81).

mod support;

use accent_proust::parse::parse;
use support::outline;

const PARAGRAPH_THEN_HR: &str = "\
document
  paragraph
    inline
      text content=\"{}\"
  hr
";

fn paragraph_then_hr(text: &str) -> String {
    PARAGRAPH_THEN_HR.replace("{}", text)
}

#[test]
fn a_plain_line_over_dashes_is_a_paragraph_and_a_rule() {
    assert_eq!(outline(&parse("Foo\n---\n")), paragraph_then_hr("Foo"));
}

#[test]
fn a_hash_without_a_space_over_dashes_is_a_paragraph_and_a_rule() {
    assert_eq!(outline(&parse("#a\n---\n")), paragraph_then_hr("#a"));
    assert_eq!(
        outline(&parse("#5 bolt\n---\n")),
        paragraph_then_hr("#5 bolt")
    );
}

#[test]
fn an_indented_hash_line_over_dashes_is_a_paragraph_and_a_rule() {
    assert_eq!(outline(&parse("  #a\n---\n")), paragraph_then_hr("#a"));
}

#[test]
fn a_hash_without_a_space_over_equals_is_one_paragraph() {
    assert_eq!(
        outline(&parse("#a\n===\n")),
        "\
document
  paragraph
    inline
      text content=\"#a\"
      softbreak
      text content=\"===\"
"
    );
}

#[test]
fn a_hash_line_over_dashes_in_a_blockquote_is_a_paragraph_and_a_rule() {
    assert_eq!(
        outline(&parse("> #a\n> ---\n")),
        "\
document
  blockquote
    paragraph
      inline
        text content=\"#a\"
    hr
"
    );
}

#[test]
fn an_atx_heading_stays_a_heading() {
    for (source, level) in [
        ("# a\n", 1),
        ("## a\n", 2),
        ("#\n", 1),
        ("# a", 1),
        ("# a\r\n", 1),
        ("#\ta\n", 1),
        ("   ### a ###\n", 3),
        ("> # a\n", 1),
    ] {
        let tree = outline(&parse(source));
        assert!(
            tree.contains(&format!("heading level={level}")),
            "{source:?} lost its heading:\n{tree}"
        );
    }
}

#[test]
fn an_atx_heading_followed_by_dashes_is_a_heading_and_a_rule() {
    assert_eq!(
        outline(&parse("# a\n---\n")),
        "\
document
  heading level=1
    inline
      text content=\"a\"
  hr
"
    );
}
