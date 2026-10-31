//! A link or image may not point at script.
//!
//! Upstream's tokenizer, markdown-it, refuses a `javascript:`, `vbscript:`,
//! `file:` or `data:` destination (`validateLink`), and the link it would have
//! made is text. pulldown-cmark makes the link, so the parser refuses it
//! instead. Each document here was compared with Markdoc 0.5.11.
//!
//! `parse_proptests.rs` states the same property over generated documents.

mod support;

use accent_proust::ast::{Node, NodeType, Value};
use accent_proust::parse::parse;
use accent_proust::{builtins, render, transform};
use support::outline;

/// The single text node a paragraph of `source` should reduce to.
fn text_only(source: &str) -> String {
    format!(
        "\
document
  paragraph
    inline
      text content={:?}
",
        source.trim_end()
    )
}

/// Every `href` and `src` in the tree.
fn destinations<'d>(document: &'d Node<'_>) -> Vec<&'d str> {
    document
        .walk()
        .filter_map(|node| match (node.get("href"), node.get("src")) {
            (Some(Value::String(url)), _) | (_, Some(Value::String(url))) => Some(url.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_link_to_script_is_text() {
    for source in [
        "[a](javascript:alert(1))\n",
        "<javascript:alert(1)>\n",
        "[a](JaVaScRiPt:alert(1))\n",
        "[a](<javascript:alert(1)>)\n",
        "[a](vbscript:x)\n",
        "[a](file:///etc/passwd)\n",
        "[a](data:text/html;base64,PHNjcmlwdD4=)\n",
        "[](javascript:x)\n",
    ] {
        assert_eq!(outline(&parse(source)), text_only(source), "{source:?}");
    }
}

#[test]
fn an_image_of_anything_but_a_bitmap_data_url_is_text() {
    for source in [
        "![i](javascript:alert(1))\n",
        "![i](data:image/svg+xml;base64,PHN2Zz4=)\n",
        // The format must be followed by a parameter, as upstream's pattern has it.
        "![i](data:image/png,AA)\n",
    ] {
        assert_eq!(outline(&parse(source)), text_only(source), "{source:?}");
    }
}

#[test]
fn nothing_a_browser_ignores_can_hide_the_scheme() {
    for source in [
        "[a](jav&#x61;script:alert(1))\n",
        "[a](java&#9;script:alert(1))\n",
        "[a](<java\tscript:alert(1)>)\n",
        "[a](< javascript:alert(1)>)\n",
        "[a](<\u{1}javascript:alert(1)>)\n",
        "[a][r]\n\n[r]: javascript:alert(1)\n",
    ] {
        let document = parse(source);
        assert_eq!(destinations(&document), Vec::<&str>::new(), "{source:?}");
    }
}

#[test]
fn a_refused_autolink_is_read_again_as_text() {
    // `\` is not an escape inside an autolink, but it is in text.
    let source = "<javascript:a\\>\n";
    assert_eq!(outline(&parse(source)), text_only("<javascript:a>"));
}

#[test]
fn a_refused_link_keeps_its_content() {
    let source = "[**a** {% $x %}](javascript:alert\\(1\\) \"t\") after\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"[\"
      strong marker=\"**\"
        text content=\"a\"
      text content=\" \"
      text content=$x
      text content=\"](javascript:alert(1) \\\"t\\\") after\"
"
    );
}

#[test]
fn a_refused_image_and_link_nest_with_safe_ones() {
    let source = "[![i](ok.png)](javascript:x) [![j](javascript:y)](https://ok)\n";
    assert_eq!(
        outline(&parse(source)),
        "\
document
  paragraph
    inline
      text content=\"[\"
      image alt=\"i\" src=\"ok.png\"
      text content=\"](javascript:x) \"
      link href=\"https://ok\"
        text content=\"![j](javascript:y)\"
"
    );
}

#[test]
fn safe_destinations_are_links() {
    let source = "\
[a](https://a.b/javascript:x) [b](/javascript:x) [c](javascript) [d](#x)
<mailto:a@b.c> [e](data:image/png;base64,AA) ![f](DATA:IMAGE/WEBP;base64,AA)
![g](data:image/gif;x) ![h](data:image/jpeg;x)
";
    assert_eq!(
        destinations(&parse(source)),
        [
            "https://a.b/javascript:x",
            "/javascript:x",
            "javascript",
            "#x",
            "mailto:a@b.c",
            "data:image/png;base64,AA",
            "DATA:IMAGE/WEBP;base64,AA",
            "data:image/gif;x",
            "data:image/jpeg;x",
        ]
    );
}

#[test]
fn the_rendered_html_holds_no_script_link() {
    let document = parse("[a](javascript:alert(1)) <javascript:alert(1)>\n");
    let tree = transform::transform(&document, &builtins::config());
    assert_eq!(
        render::render_all(&tree.into_vec()),
        "<article><p>[a](javascript:alert(1)) &lt;javascript:alert(1)&gt;</p></article>"
    );
}

#[test]
fn a_refused_link_is_one_text_node_as_upstream_has_it() {
    let document = parse("x [a](javascript:x) y\n");
    let inline = document
        .walk()
        .find(|node| node.node_type == NodeType::Inline)
        .expect("an inline run");
    assert_eq!(inline.children.len(), 1);
}
