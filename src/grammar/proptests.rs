//! Panic-freedom, asserted against generated input rather than reviewed.
//!
//! The crate promises that an open parser fed arbitrary text does not panic,
//! and this module is the promise's teeth. The property being asserted is
//! deliberately weak in what it demands and strong in what it covers: for any
//! input at all, `parse_tag` returns -- `Ok` or `Err`, either is fine -- and
//! does not unwind, abort, or overflow the stack.
//!
//! Two generators, because one of them alone would be misleading:
//!
//! - Arbitrary strings reach the *entry* of the grammar and almost always fail
//!   in the first few bytes. They cover the character handling: non-ASCII
//!   input, control characters, multi-byte characters at an offset an error
//!   then has to report, and the empty body.
//! - Tag-shaped strings, assembled from grammar fragments, reach the
//!   *interior*: nested brackets, half-closed strings, commas in places no
//!   production wants them. That is where a hand-written recursive-descent
//!   parser actually breaks, and a `.*` generator would essentially never
//!   produce one.
//!
//! The error invariants are here too, and they are not decoration. A host
//! slices its document with the offsets an error carries, so an offset past
//! the end or inside a character is a panic in the caller rather than here.
//! Attribute spans are the same hazard and get the same treatment: they exist
//! to be sliced with, so a span that landed mid-character would move a panic
//! into every consumer.

use proptest::prelude::*;

use super::{TagItem, parse_tag, parse_tag_spanned};

/// Fragments of tag syntax, in roughly the proportion a fuzzer needs: brackets
/// and separators outnumber leaves, because it is the nesting and the
/// separators that drive the parser into its corners.
fn fragment() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "foo", "bar-1", "0", "$foo", "@foo", ".cls", "#id", "primary", "f(", ")", "(", "[", "]",
        "{", "}", ",", ":", "=", ".", "/", "-", "\"", "\"a\"", "\"\\", "\\n", "1", "-1.5", "1a",
        "true", "false", "null", "$$mdtype", " ", "  ", "\n", "\t", "\r", "é", "\u{0}",
    ])
}

/// A string assembled from those fragments.
fn tag_shaped() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment(), 0..32).prop_map(|parts| parts.concat())
}

fn identifier() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["foo", "bar", "type", "a", "b-1", "title"])
}

fn literal_value() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "1",
        "-1.5",
        "0",
        "true",
        "false",
        "null",
        "\"text\"",
        "\"a b\"",
        "\"a \\\"q\\\" b\"",
        "\"héllo wörld\"",
        "[1, 2]",
        "{a: 1}",
        "$foo",
        "$foo.bar",
    ])
}

/// One attribute-list item: a shortcut or a `name=value` pair.
fn item() -> impl Strategy<Value = String> {
    prop_oneof![
        identifier().prop_map(|name| format!("#{name}")),
        identifier().prop_map(|name| format!(".{name}")),
        (identifier(), literal_value()).prop_map(|(name, value)| format!("{name}={value}")),
    ]
}

/// A body the grammar accepts, built rather than searched for.
///
/// The two generators above are the right shape for panic-freedom and the wrong
/// shape for anything that only happens after a *successful* parse: the start
/// rule has to consume the whole body, so a concatenation of fragments almost
/// never parses, and a property guarded by `let Ok(..) = ..` would pass on
/// nothing at all. This one composes valid items instead, and the property
/// asserts that the body parses, so the day this generator stops producing
/// valid bodies is the day the test fails rather than the day it goes quiet.
///
/// A trailing `/` is only added when a tag name is present: an annotation is
/// `TagAttributes _*`, so `foo=1 /` leaves input behind and is an error.
fn valid_tag_body() -> impl Strategy<Value = String> {
    (
        prop::option::of(identifier()),
        prop::collection::vec(item(), 1..6),
        any::<bool>(),
    )
        .prop_map(|(name, items, self_closing)| {
            let mut body = String::new();
            if let Some(name) = name {
                body.push_str(name);
                body.push(' ');
            }
            body.push_str(&items.join(" "));
            if name.is_some() && self_closing {
                body.push_str(" /");
            }
            body
        })
}

