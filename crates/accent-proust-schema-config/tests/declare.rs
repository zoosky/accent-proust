//! The vocabulary, read from the library's own value lattice.
//!
//! `Value` implements `Declaration`, so these tests build a configuration the
//! way a host would hand one over and assert on what comes out -- the schema
//! fields, or the path and kind of the first refusal. The host-specific
//! readings are tested where they live. Nothing here panics on its own
//! fixture: a test returns its failure, so a wrong fixture reads as a wrong
//! fixture and not as a failure in the code under test.

use accent_proust::ast::{ErrorLevel, NodeType, Value};
use accent_proust::validate::{
    MapSchemaSource, RenderPolicy, SchemaKey, SchemaMatches, SchemaSource, ValidationType,
};
use accent_proust_schema_config::{
    ATTRIBUTE_KEYS, Error, ErrorKind, Path, SCHEMA_KEYS, Shape, TOP_LEVEL, declare,
};

type Outcome = Result<(), Box<dyn std::error::Error>>;

/// An object, in authored order.
fn obj(pairs: &[(&str, Value)]) -> Value {
    Value::Hash(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
}

fn s(text: &str) -> Value {
    Value::String(text.to_owned())
}

fn list(items: &[Value]) -> Value {
    Value::Array(items.to_vec())
}

/// The first refusal, or a failure saying there was none.
fn refused(root: &Value) -> Result<Error, String> {
    match declare(root) {
        Ok(declared) => Err(format!("declared without complaint: {declared:?}")),
        Err(error) => Ok(error),
    }
}

#[test]
fn null_declares_nothing() -> Outcome {
    let declared = declare(&Value::Null)?;
    assert!(declared.tags.is_empty());
    assert!(declared.nodes.is_empty());
    assert!(declared.variables.is_none());
    Ok(())
}

#[test]
fn a_tag_declaration_maps_every_key() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[(
            "callout",
            obj(&[
                ("render", s("Callout")),
                ("children", list(&[s("paragraph"), s("tag")])),
                (
                    "attributes",
                    obj(&[(
                        "type",
                        obj(&[
                            ("type", s("String")),
                            ("default", s("note")),
                            ("required", Value::Boolean(true)),
                            ("matches", list(&[s("note"), s("warning")])),
                            ("render", Value::Boolean(true)),
                            ("errorLevel", s("warning")),
                            ("description", s("The kind of callout.")),
                        ]),
                    )]),
                ),
                (
                    "slots",
                    obj(&[(
                        "title",
                        obj(&[
                            ("render", Value::Boolean(false)),
                            ("required", Value::Boolean(true)),
                        ]),
                    )]),
                ),
                ("selfClosing", Value::Boolean(false)),
                ("inline", Value::Boolean(true)),
                ("description", s("A callout.")),
            ]),
        )]),
    )]);

    let declared = declare(&root)?;
    let callout = declared.tags.get("callout").ok_or("no callout")?;
    assert_eq!(callout.render.as_deref(), Some("Callout"));
    assert_eq!(
        callout.children,
        Some(vec![NodeType::Paragraph, NodeType::Tag])
    );
    assert!(!callout.self_closing);
    assert_eq!(callout.inline, Some(true));
    assert_eq!(callout.description.as_deref(), Some("A callout."));

    let kind = callout.attributes.get("type").ok_or("no type attribute")?;
    assert!(matches!(kind.attribute_type, Some(ValidationType::String)));
    assert!(matches!(kind.default, Some(Value::String(ref text)) if text == "note"));
    assert!(kind.required);
    assert!(
        matches!(kind.matches, Some(SchemaMatches::Values(ref values)) if values == &["note", "warning"])
    );
    assert!(matches!(kind.render, RenderPolicy::Named));
    assert!(matches!(kind.error_level, Some(ErrorLevel::Warning)));
    assert_eq!(kind.description.as_deref(), Some("The kind of callout."));

    let title = callout.slots.get("title").ok_or("no title slot")?;
    assert!(matches!(title.render, RenderPolicy::Hidden));
    assert!(title.required);
    Ok(())
}

#[test]
fn a_hook_is_refused_as_a_key_with_its_path() -> Outcome {
    // Refused by name, before anything reads what was written under it: a
    // host sees `UnknownKey` and adds its own reason.
    let root = obj(&[(
        "tags",
        obj(&[("callout", obj(&[("validate", s("whatever"))]))]),
    )]);
    let error = refused(&root)?;
    assert_eq!(error.path.to_string(), "config.tags.callout.validate");
    assert_eq!(
        error.kind,
        ErrorKind::UnknownKey {
            key: "validate".to_owned(),
            expected: SCHEMA_KEYS,
        }
    );
    let text = error.to_string();
    assert!(text.contains("unrecognised key"), "{text}");
    assert!(text.contains("render, children"), "{text}");
    Ok(())
}

