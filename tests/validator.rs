//! Upstream's `src/validator.test.ts`, ported.
//!
//! The conformance corpus grades 9 of its 105 cases on validation and 96 on a
//! rendered tree, so it measures the validator barely and by accident. This file
//! is the validator's real gate: it is the assertions upstream wrote about the
//! code being transliterated, which is the only thing that can tell a faithful
//! port from a plausible one.
//!
//! # Two upstream tests are deliberately absent
//!
//! - **"should allow async validators"** asserts that an `async validate()` on a
//!   schema is awaited. `DIVERGENCES.md` entry 3 declines async hooks, so the
//!   test asserts the thing this crate does not do.
//! - **"should validate partial file attributes"** needs the built-in `partial`
//!   tag, which is schema *content* and belongs to the transform stage. It
//!   lands with the built-in schemas, not here.
//!
//! # Where the assertions are stronger than upstream's
//!
//! Upstream mostly asserts with a `toDeepEqualSubset` matcher, which checks that
//! the reported errors contain the listed ones. These tests compare the full
//! sequence of `(id, message)` pairs instead. That is stricter on purpose: the
//! order errors are reported in is observable, and a port that produced the
//! right errors in the wrong order would pass a subset check.
//!
//! # The node schemas here are a stand-in
//!
//! Upstream's `Markdoc.validate` merges its built-in node schemas (`src/schema.ts`)
//! into the caller's config before validating, which is why its tests can pass a
//! config naming only tags. Those schemas are schema *content* and belong to the
//! transform stage; until they land, [`nodes`] declares the few node types these
//! tests actually reach, and only the fields validation reads -- no `render`, no
//! `transform`. Every declaration is copied from upstream's, so a test that
//! passes here passes for upstream's reason rather than a convenient one. When
//! the built-ins land, this function is deleted and they take its place.

use std::sync::Arc;

use indexmap::IndexMap;

use accent_proust::ast::{ErrorLevel, Node, NodeType, ValidationError, Value};
use accent_proust::parse::parse;
use accent_proust::validate::{
    validate_tree, AttributeType, Config, ConfigFunction, RenderPolicy, Schema, SchemaAttribute,
    ValidationType, Variables,
};

/// The built-in node schemas these tests reach, standing in for upstream's.
///
/// See the module docs for why this exists and when it goes away. `children` is
/// left unrestricted rather than copying upstream's lists, because no test here
/// exercises a nesting rule and an unenforced list is easier to remove honestly
/// than a half-copied one.
fn nodes() -> Arc<IndexMap<NodeType, Schema>> {
    let mut nodes = IndexMap::new();
    nodes.insert(NodeType::Document, Schema::new());
    nodes.insert(NodeType::Paragraph, Schema::new());
    nodes.insert(NodeType::Inline, Schema::new());
    nodes.insert(
        NodeType::Text,
        Schema::new().attribute("content", hidden(required(string()))),
    );
    nodes.insert(
        NodeType::Heading,
        Schema::new().attribute("level", hidden(required(number()))),
    );
    nodes.insert(
        NodeType::Fence,
        Schema::new()
            .attribute("content", hidden(required(string())))
            .attribute("language", string()),
    );
    Arc::new(nodes)
}

/// A config with the node schemas above and nothing else.
fn config() -> Config<'static> {
    Config {
        nodes: nodes(),
        ..Config::new()
    }
}

fn string() -> SchemaAttribute {
    typed(ValidationType::String)
}

fn number() -> SchemaAttribute {
    typed(ValidationType::Number)
}

fn typed(attribute_type: ValidationType) -> SchemaAttribute {
    SchemaAttribute {
        attribute_type: Some(attribute_type),
        ..SchemaAttribute::default()
    }
}

fn required(mut attribute: SchemaAttribute) -> SchemaAttribute {
    attribute.required = true;
    attribute
}

fn hidden(mut attribute: SchemaAttribute) -> SchemaAttribute {
    attribute.render = RenderPolicy::Hidden;
    attribute
}

