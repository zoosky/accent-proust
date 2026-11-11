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

/// The tree upstream gives `Foo` over `===`: one paragraph, the underline a
/// continuation line after a break of the given kind.
fn continued(indent: &str, text: &str, breaker: &str) -> String {
    format!(
        "{indent}paragraph\n{indent}  inline\n{indent}    text content=\"{text}\"\n\
         {indent}    {breaker}\n{indent}    text content=\"===\"\n"
    )
}

#[test]
fn equals_in_a_blockquote_continue_the_paragraph() {
    for source in ["> Foo\n> ===\n", "> #a\n> ===\n"] {
        let text = if source.contains("#a") { "#a" } else { "Foo" };
        assert_eq!(
            outline(&parse(source)),
            format!(
                "document\n  blockquote\n{}",
                continued("    ", text, "softbreak")
            ),
            "{source:?}"
        );
    }
}

#[test]
fn indented_equals_continue_the_paragraph_without_their_indent() {
    assert_eq!(
        outline(&parse("  Foo\n  ===\n")),
        format!("document\n{}", continued("  ", "Foo", "softbreak"))
    );
    assert_eq!(
        outline(&parse("- Foo\n  ===\n")),
        format!(
            "document\n  list ordered=false marker=\"-\"\n    item\n{}",
            continued("      ", "Foo", "softbreak")
        )
    );
}

#[test]
fn equals_after_a_hard_break_keep_it() {
    let hard = format!("document\n{}", continued("  ", "Foo", "hardbreak"));
    assert_eq!(outline(&parse("Foo  \n===\n")), hard);
    assert_eq!(outline(&parse("Foo\\\n===\n")), hard);
    // An escaped backslash is text, and the break is soft.
    assert_eq!(
        outline(&parse("Foo\\\\\n===\n")),
        format!("document\n{}", continued("  ", "Foo\\\\", "softbreak"))
    );
}

#[test]
fn bare_carriage_returns_end_lines() {
    assert_eq!(
        outline(&parse("Foo\r===\r")),
        format!("document\n{}", continued("  ", "Foo", "softbreak"))
    );
    assert_eq!(outline(&parse("#a\r---\r")), paragraph_then_hr("#a"));
    assert_eq!(outline(&parse("#a\r\n---\r\n")), paragraph_then_hr("#a"));
}

#[test]
fn the_rule_covers_only_the_underline() {
    let document = parse("#a\r---\r");
    let rule = &document.children[1];
    let location = rule.location.as_ref().expect("a location");
    assert_eq!(location.text, "---\r");
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
