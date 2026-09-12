//! The keys a declaration may carry, and how each becomes a schema.
//!
//! Ported from the WebAssembly host's first configuration walker, which was
//! the vocabulary's only copy until the command-line host needed one too.
//! The allowlists are public so that a host can cite them; the walk is one
//! function per level, and every refusal names its path.
//!
//! # Depth
//!
//! The walk recurses only as deep as the vocabulary does -- a schema, its
//! attributes, an attribute's `matches` -- which is a constant, not the
//! document's to choose. The two places a declaration carries an arbitrary
//! value, an attribute's `default` and `variables`, are handed through
//! [`Declaration::to_value`] whole, and the library's `Value` clones
//! iteratively.

use accent_proust::ast::{ErrorLevel, NodeType};
use accent_proust::validate::{
    MapSchemaSource, RenderPolicy, Schema, SchemaAttribute, SchemaMatches, SchemaSlot,
    ValidationType, Variables,
};
use indexmap::IndexMap;

use crate::declaration::{Declaration, Shape};
use crate::error::{Error, ErrorKind};
use crate::path::Path;

/// Keys a configuration may carry at the top.
pub const TOP_LEVEL: &[&str] = &["tags", "nodes", "variables"];

/// Keys a schema may carry.
pub const SCHEMA_KEYS: &[&str] = &[
    "render",
    "children",
    "attributes",
    "slots",
    "selfClosing",
    "inline",
    "description",
];

/// Keys an attribute declaration may carry.
pub const ATTRIBUTE_KEYS: &[&str] = &[
    "type",
    "default",
    "required",
    "matches",
    "render",
    "errorLevel",
    "description",
];

/// Keys a slot declaration may carry.
pub const SLOT_KEYS: &[&str] = &["render", "required"];

/// What a configuration declared: the data half of a schema set.
///
/// Schemas and variables, kept apart from any `Config` because the host
/// decides what to do with them -- which built-ins to merge over, where
/// partials come from. [`apply`](Declared::apply) does the one thing both
/// hosts do identically.
#[derive(Debug, Default)]
pub struct Declared {
    /// Tag schemas, by name, in authored order.
    pub tags: IndexMap<String, Schema>,
    /// Node schemas, by type, in authored order.
    pub nodes: IndexMap<NodeType, Schema>,
    /// Variables, when the configuration declared any. `Some` of an empty
    /// map switches variable checking on with nothing defined, as the
    /// library's own `Config` reads it.
    pub variables: Option<Variables>,
}

impl Declared {
    /// Merge the schemas over `schemas` and hand back the variables.
    ///
    /// A redeclared key keeps its position in the source and takes the
    /// declared value, which is what JavaScript's `{...nodes, ...declared}`
    /// does and what the conformance corpus depends on: a `fence` schema
    /// declared without a `transform` hook replaces the built-in hook with
    /// nothing. The replacement is total, on purpose.
    pub fn apply(self, schemas: &mut MapSchemaSource) -> Option<Variables> {
        schemas.tags_mut().extend(self.tags);
        schemas.nodes_mut().extend(self.nodes);
        self.variables
    }
}

/// Read a configuration into what it declares.
///
/// `null` declares nothing, which is what a host passes when it has no
/// configuration of its own and wants the built-ins alone.
///
/// # Errors
///
/// The first problem, with the path to it. Keys are checked before values are
/// read, so an unknown key -- a hook, say -- is refused as a key, whatever
/// was written under it.
pub fn declare<D: Declaration>(root: &D) -> Result<Declared, Error> {
    let at = Path::root();
    let mut declared = Declared::default();

    match root.shape() {
        Shape::Null => return Ok(declared),
        Shape::Object => {}
        _ => return Err(Error::new(at, ErrorKind::Expected("an object"))),
    }
    reject_unknown(root, TOP_LEVEL, &at)?;

    if let Some(tags) = root.get("tags") {
        let at = at.child("tags");
        object(&tags, &at)?;
        for name in tags.keys() {
            let at = at.child(&name);
            let declaration = present(tags.get(&name), &at)?;
            declared.tags.insert(name, schema(&declaration, &at)?);
        }
    }

    if let Some(nodes) = root.get("nodes") {
        let at = at.child("nodes");
        object(&nodes, &at)?;
        for name in nodes.keys() {
            let at = at.child(&name);
            let node = node_key(&name, &at)?;
            let declaration = present(nodes.get(&name), &at)?;
            declared.nodes.insert(node, schema(&declaration, &at)?);
        }
    }

    if let Some(variables) = root.get("variables") {
        let at = at.child("variables");
        object(&variables, &at)?;
        let mut map = Variables::new();
        for name in variables.keys() {
            let at = at.child(&name);
            let value = present(variables.get(&name), &at)?.to_value(&at)?;
            map.insert(name, value);
        }
        declared.variables = Some(map);
    }

    Ok(declared)
}

