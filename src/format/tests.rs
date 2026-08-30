//! Unit cover for the parts of the formatter a document cannot reach.
//!
//! The ported oracle is `tests/formatter.rs`, which is upstream's
//! `formatter.test.ts`. What lives here is what that file cannot express: the
//! guards against a hand-built tree, and the two helpers whose behaviour is a
//! JavaScript detail rather than a Markdoc one.

use super::*;
use crate::ast::NodeType;

#[test]
fn a_null_value_formats_as_nothing() {
    assert_eq!(format_value(&Value::Null), "");
}
