//! The renderable tree: what transform produces and a renderer consumes.
//!
//! Mirrors upstream `src/tag.ts` and the four types in `src/types.ts` that
//! describe its shape (`Scalar`, `RenderableTreeNode`, `RenderableTreeNodes`,
//! `Primitive`). Upstream keeps them apart because TypeScript separates a class
//! from the aliases that mention it; here they are one module because they are
//! one data structure and every consumer needs all four.
//!
//! It sits at the crate root rather than under `transform` or `render` because
//! it is the boundary *between* them. `Schema::transform` returns one of these,
//! [`crate::render`] walks one, and the formatter never sees one at all. A type
//! three stages share belongs above all three.
//!
//! # Owned, on purpose
//!
//! Nothing here borrows. The AST borrows its source -- a
//! [`Location`](crate::ast::Location) is a byte range plus the text it covers --
//! and that is where the lifetime stops. Transform resolves variables, runs
//! schema hooks and synthesises nodes that were never in the source, so a
//! renderable tree cannot honestly claim to be a view of a document. Making it
//! owned is also what lets a host cache or send one.
//!
//! # The runtime type guard is not ported
//!
//! Upstream tags carry `$$mdtype: 'Tag'` and a static `Tag.isTag(x)`, because a
//! JavaScript consumer holding `Tag | Scalar` has no other way to ask which it
//! has. [`RenderableTreeNode`] is an enum, so the question is answered by
//! matching and the guard has nothing left to protect. The same call was already
//! made for the AST's `$$mdtype`.

use indexmap::IndexMap;

use crate::ast::Value;

/// A JSON-shaped value: what an attribute holds and what a leaf child is.
///
/// Upstream's `Scalar = Primitive | Scalar[] | {[key: string]: Scalar}`, with
/// `Primitive = null | boolean | number | string`. The primitives are spelled
/// out as variants here rather than kept in a separate `Primitive` type,
/// because Rust has no untagged union to build one out of and a nested
/// `Scalar::Primitive(Primitive::String(..))` would only add a level of
/// wrapping for consumers to strip.
///
/// # Absence is not null
///
/// JavaScript distinguishes `null` from `undefined`, and Markdoc uses the
/// difference: an attribute whose value is `undefined` is dropped by the
/// transformer, while `null` is rendered. So absence is [`Option::None`] around
/// a `Scalar` and never [`Scalar::Null`]. Collapsing the two would make
/// `{% foo bar=null /%}` and `{% foo /%}` the same document.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Scalar {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A number. One numeric type, `f64`, as everywhere else in this crate:
    /// upstream parses every literal with `parseFloat`.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Scalar>),
    /// An object, in insertion order.
    ///
    /// [`IndexMap`] rather than `HashMap` for the reason attributes are: two
    /// runs over one document must produce identical bytes.
    Object(IndexMap<String, Scalar>),
}

impl Scalar {
    /// The scalar form of an AST value, or [`None`] if it has none.
    ///
    /// A [`Value::Function`] and a [`Value::Variable`] are unresolved
    /// references, not data: they have no scalar spelling until the transform
    /// stage resolves them against a config. Returning [`None`] rather than
    /// inventing one is what keeps "this attribute was never resolved" from
    /// silently rendering as a string.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Scalar> {
        match value {
            Value::Null => Some(Scalar::Null),
            Value::Boolean(b) => Some(Scalar::Boolean(*b)),
            Value::Number(n) => Some(Scalar::Number(*n)),
            Value::String(s) => Some(Scalar::String(s.clone())),
            Value::Array(items) => items
                .iter()
                .map(Scalar::from_value)
                .collect::<Option<Vec<_>>>()
                .map(Scalar::Array),
            Value::Hash(entries) => entries
                .iter()
                .map(|(key, value)| Scalar::from_value(value).map(|value| (key.clone(), value)))
                .collect::<Option<IndexMap<_, _>>>()
                .map(Scalar::Object),
            Value::Function(_) | Value::Variable(_) => None,
        }
    }
}

/// One node of a renderable tree: a tag, or a value.
///
/// Upstream's `RenderableTreeNode = Tag | Scalar`. A renderer walks these and a
/// host may build them by hand, which is what makes a renderer outside this
/// crate possible -- the React renderers upstream ships are not ported, and this
/// is the type that keeps writing one an option rather than a fork.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum RenderableTreeNode {
    /// An element: a name, attributes, and children.
    ///
    /// Boxed because [`Tag`] contains a `Vec<RenderableTreeNode>`, so an
    /// unboxed variant would make the enum's size the tag's.
    Tag(Box<Tag>),
    /// A value rendered in place -- most often the string of a text node.
    Scalar(Scalar),
}

impl RenderableTreeNode {
    /// Wrap a tag.
    #[must_use]
    pub fn tag(tag: Tag) -> RenderableTreeNode {
        RenderableTreeNode::Tag(Box::new(tag))
    }

    /// Wrap a string, which is what a text node renders to.
    #[must_use]
    pub fn text(text: impl Into<String>) -> RenderableTreeNode {
        RenderableTreeNode::Scalar(Scalar::String(text.into()))
    }
}