/// Convert one schema declaration.
fn schema<D: Declaration>(value: &D, at: &Path) -> Result<Schema, Error> {
    object(value, at)?;
    reject_unknown(value, SCHEMA_KEYS, at)?;

    let mut schema = Schema::default();

    if let Some(render) = value.get("render") {
        let at = at.child("render");
        schema.render = match render_policy(&render, &at)? {
            RenderPolicy::Hidden => None,
            RenderPolicy::Renamed(name) => Some(name),
            // A schema's `render` is a name or nothing; `true` has no name to
            // fall back on the way an attribute's does.
            RenderPolicy::Named => return Err(Error::new(at, ErrorKind::UnrenderableTrue)),
        };
    }

    if let Some(children) = value.get("children") {
        let at = at.child("children");
        list(&children, &at)?;
        let mut allowed = Vec::new();
        for (index, item) in children.items().iter().enumerate() {
            let at = at.index(index);
            let name = string(item, &at, "a node type name")?;
            allowed.push(node_type(&name, &at)?);
        }
        schema.children = Some(allowed);
    }

    if let Some(attributes) = value.get("attributes") {
        let at = at.child("attributes");
        object(&attributes, &at)?;
        for name in attributes.keys() {
            let at = at.child(&name);
            let declaration = present(attributes.get(&name), &at)?;
            schema
                .attributes
                .insert(name, attribute(&declaration, &at)?);
        }
    }

    if let Some(slots) = value.get("slots") {
        let at = at.child("slots");
        object(&slots, &at)?;
        for name in slots.keys() {
            let at = at.child(&name);
            let declaration = present(slots.get(&name), &at)?;
            schema.slots.insert(name, slot(&declaration, &at)?);
        }
    }

    if let Some(flag) = value.get("selfClosing") {
        schema.self_closing = boolean(&flag, &at.child("selfClosing"))?;
    }
    if let Some(flag) = value.get("inline") {
        schema.inline = Some(boolean(&flag, &at.child("inline"))?);
    }
    if let Some(text) = value.get("description") {
        schema.description = Some(string(&text, &at.child("description"), "a string")?);
    }

    Ok(schema)
}

/// Convert one attribute declaration.
fn attribute<D: Declaration>(value: &D, at: &Path) -> Result<SchemaAttribute, Error> {
    object(value, at)?;
    reject_unknown(value, ATTRIBUTE_KEYS, at)?;

    let mut attribute = SchemaAttribute::default();

    if let Some(declared) = value.get("type") {
        attribute.attribute_type = Some(attribute_type(&declared, &at.child("type"))?);
    }
    if let Some(default) = value.get("default") {
        let at = at.child("default");
        attribute.default = Some(default.to_value(&at)?);
    }
    if let Some(flag) = value.get("required") {
        attribute.required = boolean(&flag, &at.child("required"))?;
    }
    if let Some(values) = value.get("matches") {
        attribute.matches = Some(matches(&values, &at.child("matches"))?);
    }
    if let Some(render) = value.get("render") {
        attribute.render = render_policy(&render, &at.child("render"))?;
    }
    if let Some(level) = value.get("errorLevel") {
        attribute.error_level = Some(error_level(&level, &at.child("errorLevel"))?);
    }
    if let Some(text) = value.get("description") {
        attribute.description = Some(string(&text, &at.child("description"), "a string")?);
    }

    Ok(attribute)
}

/// Convert one slot declaration.
fn slot<D: Declaration>(value: &D, at: &Path) -> Result<SchemaSlot, Error> {
    object(value, at)?;
    reject_unknown(value, SLOT_KEYS, at)?;

    let mut slot = SchemaSlot::default();
    if let Some(render) = value.get("render") {
        slot.render = render_policy(&render, &at.child("render"))?;
    }
    if let Some(flag) = value.get("required") {
        slot.required = boolean(&flag, &at.child("required"))?;
    }
    Ok(slot)
}

/// Convert an attribute type: a name, or a list of them for a union.
///
/// Upstream writes these as the JavaScript constructors `String`, `Number`,
/// `Boolean`, `Object` and `Array`. A constructor is code and does not cross
/// out of JavaScript, so the name is written as a string -- and the
/// capitalisation is upstream's, so a manifest reads the same on both sides.
/// A union is one level deep: a list inside a list is refused rather than
/// walked, because upstream never writes one and the walk would otherwise be
/// as deep as the document chose.
fn attribute_type<D: Declaration>(value: &D, at: &Path) -> Result<ValidationType, Error> {
    if value.shape() == Shape::List {
        let mut union = Vec::new();
        for (index, item) in value.items().iter().enumerate() {
            let at = at.index(index);
            let name = string(item, &at, "an attribute type name inside a union")?;
            union.push(type_name(&name, &at)?);
        }
        return Ok(ValidationType::Union(union));
    }
    match value.as_str() {
        Some(name) => type_name(&name, at),
        None => Err(Error::new(
            at.clone(),
            ErrorKind::Expected("an attribute type name as a string, or an array of them"),
        )),
    }
}

