//! `{% if %}` and `{% else %}`.
//!
//! Ported from upstream `src/tags/conditional.ts`.
//!
//! # `else` is a marker, not a container
//!
//! `{% else /%}` is self-closing, so a conditional's children are one flat run
//! with `else` nodes sprinkled through it. The transform reads that run into
//! branches -- everything up to the first `else` belongs to the primary
//! condition, everything after it to that `else`'s -- and renders the first
//! branch whose condition holds. A nesting `else` would have been the obvious
//! design and is not upstream's; the parser produces the flat shape, so the
//! transform reads it.

use std::sync::Arc;

use crate::ast::{Node, NodeType, Value};
use crate::renderable::{RenderableTreeNode, RenderableTreeNodes};
use crate::transform;
use crate::validate::schema_types::Conditional;
use crate::validate::{Config, RenderPolicy, Schema, SchemaAttribute, ValidationType};

/// Whether a value satisfies an `{% if %}`.
///
/// Upstream's `truthy`: `value !== false && value !== undefined && value !==
/// null`. **This is not JavaScript's `Boolean()`**, and the difference is not
/// academic -- `0` and `""` are truthy to a conditional. The corpus pins it in
/// "Truthy things are not false", which sets `foo: 0` and `bar: ''` and expects
/// both branches to render.
///
/// [`Value::is_truthy`] is the *other* rule, JavaScript's `Boolean()`, which
/// the grammar needs for a different decision. Reaching for whichever is nearest
/// is how a port acquires a silent behaviour change, so both are spelled out and
/// both say what they are for.
#[must_use]
pub fn truthy(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null | Value::Boolean(false)))
}

/// One branch of a conditional: what has to hold, and what it renders.
struct Branch<'n, 'a> {
    condition: Option<Value>,
    children: Vec<&'n Node<'a>>,
}

/// Split a conditional's children into branches at each `{% else %}`.
fn branches<'a>(node: &'a Node<'a>, config: &Config<'a>) -> Vec<Branch<'a, 'a>> {
    let mut branches = vec![Branch {
        condition: node
            .get("primary")
            .and_then(|value| transform::resolve(value, config)),
        children: Vec::new(),
    }];

    for child in &node.children {
        if is_else(child) {
            // An `{% else %}` with no condition is the final branch and always
            // holds; one with a condition is an "else if". Upstream tests for
            // the key's presence rather than for a value, so
            // `{% else $missing /%}` is a branch that never holds rather than a
            // bare `else`.
            let condition = match child.get("primary") {
                Some(value) => transform::resolve(value, config),
                None => Some(Value::Boolean(true)),
            };
            branches.push(Branch {
                condition,
                children: Vec::new(),
            });
            continue;
        }
        if let Some(branch) = branches.last_mut() {
            branch.children.push(child);
        }
    }
    branches
}

/// A `primary` attribute that is a condition, and is never rendered.
fn primary() -> SchemaAttribute {
    SchemaAttribute {
        attribute_type: Some(ValidationType::Custom(Arc::new(Conditional))),
        render: RenderPolicy::Hidden,
        ..SchemaAttribute::default()
    }
}

/// The `{% if %}` schema.
///
/// It renders no element of its own: the selected branch's nodes take the
/// conditional's place among its siblings, which is why
/// [`transform::children`] flattens one level.
#[must_use]
pub fn tag_if() -> Schema {
    let mut schema = Schema::new().attribute("primary", primary());
    schema.transform = Some(Arc::new(|node, config| {
        for branch in branches(node, config) {
            if !truthy(branch.condition.as_ref()) {
                continue;
            }
            let mut out: Vec<RenderableTreeNode> = Vec::new();
            for child in branch.children {
                out.extend(transform::node(child, config).into_vec());
            }
            return RenderableTreeNodes::Many(out);
        }
        RenderableTreeNodes::Many(Vec::new())
    }));
    schema
}

/// The `{% else %}` schema.
///
/// Self-closing, and carries no transform: it is consumed by the `{% if %}`
/// around it. Reached on its own -- an `else` outside a conditional -- it
/// renders its children, which is what a schema with no `render` does.
#[must_use]
pub fn tag_else() -> Schema {
    let mut schema = Schema::new().attribute("primary", primary());
    schema.self_closing = true;
    schema
}

/// Whether a node is an `{% else %}`.
///
/// Used by the conditional's own transform and by the table rewrite, which
/// treats `else` as structural rather than as content.
#[must_use]
pub fn is_else(node: &Node<'_>) -> bool {
    node.node_type == NodeType::Tag && node.tag.as_deref() == Some("else")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthiness_is_markdocs_rather_than_javascripts() {
        // The three falsy values, and only those three.
        assert!(!truthy(None));
        assert!(!truthy(Some(&Value::Null)));
        assert!(!truthy(Some(&Value::Boolean(false))));
        // Everything else holds, including the two JavaScript would reject.
        assert!(truthy(Some(&Value::Number(0.0))));
        assert!(truthy(Some(&Value::String(String::new()))));
        assert!(truthy(Some(&Value::Boolean(true))));
    }
}
