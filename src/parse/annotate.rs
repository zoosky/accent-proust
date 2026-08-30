//! Applying annotations to a node.
//!
//! Ported from upstream's `annotate` in `src/parser.ts`. It is its own module
//! because three callers need it -- an inline annotation, a tag's own
//! attributes, and a fence's info string -- and because it is where two rules
//! that look incidental actually live.
//!
//! - **A repeated attribute is a warning, not an error.** The later value wins
//!   and the document still renders. Upstream reports `duplicate-attribute` at
//!   `warning`, and tooling binds to that pair.
//! - **Classes accumulate; attributes replace.** `.foo .bar` is two annotations
//!   and one `class` hash with two entries, not two hashes of one.
//!
//! The annotations are also kept verbatim on the node, alongside the attributes
//! they set. The formatter reprints what was written -- `.foo` stays `.foo`,
//! not `class={foo: true}` -- and cannot recover that from the attributes alone.

use indexmap::IndexMap;

use crate::ast::{Node, ValidationError, Value};
use crate::grammar::Attribute;

/// Apply annotations to a node, in authored order.
pub(crate) fn annotate(node: &mut Node<'_>, attributes: &[Attribute]) {
    for attribute in attributes {
        node.annotations.push(attribute.clone());
        match attribute {
            Attribute::Attribute { name, value } => {
                if node.attributes.contains_key(name) {
                    node.errors.push(ValidationError::duplicate_attribute(name));
                }
                node.set(name.clone(), value.clone());
            }
            Attribute::Class { name } => {
                if let Some(Value::Hash(classes)) = node.attributes.get_mut("class") {
                    classes.insert(name.clone(), Value::Boolean(true));
                } else {
                    let mut classes = IndexMap::new();
                    classes.insert(name.clone(), Value::Boolean(true));
                    node.set("class", Value::Hash(classes));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::NodeType;

    fn attribute(name: &str, value: &str) -> Attribute {
        Attribute::Attribute {
            name: name.to_string(),
            value: Value::String(value.to_string()),
        }
    }

    #[test]
    fn a_repeated_attribute_warns_and_the_last_value_wins() {
        let mut node = Node::new(NodeType::Tag);
        annotate(&mut node, &[attribute("bar", "1"), attribute("bar", "2")]);
        assert_eq!(node.errors.len(), 1);
        assert_eq!(node.errors[0].id, "duplicate-attribute");
        assert_eq!(node.get("bar"), Some(&Value::String("2".to_string())));
    }

    #[test]
    fn classes_accumulate_into_one_hash() {
        let mut node = Node::new(NodeType::Tag);
        annotate(
            &mut node,
            &[
                Attribute::Class {
                    name: "foo".to_string(),
                },
                Attribute::Class {
                    name: "bar".to_string(),
                },
            ],
        );
        assert!(node.errors.is_empty());
        let Some(Value::Hash(classes)) = node.get("class") else {
            panic!("expected a class hash");
        };
        let names: Vec<&str> = classes.keys().map(String::as_str).collect();
        assert_eq!(names, ["foo", "bar"]);
    }

    #[test]
    fn annotations_are_kept_verbatim_for_the_formatter() {
        let mut node = Node::new(NodeType::Fence);
        annotate(
            &mut node,
            &[
                attribute("z", "1"),
                Attribute::Class {
                    name: "cls".to_string(),
                },
            ],
        );
        assert_eq!(node.annotations.len(), 2);
        assert!(matches!(node.annotations[1], Attribute::Class { .. }));
    }
}