/// One of the five type names.
fn type_name(name: &str, at: &Path) -> Result<ValidationType, Error> {
    match name {
        "String" => Ok(ValidationType::String),
        "Number" => Ok(ValidationType::Number),
        "Boolean" => Ok(ValidationType::Boolean),
        "Object" => Ok(ValidationType::Object),
        "Array" => Ok(ValidationType::Array),
        other => Err(Error::new(
            at.clone(),
            ErrorKind::UnknownAttributeType(other.to_owned()),
        )),
    }
}

/// Convert a `matches` declaration: a list of acceptable values.
fn matches<D: Declaration>(value: &D, at: &Path) -> Result<SchemaMatches, Error> {
    if value.shape() != Shape::List {
        return Err(Error::new(at.clone(), ErrorKind::MatchesNotAList));
    }
    let mut accepted = Vec::new();
    for (index, item) in value.items().iter().enumerate() {
        let at = at.index(index);
        accepted.push(string(item, &at, "a string")?);
    }
    Ok(SchemaMatches::Values(accepted))
}

/// Convert a render policy: upstream's `true`, `false`, or a replacement name.
fn render_policy<D: Declaration>(value: &D, at: &Path) -> Result<RenderPolicy, Error> {
    if let Some(flag) = value.as_bool() {
        return Ok(if flag {
            RenderPolicy::Named
        } else {
            RenderPolicy::Hidden
        });
    }
    value.as_str().map(RenderPolicy::Renamed).ok_or_else(|| {
        Error::new(
            at.clone(),
            ErrorKind::Expected("true, false, or a name to render under"),
        )
    })
}

/// Convert an error level by its upstream spelling.
fn error_level<D: Declaration>(value: &D, at: &Path) -> Result<ErrorLevel, Error> {
    let name = value.as_str().unwrap_or_default();
    match name.as_str() {
        "debug" => Ok(ErrorLevel::Debug),
        "info" => Ok(ErrorLevel::Info),
        "warning" => Ok(ErrorLevel::Warning),
        "error" => Ok(ErrorLevel::Error),
        "critical" => Ok(ErrorLevel::Critical),
        _ => Err(Error::new(at.clone(), ErrorKind::UnknownErrorLevel(name))),
    }
}

/// Resolve a node type by its upstream spelling.
fn node_type(name: &str, at: &Path) -> Result<NodeType, Error> {
    NodeType::from_name(name)
        .ok_or_else(|| Error::new(at.clone(), ErrorKind::UnknownNodeType(name.to_owned())))
}

/// Resolve a key of the `nodes` map: a node type, and not `tag`.
///
/// `tag` is a node type -- a schema's `children` may name it, and upstream's
/// `document` does -- but a schema registered for it is never consulted,
/// because a tag is looked up by its name. A declaration under `nodes.tag` is
/// a schema that silently never applies, which is the mistake to refuse by
/// name rather than accept.
fn node_key(name: &str, at: &Path) -> Result<NodeType, Error> {
    match node_type(name, at)? {
        NodeType::Tag => Err(Error::new(at.clone(), ErrorKind::TagAsNodeType)),
        node => Ok(node),
    }
}

// --- Reading, with the path attached ----------------------------------------

/// A property that `keys` listed but `get` did not return: explicitly
/// `undefined`, in JavaScript. Refused as the object it was meant to be.
fn present<D: Declaration>(value: Option<D>, at: &Path) -> Result<D, Error> {
    value.ok_or_else(|| Error::new(at.clone(), ErrorKind::Expected("an object")))
}

/// Refuse anything that is not an object.
fn object<D: Declaration>(value: &D, at: &Path) -> Result<(), Error> {
    if value.shape() == Shape::Object {
        Ok(())
    } else {
        Err(Error::new(at.clone(), ErrorKind::Expected("an object")))
    }
}

/// Refuse anything that is not a list.
fn list<D: Declaration>(value: &D, at: &Path) -> Result<(), Error> {
    if value.shape() == Shape::List {
        Ok(())
    } else {
        Err(Error::new(at.clone(), ErrorKind::Expected("an array")))
    }
}

/// The value as a boolean.
fn boolean<D: Declaration>(value: &D, at: &Path) -> Result<bool, Error> {
    value
        .as_bool()
        .ok_or_else(|| Error::new(at.clone(), ErrorKind::Expected("true or false")))
}

/// The value as a string, with `what` saying which string was wanted.
fn string<D: Declaration>(value: &D, at: &Path, what: &'static str) -> Result<String, Error> {
    value
        .as_str()
        .ok_or_else(|| Error::new(at.clone(), ErrorKind::Expected(what)))
}

/// Refuse a key the vocabulary does not have at this level, naming it.
///
/// The alternative is a schema that half arrives, and the half that is missing
/// is invisible until an author writes the tag it was meant to check.
fn reject_unknown<D: Declaration>(
    value: &D,
    allowed: &'static [&'static str],
    at: &Path,
) -> Result<(), Error> {
    for key in value.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(Error::new(
                at.child(&key),
                ErrorKind::UnknownKey {
                    key,
                    expected: allowed,
                },
            ));
        }
    }
    Ok(())
}