/// What a `transform` hook returns: one node, or a list of them.
///
/// Upstream's `RenderableTreeNodes = RenderableTreeNode | RenderableTreeNode[]`,
/// and the plural matters. A schema with no `render` transforms to its children
/// rather than to an element, so "this node became three nodes" has to be
/// expressible; and a slot rendered into an attribute is a *list* of nodes,
/// which the conformance corpus compares as a JSON array rather than as a
/// single value. Flattening the two would change the tree the corpus grades.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum RenderableTreeNodes {
    /// Exactly one node.
    One(RenderableTreeNode),
    /// Zero or more, in document order.
    Many(Vec<RenderableTreeNode>),
}

impl RenderableTreeNodes {
    /// The nodes as a list, whichever shape they arrived in.
    ///
    /// This is upstream's `flatMap` over a `MaybeArray`, which every consumer
    /// that appends to a child list needs and which is easy to get wrong by
    /// pushing a `Many` in as one child.
    #[must_use]
    pub fn into_vec(self) -> Vec<RenderableTreeNode> {
        match self {
            RenderableTreeNodes::One(node) => vec![node],
            RenderableTreeNodes::Many(nodes) => nodes,
        }
    }
}

impl From<RenderableTreeNode> for RenderableTreeNodes {
    fn from(node: RenderableTreeNode) -> RenderableTreeNodes {
        RenderableTreeNodes::One(node)
    }
}

impl From<Vec<RenderableTreeNode>> for RenderableTreeNodes {
    fn from(nodes: Vec<RenderableTreeNode>) -> RenderableTreeNodes {
        RenderableTreeNodes::Many(nodes)
    }
}

impl From<Tag> for RenderableTreeNodes {
    fn from(tag: Tag) -> RenderableTreeNodes {
        RenderableTreeNodes::One(RenderableTreeNode::tag(tag))
    }
}

/// An element in a renderable tree.
///
/// Mirrors upstream `src/tag.ts`. The name is what a renderer emits -- `p`,
/// `article`, or whatever a schema's `render` said -- and it is a plain string
/// rather than an HTML element type, because this crate decides no HTML policy.
/// A host rendering to something that is not HTML puts its own names here.
///
/// # Why an attribute holds a whole subtree
///
/// Upstream types attributes as `Record<string, any>` and means it: an ordinary
/// attribute is a scalar, but a rendered slot is put in the attribute map as the
/// *transformed nodes* of that slot (`transformer.ts`, `attributes`). The
/// corpus fixes this -- "Basic slot" expects `attributes: {bar: [{tag: p, ...}]}`
/// -- so narrowing attributes to [`Scalar`] would fail cases that are otherwise
/// correct. [`RenderableTreeNodes`] is the honest type, and a scalar attribute
/// is `One(Scalar(..))`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    /// The element name. Upstream defaults it to `div`.
    pub name: String,
    /// The attributes, in authored order.
    pub attributes: IndexMap<String, RenderableTreeNodes>,
    /// The children, in document order.
    pub children: Vec<RenderableTreeNode>,
}

impl Tag {
    /// A tag with no attributes and no children.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Tag {
        Tag {
            name: name.into(),
            attributes: IndexMap::new(),
            children: Vec::new(),
        }
    }

    /// A tag with attributes and children.
    ///
    /// The argument order is upstream's `new Tag(name, attributes, children)`,
    /// so a ported test reads next to the TypeScript it came from.
    #[must_use]
    pub fn with(
        name: impl Into<String>,
        attributes: IndexMap<String, RenderableTreeNodes>,
        children: Vec<RenderableTreeNode>,
    ) -> Tag {
        Tag {
            name: name.into(),
            attributes,
            children,
        }
    }

    /// Set an attribute to a single value, in authored order.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<RenderableTreeNodes>) {
        self.attributes.insert(name.into(), value.into());
    }

    /// Append a child.
    pub fn push(&mut self, child: RenderableTreeNode) {
        self.children.push(child);
    }
}

impl Default for Tag {
    /// Upstream's default element is `div`, and schemas rely on it: a tag
    /// constructed with no name renders as a `div` rather than as nothing.
    fn default() -> Tag {
        Tag::new("div")
    }
}

