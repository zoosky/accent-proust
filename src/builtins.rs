//! The vocabulary Markdoc ships: node schemas, tags, and functions.
//!
//! Mirrors what upstream's `index.ts` does with `mergeConfig`, which folds the
//! built-in `nodes`, `tags` and `functions` under whatever the caller passed --
//! on every call to `transform` and `validate`.
//!
//! Doing that per call would rebuild three maps for every document, so the
//! built-ins are built once, at construction. [`config`] is that for a host
//! that adds nothing. A host that adds to the vocabulary starts from
//! [`MapSchemaSource::builtin`], inserts its own, and hands the result to
//! [`config_with`], which adds the built-in functions and builds nothing
//! twice. The result is upstream's: a source with the caller's `if` inserted
//! over the built-in one is a config in which the caller's `if` wins, exactly
//! as passing one to `Markdoc.transform` does.
//!
//! [`Config::new`] stays empty, which is the honest starting point for a host
//! that supplies every schema itself -- and, since the validator reports
//! `tag-undefined` for anything it does not know, the difference between the
//! two constructors is visible rather than silent.
//!
//! # Why the content is not in this file
//!
//! The schemas themselves live where they are read from upstream:
//! [`validate::nodes`](crate::validate::nodes) is `schema.ts`,
//! [`tags`](crate::tags) is `src/tags/`, and [`functions`](crate::functions) is
//! `src/functions/`. This module only assembles them, which is all `index.ts`
//! does -- the nodes and tags through
//! [`MapSchemaSource::builtin`](crate::validate::MapSchemaSource::builtin), so
//! that a host starting from the same vocabulary has one place to get it.

use std::sync::Arc;

use crate::validate::{Config, MapSchemaSource, SchemaSource};

/// A configuration carrying the built-in nodes, tags and functions.
///
/// Variables and partials are empty: those are content, and this crate has
/// none. To add tags of your own, see [`config_with`].
///
/// ```
/// use accent_proust::validate::SchemaKey;
///
/// let config = accent_proust::builtins::config();
/// assert!(config.schemas.find(SchemaKey::Tag("if")).is_some());
/// assert!(config.functions.contains_key("equals"));
/// ```
#[must_use]
pub fn config<'a>() -> Config<'a> {
    config_with(Arc::new(MapSchemaSource::builtin()))
}

/// A configuration carrying `schemas` and the built-in functions.
///
/// The registering case. Fill a [`MapSchemaSource::builtin`] with your own
/// tags and hand it over, and the built-in schemas are built once -- rather
/// than once by [`config`] and again by you, with the first thrown away.
///
/// ```
/// use std::sync::Arc;
///
/// use accent_proust::builtins;
/// use accent_proust::validate::{MapSchemaSource, Schema, SchemaKey};
///
/// let mut schemas = MapSchemaSource::builtin();
/// schemas.insert_tag("callout", Schema::new().render("aside"));
///
/// let config = builtins::config_with(Arc::new(schemas));
/// assert!(config.schemas.find(SchemaKey::Tag("callout")).is_some());
/// assert!(config.schemas.find(SchemaKey::Tag("if")).is_some());
/// assert!(config.functions.contains_key("equals"));
/// ```
#[must_use]
pub fn config_with<'a>(schemas: Arc<dyn SchemaSource + Send + Sync>) -> Config<'a> {
    let mut config = Config::new().with_schemas(schemas);
    config.functions = Arc::new(crate::functions::builtin());
    config
}
