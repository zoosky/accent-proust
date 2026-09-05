//! The bindings, exercised in a JavaScript runtime.
//!
//! These run under `wasm32-unknown-unknown` rather than on the host, because
//! the thing worth testing is the boundary: a `js_sys::Object` has no meaning
//! outside a JavaScript engine, and a host-side test of the same mapping would
//! be a test of a different program. `JSON.stringify` is the assertion medium
//! for that reason -- it is the engine reporting what it actually received, and
//! it pins property order, which is what makes the output comparable to
//! upstream's byte for byte.

use accent_proust_wasm::{format, render_html, transform, validate};
use js_sys::{JSON, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

/// A value as the engine serialises it.
fn json(value: &JsValue) -> String {
    JSON::stringify(value).map_or_else(|_| "<not serialisable>".to_owned(), String::from)
}

/// One property, or `undefined` when it is absent.
fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

#[wasm_bindgen_test]
fn renders_html() {
    assert_eq!(
        render_html("# Title {% #intro %}\n"),
        "<article><h1 id=\"intro\">Title </h1></article>"
    );
}

#[wasm_bindgen_test]
fn renders_a_document_that_has_errors() {
    // Upstream renders a document with a critical error rather than refusing
    // to, and a preview pane that blanks on the first mistake is worse than
    // one that shows it. The undefined tag contributes no element; its content
    // still arrives.
    assert_eq!(
        render_html("{% callout %}\nHello\n{% /callout %}\n"),
        "<article><p>Hello</p></article>"
    );
}

#[wasm_bindgen_test]
fn transform_matches_upstreams_tag_shape() {
    // `$$mdtype` first, then `name`, `attributes`, `children`: the declaration
    // order in `reference/src/tag.ts`. A text node is a bare string, not an
    // object, because upstream's `RenderableTreeNode` is `Tag | Scalar`.
    assert_eq!(
        json(&transform("# Title {% #intro %}\n").into()),
        concat!(
            r#"[{"$$mdtype":"Tag","name":"article","attributes":{},"children":"#,
            r#"[{"$$mdtype":"Tag","name":"h1","attributes":{"id":"intro"},"#,
            r#""children":["Title "]}]}]"#
        )
    );
}

#[wasm_bindgen_test]
fn transform_keeps_attribute_order() {
    let tree = transform("[link](https://example.com)\n");
    assert!(
        json(&tree.into()).contains(r#""attributes":{"href":"https://example.com"}"#),
        "attribute did not survive the boundary"
    );
}

#[wasm_bindgen_test]
fn validate_is_empty_for_a_clean_document() {
    assert_eq!(validate("# Title\n").length(), 0);
}

#[wasm_bindgen_test]
fn validate_reports_upstream_error_ids() {
    let errors = validate("{% callout %}\n{% /callout %}\n");
    assert_eq!(errors.length(), 1);

    let first = errors.get(0);
    assert_eq!(get(&first, "type").as_string().as_deref(), Some("tag"));

    // The id is the contract external tooling binds to, so it crosses the
    // boundary unchanged.
    let error = get(&first, "error");
    assert_eq!(
        get(&error, "id").as_string().as_deref(),
        Some("tag-undefined")
    );
    assert_eq!(
        get(&error, "level").as_string().as_deref(),
        Some("critical")
    );
    assert_eq!(
        get(&error, "message").as_string().as_deref(),
        Some("Undefined tag: 'callout'")
    );
}

#[wasm_bindgen_test]
fn validate_carries_a_location() {
    let errors = validate("{% callout %}\n{% /callout %}\n");
    let location = get(&errors.get(0), "location");
    // Zero-based, and `offset` is this crate's addition to upstream's shape.
    assert_eq!(
        json(&location),
        concat!(
            r#"{"start":{"line":0,"character":0,"offset":0},"#,
            r#""end":{"line":1,"character":14,"offset":28}}"#
        )
    );
}

#[wasm_bindgen_test]
fn format_is_canonical() {
    let source = "#    Title\n\n*   one\n*   two\n";
    let once = format(source);
    assert_eq!(format(&once), once, "format is not idempotent");
    assert_eq!(once, "# Title\n\n* one\n* two\n");
}

#[wasm_bindgen_test]
fn a_deep_document_does_not_trap() {
    // The mapping walks a worklist rather than the call stack. A stack overflow
    // in WebAssembly traps and poisons the instance, so depth is the failure
    // this test exists for, not the output.
    let source = format!("{}deep\n", "> ".repeat(200));
    let tree = transform(&source);
    assert_eq!(tree.length(), 1);
    assert_eq!(render_html(&source).matches("<blockquote>").count(), 200);
}
