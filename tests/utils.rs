//! Upstream's `src/utils.test.ts`, ported.
//!
//! 186 lines covering two functions that upstream keeps together and this crate
//! splits: `findTagEnd`, which the segmenter uses verbatim
//! ([`accent_proust::parse::find_tag_end`]), and `parseTags`, which upstream's
//! tokenizer plugin uses to cut a text run into markdown-it tokens.
//!
//! # `findTagEnd` is ported as itself
//!
//! Every offset assertion is upstream's, unchanged, including the multiline
//! examples whose numbers only make sense against the exact whitespace of the
//! template literal they came from. Each is written out here rather than
//! reconstructed, and each keeps upstream's second assertion -- that the byte at
//! the returned offset is `%` -- which is what makes an offset that drifted for
//! the wrong reason fail loudly instead of quietly.
//!
//! # `parseTags` is ported as behaviour
//!
//! There is no `parseTags` to call. Upstream's returns markdown-it tokens with
//! `start`/`end`/`nesting` fields; the equivalent here is the segmenter
//! ([`DIVERGENCES.md` entry 2] -- pulldown-cmark is reached through a seam
//! rather than hooked), which is `pub(crate)` and produces a different shape on
//! purpose. So the two assertions are ported to what they were really testing:
//! that a tag in a text run is found with its attributes and its surrounding
//! text intact, and that a `{%` inside a code fence is left alone.
//!
//! The second is the more valuable of the two. Upstream named it
//! "shouldn't hang when `{%` is included in code block", which records a real
//! bug: a scanner that fails to find `%}` and does not advance loops forever.
//! `src/parse/scan.rs` carries the same guard, and
//! `tests/parse_proptests.rs` asserts termination over arbitrary input.
//!
//! [`DIVERGENCES.md` entry 2]: ../DIVERGENCES.md

mod support;

use accent_proust::ast::NodeType;
use accent_proust::parse::{find_tag_end, parse};
use support::{at, attribute, outline};

/// Upstream asserts the offset and the byte it lands on. Both, every time.
fn tag_end(example: &str) -> Option<usize> {
    let end = find_tag_end(example, 0);
    if let Some(end) = end {
        assert_eq!(
            example.as_bytes().get(end).copied(),
            Some(b'%'),
            "offset {end} of {example:?} is not the `%` of a closing delimiter"
        );
    }
    end
}

// ---- findTagEnd: inline tags --------------------------------------------

#[test]
fn in_a_heading() {
    assert_eq!(tag_end("# Testing {% #foo.bar baz=1 %}"), Some(28));
}

#[test]
fn with_string() {
    assert_eq!(
        tag_end("# Testing {% #foo.bar baz=\"example\" test=true %}"),
        Some(46)
    );
}

#[test]
fn with_object_literal_attribute_value() {
    assert_eq!(
        tag_end("# Testing {% #foo.bar baz={test: 1, foo: {test: \"asdf{\"}} %}"),
        Some(58)
    );
}

#[test]
fn in_a_simple_container() {
    assert_eq!(tag_end("{% foo %}"), Some(7));
}

#[test]
fn in_a_container_with_shortcuts() {
    assert_eq!(tag_end("{% foo .bar.baz#test %}"), Some(21));
}

#[test]
fn in_a_container_with_a_string_attribute() {
    assert_eq!(tag_end("{% foo test=\"this is a test\" %}"), Some(29));
}

#[test]
fn for_an_invalid_container() {
    assert_eq!(tag_end("{% foo .bar#baz"), None);
}

#[test]
fn in_a_complex_container() {
    assert_eq!(
        tag_end("{% #foo .bar .baz test=\"this} is \\\"{test}\\\" a test\" %} this is a test"),
        Some(52)
    );
}

// ---- findTagEnd: multiline tags -----------------------------------------
//
// The offsets are upstream's, and they count the leading newline and the eight
// spaces of indentation that its template literals carry. Each example is
// written out in full for that reason: a dedented copy would be a different
// string with different offsets, and the assertions would then be measuring the
// helper rather than the scanner.

