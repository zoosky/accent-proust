//! The parse-dependent half of upstream's `src/ast/node.test.ts`.
//!
//! The rest of that file is in two other places, because it tests two other
//! things:
//!
//! - **`traversal`** builds a tree by hand and walks it. That needs no parser,
//!   so it lives beside `walk` itself, in `src/ast/node.rs`.
//! - **`transform`**, which is most of the file, tests the transformer. It
//!   lands with that stage.
//!
//! What is here is what needs both a parser and an AST: the walk order over a
//! parsed document, with and without slots, and the ordering of annotations on
//! a node that has several.

mod support;

use accent_proust::ast::Value;
use accent_proust::parse::{parse, parse_with, ParseOptions, PulldownTokenizer};
use support::{at, attribute, dedent};

fn names(document: &accent_proust::ast::Node<'_>) -> Vec<String> {
    document
        .walk()
        .map(|node| node.name().to_string())
        .collect()
}

#[test]
fn walking_a_document_without_slots() {
    let source = dedent("\n{% example %}\n# bar\n\nbaz\n{% /example %}\n");
    let document = parse_with(
        &source,
        &PulldownTokenizer::new(),
        &ParseOptions::new().slots(true),
    );
    assert_eq!(
        names(&document),
        [
            "example",
            "heading",
            "inline",
            "text",
            "paragraph",
            "inline",
            "text"
        ]
    );
}

/// A named slot is lifted out of `children` and walked first.
///
/// The order is upstream's, and it is not cosmetic: a validator reports errors
/// in walk order, so two implementations that disagree here produce different
/// diffs for the same file.
#[test]
fn walking_a_document_with_slots() {
    let source =
        dedent("\n{% example %}\n# bar\n\n{% slot \"foo\" %}\nbaz\n{% /slot %}\n{% /example %}\n");
    let document = parse_with(
        &source,
        &PulldownTokenizer::new(),
        &ParseOptions::new().slots(true),
    );
    assert_eq!(
        names(&document),
        [
            "example",
            "slot",
            "paragraph",
            "inline",
            "text",
            "heading",
            "inline",
            "text"
        ]
    );
}

/// With slots off, the same document keeps the slot tag as an ordinary child.
#[test]
fn a_slot_is_an_ordinary_tag_when_slots_are_off() {
    let source = dedent("\n{% example %}\n{% slot \"foo\" %}\nbaz\n{% /slot %}\n{% /example %}\n");
    let document = parse(&source);
    assert!(at(&document, &[0]).slots.is_empty());
    assert_eq!(at(&document, &[0, 0]).name(), "slot");
}

/// Ported from upstream's `annotations / multiple values should be ordered
/// correctly`.
///
/// Two orders are asserted, and they are different orders on purpose: the
/// attributes are in the order the annotations set them, with `class` appearing
/// where its first class did; the annotation list is the literal sequence as
/// written, which is what the formatter reprints.
#[test]
fn multiple_annotation_values_are_ordered_correctly() {
    let source = "```js {% z=true .class y=2 x=\"1\" #id %} \nContent\n```";
    let document = parse(source);
    let fence = at(&document, &[0]);

    let attributes: Vec<(&str, String)> = fence
        .attributes
        .iter()
        .map(|(name, value)| (name.as_str(), support::show(value)))
        .collect();
    assert_eq!(
        attributes,
        [
            ("content", "\"Content\\n\"".to_string()),
            ("language", "\"js\"".to_string()),
            ("z", "true".to_string()),
            ("class", "{class: true}".to_string()),
            ("y", "2".to_string()),
            ("x", "\"1\"".to_string()),
            ("id", "\"id\"".to_string()),
        ]
    );

    let annotations: Vec<String> = fence
        .annotations
        .iter()
        .map(|annotation| match annotation {
            accent_proust::grammar::Attribute::Attribute { name, value } => {
                format!("{name}={}", support::show(value))
            }
            accent_proust::grammar::Attribute::Class { name } => format!(".{name}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        annotations,
        ["z=true", ".class", "y=2", "x=\"1\"", "id=\"id\""]
    );
}

/// Upstream's `variable.test.ts` asserts the path shape by constructing a
/// `Variable`; here the same shapes are asserted where a document produces
/// them, since that is the half this goal owns.
///
/// Its resolution cases are not ported: `resolve` reads a `Config`, which
/// belongs to the transform stage.
#[test]
fn variable_paths_carry_keys_and_numeric_indices() {
    let source = dedent("{% $foo.baz[1].qux %}");
    let document = parse(&source);
    let text = at(&document, &[0, 0, 0]);
    let Some(Value::Variable(variable)) = text.get("content") else {
        panic!("expected a variable");
    };
    assert_eq!(variable.path.len(), 4);
    assert_eq!(attribute(text, "content"), "$foo.baz[1].qux");
}
