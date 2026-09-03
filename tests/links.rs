//! Upstream's `src/tokenizer/plugins/link.test.ts`, ported.
//!
//! Upstream reads the error off the markdown-it `inline` token
//! (`tokens.find(t => t.type === 'inline')?.errors?.[0]`). Here the `inline`
//! node is that token's counterpart, so the assertion reads the same field of
//! the same thing.
//!
//! The rule itself -- which text counts as a URL with a tag in it -- is unit
//! tested next to its implementation in `src/parse/scan.rs`. What these tests
//! add is that it is wired to the right node, that it is off unless asked for,
//! and that the protocol list is the caller's.

mod support;

use accent_proust::ast::{Node, NodeType};
use accent_proust::parse::{parse_with, ParseOptions, PulldownTokenizer};
use support::dedent;

fn parse_with_validation<'s>(source: &'s str, protocols: &[&str]) -> Node<'s> {
    let options = ParseOptions::new()
        .validated_protocols(protocols.iter().map(ToString::to_string).collect());
    parse_with(source, &PulldownTokenizer::new(), &options)
}

/// The first error on the first `inline` node, as upstream reads it.
fn inline_error(document: &Node<'_>) -> Option<&'static str> {
    document
        .walk()
        .find(|node| node.node_type == NodeType::Inline)
        .and_then(|node| node.errors.first())
        .map(|error| error.id)
}

const HTTP: &[&str] = &["http", "https"];

#[test]
fn accepts_valid_link_urls_and_markdoc_tag_in_one_paragraph() {
    let source = dedent("The link is https://example.com. {% tag /%})");
    assert_eq!(inline_error(&parse_with_validation(&source, HTTP)), None);
}

#[test]
fn accepts_raw_tag_content_in_markdown_link_format() {
    let source = dedent("[Link]({% tag %})");
    assert_eq!(inline_error(&parse_with_validation(&source, HTTP)), None);
}

#[test]
fn rejects_plain_link_url_with_tags() {
    let source = dedent("https://example.com/{% tag %}content{% /tag %})");
    assert_eq!(
        inline_error(&parse_with_validation(&source, HTTP)),
        Some("href-format-invalid")
    );
}

#[test]
fn rejects_url_with_parenthesis_in_link() {
    let source = dedent("https://en.wikipedia.org/wiki/Exam_(disambiguation){% tag /%}");
    assert_eq!(
        inline_error(&parse_with_validation(&source, HTTP)),
        Some("href-format-invalid")
    );
}

#[test]
fn rejects_variables_in_markdown_link_urls() {
    let source = dedent("[Link](https://{% $variable.custom_value %})");
    assert_eq!(
        inline_error(&parse_with_validation(&source, HTTP)),
        Some("href-format-invalid")
    );
}

#[test]
fn rejects_self_closing_tags_in_markdown_link_urls() {
    let source = dedent("[Link](https://example.com/{% tag /%})");
    assert_eq!(
        inline_error(&parse_with_validation(&source, HTTP)),
        Some("href-format-invalid")
    );
}

#[test]
fn rejects_non_self_closing_tags_in_markdown_link_urls() {
    let source = dedent("[Link](https://example.com/{% tag %}content{% /tag %})");
    assert_eq!(
        inline_error(&parse_with_validation(&source, HTTP)),
        Some("href-format-invalid")
    );
}

#[test]
fn rejects_custom_protocols_defined_in_the_config_with_markdoc_variable() {
    let source = dedent("[Link](vscode://{% $variable.custom_value %})");
    assert_eq!(
        inline_error(&parse_with_validation(&source, &["vscode"])),
        Some("href-format-invalid")
    );
}

/// Upstream's `allowLinkValidation` defaults to off, so the same document is
/// clean when nobody asked for the check.
#[test]
fn the_check_is_off_unless_asked_for() {
    let source = dedent("https://example.com/{% tag /%}");
    let document = accent_proust::parse::parse(&source);
    assert_eq!(inline_error(&document), None);
}

#[test]
fn the_error_message_is_upstreams() {
    let source = dedent("https://example.com/{% tag /%}");
    let document = parse_with_validation(&source, HTTP);
    let inline = document
        .walk()
        .find(|node| node.node_type == NodeType::Inline)
        .expect("an inline run");
    assert_eq!(
        inline.errors[0].message,
        "The 'href' format cannot contain Markdoc tag or variable. \
         URLs must be static strings."
    );
}
