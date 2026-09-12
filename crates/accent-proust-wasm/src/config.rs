//! A host's schema configuration, from JavaScript.
//!
//! The bindings default to [`accent_proust::builtins::config`] -- Markdoc's own
//! nodes, tags and functions. That is the right default and the wrong one to be
//! stuck with: pointed at a document written for a host that defines its own
//! components, it reports `tag-undefined` for every one of them, which is
//! correct and useless.
//!
//! This module takes the host's declarations and merges them over the built-ins,
//! which is what upstream does with a user config and is why `{% if %}` and
//! `{% partial %}` keep working after a host adds a tag.
//!
//! # What crosses, and what cannot
//!
//! A schema is data: a name, a list of allowed children, typed attributes, a
//! render policy. All of that crosses.
//!
//! A hook is code. `transform`, `validate`, a custom attribute type and a
//! `RegExp` in `matches` are Rust or JavaScript that has to run inside the
//! validator, and none of it crosses a WebAssembly boundary as a value. So the
//! browser sees what a tag declares and does not see hook-level checking, which
//! means **the editor is never stricter than the server, only faster**. The
//! authority stays where the whole document is.
//!
//! None of that is silently dropped. A configuration carrying a key this crate
//! cannot honour is refused, with the path to it -- a schema that half arrives
//! is worse than one that does not, because the half that is missing is
//! invisible until an author trips over it.

use std::sync::Arc;

use accent_proust::ast::{ErrorLevel, NodeType};
use accent_proust::validate::{
    Config, MapSchemaSource, RenderPolicy, Schema, SchemaAttribute, SchemaMatches, SchemaSlot,
    ValidationType,
};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

use crate::path::Path;
use crate::value;

/// Top-level keys a configuration may carry.
const TOP_LEVEL: &[&str] = &["tags", "nodes", "variables"];

/// Keys a schema may carry.
const SCHEMA_KEYS: &[&str] = &[
    "render",
    "children",
    "attributes",
    "slots",
    "selfClosing",
    "inline",
    "description",
];

/// Keys an attribute declaration may carry.
const ATTRIBUTE_KEYS: &[&str] = &[
    "type",
    "default",
    "required",
    "matches",
    "render",
    "errorLevel",
    "description",
];

/// Keys a slot declaration may carry.
const SLOT_KEYS: &[&str] = &["render", "required"];

/// Build a validator configuration from a host's declarations.
///
/// # Errors
///
/// Returns a message naming the path to the first problem. The configuration
/// is a document a person wrote, so the path is the actionable half of the
/// message and is never omitted.
pub(crate) fn build(value: &JsValue) -> Result<Config<'static>, String> {
    let at = Path::root();

    if value.is_null() || value.is_undefined() {
        return Ok(accent_proust::builtins::config());
    }
    let object = as_object(value, &at)?;
    reject_unknown(&object, TOP_LEVEL, &at)?;

    // Merged over the built-ins into one source, and the config assembled
    // around it at the end: built once, not built and then replaced.
    let mut schemas = MapSchemaSource::builtin();

    if let Some(tags) = property(&object, "tags", &at)? {
        let at = at.child("tags");
        let source = as_object(&tags, &at)?;
        let map = schemas.tags_mut();
        for name in value::keys(&source) {
            let at = at.child(&name);
            let declaration = property(&source, &name, &at)?.unwrap_or(JsValue::UNDEFINED);
            map.insert(name, schema(&declaration, &at)?);
        }
    }

    if let Some(nodes) = property(&object, "nodes", &at)? {
        let at = at.child("nodes");
        let source = as_object(&nodes, &at)?;
        let map = schemas.nodes_mut();
        for name in value::keys(&source) {
            let at = at.child(&name);
            let node = node_key(&name, &at)?;
            let declaration = property(&source, &name, &at)?.unwrap_or(JsValue::UNDEFINED);
            map.insert(node, schema(&declaration, &at)?);
        }
    }

    let mut config = accent_proust::builtins::config_with(Arc::new(schemas));

    if let Some(variables) = property(&object, "variables", &at)? {
        let at = at.child("variables");
        let source = as_object(&variables, &at)?;
        config.variables = Some(value::variables(&source, &at)?);
    }

    Ok(config)
}

