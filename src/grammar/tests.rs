//! The oracle: `reference/src/grammar/tag.test.ts`, ported case for case.
//!
//! A 1:1 port of a pure function is only verifiable against the assertions
//! upstream wrote for it, so these are those assertions and not a fresh set.
//! The module structure follows upstream's `describe` blocks and the test names
//! follow its `it` strings, so a case here is findable from a case there.
//!
//! What is *not* a transliteration is the JavaScript idiom. Upstream asserts on
//! `example.meta.attributes` and on `toThrowError(SyntaxError)`; here the
//! attributes come out of the [`TagItem`] variant and the error is an `Err`.
//!
//! The last module, [`fidelity`], is additional. Every case in it exists
//! because porting the grammar surfaced a behaviour that upstream's tests do
//! not cover and that a later reader would reasonably mistake for a bug --
//! including the three error messages the conformance corpus asserts verbatim.

use indexmap::IndexMap;

use super::{Attribute, MAX_VALUE_DEPTH, TagItem, parse_tag};
use crate::ast::{Function, PathSegment, Value, Variable};

/// `{ type: 'attribute', name, value }`.
fn attribute(name: &str, value: Value) -> Attribute {
    Attribute::Attribute {
        name: name.to_string(),
        value,
    }
}

/// `{ type: 'class', name, value: true }`.
fn class(name: &str) -> Attribute {
    Attribute::Class {
        name: name.to_string(),
    }
}

fn string(text: &str) -> Value {
    Value::String(text.to_string())
}

fn key(name: &str) -> PathSegment {
    PathSegment::Key(name.to_string())
}

fn index(at: f64) -> PathSegment {
    PathSegment::Index(at)
}

/// `new Variable([...])`.
fn variable(path: Vec<PathSegment>) -> Value {
    Value::Variable(Variable::new(path))
}

fn hash(entries: Vec<(&str, Value)>) -> Value {
    let mut map = IndexMap::new();
    for (name, value) in entries {
        map.insert(name.to_string(), value);
    }
    Value::Hash(map)
}

/// Upstream's `example.meta.attributes`, which its tests read without caring
/// whether the item is an annotation or a tag.
fn attributes(input: &str) -> Vec<Attribute> {
    match parse_tag(input).expect("parses") {
        TagItem::Annotation { attributes } | TagItem::TagOpen { attributes, .. } => attributes,
        other => panic!("expected attributes, got {other:?}"),
    }
}

mod tag_parsing {
    use super::*;

    #[test]
    fn with_a_simple_opening_tag() {
        assert_eq!(
            parse_tag("foo").expect("parses"),
            TagItem::TagOpen {
                name: "foo".to_string(),
                attributes: Vec::new(),
                self_closing: false,
            }
        );
    }

    #[test]
    fn with_an_opening_tag_that_has_attributes() {
        assert_eq!(
            parse_tag("foo foo=1 bar=true").expect("parses"),
            TagItem::TagOpen {
                name: "foo".to_string(),
                attributes: vec![
                    attribute("foo", Value::Number(1.0)),
                    attribute("bar", Value::Boolean(true)),
                ],
                self_closing: false,
            }
        );
    }

    #[test]
    fn with_a_self_closing_tag() {
        assert_eq!(
            parse_tag("foo /").expect("parses"),
            TagItem::TagOpen {
                name: "foo".to_string(),
                attributes: Vec::new(),
                self_closing: true,
            }
        );
    }

    #[test]
    fn with_a_self_closing_tag_that_has_attributes() {
        assert_eq!(
            parse_tag("foo foo=1 bar=true /").expect("parses"),
            TagItem::TagOpen {
                name: "foo".to_string(),
                attributes: vec![
                    attribute("foo", Value::Number(1.0)),
                    attribute("bar", Value::Boolean(true)),
                ],
                self_closing: true,
            }
        );
    }

    #[test]
    fn with_a_closing_tag() {
        assert_eq!(
            parse_tag("/foo").expect("parses"),
            TagItem::TagClose {
                name: "foo".to_string(),
            }
        );
    }

    #[test]
    fn with_an_invalid_closing_tag() {
        assert!(parse_tag("/foo/").is_err());
    }

    #[test]
    fn with_an_invalid_closing_tag_that_has_attributes() {
        assert!(parse_tag("/foo test=1").is_err());
    }
}

mod variable_parsing {
    use super::*;

