//! A host's schema configuration, exercised in a JavaScript runtime.
//!
//! The point of the whole feature is that a document written for a host stops
//! being a wall of `tag-undefined`, so most of these assert on what a
//! configured `validate` no longer says as much as on what it does.

use accent_proust_wasm::{HostConfig as Config, validate};
use js_sys::{JSON, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

/// Build a JavaScript object from a JSON string.
///
/// Written as JSON rather than assembled with `Reflect::set` so the tests read
/// as the configuration a host would actually pass.
fn json(text: &str) -> JsValue {
    JSON::parse(text).unwrap_or(JsValue::NULL)
}

/// A value as the engine serialises it.
fn show(value: &JsValue) -> String {
    JSON::stringify(value).map_or_else(|_| "<not serialisable>".to_owned(), String::from)
}

/// One property, or `undefined`.
fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// The message of a thrown error.
fn message(error: &JsValue) -> String {
    get(error, "message").as_string().unwrap_or_default()
}

const CALLOUT: &str = r#"{
  "tags": {
    "callout": {
      "render": "Callout",
      "attributes": {
        "type": { "type": "String", "default": "note",
                  "matches": ["note", "warning"] },
        "title": { "type": "String", "required": true }
      }
    }
  }
}"#;

#[wasm_bindgen_test]
fn an_undefined_tag_is_defined_once_the_host_says_so() {
    let source = "{% callout title=\"Heads up\" %}\nBody.\n{% /callout %}\n";

    // The whole problem, first: without a configuration this is a wall of red.
    assert_eq!(validate(source).length(), 1);

    let config = Config::new(&json(CALLOUT)).unwrap_or_else(|_| unreachable!());
    assert_eq!(config.validate(source).length(), 0);
}

#[wasm_bindgen_test]
fn a_configured_tag_renders_under_its_declared_name() {
    let config = Config::new(&json(CALLOUT)).unwrap_or_else(|_| unreachable!());
    let html = config.render_html("{% callout title=\"Hi\" %}\nBody.\n{% /callout %}\n");
    assert!(
        html.contains("<Callout"),
        "expected the declared element name, got {html}"
    );
}

