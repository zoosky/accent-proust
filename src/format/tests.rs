//! Unit cover for what a document cannot reach.
//!
//! The oracle for the formatter is `tests/formatter.rs`, upstream's
//! `formatter.test.ts` ported case for case. What lives here is what that file
//! cannot express: the guards against a hand-built tree, and the pieces whose
//! behaviour is a JavaScript detail rather than a Markdoc one.

use indexmap::IndexMap;

use super::{FormatOptions, MAX_FORMAT_DEPTH, format, format_value, format_with};
use crate::ast::{Function, Node, NodeType, PathSegment, Value, Variable};

/// A tag node with a name and attributes.
fn tag(name: &str, attributes: &[(&str, Value)]) -> Node<'static> {
    let mut map = IndexMap::new();
    for (key, value) in attributes {
        map.insert((*key).to_owned(), value.clone());
    }
    Node::with(NodeType::Tag, map, Vec::new(), Some(name.to_owned()))
}

#[test]
fn a_null_value_formats_as_nothing() {
    // Upstream's `expect(format(null)).toBe('')`.
    assert_eq!(format_value(&Value::Null), "");
}

#[test]
fn a_variable_reprints_each_step_in_its_shortest_spelling() {
    let variable = Value::Variable(Variable::new(vec![
        PathSegment::Key("gates".to_owned()),
        PathSegment::Key("a-b".to_owned()),
        PathSegment::Key("with space".to_owned()),
        PathSegment::Index(5.0),
    ]));
    assert_eq!(format_value(&variable), "$gates.a-b[\"with space\"][5]");
}

#[test]
fn a_function_drops_its_parameter_names() {
    // Upstream prints `Object.values(f.parameters)`, so a named parameter
    // reprints positionally. The order is authored order; see `DIVERGENCES.md`
    // entry 10.
    let mut parameters = IndexMap::new();
    parameters.insert("0".to_owned(), Value::String("test".to_owned()));
    parameters.insert("x".to_owned(), Value::Number(1.0));
    let function = Value::Function(Function::new("default".to_owned(), parameters));
    assert_eq!(format_value(&function), "default(\"test\", 1)");
}

#[test]
fn a_hash_key_is_quoted_only_when_it_is_not_an_identifier() {
    let mut inner = IndexMap::new();
    inner.insert("with space".to_owned(), Value::Number(5.0));
    let mut outer = IndexMap::new();
    outer.insert("e".to_owned(), Value::Hash(inner));
    let node = tag("x", &[("b", Value::Hash(outer))]);
    assert_eq!(format(&node).trim(), "{% x b={e: {\"with space\": 5}} /%}");
}

#[test]
fn a_string_attribute_is_json_encoded() {
    let node = tag(
        "x",
        &[("a", Value::String("quote \" slash \\ tab \t".to_owned()))],
    );
    assert_eq!(
        format(&node).trim(),
        "{% x a=\"quote \\\" slash \\\\ tab \\t\" /%}"
    );
}

#[test]
fn an_id_attribute_contracts_only_when_its_value_is_an_identifier() {
    let identifier = tag("x", &[("id", Value::String("intro".to_owned()))]);
    assert_eq!(format(&identifier).trim(), "{% x #intro /%}");

    let with_space = tag("x", &[("id", Value::String("in tro".to_owned()))]);
    assert_eq!(format(&with_space).trim(), "{% x id=\"in tro\" /%}");
}

#[test]
fn a_class_whose_name_is_not_an_identifier_prints_the_whole_hash() {
    // Upstream passes the class *hash* as the value of each class entry rather
    // than the entry's own value, so a name that cannot take the `.` sigil
    // prints `name={...}`. It is a quirk; reproducing it is the point, because
    // inventing a better spelling would emit bytes upstream never does.
    let mut classes = IndexMap::new();
    classes.insert("ok".to_owned(), Value::Boolean(true));
    classes.insert("not ok".to_owned(), Value::Boolean(true));
    let node = tag("x", &[("class", Value::Hash(classes))]);
    assert_eq!(
        format(&node).trim(),
        "{% x .ok not ok={ok: true, \"not ok\": true} /%}"
    );
}

