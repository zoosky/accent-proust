//! Panic-freedom for the segmenter and the parser, against generated input.
//!
//! The grammar has its own property test for the same reason. This one covers
//! the layer above it: the segmenter reads raw bytes and does its own arithmetic
//! on byte offsets, which is where a panic-free port most easily stops being
//! one. Every failure mode it could have is an index -- a slice past the end of
//! the source, a range whose start exceeds its end, a boundary in the middle of
//! a character -- and none of those is visible in review.
//!
//! `parse` returns no `Result`, so "did not panic" is the whole assertion. What
//! makes it worth running is the generator: `\\PC*` is arbitrary Unicode, and
//! the biased strategies below spend their budget on the characters that
//! actually drive this code -- the delimiters, the fences, and the newlines that
//! decide where a block begins.

use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse, parse_with};
use proptest::prelude::*;

/// Fragments the segmenter makes decisions about, so a generated document is
/// mostly interesting rather than mostly noise.
const FRAGMENTS: &[&str] = &[
    "{%",
    "%}",
    "/%}",
    "{% foo %}",
    "{% /foo %}",
    "{% $a.b[0] %}",
    "{% #id .cls %}",
    "```",
    "~~~",
    "<!--",
    "-->",
    "\n",
    "\n\n",
    "    ",
    "\t",
    "*",
    "**",
    "_",
    "`",
    "[",
    "](",
    "|",
    "---",
    "===",
    "#",
    "> ",
    "- ",
    "1. ",
    "\\",
    "\"",
    "\u{e9}",
    "\u{1f600}",
];

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            2 => prop::sample::select(FRAGMENTS).prop_map(ToString::to_string),
            1 => "[a-z ]{0,8}",
        ],
        0..40,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Arbitrary Unicode, including text no segmenter rule matches.
    #[test]
    fn parsing_arbitrary_text_never_panics(source in "\\PC*") {
        let _ = parse(&source);
    }

    /// Text built from the fragments the segmenter branches on.
    #[test]
    fn parsing_delimiter_soup_never_panics(source in document()) {
        let _ = parse(&source);
    }

    /// Every option on, because each one adds a branch: slots move a node into
    /// a map, comments reinterpret an HTML event, and link validation reads a
    /// span back out of the source.
    #[test]
    fn parsing_with_every_option_on_never_panics(source in document()) {
        let options = ParseOptions::new()
            .slots(true)
            .allow_comments(true)
            .validated_protocols(vec!["http".to_string(), "https".to_string()]);
        let _ = parse_with(&source, &PulldownTokenizer::new(), &options);
    }

    /// Locations off takes a different path through node construction, and a
    /// node with no location is what several later branches then read.
    #[test]
    fn parsing_without_locations_never_panics(source in document()) {
        let options = ParseOptions::new().location(false);
        let _ = parse_with(&source, &PulldownTokenizer::new(), &options);
    }

    /// Every location must be a real slice of the source it came from.
    ///
    /// Stronger than panic-freedom and the property the formatter depends on:
    /// a span that is merely close reprints a document that is merely close.
    #[test]
    fn every_location_borrows_its_own_span(source in document()) {
        let document = parse(&source);
        for node in document.walk() {
            let Some(location) = node.location else { continue };
            prop_assert!(location.start.offset <= location.end.offset);
            prop_assert!(location.end.offset <= source.len());
            prop_assert_eq!(source.get(location.span()), Some(location.text));
        }
    }
}