#[test]
fn the_top_level_and_attribute_levels_refuse_their_own_unknowns() -> Outcome {
    let error = refused(&obj(&[("functions", obj(&[]))]))?;
    assert_eq!(error.path.to_string(), "config.functions");
    assert!(matches!(error.kind, ErrorKind::UnknownKey { expected, .. } if expected == TOP_LEVEL));

    let root = obj(&[(
        "tags",
        obj(&[(
            "x",
            obj(&[("attributes", obj(&[("a", obj(&[("validate", s("no"))]))]))]),
        )]),
    )]);
    let error = refused(&root)?;
    assert_eq!(
        error.path.to_string(),
        "config.tags.x.attributes.a.validate"
    );
    assert!(
        matches!(error.kind, ErrorKind::UnknownKey { expected, .. } if expected == ATTRIBUTE_KEYS)
    );
    Ok(())
}

#[test]
fn a_schema_under_nodes_tag_is_refused_by_name() -> Outcome {
    let error = refused(&obj(&[("nodes", obj(&[("tag", obj(&[]))]))]))?;
    assert_eq!(error.path.to_string(), "config.nodes.tag");
    assert_eq!(error.kind, ErrorKind::TagAsNodeType);
    let text = error.to_string();
    assert!(text.contains("looked up by its name"), "{text}");
    assert!(text.contains("\"tags\""), "{text}");
    Ok(())
}

#[test]
fn children_may_name_tag_because_upstreams_schemas_do() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[("section", obj(&[("children", list(&[s("tag")]))]))]),
    )]);
    let declared = declare(&root)?;
    assert_eq!(
        declared
            .tags
            .get("section")
            .and_then(|schema| schema.children.clone()),
        Some(vec![NodeType::Tag])
    );
    Ok(())
}

#[test]
fn an_unknown_node_type_lists_the_known_ones() -> Outcome {
    let error = refused(&obj(&[("nodes", obj(&[("headline", obj(&[]))]))]))?;
    assert_eq!(error.path.to_string(), "config.nodes.headline");
    assert_eq!(
        error.kind,
        ErrorKind::UnknownNodeType("headline".to_owned())
    );
    let text = error.to_string();
    assert!(text.contains("unknown node type \"headline\""), "{text}");
    assert!(text.contains("heading"), "{text}");
    Ok(())
}

#[test]
fn an_unknown_attribute_type_names_its_path() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[(
            "callout",
            obj(&[("attributes", obj(&[("type", obj(&[("type", s("Str"))]))]))]),
        )]),
    )]);
    let error = refused(&root)?;
    assert_eq!(
        error.path.to_string(),
        "config.tags.callout.attributes.type.type"
    );
    assert_eq!(
        error.kind,
        ErrorKind::UnknownAttributeType("Str".to_owned())
    );
    assert!(error.to_string().contains("\"Str\""));
    Ok(())
}

#[test]
fn a_union_is_a_list_of_names_one_level_deep() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[(
            "x",
            obj(&[(
                "attributes",
                obj(&[("a", obj(&[("type", list(&[s("String"), s("Number")]))]))]),
            )]),
        )]),
    )]);
    let declared = declare(&root)?;
    let kind = declared
        .tags
        .get("x")
        .and_then(|schema| schema.attributes.get("a"))
        .and_then(|attribute| attribute.attribute_type.clone());
    assert!(
        matches!(
            kind,
            Some(ValidationType::Union(ref members))
                if matches!(members.as_slice(), [ValidationType::String, ValidationType::Number])
        ),
        "expected a union of String and Number"
    );

    let nested = obj(&[(
        "tags",
        obj(&[(
            "x",
            obj(&[(
                "attributes",
                obj(&[("a", obj(&[("type", list(&[list(&[s("String")])]))]))]),
            )]),
        )]),
    )]);
    let error = refused(&nested)?;
    assert_eq!(error.path.to_string(), "config.tags.x.attributes.a.type[0]");
    assert!(matches!(
        error.kind,
        ErrorKind::Expected {
            got: Shape::List,
            ..
        }
    ));
    Ok(())
}

#[test]
fn matches_must_be_a_list_and_says_why_a_pattern_is_not() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[(
            "x",
            obj(&[(
                "attributes",
                obj(&[("a", obj(&[("matches", s("^[a-z]+$"))]))]),
            )]),
        )]),
    )]);
    let error = refused(&root)?;
    assert_eq!(error.path.to_string(), "config.tags.x.attributes.a.matches");
    assert_eq!(error.kind, ErrorKind::MatchesNotAList);
    assert!(error.to_string().contains("regular expression"));
    Ok(())
}

#[test]
fn a_schema_render_of_true_is_refused() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[("x", obj(&[("render", Value::Boolean(true))]))]),
    )]);
    let error = refused(&root)?;
    assert_eq!(error.path.to_string(), "config.tags.x.render");
    assert_eq!(error.kind, ErrorKind::UnrenderableTrue);
    Ok(())
}

