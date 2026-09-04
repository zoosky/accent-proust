//! Turning a corpus case's `config:` block into a [`Config`].
//!
//! Upstream's runner hands `test.config` straight to `Markdoc.validate`,
//! because a JavaScript object literal already *is* a config. Here it has to be
//! mapped, and the mapping is the part of the harness most able to grade a case
//! against half its own definition -- a schema field read as absent because
//! nothing knows the key is indistinguishable, from the outside, from a schema
//! that did not declare it.
//!
//! So this module is strict in the same way [`crate::corpus`] is: an
//! unrecognised key is a hard error naming itself, and
//! [`crate::check_configs`] runs every case through it before the counts are
//! believed. A corpus refresh that introduces a config shape nothing maps fails
//! with a message saying which, rather than moving the counter.
//!
//! # What it does not build
//!
//! Anything expressed as behaviour. Upstream's config type carries `transform`
//! and `validate` hooks, which a YAML file cannot hold. The built-in node, tag
//! and function schemas are not built here either -- they come from
//! [`accent_proust::builtins::config`], which is `mergeConfig` reached at construction
//! -- and a case's own declarations are merged *over* them, keeping a redeclared
//! key in its built-in position and taking the case's value. That is what
//! JavaScript's `{...nodes, ...config.nodes}` does, and the corpus depends on
//! the replacement being total: "Using a backtick in a fenced code block string
//! attribute" supplies a `fence` schema with no transform hook and expects the
//! built-in hook to be gone with it.

use indexmap::IndexMap;

use accent_proust::ast::{Node, NodeType, ValidationError, Value as AstValue};
use accent_proust::builtins;
use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};
use accent_proust::validate::{
    AttributeType, Config, RenderPolicy, Schema, SchemaAttribute, SchemaMatches, SchemaSlot,
    ValidationType, Variables,
};

use crate::corpus::Case;
use crate::value::Value;

/// Build the config a case is graded against.
///
/// # Errors
///
/// Returns the reason when the case declares something this mapping does not
/// understand. That is always a gap in this harness, never a conformance
/// result.
pub fn build(case: &Case) -> Result<Config<'_>, String> {
    let mut config = builtins::config();
    let Some(source) = &case.config else {
        return Ok(config);
    };
    let Value::Map(entries) = source else {
        return Err(format!("config must be a mapping, got {}", source.kind()));
    };

    for (key, value) in entries {
        match key.as_str() {
            "tags" => config.tags_mut().extend(tags(value)?),
            "nodes" => config.nodes_mut().extend(nodes(value)?),
            "variables" => config.variables = Some(variables(value)?),
            "partials" => config.partials = std::sync::Arc::new(partials(value)?),
            other => {
                return Err(format!(
                    "unknown config key {other:?}. The corpus declares something this harness \
                     does not map, so every case using it would be graded against half its \
                     definition: teach `tests/conformance/config.rs` about it."
                ));
            }
        }
    }
    Ok(config)
}

fn map_entries<'v>(value: &'v Value, what: &str) -> Result<&'v [(String, Value)], String> {
    match value {
        Value::Map(entries) => Ok(entries),
        other => Err(format!("{what} must be a mapping, got {}", other.kind())),
    }
}

fn tags(value: &Value) -> Result<IndexMap<String, Schema>, String> {
    let mut out = IndexMap::new();
    for (name, declaration) in map_entries(value, "tags")? {
        out.insert(name.clone(), schema(declaration)?);
    }
    Ok(out)
}

fn nodes(value: &Value) -> Result<IndexMap<NodeType, Schema>, String> {
    let mut out = IndexMap::new();
    for (name, declaration) in map_entries(value, "nodes")? {
        let node_type = NodeType::from_name(name)
            .ok_or_else(|| format!("nodes: {name:?} is not a node type"))?;
        out.insert(node_type, schema(declaration)?);
    }
    Ok(out)
}

fn schema(value: &Value) -> Result<Schema, String> {
    let mut schema = Schema::new();
    for (key, value) in map_entries(value, "a schema")? {
        match key.as_str() {
            "render" => schema.render = Some(text(value, "render")?),
            "attributes" => schema.attributes = attributes(value)?,
            "slots" => schema.slots = slots(value)?,
            "selfClosing" => schema.self_closing = boolean(value, "selfClosing")?,
            "inline" => schema.inline = Some(boolean(value, "inline")?),
            "children" => schema.children = Some(children(value)?),
            "description" => schema.description = Some(text(value, "description")?),
            other => return Err(format!("unknown schema key {other:?}")),
        }
    }
    Ok(schema)
}

fn children(value: &Value) -> Result<Vec<NodeType>, String> {
    let Value::Seq(items) = value else {
        return Err(format!("children must be a sequence, got {}", value.kind()));
    };
    items
        .iter()
        .map(|item| {
            let name = text(item, "a child node type")?;
            NodeType::from_name(&name)
                .ok_or_else(|| format!("children: {name:?} is not a node type"))
        })
        .collect()
}

fn attributes(value: &Value) -> Result<IndexMap<String, SchemaAttribute>, String> {
    let mut out = IndexMap::new();
    for (name, declaration) in map_entries(value, "attributes")? {
        out.insert(name.clone(), attribute(declaration)?);
    }
    Ok(out)
}

fn attribute(value: &Value) -> Result<SchemaAttribute, String> {
    let mut attribute = SchemaAttribute::default();
    for (key, value) in map_entries(value, "an attribute")? {
        match key.as_str() {
            "type" => attribute.attribute_type = Some(validation_type(value)?),
            "render" => attribute.render = render_policy(value)?,
            "required" => attribute.required = boolean(value, "required")?,
            "default" => attribute.default = Some(ast_value(value)),
            "matches" => attribute.matches = Some(matches(value)?),
            "description" => attribute.description = Some(text(value, "description")?),
            other => return Err(format!("unknown attribute key {other:?}")),
        }
    }
    Ok(attribute)
}