#[test]
fn a_class_attribute_that_is_not_a_hash_stays_an_ordinary_attribute() {
    let node = tag(
        "x",
        &[("class", Value::String("class with space".to_owned()))],
    );
    assert_eq!(format(&node).trim(), "{% x class=\"class with space\" /%}");
}

#[test]
fn the_tag_opening_width_is_a_setting() {
    let node = tag(
        "tag",
        &[
            ("a", Value::Boolean(true)),
            (
                "b",
                Value::String("My very long text well over 80 characters in total".to_owned()),
            ),
        ],
    );
    assert!(format(&node).contains('\n'));

    let wide = FormatOptions::new().max_tag_opening_width(usize::MAX);
    assert_eq!(format_with(&node, &wide).lines().count(), 1);
}

#[test]
fn a_heading_level_is_bounded_rather_than_allocated() {
    // `level` is an ordinary attribute, so a host can set it to anything. An
    // unbounded `"#".repeat` would be an allocation failure, which aborts.
    let mut node = Node::new(NodeType::Heading);
    node.set("level", Value::Number(1e30));
    let formatted = format(&node);
    assert_eq!(formatted.trim().len(), super::MAX_HEADING_LEVEL);

    // Upstream's `|| 1`: an absent, zero or non-numeric level is one mark.
    for level in [Value::Number(0.0), Value::Boolean(true), Value::Null] {
        let mut node = Node::new(NodeType::Heading);
        node.set("level", level);
        assert_eq!(format(&node), "# \n");
    }
}

#[test]
fn a_deeply_nested_tree_formats_without_exhausting_the_stack() {
    // Nesting depth is attacker-controlled, and a stack overflow in Rust aborts
    // and cannot be caught. Past `MAX_FORMAT_DEPTH` a node prints as nothing;
    // its ancestors print normally. See `DIVERGENCES.md` entry 15.
    let mut node = Node::new(NodeType::Paragraph);
    for _ in 0..50_000 {
        node = Node::with(
            NodeType::Tag,
            IndexMap::new(),
            vec![node],
            Some("a".to_owned()),
        );
    }
    let formatted = format(&node);
    // One open and one close per level that was reached, and no more. The
    // deepest one self-closes: its own child is past the bound, so its content
    // comes to nothing and the tag is written `{% a /%}`.
    assert_eq!(formatted.matches("{% a %}").count(), MAX_FORMAT_DEPTH - 1);
    assert_eq!(formatted.matches("{% /a %}").count(), MAX_FORMAT_DEPTH - 1);
    assert_eq!(formatted.matches("{% a /%}").count(), 1);
}

#[test]
fn a_deeply_nested_value_formats_without_exhausting_the_stack() {
    // The same bound over values rather than nodes. The grammar cannot build
    // one this deep -- `MAX_VALUE_DEPTH` is 64 -- but a host can.
    //
    // The depth is a small multiple of the bound rather than the node test's
    // 50,000, and the reason has narrowed rather than gone away. `Value` now
    // carries an iterative `Drop`, so the destructor is no longer what caps
    // this. `Clone` is still derived and therefore recursive, and `tag` clones
    // the value it is handed, so a 50,000-deep value aborts on the way in --
    // measured, not assumed: cloning one overflows with no formatter involved.
    // Guarding the derived traversals is a separate question from guarding
    // destruction; see the panic-freedom note in `lib.rs`.
    let mut value = Value::Number(1.0);
    for _ in 0..MAX_FORMAT_DEPTH * 4 {
        value = Value::Array(vec![value]);
    }
    let node = tag("x", &[("a", value)]);

    // One bracket per level reached, and no more: the value below the bound
    // formats to nothing and the brackets above it still close. The opening
    // wraps, because 128 brackets is well past the tag-opening width.
    let formatted = format(&node);
    assert_eq!(formatted.matches('[').count(), MAX_FORMAT_DEPTH);
    assert_eq!(formatted.matches(']').count(), MAX_FORMAT_DEPTH);
    assert!(formatted.contains("a=["));
}

#[test]
fn an_error_node_prints_nothing() {
    // A tag whose internals did not parse has no source to reprint. Upstream
    // prints nothing for `error` and for a bare `node`, and so does this.
    assert_eq!(format(&Node::new(NodeType::Error)), "");
    assert_eq!(format(&Node::new(NodeType::Node)), "");
}
