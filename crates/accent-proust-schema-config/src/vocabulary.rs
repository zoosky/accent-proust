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

use accent_proust::ast::{ErrorLevel, NodeType, Value};
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
        got => return Err(expected(&at, "an object", got)),
    }
    reject_unknown(root, TOP_LEVEL, &at)?;

    let tags_at = at.child("tags");
    if let Some(tags) = root.get("tags", &tags_at)? {
        let at = tags_at;
        object(&tags, &at)?;
        for name in tags.keys() {
            let at = at.child(&name);
            let declaration = present(tags.get(&name, &at)?, &at)?;
            declared.tags.insert(name, schema(&declaration, &at)?);
        }
    }

    let nodes_at = at.child("nodes");
    if let Some(nodes) = root.get("nodes", &nodes_at)? {
        let at = nodes_at;
        object(&nodes, &at)?;
        for name in nodes.keys() {
            let at = at.child(&name);
            let node = node_key(&name, &at)?;
            let declaration = present(nodes.get(&name, &at)?, &at)?;
            declared.nodes.insert(node, schema(&declaration, &at)?);
        }
    }

    let variables_at = at.child("variables");
    if let Some(variables) = root.get("variables", &variables_at)? {
        let at = variables_at;
        object(&variables, &at)?;
        let mut map = Variables::new();
        for name in variables.keys() {
            let at = at.child(&name);
            // A variable may be any value, and an absent one -- explicitly
            // `undefined`, in JavaScript -- is `null`, which is what a host
            // that wrote `user: session?.user` meant by it.
            let value = match variables.get(&name, &at)? {
                Some(declaration) => declaration.to_value(&at)?,
                None => Value::Null,
            };
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

    let render_at = at.child("render");
    if let Some(render) = value.get("render", &render_at)? {
        schema.render = match render_policy(&render, &render_at)? {
            RenderPolicy::Hidden => None,
            RenderPolicy::Renamed(name) => Some(name),
            // A schema's `render` is a name or nothing; `true` has no name to
            // fall back on the way an attribute's does.
            RenderPolicy::Named => {
                return Err(Error::new(render_at, ErrorKind::UnrenderableTrue));
            }
        };
    }

    let children_at = at.child("children");
    if let Some(children) = value.get("children", &children_at)? {
        let at = children_at;
        list(&children, &at)?;
        let mut allowed = Vec::new();
        for (index, item) in children.items().iter().enumerate() {
            let at = at.index(index);
            let name = string(item, &at, "a node type name")?;
            allowed.push(node_type(&name, &at)?);
        }
        schema.children = Some(allowed);
    }

    let attributes_at = at.child("attributes");
    if let Some(attributes) = value.get("attributes", &attributes_at)? {
        let at = attributes_at;
        object(&attributes, &at)?;
        for name in attributes.keys() {
            let at = at.child(&name);
            let declaration = present(attributes.get(&name, &at)?, &at)?;
            schema
                .attributes
                .insert(name, attribute(&declaration, &at)?);
        }
    }

    let slots_at = at.child("slots");
    if let Some(slots) = value.get("slots", &slots_at)? {
        let at = slots_at;
        object(&slots, &at)?;
        for name in slots.keys() {
            let at = at.child(&name);
            let declaration = present(slots.get(&name, &at)?, &at)?;
            schema.slots.insert(name, slot(&declaration, &at)?);
        }
    }

    let self_closing_at = at.child("selfClosing");
    if let Some(flag) = value.get("selfClosing", &self_closing_at)? {
        schema.self_closing = boolean(&flag, &self_closing_at)?;
    }
    let inline_at = at.child("inline");
    if let Some(flag) = value.get("inline", &inline_at)? {
        schema.inline = Some(boolean(&flag, &inline_at)?);
    }
    let description_at = at.child("description");
    if let Some(text) = value.get("description", &description_at)? {
        schema.description = Some(string(&text, &description_at, "a string")?);
    }

    Ok(schema)
}

/// Convert one attribute declaration.
fn attribute<D: Declaration>(value: &D, at: &Path) -> Result<SchemaAttribute, Error> {
    object(value, at)?;
    reject_unknown(value, ATTRIBUTE_KEYS, at)?;

    let mut attribute = SchemaAttribute::default();

    let type_at = at.child("type");
    if let Some(declared) = value.get("type", &type_at)? {
        attribute.attribute_type = Some(attribute_type(&declared, &type_at)?);
    }
    let default_at = at.child("default");
    if let Some(default) = value.get("default", &default_at)? {
        attribute.default = Some(default.to_value(&default_at)?);
    }
    let required_at = at.child("required");
    if let Some(flag) = value.get("required", &required_at)? {
        attribute.required = boolean(&flag, &required_at)?;
    }
    let matches_at = at.child("matches");
    if let Some(values) = value.get("matches", &matches_at)? {
        attribute.matches = Some(matches(&values, &matches_at)?);
    }
    let render_at = at.child("render");
    if let Some(render) = value.get("render", &render_at)? {
        attribute.render = render_policy(&render, &render_at)?;
    }
    let level_at = at.child("errorLevel");
    if let Some(level) = value.get("errorLevel", &level_at)? {
        attribute.error_level = Some(error_level(&level, &level_at)?);
    }
    let description_at = at.child("description");
    if let Some(text) = value.get("description", &description_at)? {
        attribute.description = Some(string(&text, &description_at, "a string")?);
    }

    Ok(attribute)
}

/// Convert one slot declaration.
fn slot<D: Declaration>(value: &D, at: &Path) -> Result<SchemaSlot, Error> {
    object(value, at)?;
    reject_unknown(value, SLOT_KEYS, at)?;

    let mut slot = SchemaSlot::default();
    let render_at = at.child("render");
    if let Some(render) = value.get("render", &render_at)? {
        slot.render = render_policy(&render, &render_at)?;
    }
    let required_at = at.child("required");
    if let Some(flag) = value.get("required", &required_at)? {
        slot.required = boolean(&flag, &required_at)?;
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
        None => Err(expected(
            at,
            "an attribute type name as a string, or an array of them",
            value.shape(),
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
    value
        .as_str()
        .map(RenderPolicy::Renamed)
        .ok_or_else(|| expected(at, "true, false, or a name to render under", value.shape()))
}

/// Convert an error level by its upstream spelling.
fn error_level<D: Declaration>(value: &D, at: &Path) -> Result<ErrorLevel, Error> {
    let name = string(value, at, "an error level name")?;
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

/// The wrong shape at `at`.
fn expected(at: &Path, what: &'static str, got: Shape) -> Error {
    Error::new(at.clone(), ErrorKind::Expected { what, got })
}

/// A property that `keys` listed but `get` did not return: explicitly
/// `undefined`, in JavaScript. Refused as the object it was meant to be.
fn present<D: Declaration>(value: Option<D>, at: &Path) -> Result<D, Error> {
    value.ok_or_else(|| expected(at, "an object", Shape::Null))
}

/// Refuse anything that is not an object.
fn object<D: Declaration>(value: &D, at: &Path) -> Result<(), Error> {
    match value.shape() {
        Shape::Object => Ok(()),
        got => Err(expected(at, "an object", got)),
    }
}

/// Refuse anything that is not a list.
fn list<D: Declaration>(value: &D, at: &Path) -> Result<(), Error> {
    match value.shape() {
        Shape::List => Ok(()),
        got => Err(expected(at, "an array", got)),
    }
}

/// The value as a boolean.
fn boolean<D: Declaration>(value: &D, at: &Path) -> Result<bool, Error> {
    value
        .as_bool()
        .ok_or_else(|| expected(at, "true or false", value.shape()))
}

/// The value as a string, with `what` saying which string was wanted.
fn string<D: Declaration>(value: &D, at: &Path, what: &'static str) -> Result<String, Error> {
    value
        .as_str()
        .ok_or_else(|| expected(at, what, value.shape()))
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
