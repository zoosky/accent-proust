//! Upstream's `reference/src/renderers/html.test.ts`, ported, plus the cover it
//! does not provide.
//!
//! The 80 lines upstream wrote are the oracle for the transliteration, and they
//! are ported here one case per test, in upstream's order, under
//! [`upstream`]. They are also thin: eleven cases for a renderer whose central
//! act is escaping, and not one of them contains a character that needs
//! escaping. The escaper's own character-level cover lives beside it in
//! `src/render/escape.rs`; what [`beyond_upstream`] adds is the cover for the
//! parts of *this* file -- the full void-element list, the unnamed-tag
//! passthrough, attribute lowercasing, and the two array paths that look alike
//! and are not.
//!
//! Written against the renderable tree, so it runs in the
//! `--no-default-features` lane too: rendering needs no tokenizer.

use indexmap::IndexMap;
use proust::render::{is_void_element, render, render_all, VOID_ELEMENTS};
use proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};

/// Upstream's `tag(name, attributes, children)` test helper.
///
/// Attributes are given as scalars, which is what an ordinary one holds. The
/// cases that need a subtree in an attribute build it explicitly.
fn tag(name: &str, attributes: &[(&str, Scalar)], children: Vec<RenderableTreeNode>) -> Tag {
    let mut map = IndexMap::new();
    for (key, value) in attributes {
        map.insert(
            (*key).to_owned(),
            RenderableTreeNodes::One(RenderableTreeNode::Scalar(value.clone())),
        );
    }
    Tag::with(name, map, children)
}

/// A tag, as a node.
fn node(
    name: &str,
    attributes: &[(&str, Scalar)],
    children: Vec<RenderableTreeNode>,
) -> RenderableTreeNode {
    RenderableTreeNode::tag(tag(name, attributes, children))
}

/// A text child.
fn text(value: &str) -> RenderableTreeNode {
    RenderableTreeNode::text(value)
}

/// A numeric child.
fn number(value: f64) -> RenderableTreeNode {
    RenderableTreeNode::Scalar(Scalar::Number(value))
}

/// A string attribute value.
fn string(value: &str) -> Scalar {
    Scalar::String(value.to_owned())
}

mod upstream {
    use super::*;

    #[test]
    fn rendering_a_tag() {
        let example = render(&node("h1", &[], vec![text("test")]));
        assert_eq!(example.trim(), "<h1>test</h1>");
    }

    #[test]
    fn rendering_string_child_nodes() {
        let example = node("h1", &[], vec![text("test "), text("1")]);
        assert_eq!(render(&example), "<h1>test 1</h1>");
    }

    #[test]
    fn rendering_nested_tags() {
        let example = node("div", &[], vec![node("p", &[], vec![text("test")])]);
        assert_eq!(render(&example), "<div><p>test</p></div>");
    }

    #[test]
    fn rendering_parallel_tags() {
        let example = [
            node("p", &[], vec![text("foo")]),
            node("p", &[], vec![text("bar")]),
        ];
        assert_eq!(render_all(&example), "<p>foo</p><p>bar</p>");
    }

    #[test]
    fn rendering_a_tag_with_an_invalid_child() {
        // Upstream's invalid child is `{ foo: 'bar' }`: an object, which fails
        // `Tag.isTag` and renders as nothing.
        let mut object = IndexMap::new();
        object.insert("foo".to_owned(), string("bar"));
        let example = node(
            "div",
            &[],
            vec![
                text("test"),
                RenderableTreeNode::Scalar(Scalar::Object(object)),
            ],
        );
        assert_eq!(render(&example), "<div>test</div>");
    }

    #[test]
    fn rendering_a_void_element() {
        assert_eq!(render(&node("hr", &[], vec![])), "<hr>");
    }

    #[test]
    fn rendering_a_tag_with_numeric_children() {
        let content = node("p", &[], vec![number(1.0)]);
        assert_eq!(render(&content), "<p>1</p>");
    }

