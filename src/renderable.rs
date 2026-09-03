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

/// # Why `Clone`, `PartialEq` and `Debug` are written out rather than derived
///
/// For the reason [`Drop`] is: a derived implementation of any of the three
/// recurses once per level, and [`Scalar::Array`] and [`Scalar::Object`] are
/// public and recursive, so a caller can assemble one deep enough to overflow
/// the stack. That is an abort rather than a panic, and an abort cannot be
/// caught -- which would make this crate's panic-freedom promise untrue for a
/// value a host built through the public API.
///
/// The value grammar bounds what this crate *parses* at
/// [`MAX_VALUE_DEPTH`](crate::grammar::MAX_VALUE_DEPTH) (`DIVERGENCES.md`
/// entry 9). It bounds nothing a caller assembles in Rust.
///
/// Each of the three takes the shape its job allows, and they are three
/// different shapes -- which is why there is no shared helper:
///
/// * **`PartialEq`** carries a worklist of pairs: compare one level, push the
///   children pairwise, stop at the first inequality.
/// * **`Clone`** is constructive, so a worklist is not enough -- a parent
///   cannot be built until its children exist. It walks post-order onto an
///   explicit plan, then rebuilds bottom-up off a stack of finished subtrees.
///   No raw pointer into a half-built tree, which is what keeps
///   `unsafe_code = "forbid"` intact.
/// * **`Debug`** emits the derive's own text from a stack of pending tokens.
///   That format is observable -- callers assert on it -- so the `debug_parity`
///   tests compare every shape against a mirror type that still derives
///   `Debug`, in both `{:?}` and `{:#?}`.
impl Clone for Scalar {
    fn clone(&self) -> Self {
        let mut plan = vec![Step::Open(self)];
        let mut done: Vec<Scalar> = Vec::new();

        while let Some(step) = plan.pop() {
            match step {
                Step::Open(scalar) => match scalar {
                    Scalar::Array(items) => {
                        plan.push(Step::Close(scalar));
                        // Reversed, so the stack yields children left to right
                        // and `done` collects finished subtrees in order.
                        for item in items.iter().rev() {
                            plan.push(Step::Open(item));
                        }
                    }
                    Scalar::Object(entries) => {
                        plan.push(Step::Close(scalar));
                        for (_, value) in entries.iter().rev() {
                            plan.push(Step::Open(value));
                        }
                    }
                    Scalar::Null => done.push(Scalar::Null),
                    Scalar::Boolean(value) => done.push(Scalar::Boolean(*value)),
                    Scalar::Number(value) => done.push(Scalar::Number(*value)),
                    Scalar::String(value) => done.push(Scalar::String(value.clone())),
                },
                Step::Close(scalar) => match scalar {
                    Scalar::Array(items) => {
                        let start = done.len().saturating_sub(items.len());
                        let children = done.split_off(start);
                        done.push(Scalar::Array(children));
                    }
                    Scalar::Object(entries) => {
                        let start = done.len().saturating_sub(entries.len());
                        let values = done.split_off(start);
                        done.push(Scalar::Object(
                            entries.keys().cloned().zip(values).collect(),
                        ));
                    }
                    // Only the two composites are ever closed.
                    _ => {}
                },
            }
        }

        done.pop().unwrap_or(Scalar::Null)
    }
}