#[test]
fn multiline_simple() {
    let example = "\n        {% test #foo.bar\n              baz=1 %}\n        ";
    assert_eq!(tag_end(example), Some(46));
}

#[test]
fn multiline_with_string() {
    let example = "\n        {% test #foo.bar\
                   \n              baz=\"this is a test\"\
                   \n              example=1 %}\n        ";
    assert_eq!(tag_end(example), Some(85));
}

#[test]
fn multiline_with_string_and_escaped_quote() {
    let example = "\n        {% test #foo.bar\
                   \n              baz=\"this \\\"is a test\"\
                   \n              example=1 %}\n        ";
    assert_eq!(tag_end(example), Some(87));
}

#[test]
fn multiline_with_string_that_has_an_opening_brace() {
    let example = "\n        {% test #foo.bar\
                   \n              baz=\"this {is a test\"\
                   \n              example=1 %}\n        ";
    assert_eq!(tag_end(example), Some(86));
}

#[test]
fn multiline_with_string_that_has_escapes_and_braces() {
    let example = "\n        {% test #foo.bar\
                   \n              baz=\"th\\\"is {is a \\\\te\\\"st\"\
                   \n              example=1 %}\n        ";
    assert_eq!(tag_end(example), Some(92));
}

#[test]
fn multiline_with_an_object_literal_attribute_value() {
    let example = "\n        {% test #foo.bar\
                   \n              foo={testing: \"this } is a test\", bar: {baz: 1}}\
                   \n              example=1 another=\"test}\" %}\n        ";
    assert_eq!(tag_end(example), Some(129));
}

#[test]
fn multiline_with_an_invalid_object_literal_attribute_value() {
    // No closing delimiter anywhere outside a string, so the scan runs off the
    // end. Not an error: an unclosed `{%` is ordinary text.
    let example = "\n        {test #foo.bar\
                   \n              foo={testing: \"this } is a test\", bar: {baz: 1}\
                   \n              example=1 another=\"test}\"}\n        ";
    assert_eq!(tag_end(example), None);
}

// ---- parseTags ----------------------------------------------------------

/// Upstream's "simple example", ported to the tree it produces.
///
/// Its assertion is a token stream -- `text`, `tag_open` carrying
/// `{tag: 'foo', attributes: [{name: 'blah', value: 'asdf'}]}`, `text`,
/// `tag_close`, `text` -- which this crate has no equivalent of. What it fixes
/// is that an inline tag is found inside a run of text, with its attributes
/// parsed and the text on either side left whole, and that survives the change
/// of representation.
#[test]
fn a_tag_is_found_inside_a_run_of_text() {
    let document = parse("this is a {% foo blah=\"asdf\" %}test{% /foo %} of template parsing");
    assert_eq!(
        outline(&document),
        "document\n  \
           paragraph\n    \
             inline\n      \
               text content=\"this is a \"\n      \
               tag[foo] blah=\"asdf\"\n        \
                 text content=\"test\"\n      \
               text content=\" of template parsing\"\n"
    );
}

/// Upstream's "shouldn't hang when `{%` is included in code block".
///
/// The name is the assertion: a scanner that cannot find `%}` and does not
/// advance past the `{%` loops forever. Here the fence's content also has to
/// come out byte for byte, because a fence is literal by default
/// (`DIVERGENCES.md` entry 1) and its text is the `content` attribute.
#[test]
fn a_tag_delimiter_inside_a_fence_is_left_alone() {
    let document = parse("```\n{%a %b %c}\n```");
    let fence = at(&document, &[0]);
    assert_eq!(fence.node_type, NodeType::Fence);
    assert_eq!(attribute(fence, "content"), "\"{%a %b %c}\\n\"");
    assert!(fence.children.is_empty(), "a literal fence has no children");
    // Nothing in the document is a tag: the delimiter was never a tag.
    assert!(
        !document.walk().any(|node| node.node_type == NodeType::Tag),
        "the fence's `{{%` was read as a tag"
    );
}