    #[test]
    fn lowercase_attributes() {
        let content = node(
            "td",
            &[
                ("colSpan", Scalar::Number(2.0)),
                ("rowSpan", Scalar::Number(3.0)),
            ],
            vec![text("Data")],
        );
        assert_eq!(render(&content), r#"<td colspan="2" rowspan="3">Data</td>"#);
    }

    #[test]
    fn attributes_with_basic_value() {
        let example = node("foo", &[("bar", string("baz"))], vec![]);
        assert_eq!(render(&example), r#"<foo bar="baz"></foo>"#);
    }

    #[test]
    fn attributes_with_an_id_attribute() {
        let example = node(
            "h1",
            &[("id", string("foo")), ("test", string("bar"))],
            vec![text("test")],
        );
        assert_eq!(render(&example), r#"<h1 id="foo" test="bar">test</h1>"#);
    }

    #[test]
    fn attributes_with_a_number_attribute_value() {
        let example = node(
            "h1",
            &[("data-foo", Scalar::Number(42.0))],
            vec![text("test")],
        );
        assert_eq!(render(&example), r#"<h1 data-foo="42">test</h1>"#);
    }
}

mod beyond_upstream {
    use super::*;

    // --- escaping -------------------------------------------------------

    #[test]
    fn text_children_are_escaped() {
        let example = node("p", &[], vec![text(r#"<a href="x">tom & jerry</a>"#)]);
        assert_eq!(
            render(&example),
            "<p>&lt;a href=&quot;x&quot;&gt;tom &amp; jerry&lt;/a&gt;</p>"
        );
    }

    #[test]
    fn attribute_values_are_escaped() {
        // The corpus case "Escaped quotes in tag strings with html renderer".
        let example = node(
            "foo",
            &[("bar", string(r#"this is a test of "quoted" strings"#))],
            vec![],
        );
        assert_eq!(
            render(&example),
            r#"<foo bar="this is a test of &quot;quoted&quot; strings"></foo>"#
        );
    }

    #[test]
    fn the_apostrophe_is_not_escaped_anywhere() {
        // markdown-it's set is four characters. Adding `&#39;` would be a
        // divergence, and the double-quoted attribute is what makes leaving it
        // out safe.
        let example = node("p", &[("title", string("it's"))], vec![text("it's")]);
        assert_eq!(render(&example), r#"<p title="it's">it's</p>"#);
    }

    #[test]
    fn a_tag_name_is_not_escaped_because_it_cannot_be_authored() {
        // Upstream writes the name raw, and so does this. Recorded as a test
        // rather than left implicit: it is the one place the renderer trusts
        // its input, and it is only safe because the name comes from a schema,
        // never from the document.
        let example = node("my-component", &[], vec![]);
        assert_eq!(render(&example), "<my-component></my-component>");
    }

    // --- void elements --------------------------------------------------

    #[test]
    fn the_void_element_list_is_the_html_standards_fourteen() {
        assert_eq!(
            VOID_ELEMENTS,
            [
                "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
                "param", "source", "track", "wbr",
            ]
        );
    }

    #[test]
    fn every_void_element_renders_without_a_closing_tag() {
        for name in VOID_ELEMENTS {
            assert!(is_void_element(name));
            assert_eq!(render(&node(name, &[], vec![])), format!("<{name}>"));
        }
    }

    #[test]
    fn a_void_element_swallows_its_children() {
        // Upstream returns before rendering them. A tree that gives `<br>` a
        // child is malformed, and the renderer's answer to malformed input is
        // upstream's answer, not a better one.
        let example = node("br", &[], vec![text("ignored")]);
        assert_eq!(render(&example), "<br>");
    }

    #[test]
    fn a_void_element_still_renders_its_attributes() {
        let example = node("img", &[("src", string("a.png"))], vec![]);
        assert_eq!(render(&example), r#"<img src="a.png">"#);
    }

    #[test]
    fn the_void_check_is_case_sensitive() {
        // Only attribute *names* are lowercased. Upstream compares the tag name
        // as authored, so `HR` is an ordinary element with a closing tag.
        assert!(!is_void_element("HR"));
        assert_eq!(render(&node("HR", &[], vec![])), "<HR></HR>");
    }

    #[test]
    fn a_near_miss_is_not_void() {
        for name in ["area51", "bree", "hrs", "input-group", ""] {
            assert!(!is_void_element(name), "{name} must not be void");
        }
    }

    // --- the unnamed tag ------------------------------------------------

    #[test]
    fn an_unnamed_tag_renders_its_children_with_no_wrapper() {
        let example = node("", &[], vec![text("a"), node("b", &[], vec![text("c")])]);
        assert_eq!(render(&example), "a<b>c</b>");
    }

    #[test]
    fn an_unnamed_tag_drops_its_attributes() {
        // `if (!name) return render(children)` returns before the attribute
        // loop, so there is nowhere for them to go.
        let example = node("", &[("id", string("gone"))], vec![text("a")]);
        assert_eq!(render(&example), "a");
    }

    #[test]
    fn an_unnamed_tag_with_no_children_renders_nothing() {
        assert_eq!(render(&node("", &[], vec![])), "");
    }

    #[test]
    fn unnamed_tags_nest() {
        let example = node(
            "div",
            &[],
            vec![node("", &[], vec![node("", &[], vec![text("deep")])])],
        );
        assert_eq!(render(&example), "<div>deep</div>");
    }

    // --- attributes -----------------------------------------------------

    #[test]
    fn attribute_names_are_lowercased_and_values_are_not() {
        let example = node("td", &[("DATA-Foo", string("MiXeD"))], vec![]);
        assert_eq!(render(&example), r#"<td data-foo="MiXeD"></td>"#);
    }

    #[test]
    fn an_already_lowercase_name_is_unchanged() {
        let example = node("td", &[("colspan", Scalar::Number(2.0))], vec![]);
        assert_eq!(render(&example), r#"<td colspan="2"></td>"#);
    }

    #[test]
    fn attribute_order_is_authored_order() {
        // The crate's determinism promise, at the only place a reader sees it.
        let example = node(
            "x",
            &[("z", string("1")), ("a", string("2")), ("m", string("3"))],
            vec![],
        );
        assert_eq!(render(&example), r#"<x z="1" a="2" m="3"></x>"#);
    }

    #[test]
    fn non_string_attribute_values_coerce_the_way_javascript_coerces_them() {
        let example = node(
            "x",
            &[
                ("a", Scalar::Null),
                ("b", Scalar::Boolean(true)),
                ("c", Scalar::Boolean(false)),
                ("d", Scalar::Number(1.5)),
            ],
            vec![],
        );
        assert_eq!(
            render(&example),
            r#"<x a="null" b="true" c="false" d="1.5"></x>"#
        );
    }

    #[test]
    fn an_array_attribute_joins_with_commas() {
        // The corpus case "Rendering HTML with an array attribute".
        let example = node(
            "test",
            &[(
                "foo",
                Scalar::Array(vec![
                    Scalar::Number(1.0),
                    Scalar::Number(2.0),
                    Scalar::Number(3.0),
                ]),
            )],
            vec![node("p", &[], vec![text("This is a test")])],
        );
        assert_eq!(
            render(&example),
            r#"<test foo="1,2,3"><p>This is a test</p></test>"#
        );
    }

    #[test]
    fn a_tag_with_no_attributes_has_no_stray_space() {
        assert_eq!(render(&node("p", &[], vec![])), "<p></p>");
    }

    // --- scalars as children --------------------------------------------

    #[test]
    fn an_array_child_renders_its_elements_rather_than_joining_them() {
        // The distinction that is easy to lose: upstream checks `Array.isArray`
        // before `Tag.isTag`, so an array *child* is rendered element by
        // element. The same array as an *attribute* is `1,2,3`.
        let example = node(
            "p",
            &[],
            vec![RenderableTreeNode::Scalar(Scalar::Array(vec![
                Scalar::Number(1.0),
                Scalar::Number(2.0),
                Scalar::Number(3.0),
            ]))],
        );
        assert_eq!(render(&example), "<p>123</p>");
    }

    #[test]
    fn an_array_child_escapes_its_elements() {
        let example = node(
            "p",
            &[],
            vec![RenderableTreeNode::Scalar(Scalar::Array(vec![
                Scalar::String("<".to_owned()),
                Scalar::String("&".to_owned()),
            ]))],
        );
        assert_eq!(render(&example), "<p>&lt;&amp;</p>");
    }

    #[test]
    fn null_and_boolean_children_render_as_nothing() {
        let example = node(
            "p",
            &[],
            vec![
                RenderableTreeNode::Scalar(Scalar::Null),
                text("a"),
                RenderableTreeNode::Scalar(Scalar::Boolean(true)),
                text("b"),
                RenderableTreeNode::Scalar(Scalar::Boolean(false)),
            ],
        );
        assert_eq!(render(&example), "<p>ab</p>");
    }

    #[test]
    fn a_numeric_child_uses_javascripts_number_formatting() {
        // Rust's `Display` would say `1000000000000000000000` and `0.0000001`.
        let example = node("p", &[], vec![number(1e21), text(" "), number(1e-7)]);
        assert_eq!(render(&example), "<p>1e+21 1e-7</p>");
    }

    #[test]
    fn rendering_nothing_produces_nothing() {
        assert_eq!(render_all(&[]), "");
    }

    // --- depth ----------------------------------------------------------

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        // Nesting depth in a renderable tree comes from the document, which is
        // attacker-controlled. A recursive renderer aborts the process here,
        // and an abort is not something a caller can catch.
        const DEPTH: usize = 50_000;

        let mut tree = node("i", &[], vec![text("x")]);
        for _ in 0..DEPTH {
            tree = node("b", &[], vec![tree]);
        }

        let html = render(&tree);
        assert_eq!(html.matches("<b>").count(), DEPTH);
        assert_eq!(html.matches("</b>").count(), DEPTH);
        assert!(html.starts_with(&"<b>".repeat(DEPTH)));
        assert!(html.ends_with(&format!("<i>x</i>{}", "</b>".repeat(DEPTH))));

        // The tree drops itself here. `Tag` carries a manual iterative `Drop`
        // for the same reason this walk is iterative, so the fixture needs no
        // help unwinding.
    }

    // --- attributes that hold a subtree -----------------------------------

    #[test]
    fn a_tag_in_an_attribute_coerces_to_the_useless_object_string() {
        // A rendered slot is stored in the attribute map as its transformed
        // nodes. Upstream writes `String(v)` over the map, so a tag there is
        // `[object Object]`. Upstream's answer, kept because it is upstream's.
        let mut attributes = IndexMap::new();
        attributes.insert(
            "bar".to_owned(),
            RenderableTreeNodes::One(RenderableTreeNode::tag(tag("p", &[], vec![text("hi")]))),
        );
        let example = RenderableTreeNode::tag(Tag::with("foo", attributes, vec![]));
        assert_eq!(render(&example), r#"<foo bar="[object Object]"></foo>"#);
    }

    #[test]
    fn a_node_list_in_an_attribute_joins_with_commas() {
        let mut attributes = IndexMap::new();
        attributes.insert(
            "bar".to_owned(),
            RenderableTreeNodes::Many(vec![
                RenderableTreeNode::text("a"),
                RenderableTreeNode::tag(tag("p", &[], vec![])),
                RenderableTreeNode::Scalar(Scalar::Null),
                RenderableTreeNode::Scalar(Scalar::Number(2.0)),
            ]),
        );
        let example = RenderableTreeNode::tag(Tag::with("foo", attributes, vec![]));
        assert_eq!(
            render(&example),
            r#"<foo bar="a,[object Object],,2"></foo>"#
        );
    }

    #[test]
    fn an_empty_node_list_in_an_attribute_is_the_empty_string() {
        let mut attributes = IndexMap::new();
        attributes.insert("bar".to_owned(), RenderableTreeNodes::Many(vec![]));
        let example = RenderableTreeNode::tag(Tag::with("foo", attributes, vec![]));
        assert_eq!(render(&example), r#"<foo bar=""></foo>"#);
    }
}