/// One step of the post-order clone plan: see a node, then rebuild it.
enum Step<'s> {
    Open(&'s Scalar),
    Close(&'s Scalar),
}

impl PartialEq for Scalar {
    fn eq(&self, other: &Self) -> bool {
        let mut work: Vec<(&Scalar, &Scalar)> = vec![(self, other)];
        while let Some((left, right)) = work.pop() {
            match (left, right) {
                (Scalar::Null, Scalar::Null) => {}
                (Scalar::Boolean(a), Scalar::Boolean(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (Scalar::Number(a), Scalar::Number(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (Scalar::String(a), Scalar::String(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (Scalar::Array(a), Scalar::Array(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    work.extend(a.iter().zip(b.iter()));
                }
                (Scalar::Object(a), Scalar::Object(b)) => {
                    // Key lookup rather than a positional zip: `IndexMap`'s own
                    // `PartialEq` compares as an unordered collection, and this
                    // has to keep saying exactly what the derive said.
                    if a.len() != b.len() {
                        return false;
                    }
                    for (key, value) in a {
                        match b.get(key) {
                            Some(other) => work.push((value, other)),
                            None => return false,
                        }
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

impl std::fmt::Debug for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alternate = f.alternate();
        let mut stack: Vec<DebugTok<'_>> = vec![DebugTok::Node(self, 0)];

        while let Some(token) = stack.pop() {
            match token {
                DebugTok::Text(text) => f.write_str(text)?,
                DebugTok::Owned(text) => f.write_str(&text)?,
                DebugTok::Line(depth) => {
                    f.write_str("\n")?;
                    for _ in 0..depth {
                        f.write_str("    ")?;
                    }
                }
                DebugTok::Node(scalar, depth) => {
                    expand_scalar(f, &mut stack, scalar, depth, alternate)?;
                }
            }
        }
        Ok(())
    }
}

/// One pending piece of `Debug` output.
///
/// The stack is what makes the walk iterative; `Line` carries its own depth
/// because a token is emitted long after the node that queued it.
enum DebugTok<'s> {
    Node(&'s Scalar, usize),
    Text(&'static str),
    Owned(String),
    Line(usize),
}

/// Write a scalar's opening text and queue the rest of it.
///
/// Leaves are written whole: their fields cannot recurse, so there is nothing
/// to queue. Only `Array` and `Object` push.
fn expand_scalar<'s>(
    f: &mut std::fmt::Formatter<'_>,
    stack: &mut Vec<DebugTok<'s>>,
    scalar: &'s Scalar,
    depth: usize,
    alternate: bool,
) -> std::fmt::Result {
    /// The derive expands a tuple variant's field onto its own line under
    /// `{:#?}`, even when the field is a bare `bool`.
    fn leaf(
        f: &mut std::fmt::Formatter<'_>,
        name: &str,
        body: &str,
        depth: usize,
        alternate: bool,
    ) -> std::fmt::Result {
        if alternate {
            let pad = "    ".repeat(depth);
            write!(f, "{name}(\n{pad}    {body},\n{pad})")
        } else {
            write!(f, "{name}({body})")
        }
    }

    match scalar {
        Scalar::Null => f.write_str("Null"),
        Scalar::Boolean(value) => leaf(f, "Boolean", &format!("{value:?}"), depth, alternate),
        Scalar::Number(value) => leaf(f, "Number", &format!("{value:?}"), depth, alternate),
        Scalar::String(value) => leaf(f, "String", &format!("{value:?}"), depth, alternate),
        Scalar::Array(items) => {
            if items.is_empty() {
                return leaf(f, "Array", "[]", depth, alternate);
            }
            f.write_str("Array(")?;
            // Pushed in reverse: the stack emits them in the order written here.
            let mut queued: Vec<DebugTok<'s>> = Vec::new();
            if alternate {
                queued.push(DebugTok::Line(depth + 1));
                queued.push(DebugTok::Text("["));
                for item in items {
                    queued.push(DebugTok::Line(depth + 2));
                    queued.push(DebugTok::Node(item, depth + 2));
                    queued.push(DebugTok::Text(","));
                }
                queued.push(DebugTok::Line(depth + 1));
                queued.push(DebugTok::Text("],"));
                queued.push(DebugTok::Line(depth));
                queued.push(DebugTok::Text(")"));
            } else {
                queued.push(DebugTok::Text("["));
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        queued.push(DebugTok::Text(", "));
                    }
                    queued.push(DebugTok::Node(item, depth));
                }
                queued.push(DebugTok::Text("])"));
            }
            stack.extend(queued.into_iter().rev());
            Ok(())
        }
        Scalar::Object(entries) => {
            if entries.is_empty() {
                return leaf(f, "Object", "{}", depth, alternate);
            }
            f.write_str("Object(")?;
            let mut queued: Vec<DebugTok<'s>> = Vec::new();
            if alternate {
                queued.push(DebugTok::Line(depth + 1));
                queued.push(DebugTok::Text("{"));
                for (key, value) in entries {
                    queued.push(DebugTok::Line(depth + 2));
                    queued.push(DebugTok::Owned(format!("{key:?}: ")));
                    queued.push(DebugTok::Node(value, depth + 2));
                    queued.push(DebugTok::Text(","));
                }
                queued.push(DebugTok::Line(depth + 1));
                queued.push(DebugTok::Text("},"));
                queued.push(DebugTok::Line(depth));
                queued.push(DebugTok::Text(")"));
            } else {
                queued.push(DebugTok::Text("{"));
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        queued.push(DebugTok::Text(", "));
                    }
                    queued.push(DebugTok::Owned(format!("{key:?}: ")));
                    queued.push(DebugTok::Node(value, depth));
                }
                queued.push(DebugTok::Text("})"));
            }
            stack.extend(queued.into_iter().rev());
            Ok(())
        }
    }
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
pub struct Tag {
    /// The element name. Upstream defaults it to `div`.
    pub name: String,
    /// The attributes, in authored order.
    pub attributes: IndexMap<String, RenderableTreeNodes>,
    /// The children, in document order.
    pub children: Vec<RenderableTreeNode>,
}

/// The tags nested inside this one, in the order the traversals agree to use.
///
/// Attributes first, in authored order, then children. Every hand-written
/// traversal walks this order, which is what lets [`Clone`] queue subtrees and
/// then reclaim them positionally.
fn nested_tags(tag: &Tag) -> Vec<&Tag> {
    let mut out = Vec::new();
    for (_, nodes) in &tag.attributes {
        for node in nodes_slice(nodes) {
            if let RenderableTreeNode::Tag(inner) = node {
                out.push(inner.as_ref());
            }
        }
    }
    for node in &tag.children {
        if let RenderableTreeNode::Tag(inner) = node {
            out.push(inner.as_ref());
        }
    }
    out
}

/// The nodes inside a [`RenderableTreeNodes`], as a slice either way.
fn nodes_slice(nodes: &RenderableTreeNodes) -> &[RenderableTreeNode] {
    match nodes {
        RenderableTreeNodes::One(node) => std::slice::from_ref(node),
        RenderableTreeNodes::Many(many) => many.as_slice(),
    }
}

/// # Why the three traversals are written out rather than derived
///
/// The reasoning is on [`Scalar`], and applies here for the same reason it
/// applies to [`Drop`]: a tag's children are tags, so a derived `Clone`,
/// `PartialEq` or `Debug` recurses per level of a tree whose depth is
/// attacker-controlled.
///
/// [`RenderableTreeNode`] and [`RenderableTreeNodes`] keep their derives, and
/// that is safe *because* these exist: their recursion reaches a [`Tag`] or a
/// [`Scalar`] in one step, and both stop there. Nothing here may call those
/// derives on a nested tag, which is why the walks decompose them by hand.
impl Clone for Tag {
    fn clone(&self) -> Self {
        enum Step<'t> {
            Open(&'t Tag),
            Close(&'t Tag),
        }

        let mut plan = vec![Step::Open(self)];
        let mut done: Vec<Tag> = Vec::new();

        while let Some(step) = plan.pop() {
            match step {
                Step::Open(tag) => {
                    plan.push(Step::Close(tag));
                    for nested in nested_tags(tag).into_iter().rev() {
                        plan.push(Step::Open(nested));
                    }
                }
                Step::Close(tag) => {
                    let count = nested_tags(tag).len();
                    let start = done.len().saturating_sub(count);
                    let mut finished = done.split_off(start).into_iter();

                    let mut attributes = IndexMap::new();
                    for (key, nodes) in &tag.attributes {
                        let rebuilt = match nodes {
                            RenderableTreeNodes::One(node) => {
                                RenderableTreeNodes::One(clone_node_taking(node, &mut finished))
                            }
                            RenderableTreeNodes::Many(many) => RenderableTreeNodes::Many(
                                many.iter()
                                    .map(|node| clone_node_taking(node, &mut finished))
                                    .collect(),
                            ),
                        };
                        attributes.insert(key.clone(), rebuilt);
                    }
                    let children = tag
                        .children
                        .iter()
                        .map(|node| clone_node_taking(node, &mut finished))
                        .collect();

                    done.push(Tag {
                        name: tag.name.clone(),
                        attributes,
                        children,
                    });
                }
            }
        }

        done.pop().unwrap_or_else(|| Tag::new("div"))
    }
}

/// Rebuild one child node, taking an already-finished tag when it is one.
///
/// The iterator is consumed in the same order [`nested_tags`] produced, which
/// is what makes the positional hand-off correct.
fn clone_node_taking(
    node: &RenderableTreeNode,
    finished: &mut impl Iterator<Item = Tag>,
) -> RenderableTreeNode {
    match node {
        RenderableTreeNode::Tag(_) => finished.next().map_or_else(
            || RenderableTreeNode::tag(Tag::new("div")),
            |tag| RenderableTreeNode::Tag(Box::new(tag)),
        ),
        // `Scalar` carries its own iterative clone, so this recurses no further.
        RenderableTreeNode::Scalar(scalar) => RenderableTreeNode::Scalar(scalar.clone()),
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        let mut work: Vec<(&Tag, &Tag)> = vec![(self, other)];
        while let Some((left, right)) = work.pop() {
            if left.name != right.name
                || left.attributes.len() != right.attributes.len()
                || left.children.len() != right.children.len()
            {
                return false;
            }
            // Attributes are an `IndexMap`, whose own `PartialEq` is unordered.
            for (key, nodes) in &left.attributes {
                let Some(other_nodes) = right.attributes.get(key) else {
                    return false;
                };
                if !push_node_pairs(nodes, other_nodes, &mut work) {
                    return false;
                }
            }
            for (a, b) in left.children.iter().zip(right.children.iter()) {
                if !push_node_pair(a, b, &mut work) {
                    return false;
                }
            }
        }
        true
    }
}

/// Compare two attribute values shallowly, queueing any tag pair.
///
/// Returns `false` the moment they cannot be equal. `One` and `Many` are
/// different variants and so are never equal, which is what the derive said.
fn push_node_pairs<'t>(
    left: &'t RenderableTreeNodes,
    right: &'t RenderableTreeNodes,
    work: &mut Vec<(&'t Tag, &'t Tag)>,
) -> bool {
    match (left, right) {
        (RenderableTreeNodes::One(a), RenderableTreeNodes::One(b)) => push_node_pair(a, b, work),
        (RenderableTreeNodes::Many(a), RenderableTreeNodes::Many(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .zip(b.iter())
                .all(|(x, y)| push_node_pair(x, y, work))
        }
        _ => false,
    }
}

/// Compare two child nodes shallowly, queueing the pair when both are tags.
fn push_node_pair<'t>(
    left: &'t RenderableTreeNode,
    right: &'t RenderableTreeNode,
    work: &mut Vec<(&'t Tag, &'t Tag)>,
) -> bool {
    match (left, right) {
        (RenderableTreeNode::Tag(a), RenderableTreeNode::Tag(b)) => {
            work.push((a.as_ref(), b.as_ref()));
            true
        }
        // `Scalar`'s own equality is iterative, so this recurses no further.
        (RenderableTreeNode::Scalar(a), RenderableTreeNode::Scalar(b)) => a == b,
        _ => false,
    }
}

impl std::fmt::Debug for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alternate = f.alternate();
        let mut stack: Vec<TagTok<'_>> = vec![TagTok::Tag(self, 0)];

        while let Some(token) = stack.pop() {
            match token {
                TagTok::Text(text) => f.write_str(text)?,
                TagTok::Owned(text) => f.write_str(&text)?,
                TagTok::Line(depth) => {
                    f.write_str("\n")?;
                    for _ in 0..depth {
                        f.write_str("    ")?;
                    }
                }
                TagTok::Tag(tag, depth) => expand_tag(f, &mut stack, tag, depth, alternate)?,
                TagTok::Nodes(nodes, depth) => {
                    expand_nodes(&mut stack, nodes, depth, alternate);
                }
                TagTok::Node(node, depth) => {
                    expand_node(f, &mut stack, node, depth, alternate)?;
                }
            }
        }
        Ok(())
    }
}

/// One pending piece of `Debug` output for a renderable tree.
enum TagTok<'t> {
    Tag(&'t Tag, usize),
    Nodes(&'t RenderableTreeNodes, usize),
    Node(&'t RenderableTreeNode, usize),
    Text(&'static str),
    Owned(String),
    Line(usize),
}

/// Re-pad every line after the first, so a block formatted at column zero can
/// be spliced in at `depth`.
fn indent_block(body: &str, depth: usize) -> String {
    let pad = "    ".repeat(depth);
    body.replace('\n', &format!("\n{pad}"))
}

/// `Tag { name: .., attributes: .., children: .. }`, as the derive writes it.
fn expand_tag<'t>(
    f: &mut std::fmt::Formatter<'_>,
    stack: &mut Vec<TagTok<'t>>,
    tag: &'t Tag,
    depth: usize,
    alternate: bool,
) -> std::fmt::Result {
    let name = format!("{:?}", tag.name);
    let mut queued: Vec<TagTok<'t>> = Vec::new();

    if alternate {
        f.write_str("Tag {")?;
        queued.push(TagTok::Line(depth + 1));
        queued.push(TagTok::Owned(format!("name: {name},")));
        queued.push(TagTok::Line(depth + 1));
        if tag.attributes.is_empty() {
            queued.push(TagTok::Text("attributes: {},"));
        } else {
            queued.push(TagTok::Text("attributes: {"));
            for (key, nodes) in &tag.attributes {
                queued.push(TagTok::Line(depth + 2));
                queued.push(TagTok::Owned(format!("{key:?}: ")));
                queued.push(TagTok::Nodes(nodes, depth + 2));
                queued.push(TagTok::Text(","));
            }
            queued.push(TagTok::Line(depth + 1));
            queued.push(TagTok::Text("},"));
        }
        queued.push(TagTok::Line(depth + 1));
        if tag.children.is_empty() {
            queued.push(TagTok::Text("children: [],"));
        } else {
            queued.push(TagTok::Text("children: ["));
            for child in &tag.children {
                queued.push(TagTok::Line(depth + 2));
                queued.push(TagTok::Node(child, depth + 2));
                queued.push(TagTok::Text(","));
            }
            queued.push(TagTok::Line(depth + 1));
            queued.push(TagTok::Text("],"));
        }
        queued.push(TagTok::Line(depth));
        queued.push(TagTok::Text("}"));
    } else {
        write!(f, "Tag {{ name: {name}, attributes: ")?;
        if tag.attributes.is_empty() {
            queued.push(TagTok::Text("{}"));
        } else {
            queued.push(TagTok::Text("{"));
            for (index, (key, nodes)) in tag.attributes.iter().enumerate() {
                if index > 0 {
                    queued.push(TagTok::Text(", "));
                }
                queued.push(TagTok::Owned(format!("{key:?}: ")));
                queued.push(TagTok::Nodes(nodes, depth));
            }
            queued.push(TagTok::Text("}"));
        }
        queued.push(TagTok::Text(", children: ["));
        for (index, child) in tag.children.iter().enumerate() {
            if index > 0 {
                queued.push(TagTok::Text(", "));
            }
            queued.push(TagTok::Node(child, depth));
        }
        queued.push(TagTok::Text("] }"));
    }

    stack.extend(queued.into_iter().rev());
    Ok(())
}

/// `One(..)` or `Many([..])`.
fn expand_nodes<'t>(
    stack: &mut Vec<TagTok<'t>>,
    nodes: &'t RenderableTreeNodes,
    depth: usize,
    alternate: bool,
) {
    let mut queued: Vec<TagTok<'t>> = Vec::new();
    match nodes {
        RenderableTreeNodes::One(node) => {
            queued.push(TagTok::Text("One("));
            if alternate {
                queued.push(TagTok::Line(depth + 1));
                queued.push(TagTok::Node(node, depth + 1));
                queued.push(TagTok::Text(","));
                queued.push(TagTok::Line(depth));
            } else {
                queued.push(TagTok::Node(node, depth));
            }
            queued.push(TagTok::Text(")"));
        }
        RenderableTreeNodes::Many(many) if many.is_empty() => {
            queued.push(TagTok::Text(if alternate { "Many(" } else { "Many([])" }));
            if alternate {
                queued.push(TagTok::Line(depth + 1));
                queued.push(TagTok::Text("[],"));
                queued.push(TagTok::Line(depth));
                queued.push(TagTok::Text(")"));
            }
        }
        RenderableTreeNodes::Many(many) => {
            queued.push(TagTok::Text("Many("));
            if alternate {
                queued.push(TagTok::Line(depth + 1));
                queued.push(TagTok::Text("["));
                for node in many {
                    queued.push(TagTok::Line(depth + 2));
                    queued.push(TagTok::Node(node, depth + 2));
                    queued.push(TagTok::Text(","));
                }
                queued.push(TagTok::Line(depth + 1));
                queued.push(TagTok::Text("],"));
                queued.push(TagTok::Line(depth));
            } else {
                queued.push(TagTok::Text("["));
                for (index, node) in many.iter().enumerate() {
                    if index > 0 {
                        queued.push(TagTok::Text(", "));
                    }
                    queued.push(TagTok::Node(node, depth));
                }
                queued.push(TagTok::Text("]"));
            }
            queued.push(TagTok::Text(")"));
        }
    }
    stack.extend(queued.into_iter().rev());
}

/// `Tag(..)` or `Scalar(..)`.
fn expand_node<'t>(
    f: &mut std::fmt::Formatter<'_>,
    stack: &mut Vec<TagTok<'t>>,
    node: &'t RenderableTreeNode,
    depth: usize,
    alternate: bool,
) -> std::fmt::Result {
    match node {
        RenderableTreeNode::Tag(inner) => {
            let mut queued: Vec<TagTok<'t>> = Vec::new();
            f.write_str("Tag(")?;
            if alternate {
                queued.push(TagTok::Line(depth + 1));
                queued.push(TagTok::Tag(inner.as_ref(), depth + 1));
                queued.push(TagTok::Text(","));
                queued.push(TagTok::Line(depth));
            } else {
                queued.push(TagTok::Tag(inner.as_ref(), depth));
            }
            queued.push(TagTok::Text(")"));
            stack.extend(queued.into_iter().rev());
            Ok(())
        }
        // `Scalar` carries its own iterative `Debug`, so delegating stops here.
        // The alternate block is re-indented, because it formats from column
        // zero and is being spliced in one level down.
        RenderableTreeNode::Scalar(scalar) => {
            if alternate {
                let pad = "    ".repeat(depth);
                let block = indent_block(&format!("{scalar:#?}"), depth + 1);
                write!(f, "Scalar(\n{pad}    {block},\n{pad})")
            } else {
                write!(f, "Scalar({scalar:?})")
            }
        }
    }
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
/// [`Scalar`] carries its own guard, below, for a different reason: its nesting
/// is bounded for values this crate builds and unbounded for values a caller
/// builds.
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

/// Dropping a scalar is iterative, for the reason dropping a [`Value`] is.
///
/// Scalar nesting inside this crate comes from the value grammar, which is
/// bounded at [`MAX_VALUE_DEPTH`](crate::grammar::MAX_VALUE_DEPTH)
/// (`DIVERGENCES.md` entry 9): every `Scalar` the crate builds passes through
/// [`Scalar::from_value`], so no document can produce one deep enough to
/// overflow a recursive drop.
///
/// **That bound does not bind a caller.** [`Scalar::Array`] and
/// [`Scalar::Object`] are public and recursive, so a host can assemble one of
/// any depth and a derived drop would abort the process on it. An abort cannot
/// be caught, so the crate's panic-freedom promise does not survive one. Same
/// exposure as [`Value`], same guard.
impl Drop for Scalar {
    fn drop(&mut self) {
        let mut pending: Vec<Scalar> = Vec::new();
        unlink_scalar(self, &mut pending);
        while let Some(mut scalar) = pending.pop() {
            unlink_scalar(&mut scalar, &mut pending);
            // `scalar` is dropped here already emptied, so this recurses once.
        }
    }
}

/// Move every scalar directly inside `scalar` onto `pending`, leaving it empty.
fn unlink_scalar(scalar: &mut Scalar, pending: &mut Vec<Scalar>) {
    match scalar {
        Scalar::Array(items) => pending.append(items),
        Scalar::Object(entries) => pending.extend(entries.drain(..).map(|(_, value)| value)),
        _ => {}
    }
}

/// `Debug` output is observable -- callers assert on it -- so the hand-written
/// emitters are pinned against the derive rather than against a reading of it.
///
/// `mirror` holds structurally identical types that still `#[derive(Debug)]`.
/// Rust prints a variant by its own name and a struct by the last segment of
/// its path, so a mirror declared in a nested module formats identically to the
/// real type as long as the shapes agree. Any drift in the emitters shows up
/// here as a string mismatch, in both `{:?}` and `{:#?}`.
#[cfg(test)]
mod debug_parity {
    use super::*;

    mod mirror {
        // Every field exists to be formatted by the derive and is never read
        // otherwise -- that is the whole point of the type.
        #![allow(dead_code)]

        use indexmap::IndexMap;

        #[derive(Debug)]
        pub enum Scalar {
            Null,
            Boolean(bool),
            Number(f64),
            String(String),
            Array(Vec<Scalar>),
            Object(IndexMap<String, Scalar>),
        }
    }

    fn to_mirror(scalar: &Scalar) -> mirror::Scalar {
        match scalar {
            Scalar::Null => mirror::Scalar::Null,
            Scalar::Boolean(value) => mirror::Scalar::Boolean(*value),
            Scalar::Number(value) => mirror::Scalar::Number(*value),
            Scalar::String(value) => mirror::Scalar::String(value.clone()),
            Scalar::Array(items) => mirror::Scalar::Array(items.iter().map(to_mirror).collect()),
            Scalar::Object(entries) => mirror::Scalar::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), to_mirror(value)))
                    .collect(),
            ),
        }
    }

    fn assert_parity(scalar: &Scalar) {
        let reference = to_mirror(scalar);
        assert_eq!(
            format!("{scalar:?}"),
            format!("{reference:?}"),
            "plain Debug diverged from the derive"
        );
        assert_eq!(
            format!("{scalar:#?}"),
            format!("{reference:#?}"),
            "alternate Debug diverged from the derive"
        );
    }

    fn object(pairs: Vec<(&str, Scalar)>) -> Scalar {
        Scalar::Object(
            pairs
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    mod tag_mirror {
        #![allow(dead_code)]

        use indexmap::IndexMap;

        #[derive(Debug)]
        pub enum RenderableTreeNode {
            Tag(Box<Tag>),
            Scalar(crate::renderable::Scalar),
        }

        #[derive(Debug)]
        pub enum RenderableTreeNodes {
            One(RenderableTreeNode),
            Many(Vec<RenderableTreeNode>),
        }

        #[derive(Debug)]
        pub struct Tag {
            pub name: String,
            pub attributes: IndexMap<String, RenderableTreeNodes>,
            pub children: Vec<RenderableTreeNode>,
        }
    }

    fn tag_to_mirror(tag: &Tag) -> tag_mirror::Tag {
        tag_mirror::Tag {
            name: tag.name.clone(),
            attributes: tag
                .attributes
                .iter()
                .map(|(key, nodes)| (key.clone(), nodes_to_mirror(nodes)))
                .collect(),
            children: tag.children.iter().map(node_to_mirror).collect(),
        }
    }

    fn nodes_to_mirror(nodes: &RenderableTreeNodes) -> tag_mirror::RenderableTreeNodes {
        match nodes {
            RenderableTreeNodes::One(node) => {
                tag_mirror::RenderableTreeNodes::One(node_to_mirror(node))
            }
            RenderableTreeNodes::Many(many) => {
                tag_mirror::RenderableTreeNodes::Many(many.iter().map(node_to_mirror).collect())
            }
        }
    }

    fn node_to_mirror(node: &RenderableTreeNode) -> tag_mirror::RenderableTreeNode {
        match node {
            RenderableTreeNode::Tag(inner) => {
                tag_mirror::RenderableTreeNode::Tag(Box::new(tag_to_mirror(inner)))
            }
            RenderableTreeNode::Scalar(scalar) => {
                tag_mirror::RenderableTreeNode::Scalar(scalar.clone())
            }
        }
    }

    fn assert_tag_parity(tag: &Tag) {
        let reference = tag_to_mirror(tag);
        assert_eq!(format!("{tag:?}"), format!("{reference:?}"), "plain Debug");
        assert_eq!(
            format!("{tag:#?}"),
            format!("{reference:#?}"),
            "alternate Debug"
        );
    }

    #[test]
    fn every_tag_shape_formats_as_the_derive_would() {
        let leaf = Tag::new("leaf");

        let mut with_scalar_attr = Tag::new("p");
        with_scalar_attr.set("k", RenderableTreeNode::text("hi"));

        let mut with_tag_attr = Tag::new("slotted");
        with_tag_attr.set("slot", RenderableTreeNode::tag(Tag::new("inner")));

        let mut many_attr = Tag::new("many");
        many_attr.set(
            "list",
            vec![
                RenderableTreeNode::text("a"),
                RenderableTreeNode::tag(Tag::new("b")),
            ],
        );

        let mut empty_many = Tag::new("emptymany");
        empty_many.set("list", Vec::new());

        let nested = Tag::with(
            "outer",
            IndexMap::new(),
            vec![
                RenderableTreeNode::tag(Tag::with(
                    "middle",
                    IndexMap::new(),
                    vec![RenderableTreeNode::tag(leaf.clone())],
                )),
                RenderableTreeNode::Scalar(Scalar::Array(vec![Scalar::Null])),
            ],
        );

        for shape in &[
            leaf,
            with_scalar_attr,
            with_tag_attr,
            many_attr,
            empty_many,
            nested,
        ] {
            assert_tag_parity(shape);
        }
    }

    #[test]
    fn a_deep_tag_survives_all_three_traversals() {
        let mut tag = Tag::new("leaf");
        for _ in 0..100_000 {
            tag = Tag::with("a", IndexMap::new(), vec![RenderableTreeNode::tag(tag)]);
        }
        let copy = tag.clone();
        assert!(copy == tag, "an iterative clone must equal its source");
        assert!(format!("{tag:?}").starts_with("Tag { name: \"a\""));
    }

    #[test]
    fn a_tag_deep_through_attributes_survives_all_three() {
        // Depth reached with no child at all, which is the shape `Drop` needed
        // its own guard for.
        let mut tag = Tag::new("leaf");
        for _ in 0..100_000 {
            let mut outer = Tag::new("a");
            outer.set("slot", RenderableTreeNode::tag(tag));
            tag = outer;
        }
        let copy = tag.clone();
        assert_eq!(copy, tag);
    }

    #[test]
    fn tag_equality_distinguishes_one_from_many() {
        // The derive compares variants, so `One(x)` never equals `Many([x])`.
        let mut one = Tag::new("t");
        one.set("k", RenderableTreeNode::text("x"));
        let mut many = Tag::new("t");
        many.set("k", vec![RenderableTreeNode::text("x")]);
        assert_ne!(one, many);
    }

    #[test]
    fn every_scalar_shape_formats_as_the_derive_would() {
        let shapes = vec![
            Scalar::Null,
            Scalar::Boolean(true),
            Scalar::Boolean(false),
            Scalar::Number(1.0),
            Scalar::Number(-0.5),
            Scalar::String("hi".to_owned()),
            // Escaping is the string's own Debug, not ours -- pinned anyway.
            Scalar::String("a \"quote\" and a \\ and a \n".to_owned()),
            Scalar::Array(Vec::new()),
            object(Vec::new()),
            Scalar::Array(vec![Scalar::Null]),
            Scalar::Array(vec![Scalar::Null, Scalar::Boolean(true)]),
            object(vec![("a", Scalar::Null)]),
            object(vec![("a", Scalar::Null), ("b", Scalar::Number(2.0))]),
            // Nesting through both composites, and an empty one inside a full.
            Scalar::Array(vec![
                Scalar::Array(vec![Scalar::Number(1.0)]),
                object(vec![("k", Scalar::Array(Vec::new()))]),
                Scalar::String("x".to_owned()),
            ]),
            object(vec![(
                "outer",
                object(vec![("inner", Scalar::Array(vec![Scalar::Null]))]),
            )]),
        ];
        for shape in &shapes {
            assert_parity(shape);
        }
    }

    #[test]
    fn a_deep_scalar_formats_without_aborting() {
        // The reason the emitter exists. The mirror is not built here: a
        // recursive `to_mirror` would overflow before the assertion could run,
        // which is the defect restated.
        let mut scalar = Scalar::Null;
        for _ in 0..100_000 {
            scalar = Scalar::Array(vec![scalar]);
        }
        let rendered = format!("{scalar:?}");
        assert!(rendered.starts_with("Array([Array("));
        assert!(rendered.ends_with(")])"));
    }

    #[test]
    fn a_deep_scalar_clones_and_compares_without_aborting() {
        let mut scalar = Scalar::Null;
        for _ in 0..100_000 {
            scalar = Scalar::Array(vec![scalar]);
        }
        let copy = scalar.clone();
        assert!(copy == scalar, "an iterative clone must equal its source");
    }

    #[test]
    fn cloning_preserves_order_and_shape() {
        let original = object(vec![
            ("z", Scalar::Array(vec![Scalar::Number(1.0), Scalar::Null])),
            ("a", Scalar::String("x".to_owned())),
        ]);
        let copy = original.clone();
        assert_eq!(format!("{copy:?}"), format!("{original:?}"));
        let Scalar::Object(entries) = &copy else {
            panic!("expected an object")
        };
        assert_eq!(entries.keys().collect::<Vec<_>>(), ["z", "a"]);
    }

    #[test]
    fn equality_ignores_object_order_as_indexmap_does() {
        // `IndexMap::eq` compares as an unordered collection. The hand-written
        // `PartialEq` has to keep saying that, so this pins it.
        let left = object(vec![("a", Scalar::Null), ("b", Scalar::Number(1.0))]);
        let right = object(vec![("b", Scalar::Number(1.0)), ("a", Scalar::Null)]);
        assert_eq!(left, right);

        let different = object(vec![("a", Scalar::Null), ("b", Scalar::Number(2.0))]);
        assert_ne!(left, different);
        assert_ne!(left, object(vec![("a", Scalar::Null)]));
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
    fn dropping_a_deep_scalar_array_does_not_abort() {
        // The crate never builds one this deep -- the value grammar is bounded
        // at MAX_VALUE_DEPTH -- but `Scalar` is public and `Array` is
        // recursive, so a host can. A derived drop aborts here, and an abort is
        // not catchable, so the panic-freedom promise would not survive it.
        let mut scalar = Scalar::Null;
        for _ in 0..100_000 {
            scalar = Scalar::Array(vec![scalar]);
        }
        drop(scalar);
    }

    #[test]
    fn dropping_a_deep_scalar_object_does_not_abort() {
        // The other recursive variant, which a guard over `Array` alone leaves.
        let mut scalar = Scalar::Null;
        for _ in 0..100_000 {
            let mut object = IndexMap::new();
            object.insert("k".to_string(), scalar);
            scalar = Scalar::Object(object);
        }
        drop(scalar);
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