/// Resolve a node type by its upstream spelling.
///
/// The library's parser, as the conformance harness uses it, and the
/// library's list for the message: one list to keep in step with the enum,
/// not one per host.
fn node_type(name: &str, at: &Path) -> Result<NodeType, String> {
    NodeType::from_name(name).ok_or_else(|| {
        let known: Vec<&str> = NodeType::ALL.iter().map(|node| node.as_str()).collect();
        format!(
            "{at}: unknown node type {name:?}; expected one of {}",
            known.join(", ")
        )
    })
}

/// Resolve a key of the `nodes` map: a node type, and not `tag`.
///
/// `tag` is a node type -- a schema's `children` may name it, and upstream's
/// `document` does -- but a schema registered for it is never consulted,
/// because a tag is looked up by its name. A declaration under `nodes.tag` is
/// a schema that silently never applies, which is the mistake to refuse by
/// name rather than accept.
fn node_key(name: &str, at: &Path) -> Result<NodeType, String> {
    match node_type(name, at)? {
        NodeType::Tag => Err(format!(
            "{at}: a tag is looked up by its name, never as the node type \"tag\"; declare it under \"tags\""
        )),
        node => Ok(node),
    }
}

/// Convert one schema declaration.
fn schema(value: &JsValue, at: &Path) -> Result<Schema, String> {
    let object = as_object(value, at)?;
    reject_unknown(&object, SCHEMA_KEYS, at)?;

    let mut schema = Schema::default();

    if let Some(render) = property(&object, "render", at)? {
        schema.render = match render_policy(&render, &at.child("render"))? {
            RenderPolicy::Hidden => None,
            RenderPolicy::Renamed(name) => Some(name),
            // A schema's `render` is a name or nothing; `true` has no name to
            // fall back on the way an attribute's does.
            RenderPolicy::Named => {
                return Err(format!(
                    "{}: expected an element name or false, not true",
                    at.child("render")
                ));
            }
        };
    }

    if let Some(children) = property(&object, "children", at)? {
        let at = at.child("children");
        let list = as_array(&children, &at)?;
        let mut allowed = Vec::new();
        for (index, item) in list.iter().enumerate() {
            let at = at.index(u32::try_from(index).unwrap_or(u32::MAX));
            let name = item
                .as_string()
                .ok_or_else(|| format!("{at}: expected a node type name"))?;
            allowed.push(node_type(&name, &at)?);
        }
        schema.children = Some(allowed);
    }

    if let Some(attributes) = property(&object, "attributes", at)? {
        let at = at.child("attributes");
        let source = as_object(&attributes, &at)?;
        for name in value::keys(&source) {
            let at = at.child(&name);
            let declaration = property(&source, &name, &at)?.unwrap_or(JsValue::UNDEFINED);
            schema
                .attributes
                .insert(name, attribute(&declaration, &at)?);
        }
    }

    if let Some(slots) = property(&object, "slots", at)? {
        let at = at.child("slots");
        let source = as_object(&slots, &at)?;
        for name in value::keys(&source) {
            let at = at.child(&name);
            let declaration = property(&source, &name, &at)?.unwrap_or(JsValue::UNDEFINED);
            schema.slots.insert(name, slot(&declaration, &at)?);
        }
    }

    if let Some(flag) = property(&object, "selfClosing", at)? {
        schema.self_closing = boolean(&flag, &at.child("selfClosing"))?;
    }
    if let Some(flag) = property(&object, "inline", at)? {
        schema.inline = Some(boolean(&flag, &at.child("inline"))?);
    }
    if let Some(text) = property(&object, "description", at)? {
        schema.description = Some(string(&text, &at.child("description"))?);
    }

    Ok(schema)
}