fn tags(pairs: Vec<(&str, Schema)>) -> Arc<IndexMap<String, Schema>> {
    Arc::new(
        pairs
            .into_iter()
            .map(|(name, schema)| (name.to_string(), schema))
            .collect(),
    )
}

fn functions(pairs: Vec<(&str, ConfigFunction)>) -> IndexMap<String, ConfigFunction> {
    pairs
        .into_iter()
        .map(|(name, function)| (name.to_string(), function))
        .collect()
}

fn parameters(pairs: Vec<(&str, SchemaAttribute)>) -> IndexMap<String, SchemaAttribute> {
    pairs
        .into_iter()
        .map(|(name, attribute)| (name.to_string(), attribute))
        .collect()
}

/// Every problem, as `(id, message)` in the order it was reported.
fn errors<'a>(document: &'a Node<'a>, config: &Config<'a>) -> Vec<(&'static str, String)> {
    validate_tree(document, config)
        .into_iter()
        .map(|found| (found.error.id, found.error.message))
        .collect()
}

/// Sugar for the expectation side, so a test reads as a list of pairs.
fn expected(pairs: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    pairs
        .iter()
        .map(|(id, message)| (*id, (*message).to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// function validation
// ---------------------------------------------------------------------------

/// The config upstream's `return type checking` and `checks parameters` blocks
/// share, with function validation switched on.
fn function_config(functions: IndexMap<String, ConfigFunction>) -> Config<'static> {
    let mut config = config();
    config.validation.validate_functions = true;
    config.functions = Arc::new(functions);
    config.tags = tags(vec![
        ("foo", Schema::new().attribute("bar", string())),
        (
            "union-tag-1",
            Schema::new()
                .attribute("foo", string())
                .attribute("bar", number())
                .attribute("baz", typed(ValidationType::Boolean)),
        ),
    ]);
    config
}

fn returns(value_type: ValidationType) -> ConfigFunction {
    ConfigFunction {
        returns: Some(value_type),
        ..ConfigFunction::default()
    }
}

/// The `functions` block upstream declares once for its return-type tests.
fn return_type_functions() -> IndexMap<String, ConfigFunction> {
    functions(vec![
        ("baz", returns(ValidationType::String)),
        ("number", returns(ValidationType::Number)),
        (
            "nested",
            ConfigFunction {
                returns: Some(ValidationType::String),
                parameters: Some(parameters(vec![("0", string()), ("1", number())])),
                ..ConfigFunction::default()
            },
        ),
        (
            "withUnion",
            ConfigFunction {
                returns: Some(ValidationType::Union(vec![
                    ValidationType::String,
                    ValidationType::Number,
                ])),
                parameters: Some(IndexMap::new()),
                ..ConfigFunction::default()
            },
        ),
    ])
}

#[test]
fn ensures_that_function_exists() {
    let config = function_config(IndexMap::new());
    let document = parse("{% foo bar=baz() /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[("function-undefined", "Undefined function: 'baz'")])
    );
}

#[test]
fn correctly_handles_union_types() {
    let config = function_config(return_type_functions());

    let document = parse("{% union-tag-1 foo=withUnion() bar=withUnion() /%}");
    assert!(errors(&document, &config).is_empty());

    // `baz` is typed `Boolean`, which is not one of the union's members.
    let document = parse("{% union-tag-1 foo=withUnion() bar=withUnion() baz=withUnion() /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "attribute-type-invalid",
            "Attribute 'baz' must be type of 'Boolean'"
        )])
    );
}

#[test]
fn correctly_handles_return_types_for_nested_function_calls() {
    let config = function_config(return_type_functions());

    let document = parse("{% foo bar=nested(baz(), number()) /%}");
    assert!(errors(&document, &config).is_empty());

    let document = parse("{% foo bar=nested(number(), baz()) /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[
            (
                "parameter-type-invalid",
                "Parameter '0' of 'nested' must be type of 'String'"
            ),
            (
                "parameter-type-invalid",
                "Parameter '1' of 'nested' must be type of 'Number'"
            ),
        ])
    );
}

