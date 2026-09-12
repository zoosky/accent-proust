//! The vocabulary Markdoc ships: node schemas, tags, and functions.
//!
//! Mirrors what upstream's `index.ts` does with `mergeConfig`, which folds the
//! built-in `nodes`, `tags` and `functions` under whatever the caller passed --
//! on every call to `transform` and `validate`.
//!
//! Doing that per call would rebuild three maps for every document, so
//! [`config`] builds them once and the caller overrides what it wants. The
//! result is the same, reached at construction instead of at use:
//! `MapSchemaSource::builtin()` then `schemas.insert_tag("if", mine)` leaves a
//! source in which the caller's `if` wins, exactly as passing one to
//! `Markdoc.transform` does.
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

use crate::validate::{Config, MapSchemaSource};

/// A configuration carrying the built-in nodes, tags and functions.
///
/// Variables and partials are empty: those are content, and this crate has
/// none.
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
    let mut config = Config::new().with_schemas(Arc::new(MapSchemaSource::builtin()));
    config.functions = Arc::new(crate::functions::builtin());
    config
}