/// Convert one attribute declaration.
fn attribute(value: &JsValue, at: &Path) -> Result<SchemaAttribute, String> {
    let object = as_object(value, at)?;
    reject_unknown(&object, ATTRIBUTE_KEYS, at)?;

    let mut attribute = SchemaAttribute::default();

    if let Some(declared) = property(&object, "type", at)? {
        attribute.attribute_type = Some(attribute_type(&declared, &at.child("type"))?);
    }
    if let Some(default) = property(&object, "default", at)? {
        attribute.default = Some(value::value(&default, &at.child("default"))?);
    }
    if let Some(flag) = property(&object, "required", at)? {
        attribute.required = boolean(&flag, &at.child("required"))?;
    }
    if let Some(values) = property(&object, "matches", at)? {
        attribute.matches = Some(matches(&values, &at.child("matches"))?);
    }
    if let Some(render) = property(&object, "render", at)? {
        attribute.render = render_policy(&render, &at.child("render"))?;
    }
    if let Some(level) = property(&object, "errorLevel", at)? {
        attribute.error_level = Some(error_level(&level, &at.child("errorLevel"))?);
    }
    if let Some(text) = property(&object, "description", at)? {
        attribute.description = Some(string(&text, &at.child("description"))?);
    }

    Ok(attribute)
}

/// Convert one slot declaration.
fn slot(value: &JsValue, at: &Path) -> Result<SchemaSlot, String> {
    let object = as_object(value, at)?;
    reject_unknown(&object, SLOT_KEYS, at)?;

    let mut slot = SchemaSlot::default();
    if let Some(render) = property(&object, "render", at)? {
        slot.render = render_policy(&render, &at.child("render"))?;
    }
    if let Some(flag) = property(&object, "required", at)? {
        slot.required = boolean(&flag, &at.child("required"))?;
    }
    Ok(slot)
}

/// Convert an attribute type: a name, or an array of them for a union.
///
/// Upstream writes these as the JavaScript constructors `String`, `Number`,
/// `Boolean`, `Object` and `Array`. A constructor is a function and does not
/// cross, so the name is written as a string -- and the capitalisation is
/// upstream's, so a manifest reads the same on both sides.
fn attribute_type(value: &JsValue, at: &Path) -> Result<ValidationType, String> {
    if Array::is_array(value) {
        let list = Array::from(value);
        let mut union = Vec::new();
        for (index, item) in list.iter().enumerate() {
            let at = at.index(u32::try_from(index).unwrap_or(u32::MAX));
            union.push(attribute_type(&item, &at)?);
        }
        return Ok(ValidationType::Union(union));
    }

    match value.as_string().as_deref() {
        Some("String") => Ok(ValidationType::String),
        Some("Number") => Ok(ValidationType::Number),
        Some("Boolean") => Ok(ValidationType::Boolean),
        Some("Object") => Ok(ValidationType::Object),
        Some("Array") => Ok(ValidationType::Array),
        Some(other) => Err(format!(
            "{at}: unknown attribute type {other:?}; expected String, Number, \
             Boolean, Object, Array, or an array of those"
        )),
        None if value.is_function() => Err(format!(
            "{at}: a custom attribute type is code and cannot cross into \
             WebAssembly; declare one of String, Number, Boolean, Object or \
             Array, and leave the custom check to the server"
        )),
        None => Err(format!(
            "{at}: expected an attribute type name as a string, or an array of them"
        )),
    }
}