#[test]
fn accepts_a_correct_return_type() {
    let config = function_config(return_type_functions());
    let document = parse("{% foo bar=baz() /%}");
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn correctly_handles_no_return_type() {
    let config = function_config(functions(vec![("baz", ConfigFunction::default())]));
    let document = parse("{% foo bar=baz() /%}");
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn identifies_an_incorrect_return_type() {
    let config = function_config(functions(vec![("baz", returns(ValidationType::Number))]));
    let document = parse("{% foo bar=baz() /%}");
    assert_eq!(
        errors(&document, &config)
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        ["attribute-type-invalid"]
    );
}

/// The `checks parameters` block's functions.
fn parameter_functions() -> IndexMap<String, ConfigFunction> {
    functions(vec![
        (
            "baz",
            ConfigFunction {
                returns: Some(ValidationType::String),
                parameters: Some(IndexMap::new()),
                ..ConfigFunction::default()
            },
        ),
        (
            "qux",
            ConfigFunction {
                returns: Some(ValidationType::String),
                parameters: Some(parameters(vec![("test", string())])),
                ..ConfigFunction::default()
            },
        ),
        ("noTyping", ConfigFunction::default()),
        (
            "requiredParam",
            ConfigFunction {
                returns: Some(ValidationType::String),
                parameters: Some(parameters(vec![
                    ("test", string()),
                    ("req", required(string())),
                ])),
                ..ConfigFunction::default()
            },
        ),
    ])
}

#[test]
fn with_a_missing_optional_parameter() {
    let config = function_config(parameter_functions());
    let document = parse("{% foo bar=qux() /%}");
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn with_a_missing_required_parameter() {
    let config = function_config(parameter_functions());
    let document = parse("{% foo bar=requiredParam() /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "parameter-missing-required",
            "Missing required parameter: 'req'"
        )])
    );
}

#[test]
fn accepts_defined_parameters_with_a_keyed_parameter() {
    let config = function_config(parameter_functions());
    let document = parse(r#"{% foo bar=qux(test="example") /%}"#);
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn ignores_parameters_when_there_is_no_typing() {
    let config = function_config(parameter_functions());
    let document = parse("{% foo bar=noTyping(foo=1) /%}");
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn rejects_undeclared_parameters_with_a_keyed_parameter() {
    let config = function_config(parameter_functions());
    let document = parse("{% foo bar=baz(foo=1) /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[("parameter-undefined", "Invalid parameter: 'foo'")])
    );
}

#[test]
fn rejects_undeclared_parameters_with_a_positional_parameter() {
    let config = function_config(parameter_functions());

    let document = parse("{% foo bar=baz(1) /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[("parameter-undefined", "Invalid parameter: '0'")])
    );

    // Upstream's subset assertion names only the positional one. Both are
    // reported, because `baz` declares no parameters at all.
    let document = parse("{% foo bar=baz(1, test=2) /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[
            ("parameter-undefined", "Invalid parameter: '0'"),
            ("parameter-undefined", "Invalid parameter: 'test'"),
        ])
    );
}

// ---------------------------------------------------------------------------
// inline rule
// ---------------------------------------------------------------------------

fn inline_config() -> Config<'static> {
    let mut config = config();
    config.tags = tags(vec![
        (
            "foo",
            Schema {
                inline: Some(true),
                ..Schema::new()
            },
        ),
        (
            "bar",
            Schema {
                inline: Some(false),
                ..Schema::new()
            },
        ),
        ("baz", Schema::new()),
    ]);
    config
}

