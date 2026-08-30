//! `{% table %}`, the advanced table syntax.
//!
//! Ported from upstream `src/tags/table.ts`, which is six lines: the tag
//! declares that its only child is a `table` node and that it is not inline.
//!
//! Everything else has already happened by the time the transform stage sees it.
//! The rewrite that turns lists and thematic breaks into `thead`, `tbody`, `tr`
//! and `td` runs at the end of the parse
//! ([`transform::table`](crate::transform::table)), so this schema's job is to
//! render nothing and let the `table` node it now contains render itself.

use crate::ast::NodeType;
use crate::validate::Schema;

/// The `{% table %}` schema.
#[must_use]
pub fn table() -> Schema {
    let mut schema = Schema::new();
    schema.children = Some(vec![NodeType::Table]);
    schema.inline = Some(false);
    schema
}