/// Convert a `matches` declaration.
fn matches(value: &JsValue, at: &Path) -> Result<SchemaMatches, String> {
    if !Array::is_array(value) {
        return Err(format!(
            "{at}: expected an array of acceptable values. A regular expression \
             is not supported here: the engine carries no regular expression \
             engine on purpose, and a host pattern is code that cannot cross \
             into WebAssembly"
        ));
    }
    let list = Array::from(value);
    let mut accepted = Vec::new();
    for (index, item) in list.iter().enumerate() {
        let at = at.index(u32::try_from(index).unwrap_or(u32::MAX));
        accepted.push(
            item.as_string()
                .ok_or_else(|| format!("{at}: expected a string"))?,
        );
    }
    Ok(SchemaMatches::Values(accepted))
}

/// Convert a render policy: upstream's `true`, `false`, or a replacement name.
fn render_policy(value: &JsValue, at: &Path) -> Result<RenderPolicy, String> {
    if let Some(flag) = value.as_bool() {
        return Ok(if flag {
            RenderPolicy::Named
        } else {
            RenderPolicy::Hidden
        });
    }
    value
        .as_string()
        .map(RenderPolicy::Renamed)
        .ok_or_else(|| format!("{at}: expected true, false, or a name to render under"))
}

/// Convert an error level by its upstream spelling.
fn error_level(value: &JsValue, at: &Path) -> Result<ErrorLevel, String> {
    match value.as_string().as_deref() {
        Some("debug") => Ok(ErrorLevel::Debug),
        Some("info") => Ok(ErrorLevel::Info),
        Some("warning") => Ok(ErrorLevel::Warning),
        Some("error") => Ok(ErrorLevel::Error),
        Some("critical") => Ok(ErrorLevel::Critical),
        _ => Err(format!(
            "{at}: expected one of debug, info, warning, error, critical"
        )),
    }
}

// --- Reading, with the path attached ----------------------------------------

/// The value as an object, or a message saying it is not one.
fn as_object(value: &JsValue, at: &Path) -> Result<Object, String> {
    if value.is_object() && !Array::is_array(value) && !value.is_function() {
        Ok(value.clone().unchecked_into())
    } else {
        Err(format!("{at}: expected an object"))
    }
}

/// The value as an array, or a message saying it is not one.
fn as_array(value: &JsValue, at: &Path) -> Result<Array, String> {
    if Array::is_array(value) {
        Ok(Array::from(value))
    } else {
        Err(format!("{at}: expected an array"))
    }
}

/// The value as a boolean.
fn boolean(value: &JsValue, at: &Path) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("{at}: expected true or false"))
}

/// The value as a string.
fn string(value: &JsValue, at: &Path) -> Result<String, String> {
    value
        .as_string()
        .ok_or_else(|| format!("{at}: expected a string"))
}

/// One property, or [`None`] when it is absent or explicitly `undefined`.
fn property(object: &Object, key: &str, at: &Path) -> Result<Option<JsValue>, String> {
    let value = Reflect::get(object, &JsValue::from_str(key))
        .map_err(|_| format!("{}: cannot be read", at.child(key)))?;
    Ok(if value.is_undefined() {
        None
    } else {
        Some(value)
    })
}

/// Refuse a key this crate cannot honour, naming it and saying why.
///
/// The alternative is a schema that half arrives, and the half that is missing
/// is invisible until an author writes the tag it was meant to check.
fn reject_unknown(object: &Object, allowed: &[&str], at: &Path) -> Result<(), String> {
    for key in value::keys(object) {
        if allowed.contains(&key.as_str()) {
            continue;
        }
        let why = match key.as_str() {
            "transform" | "validate" => {
                " -- a hook is code, and code does not cross into WebAssembly. \
                 Declare what you can and leave the rest to the server, which \
                 sees the whole document"
            }
            "functions" => {
                " -- a function is code. Markdoc's own are already present; a \
                 host's own cannot cross"
            }
            "partials" => {
                " -- partials are not supported yet: a parsed partial borrows \
                 its source, and holding both across the boundary needs a \
                 design this does not have"
            }
            _ => "",
        };
        return Err(format!(
            "{}: unrecognised key{why}. Expected one of {}",
            at.child(&key),
            allowed.join(", ")
        ));
    }
    Ok(())
}