fn slots(value: &Value) -> Result<IndexMap<String, SchemaSlot>, String> {
    let mut out = IndexMap::new();
    for (name, declaration) in map_entries(value, "slots")? {
        let mut slot = SchemaSlot::default();
        for (key, value) in map_entries(declaration, "a slot")? {
            match key.as_str() {
                "render" => slot.render = render_policy(value)?,
                "required" => slot.required = boolean(value, "required")?,
                other => return Err(format!("unknown slot key {other:?}")),
            }
        }
        out.insert(name.clone(), slot);
    }
    Ok(out)
}

fn matches(value: &Value) -> Result<SchemaMatches, String> {
    let Value::Seq(items) = value else {
        // Upstream also accepts a `RegExp` here, which YAML cannot spell and
        // `DIVERGENCES.md` entry 12 replaces with a host-supplied pattern. No
        // corpus case sets `matches` at all.
        return Err(format!(
            "matches must be a sequence of strings, got {}",
            value.kind()
        ));
    };
    items
        .iter()
        .map(|item| text(item, "a matches entry"))
        .collect::<Result<Vec<_>, _>>()
        .map(SchemaMatches::Values)
}

/// A named type, as the corpus writes it.
///
/// The five built-ins are upstream's `TypeMappings`. `Node` is not one of them:
/// upstream looks it up in `TypeMappings`, finds `undefined`, and falls through
/// to `value.constructor === undefined`, which no value satisfies. [`Unmapped`]
/// reproduces that -- a type that accepts nothing -- rather than quietly
/// accepting everything, which is what treating it as absent would do.
fn validation_type(value: &Value) -> Result<ValidationType, String> {
    if let Value::Seq(items) = value {
        return items
            .iter()
            .map(validation_type)
            .collect::<Result<Vec<_>, _>>()
            .map(ValidationType::Union);
    }
    Ok(match text(value, "type")?.as_str() {
        "String" => ValidationType::String,
        "Number" => ValidationType::Number,
        "Boolean" => ValidationType::Boolean,
        "Object" => ValidationType::Object,
        "Array" => ValidationType::Array,
        "Node" => ValidationType::Custom(std::sync::Arc::new(Unmapped)),
        other => return Err(format!("unknown attribute type {other:?}")),
    })
}

/// A type upstream names but does not map, which therefore accepts nothing.
struct Unmapped;

impl AttributeType for Unmapped {
    fn name(&self) -> &'static str {
        "Node"
    }

    fn validate<'a>(
        &self,
        _value: &AstValue,
        _config: &Config<'a>,
        _name: &str,
    ) -> Option<Vec<ValidationError<'a>>> {
        // `None` is "declares no validation", which upstream resolves to
        // rejecting every value. That is exactly what it does with `Node`.
        None
    }
}

fn render_policy(value: &Value) -> Result<RenderPolicy, String> {
    Ok(match value {
        Value::Bool(true) => RenderPolicy::Named,
        Value::Bool(false) => RenderPolicy::Hidden,
        Value::Str(name) => RenderPolicy::Renamed(name.clone()),
        other => {
            return Err(format!(
                "render must be a boolean or a string, got {}",
                other.kind()
            ));
        }
    })
}

fn variables(value: &Value) -> Result<Variables, String> {
    let mut out = Variables::new();
    for (name, value) in map_entries(value, "variables")? {
        out.insert(name.clone(), ast_value(value));
    }
    Ok(out)
}

/// Parse each partial, so the config carries documents rather than text.
///
/// This crate performs no I/O, so a host reads and parses a partial before
/// handing it over. The runner is that host here, and it parses with the same
/// options it parses a case with -- upstream's runner does the same, passing the
/// partial's name as its file.
fn partials(value: &Value) -> Result<IndexMap<String, Node<'_>>, String> {
    let mut out = IndexMap::new();
    for (name, source) in map_entries(value, "partials")? {
        let Value::Str(source) = source else {
            return Err(format!("partial {name:?} must be a string"));
        };
        let options = ParseOptions::new().allow_comments(true).file(name);
        out.insert(
            name.clone(),
            parse_with(source, &PulldownTokenizer::new(), &options),
        );
    }
    Ok(out)
}

/// A corpus value as an AST value.
///
/// Used for `default` and for `variables`, both of which hold ordinary data.
/// The corpus's integers and floats collapse to one `f64`, because the value
/// lattice has one numeric type -- upstream parses every literal with
/// `parseFloat`, so `1` and `1.0` are already the same value there.
fn ast_value(value: &Value) -> AstValue {
    match value {
        Value::Null => AstValue::Null,
        Value::Bool(boolean) => AstValue::Boolean(*boolean),
        #[allow(clippy::cast_precision_loss)]
        Value::Int(integer) => AstValue::Number(*integer as f64),
        Value::Float(float) => AstValue::Number(*float),
        Value::Str(text) => AstValue::String(text.clone()),
        Value::Seq(items) => AstValue::Array(items.iter().map(ast_value).collect()),
        Value::Map(entries) => AstValue::Hash(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), ast_value(value)))
                .collect(),
        ),
    }
}

fn text(value: &Value, what: &str) -> Result<String, String> {
    match value {
        Value::Str(text) => Ok(text.clone()),
        other => Err(format!("{what} must be a string, got {}", other.kind())),
    }
}

fn boolean(value: &Value, what: &str) -> Result<bool, String> {
    match value {
        Value::Bool(boolean) => Ok(*boolean),
        other => Err(format!("{what} must be a boolean, got {}", other.kind())),
    }
}
