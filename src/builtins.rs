//! The vocabulary Markdoc ships: node schemas, tags, and functions.
//!
//! Mirrors what upstream's `index.ts` does with `mergeConfig`, which folds the
//! built-in `nodes`, `tags` and `functions` under whatever the caller passed --
//! on every call to `transform` and `validate`.
//!
//! Doing that per call would rebuild three maps for every document, so
//! [`config`] builds them once and the caller overrides what it wants. The
//! result is the same, reached at construction instead of at use:
//! `builtins::config()` then `config.tags_mut().insert("if", mine)` leaves a
//! config in which the caller's `if` wins, exactly as passing one to
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
//! does.

use crate::validate::Config;

/// A configuration carrying the built-in nodes, tags and functions.
///
/// Variables and partials are empty: those are content, and this crate has
/// none.
///
/// ```
/// let config = proust::builtins::config();
/// assert!(config.tags.contains_key("if"));
/// assert!(config.functions.contains_key("equals"));
/// ```
#[must_use]
pub fn config<'a>() -> Config<'a> {
    let mut config = Config::new();
    config.nodes = std::sync::Arc::new(crate::validate::nodes::builtin());
    config.tags = std::sync::Arc::new(crate::tags::builtin());
    config.functions = std::sync::Arc::new(crate::functions::builtin());
    config
}