    /// Upstream asserts `{type: 'variable', meta: {variable}}`; the variant
    /// carries the value directly.
    fn parsed(input: &str) -> Value {
        match parse_tag(input).expect("parses") {
            TagItem::Variable(value) => value,
            other => panic!("expected a variable, got {other:?}"),
        }
    }

    #[test]
    fn with_a_simple_variable() {
        assert_eq!(parsed("$foo"), variable(vec![key("foo")]));
    }

    #[test]
    fn with_multiple_levels_of_depth() {
        assert_eq!(
            parsed("$foo.bar.baz"),
            variable(vec![key("foo"), key("bar"), key("baz")])
        );
    }

    #[test]
    fn with_an_array_index() {
        assert_eq!(parsed("$foo[1]"), variable(vec![key("foo"), index(1.0)]));
    }

    #[test]
    fn with_multiple_array_indexes() {
        assert_eq!(
            parsed("$foo[1][2]"),
            variable(vec![key("foo"), index(1.0), index(2.0)])
        );
    }

    #[test]
    fn with_array_indexes_and_properties() {
        assert_eq!(
            parsed("$foo[1].bar.baz[2].test"),
            variable(vec![
                key("foo"),
                index(1.0),
                key("bar"),
                key("baz"),
                index(2.0),
                key("test"),
            ])
        );
    }

    #[test]
    fn with_an_invalid_array_index() {
        assert!(parse_tag("$foo[asdf]").is_err());
    }

    #[test]
    fn with_an_invalid_namespace() {
        assert!(parse_tag("$.foo:bar.baz").is_err());
    }
}

mod parsing_attributes {
    use super::*;

    #[test]
    fn parsing_annotation_with_a_single_attribute() {
        assert_eq!(
            attributes("test=1"),
            [attribute("test", Value::Number(1.0))]
        );
    }

    #[test]
    fn with_an_id() {
        assert_eq!(attributes("#test"), [attribute("id", string("test"))]);
    }

    #[test]
    fn with_hyphens() {
        assert_eq!(
            attributes("#test-1 .foo-bar"),
            [attribute("id", string("test-1")), class("foo-bar")]
        );
    }

    #[test]
    fn with_chained_classes() {
        assert_eq!(attributes(".foo .bar"), [class("foo"), class("bar")]);
    }

    #[test]
    fn with_chained_id_and_classes() {
        assert_eq!(
            attributes("#test-1 .foo .bar"),
            [
                attribute("id", string("test-1")),
                class("foo"),
                class("bar")
            ]
        );
    }

    #[test]
    fn with_an_invalid_id() {
        assert!(parse_tag("#foo@bar.baz@test").is_err());
    }