#[test]
fn allows_inline_or_block_when_undefined() {
    let config = inline_config();

    let document = parse("this is inline {% baz %}bar{% /baz %}");
    assert!(errors(&document, &config).is_empty());

    let document = parse("\n{% baz %}\nbar\n{% /baz %}\n      ");
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn validates_inline_tag() {
    let config = inline_config();

    let document = parse("this is inline {% foo %}bar{% /foo %}");
    assert!(errors(&document, &config).is_empty());

    let document = parse("\n{% foo %}\nbar\n{% /foo %}\n      ");
    let found = errors(&document, &config);
    assert_eq!(
        found.first().map(|(id, _)| *id),
        Some("tag-placement-invalid")
    );
    assert!(found[0].1.contains("should be inline"), "{found:?}");
}

#[test]
fn validates_block_tag() {
    let config = inline_config();

    let document = parse("\n{% bar %}\nbar\n{% /bar %}\n");
    assert!(errors(&document, &config).is_empty());

    let document = parse("this is inline {% bar %}bar{% /bar %}");
    let found = errors(&document, &config);
    assert_eq!(
        found.first().map(|(id, _)| *id),
        Some("tag-placement-invalid")
    );
    assert!(found[0].1.contains("should be block"), "{found:?}");
}

// ---------------------------------------------------------------------------
// attribute validation
// ---------------------------------------------------------------------------

#[test]
fn an_attribute_validate_hook_using_a_simple_conditional() {
    let mut config = config();
    config.tags = tags(vec![(
        "foo",
        Schema::new().attribute(
            "bar",
            SchemaAttribute {
                attribute_type: Some(ValidationType::Number),
                validate: Some(Arc::new(
                    |value: &Value, _config: &Config<'_>, _key: &str| {
                        let greater = matches!(value, Value::Number(n) if *n > 10.0);
                        if greater {
                            return Vec::new();
                        }
                        vec![ValidationError::new(
                            "attribute-should-be-greater-than-ten",
                            ErrorLevel::Error,
                            r#"Attribute "bar" must have value greater than 10."#,
                        )]
                    },
                )),
                ..SchemaAttribute::default()
            },
        ),
    )]);

    let document = parse("{% foo bar=20 /%}");
    assert!(errors(&document, &config).is_empty());

    let document = parse("{% foo bar=5 /%}");
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "attribute-should-be-greater-than-ten",
            r#"Attribute "bar" must have value greater than 10."#
        )])
    );
}

fn matches_config(allowed: Vec<&str>) -> Config<'static> {
    use accent_proust::validate::SchemaMatches;

    let mut config = config();
    config.tags = tags(vec![(
        "foo",
        Schema::new().attribute(
            "jawn",
            SchemaAttribute {
                attribute_type: Some(ValidationType::String),
                matches: Some(SchemaMatches::Values(
                    allowed.into_iter().map(str::to_string).collect(),
                )),
                ..SchemaAttribute::default()
            },
        ),
    )]);
    config
}

#[test]
fn should_return_error_on_failure_to_match_array() {
    let config = matches_config(vec!["bar", "baz", "bat"]);
    let document = parse(r#"{% foo jawn="cat" /%}"#);
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "attribute-value-invalid",
            r#"Attribute 'jawn' must match one of ["bar","baz","bat"]. Got 'cat' instead."#
        )])
    );
}

#[test]
fn elides_excess_values_in_matches_check() {
    // Upstream's `Array.from('foobarbazqux')`: twelve single-character values,
    // four more than the eight the message shows.
    let config = matches_config("foobarbazqux".split("").filter(|s| !s.is_empty()).collect());

    let document = parse(r#"{% foo jawn="cat" /%}"#);
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "attribute-value-invalid",
            r#"Attribute 'jawn' must match one of ["f","o","o","b","a","r","b","a", ... 4 more]. Got 'cat' instead."#
        )])
    );
}

#[test]
fn properly_validates_ids() {
    let config = config();

    let document = parse("# foo {% #bar %}");
    assert!(errors(&document, &config).is_empty());

    let document = parse("# foo {% #1bar %}");
    assert_eq!(
        errors(&document, &config).first().map(|(id, _)| *id),
        Some("attribute-value-invalid")
    );

    let document = parse(r##"# foo {% id="#bar" %}"##);
    assert_eq!(
        errors(&document, &config).first().map(|(id, _)| *id),
        Some("attribute-value-invalid")
    );
}

// ---------------------------------------------------------------------------
// custom type registration
// ---------------------------------------------------------------------------

/// Upstream registers a `Link` class with a `validate` method. Here it is an
/// `AttributeType`, which is the same object with the optionality of its methods
/// made explicit.
struct Link;

impl AttributeType for Link {
    fn name(&self) -> &'static str {
        "Link"
    }

    fn validate<'a>(
        &self,
        value: &Value,
        _config: &Config<'a>,
        _name: &str,
    ) -> Option<Vec<ValidationError<'a>>> {
        if matches!(value, Value::String(text) if text.starts_with("http")) {
            return Some(Vec::new());
        }
        Some(vec![ValidationError::new(
            "attribute-type-invalid",
            ErrorLevel::Error,
            "Attribute 'href' must be type of 'Link'",
        )])
    }
}

