//! The transformer: an AST node becomes renderable nodes.
//!
//! Ported from upstream `src/transformer.ts`, which is 89 lines and four
//! functions -- `findSchema`, [`attributes`], [`children`] and [`node`] -- that
//! call each other in a cycle. Upstream also hangs each one off `Node` as a
//! method (`node.transform(config)`); this crate does not, because a leaf type
//! with a method that needs the whole configuration surface depends on the
//! stage above it. The functions here take `&Node` instead, which is the same
//! call with the arrow pointing the right way.
//!
//! # Attribute order is the schema's, not the document's
//!
//! Worth stating because it surprises people reading the corpus: a rendered
//! tag's attributes are the ones its *schema* declares, in the order the schema
//! declares them, preceded by the two global ones. An attribute the author
//! wrote that no schema names is not rendered at all. That is upstream's rule,
//! it is what makes output reproducible, and it is why `{% foo bar=1 %}`
//! against `{render: "p"}` renders as a bare `<p>`.

use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use crate::ast::Node;
use crate::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};
use crate::transform::resolve::resolve;
use crate::validate::schema_types::{Class, Id};
use crate::validate::{Config, Schema, SchemaAttribute, ValidationType};

/// How deep the tree walk may go before it stops descending.
///
/// Transform is recursive because schema hooks are: a hook receives a node and
/// calls back into [`children`] for its content, so there is no stack to unroll
/// it onto without giving every hook a continuation. Nesting depth is
/// attacker-controlled -- `{% a %}` repeated is one level per line -- and a
/// stack overflow aborts the process rather than raising anything a caller
/// could catch, so the recursion is bounded. Content past the bound renders as
/// nothing. `DIVERGENCES.md` declares it.
///
/// The number is far above any document a person writes and far below the
/// frames a small thread stack holds: HTML stops nesting meaningfully long
/// before this.
///
/// # What this stage owes the renderable tree
///
/// [`Tag`] carries a manual iterative [`Drop`] because its nesting tracks the
/// document's. [`Scalar`] deliberately does **not**, on the grounds that scalar
/// nesting comes from the value grammar, which
/// [`MAX_RESOLVE_DEPTH`](crate::transform::MAX_RESOLVE_DEPTH) bounds at 64.
/// This stage is the one positioned to break that assumption, and it holds it:
///
/// - Every `Scalar` it produces comes from `Scalar::from_value` over a value
///   resolution has already bounded. Nothing here synthesises a nested array or
///   object out of document structure.
/// - Slot content, which *does* track document depth, goes into the attribute
///   map as [`RenderableTreeNodes`] rather than as a scalar -- so it is `Tag`
///   underneath, and guarded.
///
/// A later stage that builds a scalar whose depth follows the document rather
/// than the grammar invalidates that, and needs to say so rather than assume
/// the bound still holds.
pub const MAX_TRANSFORM_DEPTH: usize = 512;

thread_local! {
    /// How many [`node`] calls are on the stack right now.
    ///
    /// A thread-local rather than a parameter, because the recursion is not this
    /// module's alone: a schema hook takes `(&Node, &Config)` -- upstream's
    /// signature, and the one a host writes against -- and calls [`children`]
    /// from inside it. Threading a depth argument would put it in that
    /// signature, where every host implementation would have to carry it
    /// correctly for the bound to hold. Counting here holds it whatever a hook
    /// does.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Holds one level of the recursion, and gives it back on the way out.
struct Depth;

impl Depth {
    /// Claim a level, or [`None`] if the bound is reached.
    fn enter() -> Option<Depth> {
        DEPTH.with(|depth| {
            let current = depth.get();
            if current >= MAX_TRANSFORM_DEPTH {
                return None;
            }
            depth.set(current + 1);
            Some(Depth)
        })
    }
}

impl Drop for Depth {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// The attributes every node has, whatever its schema declares.
///
/// Upstream's `globalAttributes`. Both are custom types with hooks: `class`
/// joins the hash an annotation builds (`.foo.bar` is `{foo: true, bar: true}`)
/// into a class list, and `id` checks that the value starts with a letter.
///
/// Built once. The pair is read for every node in every document, and rebuilding
/// two `Arc`s and two `String`s per node is the kind of cost that is invisible
/// until a documentation site has ten thousand of them.
#[must_use]
pub fn global_attributes() -> &'static [(&'static str, SchemaAttribute); 2] {
    static GLOBALS: OnceLock<[(&'static str, SchemaAttribute); 2]> = OnceLock::new();
    GLOBALS.get_or_init(|| {
        let custom = |kind: ValidationType| SchemaAttribute {
            attribute_type: Some(kind),
            ..SchemaAttribute::default()
        };
        [
            ("class", custom(ValidationType::Custom(Arc::new(Class)))),
            ("id", custom(ValidationType::Custom(Arc::new(Id)))),
        ]
    })
}

/// Transform a node into the renderable nodes it produces.
///
/// Upstream's `transformer.node`, and the entry point the other two are reached
/// through.
#[must_use]
pub fn node<'a>(node: &'a Node<'a>, config: &Config<'a>) -> RenderableTreeNodes {
    let Some(_depth) = Depth::enter() else {
        return RenderableTreeNodes::Many(Vec::new());
    };
    let schema = config.find_schema(node);

    // A transform hook replaces everything below it: it decides its own
    // children, its own attributes, and whether it renders an element at all.
    if let Some(transform) = schema.and_then(|schema| schema.transform.as_ref()) {
        return transform(node, config);
    }

    let children = children(node, config);
    let Some(render) = schema.and_then(|schema| schema.render.as_deref()) else {
        // No schema, or a schema that renders nothing: the node contributes its
        // children and disappears. That is what an unregistered tag does, and
        // what `inline` does by design.
        return RenderableTreeNodes::Many(children);
    };

    RenderableTreeNodes::One(RenderableTreeNode::tag(Tag::with(
        render,
        attributes(node, config),
        children,
    )))
}

/// Transform a node's children, flattened one level.
///
/// Upstream's `transformer.children`, whose `flatMap` is why a hook may return
/// several nodes: `{% if %}` returns the branch it selected, and those nodes
/// take the conditional's place among its siblings rather than nesting under
/// it.
#[must_use]
pub fn children<'a>(parent: &'a Node<'a>, config: &Config<'a>) -> Vec<RenderableTreeNode> {
    let mut out = Vec::with_capacity(parent.children.len());
    for child in &parent.children {
        out.extend(node(child, config).into_vec());
    }
    out
}