#[wasm_bindgen_test]
fn a_default_is_applied_and_reaches_the_tree() {
    let config = Config::new(&json(CALLOUT)).unwrap_or_else(|_| unreachable!());
    let tree = config.transform("{% callout title=\"Hi\" /%}\n");
    // `type` was not written, so the declared default is what the host sees.
    assert!(
        show(&tree.into()).contains(r#""type":"note""#),
        "the default did not reach the renderable tree"
    );
}

#[wasm_bindgen_test]
fn declared_attribute_rules_are_enforced() {
    let config = Config::new(&json(CALLOUT)).unwrap_or_else(|_| unreachable!());

    // `matches` is a closed set.
    let errors = config.validate("{% callout title=\"Hi\" type=\"shout\" /%}\n");
    assert_eq!(errors.length(), 1);
    let id = get(&get(&errors.get(0), "error"), "id");
    assert_eq!(id.as_string().as_deref(), Some("attribute-value-invalid"));

    // `required` is enforced.
    let errors = config.validate("{% callout /%}\n");
    assert_eq!(errors.length(), 1);
    let id = get(&get(&errors.get(0), "error"), "id");
    assert_eq!(
        id.as_string().as_deref(),
        Some("attribute-missing-required")
    );

    // And the declared type is.
    let errors = config.validate("{% callout title=42 /%}\n");
    assert_eq!(errors.length(), 1);
    let id = get(&get(&errors.get(0), "error"), "id");
    assert_eq!(id.as_string().as_deref(), Some("attribute-type-invalid"));
}

#[wasm_bindgen_test]
fn the_built_ins_survive_a_host_configuration() {
    // Declarations merge over the built-ins rather than replacing them, which
    // is what upstream does and what keeps a host from having to redeclare
    // `if` to add a tag of its own.
    let config = Config::new(&json(CALLOUT)).unwrap_or_else(|_| unreachable!());
    assert_eq!(
        config.validate("{% if true %}\nyes\n{% /if %}\n").length(),
        0
    );
    assert!(config.render_html("# Title\n").contains("<h1>Title</h1>"));
}

#[wasm_bindgen_test]
fn variables_cross_the_boundary() {
    let config = Config::new(&json(
        r#"{ "variables": { "flags": { "beta": true }, "count": 3 } }"#,
    ))
    .unwrap_or_else(|_| unreachable!());
    let html = config.render_html("{% if $flags.beta %}\nbeta on\n{% /if %}\n");
    assert!(
        html.contains("beta on"),
        "the variable did not arrive: {html}"
    );
}

#[wasm_bindgen_test]
fn a_nested_variable_keeps_its_shape_and_its_key_order() {
    // The inbound walk closes a container over values its children left
    // behind, and closing it in the wrong order produces a plausible wrong
    // answer rather than a crash -- an empty or reshuffled object that only
    // shows up as a condition quietly evaluating false. This pins the shape
    // and the order, which is the only way that failure is visible.
    let config = Config::new(&json(
        r#"{
             "tags": { "box": { "render": "Box",
                                "attributes": { "data": {} },
                                "selfClosing": true } },
             "variables": { "outer": { "z": 1, "a": [ { "deep": "x" }, 2, null ],
                                       "m": { "n": true } } }
           }"#,
    ))
    .unwrap_or_else(|_| unreachable!());

    let tree = config.transform("{% box data=$outer /%}\n");
    assert!(
        show(&tree.into()).contains(r#""data":{"z":1,"a":[{"deep":"x"},2,null],"m":{"n":true}}"#),
        "shape or order changed crossing the boundary"
    );
}

#[wasm_bindgen_test]
fn a_node_schema_can_be_overridden() {
    let config = Config::new(&json(
        r#"{ "nodes": { "heading": { "render": "Heading" } } }"#,
    ))
    .unwrap_or_else(|_| unreachable!());
    assert!(config.render_html("# Title\n").contains("<Heading>"));
}

#[wasm_bindgen_test]
fn an_empty_configuration_is_the_built_ins() {
    let config = Config::new(&JsValue::NULL).unwrap_or_else(|_| unreachable!());
    assert_eq!(config.validate("# Title\n").length(), 0);
    assert_eq!(config.validate("{% callout /%}\n").length(), 1);
}

// --- What is refused, and how it says so ------------------------------------

#[wasm_bindgen_test]
fn a_hook_is_refused_by_name() {
    // Silence here would be the worst outcome: the host would believe its
    // check was running.
    let error = Config::new(&json(
        r#"{ "tags": { "callout": { "render": "Callout" } } }"#,
    ));
    assert!(error.is_ok());

    let object = Object::new();
    let tags = Object::new();
    let callout = Object::new();
    let _ = Reflect::set(
        &callout,
        &JsValue::from_str("validate"),
        &js_sys::Function::new_no_args("return []"),
    );
    let _ = Reflect::set(&tags, &JsValue::from_str("callout"), &callout);
    let _ = Reflect::set(&object, &JsValue::from_str("tags"), &tags);

    let thrown = Config::new(&object.unchecked_into::<JsValue>())
        .err()
        .unwrap_or(JsValue::NULL);
    let text = message(&thrown);
    assert!(text.contains("config.tags.callout.validate"), "{text}");
    assert!(text.contains("code does not cross"), "{text}");
}

#[wasm_bindgen_test]
fn an_error_names_the_path_to_the_problem() {
    let thrown = Config::new(&json(
        r#"{ "tags": { "callout": { "attributes": { "type": { "type": "Str" } } } } }"#,
    ))
    .err()
    .unwrap_or(JsValue::NULL);
    let text = message(&thrown);
    assert!(
        text.contains("config.tags.callout.attributes.type.type"),
        "the path is the actionable half: {text}"
    );
    assert!(text.contains("\"Str\""), "{text}");
}

#[wasm_bindgen_test]
fn partials_and_functions_say_why_rather_than_just_no() {
    let thrown = Config::new(&json(r#"{ "partials": {} }"#))
        .err()
        .unwrap_or(JsValue::NULL);
    assert!(
        message(&thrown).contains("not supported yet"),
        "{}",
        message(&thrown)
    );

    let thrown = Config::new(&json(r#"{ "functions": {} }"#))
        .err()
        .unwrap_or(JsValue::NULL);
    assert!(
        message(&thrown).contains("a function is code"),
        "{}",
        message(&thrown)
    );
}

#[wasm_bindgen_test]
fn an_unknown_node_type_lists_what_is_known() {
    let thrown = Config::new(&json(r#"{ "nodes": { "headline": {} } }"#))
        .err()
        .unwrap_or(JsValue::NULL);
    let text = message(&thrown);
    assert!(text.contains("unknown node type \"headline\""), "{text}");
    assert!(
        text.contains("heading"),
        "it should list the real ones: {text}"
    );
}

#[wasm_bindgen_test]
fn children_may_name_tag_because_upstreams_schemas_do() {
    // Upstream's `document` schema lists `'tag'` among its children, so a host
    // porting an ordinary schema writes it. It must not be told `tag` is
    // unknown -- which it was, while the known names came from the built-in
    // node map, which has no entry for `tag` on purpose.
    let result = Config::new(&json(
        r#"{ "tags": { "section": { "children": ["paragraph", "tag"] } } }"#,
    ));
    assert!(
        result.is_ok(),
        "{}",
        result.as_ref().err().map(message).unwrap_or_default()
    );
}

#[wasm_bindgen_test]
fn a_schema_under_nodes_tag_is_refused_by_name() {
    // A tag is looked up by its name, so a schema registered under the node
    // type `tag` would never apply. Refused, and the message says where it
    // belongs.
    let thrown = Config::new(&json(r#"{ "nodes": { "tag": {} } }"#))
        .err()
        .unwrap_or(JsValue::NULL);
    let text = message(&thrown);
    assert!(text.contains("looked up by its name"), "{text}");
    assert!(text.contains("\"tags\""), "{text}");
}

#[wasm_bindgen_test]
fn a_regexp_in_matches_explains_the_divergence() {
    let object = Object::new();
    let tags = Object::new();
    let callout = Object::new();
    let attributes = Object::new();
    let kind = Object::new();
    let _ = Reflect::set(
        &kind,
        &JsValue::from_str("matches"),
        &js_sys::RegExp::new("^a", "").into(),
    );
    let _ = Reflect::set(&attributes, &JsValue::from_str("type"), &kind);
    let _ = Reflect::set(&callout, &JsValue::from_str("attributes"), &attributes);
    let _ = Reflect::set(&tags, &JsValue::from_str("callout"), &callout);
    let _ = Reflect::set(&object, &JsValue::from_str("tags"), &tags);

    let thrown = Config::new(&object.unchecked_into::<JsValue>())
        .err()
        .unwrap_or(JsValue::NULL);
    let text = message(&thrown);
    assert!(text.contains("regular expression"), "{text}");
}

#[wasm_bindgen_test]
fn a_non_plain_object_is_refused_rather_than_flattened() {
    // `new Date()` stringifies to something; silently converting it to `{}`
    // would be the surprising outcome.
    let object = Object::new();
    let variables = Object::new();
    let _ = Reflect::set(
        &variables,
        &JsValue::from_str("when"),
        &js_sys::Date::new_0().into(),
    );
    let _ = Reflect::set(&object, &JsValue::from_str("variables"), &variables);

    let thrown = Config::new(&object.unchecked_into::<JsValue>())
        .err()
        .unwrap_or(JsValue::NULL);
    let text = message(&thrown);
    assert!(text.contains("config.variables.when"), "{text}");
}