#[test]
fn a_custom_type_returns_error_on_failure() {
    let mut config = config();
    config.tags = tags(vec![(
        "link",
        Schema::new()
            .render("a")
            .attribute("href", typed(ValidationType::Custom(Arc::new(Link)))),
    )]);

    let document = parse(r#"{% link href="/relative-link"  /%}"#);
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "attribute-type-invalid",
            "Attribute 'href' must be type of 'Link'"
        )])
    );
}

#[test]
fn a_custom_type_returns_no_errors_when_valid() {
    let mut config = config();
    config.tags = tags(vec![(
        "link",
        Schema {
            self_closing: true,
            ..Schema::new()
                .render("a")
                .attribute("href", typed(ValidationType::Custom(Arc::new(Link))))
        },
    )]);

    let document = parse(r#"{% link href="http://google.com"  /%}"#);
    assert!(errors(&document, &config).is_empty());
}

// ---------------------------------------------------------------------------
// variable validation
// ---------------------------------------------------------------------------

#[test]
fn should_only_validate_if_the_variables_config_is_passed() {
    let config = config();
    let document = parse("{% $valid.variable %}");
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn should_warn_against_missing_variables() {
    let mut config = config();
    config.variables = Some(Variables::new());

    let document = parse("{% $undefinedVariable %}");
    assert_eq!(
        errors(&document, &config),
        expected(&[(
            "variable-undefined",
            "Undefined variable: 'undefinedVariable'"
        )])
    );
    // Upstream also asserts the node the error is attached to.
    assert_eq!(
        validate_tree(&document, &config)
            .first()
            .map(|found| found.node_type),
        Some(NodeType::Text)
    );
}

#[test]
fn should_not_warn_if_variable_exists() {
    let mut config = config();
    let mut valid = IndexMap::new();
    valid.insert("variable".to_string(), Value::Boolean(false));
    let mut variables = Variables::new();
    variables.insert("valid".to_string(), Value::Hash(valid));
    config.variables = Some(variables);

    let document = parse("{% $valid.variable %}");
    assert!(errors(&document, &config).is_empty());
}

// ---------------------------------------------------------------------------
// indented code
// ---------------------------------------------------------------------------

#[test]
fn should_not_error_for_missing_support_for_code_block() {
    // Upstream disables markdown-it's `code` rule, so its parse of this document
    // is a heading followed by a paragraph; here the indented line is a
    // CommonMark indented code block, which is `DIVERGENCES.md` entry 11. The
    // assertion is unaffected -- neither shape produces a validation error --
    // and it is worth keeping for exactly that reason: the difference is in the
    // tree, not in what the validator says about it.
    let config = config();
    let document = parse(
        "   # https://spec.commonmark.org/0.30/#indented-code-block\n    4-space indented code",
    );
    assert!(errors(&document, &config).is_empty());
}

// ---------------------------------------------------------------------------
// attribute validation key
// ---------------------------------------------------------------------------

/// The message both `attribute validation key` tests assert, which is the point
/// of the block: the attribute's *name* reaches the check, so one function can
/// serve several attributes and still say which one it is talking about.
fn less_than_five<'a>(value: &Value, name: &str) -> Vec<ValidationError<'a>> {
    let small = match value {
        Value::Hash(entries) => matches!(entries.get("baz"), Some(Value::Number(n)) if *n < 5.0),
        _ => false,
    };
    if !small {
        return Vec::new();
    }
    vec![ValidationError::new(
        "invalid-foo-bar",
        ErrorLevel::Error,
        format!("The value of '{name}.baz' must be less than five"),
    )]
}

#[test]
fn an_attribute_validate_hook_receives_the_attribute_name() {
    let mut config = config();
    let attribute = || SchemaAttribute {
        attribute_type: Some(ValidationType::Object),
        validate: Some(Arc::new(
            |value: &Value, _config: &Config<'_>, name: &str| less_than_five(value, name),
        )),
        ..SchemaAttribute::default()
    };
    config.tags = tags(vec![(
        "foo",
        Schema::new()
            .attribute("bar", attribute())
            .attribute("blah", attribute()),
    )]);

    let document = parse("{% foo bar={baz: 3} /%}");
    assert_eq!(
        errors(&document, &config)
            .first()
            .map(|(_, message)| message.clone()),
        Some("The value of 'bar.baz' must be less than five".to_string())
    );
}

#[test]
fn a_custom_attribute_type_receives_the_attribute_name() {
    struct CustomType;
    impl AttributeType for CustomType {
        fn name(&self) -> &'static str {
            "CustomType"
        }

        fn validate<'a>(
            &self,
            value: &Value,
            _config: &Config<'a>,
            name: &str,
        ) -> Option<Vec<ValidationError<'a>>> {
            Some(less_than_five(value, name))
        }
    }

    let mut config = config();
    let custom = ValidationType::Custom(Arc::new(CustomType));
    config.tags = tags(vec![(
        "foo",
        Schema::new()
            .attribute("bar", typed(custom.clone()))
            .attribute("blah", typed(custom)),
    )]);

    let document = parse("{% foo bar={baz: 3} /%}");
    assert_eq!(
        errors(&document, &config)
            .first()
            .map(|(_, message)| message.clone()),
        Some("The value of 'bar.baz' must be less than five".to_string())
    );
}