#[test]
fn the_wrong_shape_is_named_where_it_is() -> Outcome {
    let error = refused(&obj(&[("tags", s("callout"))]))?;
    assert_eq!(error.path.to_string(), "config.tags");
    assert_eq!(
        error.kind,
        ErrorKind::Expected {
            what: "an object",
            got: Shape::String
        }
    );
    assert_eq!(
        error.to_string(),
        "config.tags: expected an object, not a string"
    );

    let error = refused(&s("not a config"))?;
    assert_eq!(error.path.to_string(), "config");
    assert!(matches!(
        error.kind,
        ErrorKind::Expected {
            what: "an object",
            ..
        }
    ));
    Ok(())
}

#[test]
fn a_non_string_error_level_is_the_wrong_shape_not_an_unknown_name() -> Outcome {
    let root = obj(&[(
        "tags",
        obj(&[(
            "x",
            obj(&[(
                "attributes",
                obj(&[("a", obj(&[("errorLevel", Value::Number(3.0))]))]),
            )]),
        )]),
    )]);
    let error = refused(&root)?;
    assert_eq!(
        error.path.to_string(),
        "config.tags.x.attributes.a.errorLevel"
    );
    assert!(matches!(
        error.kind,
        ErrorKind::Expected {
            got: Shape::Number,
            ..
        }
    ));
    Ok(())
}

#[test]
fn a_reason_is_appended_to_the_vocabularys_sentence() {
    let error = Error::new(
        Path::root().child("tags").child("x").child("validate"),
        ErrorKind::UnknownKey {
            key: "validate".to_owned(),
            expected: SCHEMA_KEYS,
        },
    )
    .explained("a hook is code");
    let text = error.to_string();
    assert!(
        text.starts_with("config.tags.x.validate: unrecognised key."),
        "{text}"
    );
    assert!(text.ends_with(" -- a hook is code"), "{text}");
}

#[test]
fn variables_are_carried_through_whole() -> Outcome {
    let root = obj(&[(
        "variables",
        obj(&[
            ("when", s("now")),
            ("n", Value::Number(3.0)),
            ("nested", obj(&[("a", list(&[Value::Number(1.0)]))])),
        ]),
    )]);
    let declared = declare(&root)?;
    let variables = declared.variables.ok_or("no variables")?;
    assert_eq!(
        variables.keys().collect::<Vec<_>>(),
        ["when", "n", "nested"]
    );
    assert!(matches!(variables.get("n"), Some(Value::Number(n)) if (n - 3.0).abs() < f64::EPSILON));
    Ok(())
}

#[test]
fn a_value_with_no_counterpart_says_so() {
    let error = Error::new(
        Path::root().child("variables").child("d"),
        ErrorKind::NoCounterpart("date".to_owned()),
    );
    let text = error.to_string();
    assert!(
        text.starts_with("config.variables.d: a date has no Markdoc counterpart"),
        "{text}"
    );
}

#[test]
fn apply_merges_over_the_built_ins_keeping_position_and_taking_the_value() -> Outcome {
    // The corpus's own reliance: a redeclared `fence` with no transform hook
    // replaces the built-in hook with nothing. Total replacement, in place.
    let root = obj(&[
        ("nodes", obj(&[("fence", obj(&[("render", s("pre"))]))])),
        ("tags", obj(&[("callout", obj(&[("render", s("aside"))]))])),
        ("variables", obj(&[("x", Value::Boolean(true))])),
    ]);
    let declared = declare(&root)?;

    let mut schemas = MapSchemaSource::builtin();
    let position_before = schemas.nodes().get_index_of(&NodeType::Fence);
    let tags_before = schemas.tags().len();

    let variables = declared.apply(&mut schemas);

    assert_eq!(
        schemas.nodes().get_index_of(&NodeType::Fence),
        position_before
    );
    let fence = schemas.find(SchemaKey::Node(NodeType::Fence));
    assert!(fence.is_some_and(|schema| schema.transform.is_none()));
    assert!(fence.is_some_and(|schema| schema.render.as_deref() == Some("pre")));
    assert_eq!(schemas.tags().len(), tags_before + 1);
    assert!(schemas.find(SchemaKey::Tag("callout")).is_some());
    assert!(schemas.find(SchemaKey::Tag("if")).is_some());
    assert!(variables.is_some_and(|map| map.contains_key("x")));
    Ok(())
}

#[test]
fn the_key_lists_are_the_format() {
    // Cited by both hosts, so a change here is a change to the format.
    assert_eq!(TOP_LEVEL, ["tags", "nodes", "variables"]);
    assert_eq!(
        SCHEMA_KEYS,
        [
            "render",
            "children",
            "attributes",
            "slots",
            "selfClosing",
            "inline",
            "description"
        ]
    );
    assert_eq!(
        ATTRIBUTE_KEYS,
        [
            "type",
            "default",
            "required",
            "matches",
            "render",
            "errorLevel",
            "description"
        ]
    );
}