/// Checks the invariants a caller is entitled to rely on after a failure.
fn check_error_invariants(input: &str) {
    let Err(error) = parse_tag(input) else {
        return;
    };
    assert!(
        error.start() <= error.end(),
        "start {} is past end {} for {input:?}",
        error.start(),
        error.end()
    );
    assert!(
        error.end() <= input.len(),
        "end {} is past the input length {} for {input:?}",
        error.end(),
        input.len()
    );
    // The pair has to be sliceable, which is the whole reason the offsets are
    // byte offsets and not character counts.
    assert!(
        input.get(error.start()..error.end()).is_some(),
        "offsets {}..{} do not land on character boundaries for {input:?}",
        error.start(),
        error.end()
    );
    assert!(
        !error.message().is_empty(),
        "an error with no message for {input:?}"
    );
}

/// Checks the invariants a caller is entitled to rely on after a parse that
/// succeeded, which is where the spans are.
fn check_span_invariants(input: &str) {
    let Ok((item, spans)) = parse_tag_spanned(input) else {
        return;
    };
    let attributes = match &item {
        TagItem::Annotation { attributes } | TagItem::TagOpen { attributes, .. } => {
            attributes.len()
        }
        _ => 0,
    };
    assert_eq!(
        attributes,
        spans.len(),
        "one span per attribute for {input:?}"
    );

    for span in &spans {
        assert!(
            span.all.start <= span.all.end,
            "span {:?} is reversed for {input:?}",
            span.all
        );
        assert!(
            input.get(span.all.clone()).is_some(),
            "span {:?} does not land on character boundaries for {input:?}",
            span.all
        );
        let Some(value) = span.value.clone() else {
            continue;
        };
        assert!(
            input.get(value.clone()).is_some(),
            "value span {value:?} does not land on character boundaries for {input:?}"
        );
        // A value is part of the item it belongs to, never beside it.
        assert!(
            span.all.start <= value.start && value.end <= span.all.end,
            "value span {value:?} escapes its item {:?} for {input:?}",
            span.all
        );
    }
}

proptest! {
    /// The gate: arbitrary input, no panic.
    #[test]
    fn never_panics_on_arbitrary_input(input in any::<String>()) {
        let _ = parse_tag(&input);
    }

    /// The same promise where it is actually at risk: input that gets deep
    /// enough into the grammar to reach a production's edge cases.
    #[test]
    fn never_panics_on_tag_shaped_input(input in tag_shaped()) {
        let _ = parse_tag(&input);
    }

    #[test]
    fn errors_carry_sliceable_offsets(input in any::<String>()) {
        check_error_invariants(&input);
    }

    #[test]
    fn errors_carry_sliceable_offsets_for_tag_shaped_input(input in tag_shaped()) {
        check_error_invariants(&input);
    }

    #[test]
    fn spans_carry_sliceable_offsets(input in any::<String>()) {
        check_span_invariants(&input);
    }

    #[test]
    fn spans_carry_sliceable_offsets_for_tag_shaped_input(input in tag_shaped()) {
        check_span_invariants(&input);
    }

    /// The one that actually exercises the spans. The two above reach the
    /// grammar's entry and almost never parse, so they assert the property on
    /// an empty set; this asserts the body parses first, which is what keeps
    /// the check from being vacuous.
    #[test]
    fn spans_of_a_body_that_parses_are_sliceable(input in valid_tag_body()) {
        prop_assert!(
            parse_tag_spanned(&input).is_ok(),
            "the generator produced a body the grammar rejects: {input:?}"
        );
        check_span_invariants(&input);
    }

    /// Parsing is a pure function of its input. Nothing in the parser is
    /// cached, ordered by a hash, or otherwise able to differ between runs --
    /// which is what makes rendered output byte-reproducible.
    #[test]
    fn parsing_is_deterministic(input in tag_shaped()) {
        let first = parse_tag(&input);
        let second = parse_tag(&input);
        match (first, second) {
            (Ok(first), Ok(second)) => prop_assert_eq!(first, second),
            (Err(first), Err(second)) => {
                prop_assert_eq!(first.message(), second.message());
                prop_assert_eq!(first.start(), second.start());
            }
            _ => prop_assert!(false, "one run parsed and the other did not"),
        }
    }

    /// Deep nesting is bounded rather than recursive to exhaustion. Upstream
    /// throws a `RangeError` here; this returns an error, and the point of the
    /// property is that it returns at all.
    #[test]
    fn deep_nesting_returns_rather_than_overflowing(depth in 1usize..2_000) {
        let input = format!("a={}{}", "[".repeat(depth), "]".repeat(depth));
        let _ = parse_tag(&input);

        let unbalanced = format!("a={}", "{".repeat(depth));
        let _ = parse_tag(&unbalanced);
    }
}
