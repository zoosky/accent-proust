//! `{% partial %}`, which splices another parsed document into this one.
//!
//! Ported from upstream `src/tags/partial.ts`.
//!
//! # This crate does not read the file
//!
//! `file="foo.md"` is a key into `config.partials`, which the caller filled with
//! documents it parsed itself. That is the whole of the design and not an
//! accident of the port: a crate that resolved the name would be doing I/O, and
//! the seam that keeps it from doing I/O is the same one that lets a host serve
//! partials from a database, a bundle, or a plugin.
//!
//! # Scope
//!
//! A partial renders under the caller's variables plus its own, plus
//! `$$partial:filename` -- upstream's bookkeeping key, kept verbatim because a
//! partial may read it. Its *children* are transformed, not the document node
//! itself, so a partial does not wrap what it contributes in an `<article>`.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::ast::Value;
use crate::renderable::{RenderableTreeNode, RenderableTreeNodes};
use crate::transform;
use crate::validate::{Config, RenderPolicy, Schema, SchemaAttribute, ValidationType};

/// The key upstream writes into a partial's scope so it can name its own file.
pub const PARTIAL_FILENAME: &str = "$$partial:filename";

/// A hidden attribute of the given type.
fn hidden(kind: ValidationType) -> SchemaAttribute {
    SchemaAttribute {
        attribute_type: Some(kind),
        render: RenderPolicy::Hidden,
        ..SchemaAttribute::default()
    }
}

/// The `{% partial %}` schema.
#[must_use]
pub fn partial() -> Schema {
    let mut file = hidden(ValidationType::Custom(Arc::new(PartialFile)));
    file.required = true;
    let mut schema = Schema::new()
        .attribute("file", file)
        .attribute("variables", hidden(ValidationType::Object));
    schema.self_closing = true;
    schema.inline = Some(false);
    schema.transform = Some(Arc::new(|node, config| {
        // Borrowed rather than destructured: `Value` carries a manual `Drop`,
        // so a variant's contents cannot be moved out of it. Nothing here needs
        // to own the name.
        let resolved = node
            .get("file")
            .and_then(|value| transform::resolve(value, config));
        let Some(Value::String(file)) = resolved.as_ref() else {
            // Upstream's `if (!partial) return null`, reached one step earlier:
            // a `file` that is not a string cannot name one.
            return RenderableTreeNodes::One(RenderableTreeNode::Scalar(
                crate::renderable::Scalar::Null,
            ));
        };
        let Some(document) = config.partials.get(file.as_str()) else {
            return RenderableTreeNodes::One(RenderableTreeNode::Scalar(
                crate::renderable::Scalar::Null,
            ));
        };
        RenderableTreeNodes::Many(transform::children(document, &scope(config, file, node)))
    }));
    schema
}

/// The configuration a partial's body is transformed under.
///
/// Upstream spreads the caller's config and replaces `variables`; the same here,
/// which means a copy. Only [`Config::variables`] differs, so a config whose
/// partials are large pays for them once per expansion -- worth knowing before
/// a host puts a book in one.
fn scope<'a>(config: &Config<'a>, file: &str, node: &crate::ast::Node<'a>) -> Config<'a> {
    let mut variables: IndexMap<String, Value> = config.variables.clone().unwrap_or_default();
    // Taken, not cloned: `Value`'s manual `Drop` forbids moving the map out of
    // the variant, and `take` leaves an empty husk that drops for free.
    let mut resolved = node
        .get("variables")
        .and_then(|value| transform::resolve(value, config));
    if let Some(Value::Hash(passed)) = resolved.as_mut() {
        variables.extend(std::mem::take(passed));
    }
    variables.insert(
        PARTIAL_FILENAME.to_string(),
        Value::String(file.to_string()),
    );
    let mut scoped = config.clone();
    scoped.variables = Some(variables);
    scoped
}

/// The `file` attribute's type: a name `config.partials` knows.
///
/// Mirrors upstream's inline `PartialFile` class, whose whole purpose is the
/// error message. It reports the *configuration* problem rather than a type
/// problem, because `file="nope.md"` is well-formed and simply names nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct PartialFile;

impl crate::validate::AttributeType for PartialFile {
    fn name(&self) -> &'static str {
        "PartialFile"
    }

    fn validate<'a>(
        &self,
        value: &Value,
        config: &Config<'a>,
        _name: &str,
    ) -> Option<Vec<crate::ast::ValidationError<'a>>> {
        let known = match value {
            Value::String(file) => config.partials.contains_key(file.as_str()),
            _ => false,
        };
        if known {
            return Some(Vec::new());
        }
        let file = match value {
            Value::String(file) => file.clone(),
            other => format!("{other:?}"),
        };
        Some(vec![crate::ast::ValidationError::new(
            "attribute-value-invalid",
            crate::ast::ErrorLevel::Error,
            format!(
                "Partial `{file}` not found. The 'file' attribute must be set in \
                 `config.partials`"
            ),
        )])
    }
}