/// Dropping a renderable tree is iterative, for the reason dropping an AST is.
///
/// [`Node`](crate::ast::Node) carries a manual `Drop` because nesting depth is
/// attacker-controlled and the derived recursive drop aborts the process on a
/// deep document. A renderable tree is *built from* that AST, one tag per
/// nested tag, so it inherits the same exposure and needs the same guard. An
/// abort cannot be caught, so the crate's panic-freedom promise is not true
/// without it.
///
/// One `Drop` covers the whole tree. A [`RenderableTreeNode`] and a
/// [`RenderableTreeNodes`] are shallow wrappers whose derived drops recurse
/// exactly one level before reaching a [`Tag`], and this implementation unlinks
/// every descendant onto the heap before any of them is dropped -- so each tag
/// it drops is already empty and recurses no further. Putting a manual `Drop`
/// on the enums instead would forbid moving a tag *out* of one, which is what
/// a renderer does on every node.
///
/// [`Scalar`] is deliberately not guarded, for the same reason
/// [`Value`](crate::ast::Value) is not: scalar nesting comes from the value
/// grammar, which is bounded at `grammar::MAX_VALUE_DEPTH` (`DIVERGENCES.md`
/// entry 9). Tag nesting has no such bound.
///
/// The cost, stated because it is invisible until someone hits it: a type with a
/// manual `Drop` cannot have a field moved out of it, so taking ownership of
/// [`Tag::children`] needs [`std::mem::take`] rather than a partial move. That
/// is the same tax `Node` charges, paid for the same reason.
impl Drop for Tag {
    fn drop(&mut self) {
        let mut pending: Vec<Tag> = Vec::new();
        unlink(self, &mut pending);
        while let Some(mut tag) = pending.pop() {
            unlink(&mut tag, &mut pending);
            // `tag` is dropped here already emptied, so this recurses once.
        }
    }
}

/// Move every tag directly inside `tag` onto `pending`, leaving it empty.
///
/// Attributes are walked as well as children: a rendered slot is stored in the
/// attribute map as that slot's transformed nodes, so a tree can be arbitrarily
/// deep through attributes alone.
fn unlink(tag: &mut Tag, pending: &mut Vec<Tag>) {
    let children = std::mem::take(&mut tag.children);
    let attributes = std::mem::take(&mut tag.attributes);
    pending.extend(children.into_iter().filter_map(into_tag));
    for (_, value) in attributes {
        pending.extend(value.into_vec().into_iter().filter_map(into_tag));
    }
}

/// The tag inside a node, if it is one. Scalars drop where they stand.
fn into_tag(node: RenderableTreeNode) -> Option<Tag> {
    match node {
        RenderableTreeNode::Tag(tag) => Some(*tag),
        RenderableTreeNode::Scalar(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_element_is_a_div() {
        assert_eq!(Tag::default().name, "div");
    }

    #[test]
    fn an_attribute_may_hold_a_rendered_subtree() {
        // The shape the corpus's "Basic slot" case expects: a slot transformed
        // into a list of nodes, stored under the slot's name.
        let mut foo = Tag::new("foo");
        let paragraph = Tag::with("p", IndexMap::new(), vec![RenderableTreeNode::text("hi")]);
        foo.set("bar", vec![RenderableTreeNode::tag(paragraph)]);
        assert!(matches!(
            foo.attributes.get("bar"),
            Some(RenderableTreeNodes::Many(nodes)) if nodes.len() == 1
        ));
    }

    #[test]
    fn attribute_order_is_authored_order() {
        let mut tag = Tag::new("foo");
        tag.set("z", RenderableTreeNode::text("1"));
        tag.set("a", RenderableTreeNode::text("2"));
        let keys: Vec<&str> = tag.attributes.keys().map(String::as_str).collect();
        assert_eq!(keys, ["z", "a"]);
    }

    #[test]
    fn scalars_come_from_resolved_values_only() {
        use crate::ast::Variable;

        assert_eq!(
            Scalar::from_value(&Value::String("x".into())),
            Some(Scalar::String("x".into()))
        );
        assert_eq!(
            Scalar::from_value(&Value::Array(vec![Value::Number(1.0)])),
            Some(Scalar::Array(vec![Scalar::Number(1.0)]))
        );
        // Unresolved references have no scalar spelling.
        assert_eq!(
            Scalar::from_value(&Value::Variable(Variable::default())),
            None
        );
        // ... and neither does a collection containing one.
        assert_eq!(
            Scalar::from_value(&Value::Array(vec![Value::Variable(Variable::default())])),
            None
        );
    }

    #[test]
    fn dropping_a_deep_tree_does_not_abort() {
        // Nesting depth is attacker-controlled: `{% a %}` repeated is one tag
        // per line, and every one of them becomes a tag here. A derived
        // recursive drop aborts the process on this, which no caller can catch.
        let mut tag = Tag::new("leaf");
        for _ in 0..100_000 {
            tag = Tag::with("a", IndexMap::new(), vec![RenderableTreeNode::tag(tag)]);
        }
        drop(tag);
    }

    #[test]
    fn dropping_a_tree_nested_through_attributes_does_not_abort() {
        // A rendered slot lands in the attribute map, so depth can be reached
        // without a single child. Guarding only `children` would leave this.
        let mut tag = Tag::new("leaf");
        for _ in 0..100_000 {
            let mut outer = Tag::new("a");
            outer.set("slot", RenderableTreeNode::tag(tag));
            tag = outer;
        }
        drop(tag);
    }

    #[test]
    fn many_and_one_flatten_the_same_way() {
        let one = RenderableTreeNodes::One(RenderableTreeNode::text("a"));
        assert_eq!(one.into_vec().len(), 1);
        let many = RenderableTreeNodes::Many(vec![
            RenderableTreeNode::text("a"),
            RenderableTreeNode::text("b"),
        ]);
        assert_eq!(many.into_vec().len(), 2);
    }
}