/// Transform a node's attributes into the ones its schema renders.
///
/// Upstream's `transformer.attributes`. Four things happen per attribute, in
/// this order, and each is observable:
///
/// 1. `render: false` drops it, and a renamed `render` renames it -- which is
///    how `colspan` reaches the tree as `colSpan`.
/// 2. A custom attribute type may rewrite the value.
/// 3. An absent value falls back to the schema's default.
/// 4. A value still absent is not rendered. `null` is a value and *is*
///    rendered; only `undefined` disappears.
#[must_use]
pub fn attributes<'a>(
    node: &'a Node<'a>,
    config: &Config<'a>,
) -> IndexMap<String, RenderableTreeNodes> {
    let schema = config.find_schema(node);
    let mut output: IndexMap<String, RenderableTreeNodes> = IndexMap::new();

    // Upstream merges with `{...globalAttributes, ...schema.attributes}`, which
    // keeps a repeated key's first position and takes the last value. So the two
    // globals come first, each overridden in place by a schema that redeclares
    // it, and the schema's own attributes follow in its order.
    for (key, global) in global_attributes() {
        let declared = schema
            .and_then(|schema| schema.attributes.get(*key))
            .unwrap_or(global);
        render_attribute(node, config, key, declared, &mut output);
    }
    if let Some(schema) = schema {
        for (key, declared) in &schema.attributes {
            if global_attributes().iter().any(|(global, _)| global == key) {
                continue;
            }
            render_attribute(node, config, key, declared, &mut output);
        }
    }

    // Slots are attached after attributes and therefore win a name collision,
    // which the corpus fixes in "Handling overlapping slot and attribute".
    let Some(schema) = schema else {
        return output;
    };
    for (key, slot) in &schema.slots {
        let Some(name) = slot.render.output_name(key) else {
            continue;
        };
        if let Some(filled) = node.slots.get(key.as_str()) {
            output.insert(name.to_string(), self::node(filled, config));
        }
    }
    output
}

/// Resolve one declared attribute and, if it has a value, render it.
fn render_attribute<'a>(
    node: &'a Node<'a>,
    config: &Config<'a>,
    key: &str,
    declared: &SchemaAttribute,
    output: &mut IndexMap<String, RenderableTreeNodes>,
) {
    let Some(name) = declared.render.output_name(key) else {
        return;
    };
    let resolved = node.get(key).and_then(|value| resolve(value, config));
    let value = match &declared.attribute_type {
        // Upstream constructs the declared type and calls `transform` if the
        // instance has one. The built-in types have none, so only a custom type
        // can rewrite a value -- and it is handed `undefined` too, which is how
        // `Class` passes an absent class straight through.
        Some(ValidationType::Custom(custom)) => custom.transform(resolved.as_ref(), config),
        _ => resolved.as_ref().and_then(Scalar::from_value),
    };
    let value = value.or_else(|| declared.default.as_ref().and_then(Scalar::from_value));
    if let Some(value) = value {
        output.insert(
            name.to_string(),
            RenderableTreeNodes::One(RenderableTreeNode::Scalar(value)),
        );
    }
}

/// The schema for a node, or [`None`].
///
/// Upstream's `transformer.findSchema`, which lives on
/// [`Config`](crate::validate::Config) because it reads nothing else, and is
/// re-exported here so a reader following `transformer.ts` finds it where
/// upstream keeps it.
#[must_use]
pub fn find_schema<'c>(node: &Node<'_>, config: &'c Config<'_>) -> Option<&'c Schema> {
    config.find_schema(node)
}

/// One literal value, or nothing.
///
/// `text` returns its content and `softbreak` returns a space; both are a
/// single scalar with no element around them. An `undefined` result renders as
/// nothing rather than as `null`, which is what upstream's `text.transform`
/// produces for an unset variable and what every consumer of the tree does with
/// it.
#[must_use]
pub(crate) fn scalar(value: Option<Scalar>) -> RenderableTreeNodes {
    match value {
        Some(value) => RenderableTreeNodes::One(RenderableTreeNode::Scalar(value)),
        None => RenderableTreeNodes::Many(Vec::new()),
    }
}