    #[test]
    fn with_key_value_pairs() {
        assert_eq!(
            attributes(r#"foo="bar" baz=3 test=true"#),
            [
                attribute("foo", string("bar")),
                attribute("baz", Value::Number(3.0)),
                attribute("test", Value::Boolean(true)),
            ]
        );
    }

    #[test]
    fn with_shortcuts_and_key_value_pairs() {
        assert_eq!(
            attributes(r#"#foo .bar test="asdf""#),
            [
                attribute("id", string("foo")),
                class("bar"),
                attribute("test", string("asdf")),
            ]
        );
    }

    #[test]
    fn with_boolean_key_value_pairs() {
        assert_eq!(
            attributes("test=true foo=false bar=true"),
            [
                attribute("test", Value::Boolean(true)),
                attribute("foo", Value::Boolean(false)),
                attribute("bar", Value::Boolean(true)),
            ]
        );
    }

    #[test]
    fn with_null_key_value_pair() {
        assert_eq!(attributes("foo=null"), [attribute("foo", Value::Null)]);
    }

    /// Upstream's final case, and the one that pins the alternation order: a
    /// bare identifier is not a value, and a number followed by one is a
    /// number with trailing input rather than an identifier.
    #[test]
    fn with_an_invalid_value() {
        for example in ["foo=bar", "foo=a1", "foo=1a"] {
            assert!(parse_tag(example).is_err(), "{example} should not parse");
        }
    }

    mod with_variables_as_values {
        use super::*;

        #[test]
        fn with_a_simple_variable() {
            assert_eq!(
                attributes("test=$foo"),
                [attribute("test", variable(vec![key("foo")]))]
            );
        }

        #[test]
        fn with_multiple_levels_of_depth() {
            assert_eq!(
                attributes("test=$foo.bar.baz"),
                [attribute(
                    "test",
                    variable(vec![key("foo"), key("bar"), key("baz")])
                )]
            );
        }

        #[test]
        fn with_an_array_index() {
            assert_eq!(
                attributes("test=$foo[1]"),
                [attribute("test", variable(vec![key("foo"), index(1.0)]))]
            );
        }

        #[test]
        fn with_multiple_array_indexes() {
            assert_eq!(
                attributes("test=$foo[1][2]"),
                [attribute(
                    "test",
                    variable(vec![key("foo"), index(1.0), index(2.0)])
                )]
            );
        }

        #[test]
        fn with_array_indexes_and_properties() {
            assert_eq!(
                attributes("test=$foo[1].bar.baz[2].test"),
                [attribute(
                    "test",
                    variable(vec![
                        key("foo"),
                        index(1.0),
                        key("bar"),
                        key("baz"),
                        index(2.0),
                        key("test"),
                    ])
                )]
            );
        }

        #[test]
        fn with_an_invalid_array_index() {
            assert!(parse_tag("test=$foo[asdf]").is_err());
        }
    }

    mod with_complex_values {
        use super::*;

        #[test]
        fn with_a_simple_hash_literal_value() {
            assert_eq!(
                attributes("foo={bar: true}"),
                [attribute("foo", hash(vec![("bar", Value::Boolean(true))]))]
            );
        }

        /// Both spellings, spaced and unspaced, because whitespace inside a
        /// hash is optional everywhere the grammar allows it at all.
        #[test]
        fn with_a_nested_hash_literal_value() {
            let expected = [attribute(
                "foo",
                hash(vec![
                    ("bar", Value::Boolean(true)),
                    ("baz", hash(vec![("test", string("this is a test"))])),
                ]),
            )];

            assert_eq!(
                attributes(r#"foo={bar: true, baz: {test: "this is a test"}}"#),
                expected
            );
            assert_eq!(
                attributes(r#"foo={bar:true,baz:{test:"this is a test"}}"#),
                expected
            );
        }

        #[test]
        fn with_a_hash_literal_that_has_string_keys() {
            assert_eq!(
                attributes(r#"foo={bar: true, "baz": 1}"#),
                [attribute(
                    "foo",
                    hash(vec![
                        ("bar", Value::Boolean(true)),
                        ("baz", Value::Number(1.0)),
                    ])
                )]
            );
        }

        #[test]
        fn with_multiple_hash_literal_values() {
            assert_eq!(
                attributes(r#"foo={bar: true} baz={test: "testing"}"#),
                [
                    attribute("foo", hash(vec![("bar", Value::Boolean(true))])),
                    attribute("baz", hash(vec![("test", string("testing"))])),
                ]
            );
        }

        #[test]
        fn with_an_array_literal_value() {
            let expected = [attribute(
                "foo",
                Value::Array(vec![
                    Value::Number(1.0),
                    Value::Number(2.0),
                    Value::Number(3.0),
                ]),
            )];

            assert_eq!(attributes("foo=[1, 2, 3]"), expected);
            assert_eq!(attributes("foo=[1,2,3]"), expected);
        }

        #[test]
        fn with_nested_array_literal_values() {
            let expected = [attribute(
                "foo",
                Value::Array(vec![
                    Value::Number(1.0),
                    Value::Number(2.0),
                    Value::Array(vec![string("test"), Value::Boolean(true), Value::Null]),
                ]),
            )];

            assert_eq!(attributes(r#"foo=[1, 2, ["test", true, null]]"#), expected);
            assert_eq!(attributes(r#"foo=[1,2,["test",true,null]]"#), expected);
        }

        #[test]
        fn with_multiple_nested_array_literal_values() {
            assert_eq!(
                attributes(r#"foo=[1, 2, ["test", true, null]] bar=["baz"]"#),
                [
                    attribute(
                        "foo",
                        Value::Array(vec![
                            Value::Number(1.0),
                            Value::Number(2.0),
                            Value::Array(vec![string("test"), Value::Boolean(true), Value::Null]),
                        ])
                    ),
                    attribute("bar", Value::Array(vec![string("baz")])),
                ]
            );
        }

        #[test]
        fn with_array_and_object_literals() {
            assert_eq!(
                attributes(r#"foo=[1, 2, {bar: "baz", test: [1, 2, 3]}]"#),
                [attribute(
                    "foo",
                    Value::Array(vec![
                        Value::Number(1.0),
                        Value::Number(2.0),
                        hash(vec![
                            ("bar", string("baz")),
                            (
                                "test",
                                Value::Array(vec![
                                    Value::Number(1.0),
                                    Value::Number(2.0),
                                    Value::Number(3.0),
                                ])
                            ),
                        ]),
                    ])
                )]
            );
        }
    }
}

/// `reference/src/tag.test.ts`, and what happens to it in Rust.
///
/// That file has two cases, both about `Tag.isTag`: a runtime type guard that
/// reads a `$$mdtype` property to decide whether an arbitrary JavaScript value
/// is a `Tag`. There is nothing to port. Rust decides that question at compile
/// time, and a guard that answered it at run time would be answering a question
/// the type system does not permit you to ask -- `isTag(8)` has no spelling.
///
/// One half of the file does survive the port, though, and it is the half worth
/// keeping: the guard only works because `$$mdtype` cannot be forged, which is
/// why the grammar discards a `$$mdtype` hash key. That much is testable here,
/// and the case below is upstream's `isTag({my: 'object'}) === false` in the
/// only form this crate can express it.
///
/// Note which spelling the guard is about. `$` is not an identifier character,
/// so a bare `{$$mdtype: ...}` never reaches the rule at all -- it fails as a
/// malformed hash. The quoted key is the one that parses, and the one that is
/// discarded.
///
/// The `Tag` type itself belongs to the renderable tree, which lands with the
/// HTML renderer, not with the grammar.
mod tag_is_tag {
    use super::*;

    #[test]
    fn a_hash_literal_cannot_forge_a_runtime_type_tag() {
        assert_eq!(
            attributes(r#"foo={"$$mdtype": "Tag"}"#),
            [attribute("foo", hash(Vec::new()))]
        );
        // The entry vanishes; its siblings do not.
        assert_eq!(
            attributes(r#"foo={a: 1, "$$mdtype": "Tag", b: 2}"#),
            [attribute(
                "foo",
                hash(vec![("a", Value::Number(1.0)), ("b", Value::Number(2.0))])
            )]
        );
        // The unquoted spelling cannot even be written.
        assert!(parse_tag(r#"foo={$$mdtype: "Tag"}"#).is_err());
    }
}

/// Cases upstream does not have, for behaviour upstream does have.
mod fidelity {
    use super::*;

    /// The three messages the conformance corpus asserts verbatim
    /// (`spec/marktest/tests.yaml`). They are the reason `error.rs` reproduces
    /// peggy's expectation algorithm rather than writing its own message: the
    /// corpus grades the string, and tooling reads it.
    ///
    /// The corpus cases are graded by the tokenizer, which passes the trimmed
    /// tag body; these are those bodies.
    #[test]
    fn the_corpus_error_messages_come_out_verbatim() {
        assert_eq!(
            parse_tag("test foo={,} /").expect_err("fails").message(),
            r#"Expected "}", identifier, string, or whitespace but "," found."#
        );
        assert_eq!(
            parse_tag("test foo=[,] /").expect_err("fails").message(),
            r#"Expected "[", "]", "{", boolean, identifier, null, number, string, variable, or whitespace but "," found."#
        );
        assert_eq!(
            parse_tag("test foo=[1 2 3] /")
                .expect_err("fails")
                .message(),
            r#"Expected ",", "]", or whitespace but "2" found."#
        );
    }

    /// An empty body reaches every alternative of `Top`, so its message names
    /// all of them. This is the expectation machinery's widest case.
    #[test]
    fn an_empty_body_names_every_alternative() {
        assert_eq!(
            parse_tag("").expect_err("fails").message(),
            r#"Expected "/", class, id, identifier, tag name, or variable but end of input found."#
        );
    }

    /// Trailing input is an error, not a shorter parse. A PEG start rule that
    /// matches without consuming the whole input fails, and no later
    /// alternative is tried -- which is why `foo=1a` (above) fails rather than
    /// parsing as a tag named `foo`.
    #[test]
    fn a_matched_prefix_is_still_an_error() {
        let error = parse_tag("$foo bar").expect_err("fails");
        assert_eq!(error.message(), r#"Expected end of input but " " found."#);
        assert_eq!(error.start(), 4);
    }

    /// A failure is reported at the furthest position any rule reached, not at
    /// the position the parse stopped at.
    ///
    /// `foo=1 bar` parses as an annotation of one attribute and stops at offset
    /// 6, but a rule got as far as offset 9 looking for the `=` that would have
    /// made `bar` a second attribute. peggy keeps that high-water mark and
    /// drops every expectation behind it, so the message names the `=` and not
    /// the leftover text.
    #[test]
    fn a_failure_is_reported_at_the_furthest_position_reached() {
        let error = parse_tag("foo=1 bar").expect_err("fails");
        assert_eq!(error.message(), r#"Expected "=" but end of input found."#);
        assert_eq!(error.start(), 9);
        assert_eq!(error.end(), 9);
    }

    /// Upstream unshifts the primary attribute under `if (primary)`, so a
    /// primary value JavaScript calls falsy is parsed and then thrown away. The
    /// value is consumed either way -- these bodies are not errors, they are
    /// tags with no attributes.
    #[test]
    fn a_falsy_primary_value_is_parsed_and_dropped() {
        for example in ["foo 0", "foo false", "foo null", r#"foo """#] {
            assert_eq!(
                parse_tag(example).expect("parses"),
                TagItem::TagOpen {
                    name: "foo".to_string(),
                    attributes: Vec::new(),
                    self_closing: false,
                },
                "{example} should drop its primary value"
            );
        }
    }

    #[test]
    fn a_truthy_primary_value_becomes_the_first_attribute() {
        assert_eq!(
            parse_tag(r#"foo "bar" baz=1"#).expect("parses"),
            TagItem::TagOpen {
                name: "foo".to_string(),
                attributes: vec![
                    attribute("primary", string("bar")),
                    attribute("baz", Value::Number(1.0)),
                ],
                self_closing: false,
            }
        );
        // Empty collections are truthy in JavaScript, so these are kept.
        assert_eq!(
            parse_tag("foo []").expect("parses"),
            TagItem::TagOpen {
                name: "foo".to_string(),
                attributes: vec![attribute("primary", Value::Array(Vec::new()))],
                self_closing: false,
            }
        );
    }

    /// `primary:( value:Value _? )` allows exactly one whitespace character
    /// after the primary value, not `_*`. Two spaces is a syntax error.
    #[test]
    fn only_one_space_may_follow_a_primary_value() {
        assert!(parse_tag(r#"foo "bar" baz=1"#).is_ok());
        assert!(parse_tag(r#"foo "bar"  baz=1"#).is_err());
    }

    /// A trailing comma is permitted in an array and a hash, and nowhere else.
    #[test]
    fn trailing_commas_are_allowed_in_arrays_and_hashes() {
        assert_eq!(
            attributes("foo=[1, 2,]"),
            [attribute(
                "foo",
                Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
            )]
        );
        assert_eq!(
            attributes("foo={i: 1, j: 2,}"),
            [attribute(
                "foo",
                hash(vec![("i", Value::Number(1.0)), ("j", Value::Number(2.0))])
            )]
        );
    }

    /// The parameter list has no `TrailingComma` rule, so `f(1,)` fails where
    /// `[1,]` succeeds. The asymmetry is upstream's.
    #[test]
    fn a_function_parameter_list_has_no_trailing_comma() {
        assert!(parse_tag("foo=f(1,)").is_err());
    }

    /// `Function` has `_*` after the opening parenthesis and none before the
    /// closing one.
    #[test]
    fn whitespace_before_a_closing_parenthesis_is_an_error() {
        assert!(parse_tag("foo=f( 1)").is_ok());
        assert!(parse_tag("foo=f(1 )").is_err());
    }

    /// When the first parameter fails but the tail matches, upstream's action
    /// returns an empty list and keeps what the tail consumed. `f(,1)` is a
    /// call with no parameters rather than a syntax error.
    #[test]
    fn a_leading_comma_swallows_the_parameter_list() {
        assert_eq!(
            attributes("foo=f(,1)"),
            [attribute(
                "foo",
                Value::Function(Function::new("f".to_string(), IndexMap::new()))
            )]
        );
    }

    /// Positional parameters key on their index in the whole argument list,
    /// coerced to a decimal string, and named ones on their name.
    #[test]
    fn parameters_key_on_name_or_positional_index() {
        let mut expected = IndexMap::new();
        expected.insert("0".to_string(), Value::Number(1.0));
        expected.insert("x".to_string(), Value::Number(2.0));
        expected.insert("2".to_string(), string("three"));

        assert_eq!(
            attributes(r#"foo=f(1, x=2, "three")"#),
            [attribute(
                "foo",
                Value::Function(Function::new("f".to_string(), expected))
            )]
        );
    }

    /// A function call in tag position is a `variable` item, not a tag.
    #[test]
    fn a_bare_function_call_is_a_top_level_value() {
        let mut parameters = IndexMap::new();
        parameters.insert("0".to_string(), Value::Number(1.0));
        parameters.insert("1".to_string(), Value::Number(2.0));

        assert_eq!(
            parse_tag("equals(1, 2)").expect("parses"),
            TagItem::Variable(Value::Function(Function::new(
                "equals".to_string(),
                parameters
            )))
        );
    }

    /// An `@`-prefixed path is a plain array of its steps, not a `Variable`.
    /// Upstream returns a bare JavaScript array for it, and both prefixes reach
    /// the same `Top` alternative.
    #[test]
    fn an_at_prefixed_path_is_an_array_not_a_variable() {
        assert_eq!(
            parse_tag("@foo.bar[1]").expect("parses"),
            TagItem::Variable(Value::Array(vec![
                string("foo"),
                string("bar"),
                Value::Number(1.0),
            ]))
        );
    }

    /// A `["key"]` step and a `.key` step are the same path segment. Which
    /// spelling to reprint is the formatter's decision.
    #[test]
    fn a_string_index_is_a_key_step() {
        assert_eq!(
            parse_tag(r#"$foo["bar baz"]"#).expect("parses"),
            TagItem::Variable(variable(vec![key("foo"), key("bar baz")]))
        );
    }

    #[test]
    fn strings_unescape_exactly_five_sequences() {
        assert_eq!(
            attributes(r#"foo="a\"b\\c\nd\re\tf""#),
            [attribute("foo", string("a\"b\\c\nd\re\tf"))]
        );
        // `\u` is not an escape, and a backslash before anything else ends the
        // string body, which then fails to find its closing quote.
        assert!(parse_tag(r#"foo="\u0041""#).is_err());
        assert!(parse_tag(r#"foo="\q""#).is_err());
    }

    /// The whitespace class is space, newline and tab. A carriage return is not
    /// whitespace to this grammar, and a tag body is trimmed before it arrives.
    #[test]
    fn carriage_return_is_not_whitespace() {
        assert!(parse_tag("foo\nbar=1").is_ok());
        assert!(parse_tag("foo\tbar=1").is_ok());
        assert!(parse_tag("foo\rbar=1").is_err());
    }

    /// Numbers are `f64`, with no exponent and no bare leading dot.
    #[test]
    fn numbers_are_parsefloat_shaped() {
        assert_eq!(attributes("a=-1.5"), [attribute("a", Value::Number(-1.5))]);
        assert!(parse_tag("a=1e3").is_err());
        assert!(parse_tag("a=.5").is_err());
        assert!(parse_tag("a=1.").is_err());
    }

    /// Nesting is bounded, which upstream's is not. See `DIVERGENCES.md`.
    #[test]
    fn nesting_deeper_than_the_limit_is_an_error_not_a_stack_overflow() {
        let shallow = format!(
            "a={}{}",
            "[".repeat(MAX_VALUE_DEPTH),
            "]".repeat(MAX_VALUE_DEPTH)
        );
        assert!(parse_tag(&shallow).is_ok());

        let deep = format!(
            "a={}{}",
            "[".repeat(MAX_VALUE_DEPTH + 1),
            "]".repeat(MAX_VALUE_DEPTH + 1)
        );
        let error = parse_tag(&deep).expect_err("fails");
        assert_eq!(
            error.message(),
            format!("Value nesting exceeds the maximum depth of {MAX_VALUE_DEPTH}.")
        );

        // Unbalanced and far past the limit: the bound holds without the
        // closing brackets that make the value well-formed.
        assert!(parse_tag(&format!("a={}", "[".repeat(100_000))).is_err());
    }

    /// Error offsets are byte offsets into the body you passed, and both ends
    /// land on character boundaries even when the offending character is not
    /// ASCII.
    #[test]
    fn error_offsets_are_byte_offsets_on_character_boundaries() {
        let input = "foo=1 é";
        let error = parse_tag(input).expect_err("fails");
        assert_eq!(error.start(), 6);
        assert_eq!(error.end(), 8);
        assert_eq!(input.get(error.start()..error.end()), Some("é"));
        assert_eq!(
            error.message(),
            r#"Expected class, end of input, id, identifier, or whitespace but "é" found."#
        );
    }
}
