//! The built-in tags every Markdoc document can use without a schema.
//!
//! Mirrors upstream `src/tags/`: `if`/`else`, `partial`, `slot`, and `table`.
//!
//! These are the tags that are part of the *language* rather than part of a
//! host's component set. Everything else a document can say is a schema the
//! host registers.
//!
//! A caller replaces one by registering a schema of the same name: [`builtin`]
//! is what [`builtins::config`](crate::builtins::config) starts from, and a host
//! that inserts its own `if` afterwards has replaced it, which is upstream's
//! `mergeConfig` reached at construction instead of at use.

mod conditional;
mod partial;
mod slot;
mod table;

pub use conditional::{is_else, tag_else, tag_if, truthy};
pub use partial::{PARTIAL_FILENAME, PartialFile, partial};
pub use slot::slot;
pub use table::table;

use indexmap::IndexMap;

use crate::validate::Schema;

/// Every built-in tag, by the name a document writes.
#[must_use]
pub fn builtin() -> IndexMap<String, Schema> {
    let mut tags = IndexMap::new();
    tags.insert("else".to_string(), tag_else());
    tags.insert("if".to_string(), tag_if());
    tags.insert("partial".to_string(), partial());
    tags.insert("slot".to_string(), slot());
    tags.insert("table".to_string(), table());
    tags
}
