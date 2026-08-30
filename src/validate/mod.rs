//! Schema definitions and validation.
//!
//! Mirrors upstream `src/validator.ts`, `src/schema.ts`, and
//! `src/schema-types/`.
//!
//! Validation errors are **data**, not failures: validating returns a `Vec` of
//! them, each carrying an id, a level, and a source location. A document with
//! errors is still a document, which is what lets an editor show every problem
//! at once instead of the first one.
//!
//! **Upstream error ids are identical and stay identical.** That is the part
//! external tooling binds to, so it is the one place where divergence is
//! disallowed outright rather than merely declared.
//!
//! This crate owns the schema *shape*. It does not own schema *content*: where
//! a schema comes from is the host's decision, reached through `SchemaSource`.

mod attribute_type;
mod config;
mod schema;
pub mod schema_types;
mod validator;

pub use attribute_type::{type_to_string, AttributeType, ValidationType};
pub use config::{Config, ConfigFunction, ValidationOptions, Variables};
pub use schema::{
    AttributeValidateHook, FunctionTransformHook, FunctionValidateHook, MatchPattern, MatchesHook,
    RenderPolicy, Schema, SchemaAttribute, SchemaMatches, SchemaSlot, TransformHook, ValidateHook,
};
pub use validator::{
    global_attributes, validate_tree, validate_type, validator, walk_with_parents, TypeCheck,
    ValidateError,
};
