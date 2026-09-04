//! The `Tokenizer` seam, exercised without pulldown-cmark.
//!
//! Nothing upstream corresponds to this. It exists because the README makes a
//! promise the rest of the suite cannot check: a host that already parses
//! CommonMark turns the `pulldown-cmark-tokenizer` feature off and implements
//! [`Tokenizer`] over the parser it already pins. Every other integration test
//! here needs the bundled tokenizer and is skipped in that configuration, so
//! without this file the `--no-default-features` lane would compile the crate
//! and prove nothing about the seam it is supposed to be checking.
//!
//! The tokenizer below is not a CommonMark implementation and does not try to
//! be. It emits one paragraph per blank-line-separated block, which is enough to
//! drive every part of the parser that does not belong to the CommonMark engine:
//! block tags split the document, inline tags split a text run, the `inline`
//! node is synthesised, tag nesting is tracked, and unclosed tags are reported.
//! That list is exactly what the seam is meant to keep independent of the
//! engine, so a test of the seam should be able to make it work with a
//! deliberately poor one.

use std::borrow::Cow;

use accent_proust::ast::{Node, NodeType};
use accent_proust::parse::{
    Container, ContainerKind, Event, ParseOptions, Spanned, Tokenizer, parse_with,
};

/// A CommonMark tokenizer that knows about paragraphs and nothing else.
struct Paragraphs;

impl Tokenizer for Paragraphs {
    fn tokenize<'s>(&self, source: &'s str) -> Vec<Spanned<'s>> {
        let mut out = Vec::new();
        let mut offset = 0;
        for block in source.split("\n\n") {
            let start = offset;
            offset += block.len() + 2;
            let trimmed = block.trim();
            if trimmed.is_empty() {
                continue;
            }
            // The range has to cover the text exactly, because the parser reads
            // the original source back through it.
            let text_start = start + (block.len() - block.trim_start().len());
            let text_end = text_start + trimmed.len();
            out.push((Event::Start(Container::Paragraph), start..text_end));
            out.push((Event::Text(Cow::Borrowed(trimmed)), text_start..text_end));
            out.push((Event::End(ContainerKind::Paragraph), start..text_end));
        }
        out
    }
}

fn parse(source: &str) -> Node<'_> {
    parse_with(source, &Paragraphs, &ParseOptions::new())
}

#[test]
fn a_host_tokenizer_drives_the_whole_parser() {
    let source = "{% callout %}\nThis is a test\n{% /callout %}\n";
    let document = parse(source);
    let callout = &document.children[0];
    assert_eq!(callout.node_type, NodeType::Tag);
    assert_eq!(callout.tag.as_deref(), Some("callout"));
    assert_eq!(callout.children[0].node_type, NodeType::Paragraph);
    assert_eq!(callout.children[0].children[0].node_type, NodeType::Inline);
}

#[test]
fn inline_tags_split_a_text_run_from_any_tokenizer() {
    let source = "Example {% foo %}bar{% /foo %} baz";
    let document = parse(source);
    let inline = &document.children[0].children[0];
    let kinds: Vec<&str> = inline.children.iter().map(Node::name).collect();
    assert_eq!(kinds, ["text", "foo", "text"]);
}

#[test]
fn structural_errors_are_reported_without_the_bundled_tokenizer() {
    let document = parse("{% foo %}\nunclosed\n");
    let ids: Vec<&str> = document.children[0]
        .errors
        .iter()
        .map(|error| error.id)
        .collect();
    assert_eq!(ids, ["missing-closing"]);
}

#[test]
fn locations_still_borrow_the_source() {
    let source = "{% foo /%}\n";
    let document = parse(source);
    let location = document.children[0].location.expect("a location");
    assert_eq!(location.text, "{% foo /%}");
    assert_eq!(source.get(location.span()), Some(location.text));
}