// ---------------------------------------------------------------------------
// parent validation
// ---------------------------------------------------------------------------

#[test]
fn parent_validation_for_deep_nesting() {
    let mut config = config();
    config.tags = tags(vec![
        ("foo", Schema::new()),
        ("bar", Schema::new()),
        ("baz", Schema::new()),
    ]);
    let heading = Schema {
        validate: Some(Arc::new(|_node: &Node<'_>, config: &Config<'_>| {
            if config
                .validation
                .parents
                .iter()
                .any(|parent| parent.tag.as_deref() == Some("foo"))
            {
                return vec![ValidationError::new(
                    "heading-in-foo",
                    ErrorLevel::Error,
                    "Can't nest a heading in tag 'foo'",
                )];
            }
            Vec::new()
        })),
        ..Schema::new().attribute("level", hidden(required(number())))
    };
    config.nodes_mut().insert(NodeType::Heading, heading);

    let document =
        parse("\n{% foo %}\n{% bar %}\n{% baz %}\n# testing\n{% /baz %}\n{% /bar %}\n{% /foo %}\n");
    let found = errors(&document, &config);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "heading-in-foo");

    let document = parse(
        "\n{% foo %}\n{% bar %}\n{% /bar %}\n{% /foo %}\n\n{% bar %}\n{% baz %}\n# testing\n{% /baz %}\n{% /bar %}\n",
    );
    assert!(errors(&document, &config).is_empty());
}

#[test]
fn parent_validation_with_function_validation_enabled() {
    let mut config = config();
    config.validation.validate_functions = true;
    config.tags = tags(vec![
        ("foo", Schema::new()),
        (
            "bar",
            Schema {
                validate: Some(Arc::new(|_node: &Node<'_>, config: &Config<'_>| {
                    let parents: Vec<NodeType> = config
                        .validation
                        .parents
                        .iter()
                        .map(|parent| parent.node_type)
                        .collect();
                    assert_eq!(
                        parents,
                        [
                            NodeType::Document,
                            NodeType::Paragraph,
                            NodeType::Inline,
                            NodeType::Tag
                        ]
                    );
                    Vec::new()
                })),
                ..Schema::new()
            },
        ),
    ]);

    let document = parse("{% foo %}{% bar %}this is a test{% /bar %}{% /foo %}");
    assert!(errors(&document, &config).is_empty());
}
