//! `{% slot "name" %}`, which fills a named region of the tag around it.
//!
//! Ported from upstream `src/tags/slot.ts`, which is seven lines and has no
//! transform. That is the point: a slot never renders where it is written. The
//! parser lifts it out of its parent's children into the parent's slot map --
//! only when slots are enabled -- and the transform stage renders it into an
//! *attribute* named by the enclosing schema's `slots` declaration.
//!
//! So this schema exists to say two things and no more: the tag takes a primary
//! attribute, and that attribute is a required string. Both are the validator's
//! business. With slots disabled the lift never happens and a document's own
//! `{% slot %}` tag is an ordinary tag, which the corpus checks in "User slot
//! tag when slots are disabled".

use crate::validate::{Schema, SchemaAttribute, ValidationType};

/// The `{% slot %}` schema.
#[must_use]
pub fn slot() -> Schema {
    Schema::new().attribute(
        "primary",
        SchemaAttribute {
            attribute_type: Some(ValidationType::String),
            required: true,
            ..SchemaAttribute::default()
        },
    )
}
