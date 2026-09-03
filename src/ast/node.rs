//! The document tree.
//!
//! Mirrors upstream `src/ast/node.ts`. A [`Node`] is one element of a parsed
//! document -- a paragraph, a heading, a tag, a run of text -- carrying its
//! attributes, its children, the annotations that were written on it, the
//! problems found in it, and where it came from.
//!
//! Upstream's `Node` also carries `resolve`, `findSchema`, `transformAttributes`
//! and `transform`, which are one-line delegations to the transformer. They are
//! not ported here: the transformer reads a `Config`, and putting a method on
//! the AST that needs the whole configuration surface would make the leaf type
//! depend on the stage above it. The transform stage takes `&Node` instead,
//! which is the same call with the arrow pointing the right way.
//!
//! # Two shapes fixed here
//!
//! - **Attributes are an [`IndexMap`], in authored order.** `{% foo a=1 b=2 %}`
//!   and `{% foo b=2 a=1 %}` are different documents and must render as
//!   different bytes.
//! - **A node borrows its source.** [`Node::location`] holds a
//!   [`Location`], which borrows the text it spans. The lifetime stops at the
//!   AST -- transform produces an owned renderable tree -- so only this layer
//!   and the formatter carry it.

use indexmap::IndexMap;

use crate::ast::{Location, ValidationError, Value};
use crate::grammar::Attribute;

/// What kind of node this is.
///
/// Upstream's `NodeType` is a union of string literals in `types.ts`, and the
/// spellings here are those strings exactly, because a schema is looked up by
/// them: a host writing `nodes: { fence: ... }` is naming this enum.
///
/// `#[non_exhaustive]` because Markdoc gained node types across its 0.5.x line.
/// Matching exhaustively on it in a host would turn each new one into a
/// breaking release of this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum NodeType {
    /// `> quoted`.
    Blockquote,
    /// Inline code, `` `x` ``.
    Code,
    /// An HTML comment, when comments are enabled.
    Comment,
    /// The root of a parsed document.
    Document,
    /// `*emphasis*`.
    Em,
    /// A tag whose internals did not parse. Carries the failure in
    /// [`Node::errors`].
    Error,
    /// A fenced code block.
    Fence,
    /// A line break written as two trailing spaces or a backslash.
    Hardbreak,
    /// `# heading`.
    Heading,
    /// A thematic break.
    Hr,
    /// `![alt](src)`.
    Image,
    /// The inline content of a block.
    ///
    /// Not a Markdoc construct: it is the seam markdown-it puts between a block
    /// and its inline children, and this port keeps it because the annotation
    /// rules depend on it. An annotation is applied to the node that owns the
    /// inline run, so `# Title {% #id %}` sets `id` on the heading rather than
    /// on the text beside it, and an annotation with no inline run above it is
    /// the `no-inline-annotations` error.
    Inline,
    /// A list item.
    Item,
    /// `[text](href)`.
    Link,
    /// An ordered or unordered list.
    List,
    /// The default, and what a host-constructed node is unless it says
    /// otherwise.
    #[default]
    Node,
    /// A paragraph.
    Paragraph,
    /// `~~strikethrough~~`.
    S,
    /// A newline inside a block.
    Softbreak,
    /// `**strong**`.
    Strong,
    /// A table.
    Table,
    /// A Markdoc tag. The tag name is in [`Node::tag`].
    Tag,
    /// A table body.
    Tbody,
    /// A table body cell.
    Td,
    /// A run of literal text. Its content is the `content` attribute, which may
    /// hold a [`Value::Variable`] rather than a string.
    Text,
    /// A table header cell.
    Th,
    /// A table header.
    Thead,
    /// A table row.
    Tr,
}

impl NodeType {
    /// Upstream's spelling, which is the key a schema is registered under.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            NodeType::Blockquote => "blockquote",
            NodeType::Code => "code",
            NodeType::Comment => "comment",
            NodeType::Document => "document",
            NodeType::Em => "em",
            NodeType::Error => "error",
            NodeType::Fence => "fence",
            NodeType::Hardbreak => "hardbreak",
            NodeType::Heading => "heading",
            NodeType::Hr => "hr",
            NodeType::Image => "image",
            NodeType::Inline => "inline",
            NodeType::Item => "item",
            NodeType::Link => "link",
            NodeType::List => "list",
            NodeType::Node => "node",
            NodeType::Paragraph => "paragraph",
            NodeType::S => "s",
            NodeType::Softbreak => "softbreak",
            NodeType::Strong => "strong",
            NodeType::Table => "table",
            NodeType::Tag => "tag",
            NodeType::Tbody => "tbody",
            NodeType::Td => "td",
            NodeType::Text => "text",
            NodeType::Th => "th",
            NodeType::Thead => "thead",
            NodeType::Tr => "tr",
        }
    }

    /// The node type upstream spells `name`, or [`None`].
    ///
    /// The inverse of [`NodeType::as_str`]. A host keys its `nodes` schema map
    /// by these strings -- upstream's config is a JavaScript object literal, and
    /// anything read from a file or a manifest arrives as text -- so the mapping
    /// has to run in both directions. Returning [`Option`] rather than defaulting
    /// to [`NodeType::Node`] is the point: a misspelled key is a schema that
    /// silently never applies, which is the hardest kind of schema bug to see.
    #[must_use]
    pub fn from_name(name: &str) -> Option<NodeType> {
        // Written as a match on the same list `as_str` produces, so adding a
        // variant fails to compile here too rather than quietly losing a name.
        Some(match name {
            "blockquote" => NodeType::Blockquote,
            "code" => NodeType::Code,
            "comment" => NodeType::Comment,
            "document" => NodeType::Document,
            "em" => NodeType::Em,
            "error" => NodeType::Error,
            "fence" => NodeType::Fence,
            "hardbreak" => NodeType::Hardbreak,
            "heading" => NodeType::Heading,
            "hr" => NodeType::Hr,
            "image" => NodeType::Image,
            "inline" => NodeType::Inline,
            "item" => NodeType::Item,
            "link" => NodeType::Link,
            "list" => NodeType::List,
            "node" => NodeType::Node,
            "paragraph" => NodeType::Paragraph,
            "s" => NodeType::S,
            "softbreak" => NodeType::Softbreak,
            "strong" => NodeType::Strong,
            "table" => NodeType::Table,
            "tag" => NodeType::Tag,
            "tbody" => NodeType::Tbody,
            "td" => NodeType::Td,
            "text" => NodeType::Text,
            "th" => NodeType::Th,
            "thead" => NodeType::Thead,
            "tr" => NodeType::Tr,
            _ => return None,
        })
    }
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One node of a parsed document.
///
/// Constructed by [`parse`](crate::parse), and by hosts and tests that want a
/// tree without a source document. [`Node::new`] gives the latter a node with
/// no location, which is the same shape upstream produces when its `location`
/// option is off.
#[derive(Default)]
pub struct Node<'a> {
    /// What kind of node this is.
    pub node_type: NodeType,
    /// The tag name, for a [`NodeType::Tag`]. `None` for every other kind.
    pub tag: Option<String>,
    /// The attributes, in authored order.
    ///
    /// Holds both what the syntax implies -- a heading's `level`, a fence's
    /// `content` -- and what annotations set. Values are unresolved: a
    /// [`Value::Variable`] here is still a reference, because resolving it needs
    /// the transform stage's configuration.
    pub attributes: IndexMap<String, Value>,
    /// The children, in document order.
    pub children: Vec<Node<'a>>,
    /// Named slots, for a tag that uses them.
    ///
    /// A `{% slot "name" %}` inside a tag is lifted out of `children` and put
    /// here, so a tag's ordinary content and its named regions stay separable.
    /// Only populated when slots are enabled.
    pub slots: IndexMap<String, Node<'a>>,
    /// Problems found in this node.
    ///
    /// Data, not a failure: a document with a broken tag still parses, because
    /// an editor wants the rest of the file.
    pub errors: Vec<ValidationError<'a>>,
    /// The source lines this node spans, as upstream records them: the opening
    /// token's `[start, end]`, extended with the closing token's pair when the
    /// node closes.
    pub lines: Vec<usize>,
    /// The annotations written on this node, in authored order.
    ///
    /// Kept alongside [`Node::attributes`] rather than folded into it, because
    /// the formatter reprints the annotation it was given -- `.foo` stays
    /// `.foo`, not `class={foo: true}`.
    pub annotations: Vec<Attribute>,
    /// Whether this node sits inside an inline run.
    pub inline: bool,
    /// Where the node came from, unless locations were switched off.
    pub location: Option<Location<'a>>,
}

/// # Why the three traversals are written out rather than derived
///
/// The reasoning is on [`Scalar`](crate::renderable::Scalar), and applies here
/// for the reason [`Drop`] applies: `{% a %}` repeated is one nesting level per
/// line, so a derived `Clone`, `PartialEq` or `Debug` recurses once per line of
/// an attacker-supplied document.
///
/// Only [`Node::children`] and [`Node::slots`] recurse. The other eight fields
/// bottom out in types that are already safe -- [`Value`] carries its own
/// iterative traversals, and everything else is flat -- so they are handled
/// whole rather than walked.
impl Clone for Node<'_> {
    fn clone(&self) -> Self {
        enum Step<'s, 'a> {
            Open(&'s Node<'a>),
            Close(&'s Node<'a>),
        }

        let mut plan = vec![Step::Open(self)];
        let mut done: Vec<Node<'_>> = Vec::new();

        while let Some(step) = plan.pop() {
            match step {
                Step::Open(node) => {
                    plan.push(Step::Close(node));
                    // Slots then children, reversed, so `done` receives
                    // finished subtrees in the order `Close` reclaims them.
                    for child in node.children.iter().rev() {
                        plan.push(Step::Open(child));
                    }
                    for (_, slot) in node.slots.iter().rev() {
                        plan.push(Step::Open(slot));
                    }
                }
                Step::Close(node) => {
                    let total = node.slots.len() + node.children.len();
                    let start = done.len().saturating_sub(total);
                    let mut finished = done.split_off(start).into_iter();

                    let slots: IndexMap<String, Node<'_>> = node
                        .slots
                        .keys()
                        .cloned()
                        .zip(finished.by_ref().take(node.slots.len()))
                        .collect();
                    let children: Vec<Node<'_>> = finished.collect();

                    done.push(Node {
                        node_type: node.node_type,
                        tag: node.tag.clone(),
                        attributes: node.attributes.clone(),
                        children,
                        slots,
                        errors: node.errors.clone(),
                        lines: node.lines.clone(),
                        annotations: node.annotations.clone(),
                        inline: node.inline,
                        location: node.location,
                    });
                }
            }
        }

        done.pop().unwrap_or_default()
    }
}

impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        let mut work: Vec<(&Node<'_>, &Node<'_>)> = vec![(self, other)];
        while let Some((left, right)) = work.pop() {
            // Every field that cannot recurse, compared whole.
            if left.node_type != right.node_type
                || left.tag != right.tag
                || left.attributes != right.attributes
                || left.errors != right.errors
                || left.lines != right.lines
                || left.annotations != right.annotations
                || left.inline != right.inline
                || left.location != right.location
                || left.children.len() != right.children.len()
                || left.slots.len() != right.slots.len()
            {
                return false;
            }
            work.extend(left.children.iter().zip(right.children.iter()));
            // Unordered, because that is what `IndexMap::eq` does.
            for (key, slot) in &left.slots {
                match right.slots.get(key) {
                    Some(other_slot) => work.push((slot, other_slot)),
                    None => return false,
                }
            }
        }
        true
    }
}

impl std::fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alternate = f.alternate();
        let mut stack: Vec<NodeTok<'_, '_>> = vec![NodeTok::Node(self, 0)];

        while let Some(token) = stack.pop() {
            match token {
                NodeTok::Text(text) => f.write_str(text)?,
                NodeTok::Owned(text) => f.write_str(&text)?,
                NodeTok::Line(depth) => {
                    f.write_str("\n")?;
                    for _ in 0..depth {
                        f.write_str("    ")?;
                    }
                }
                NodeTok::Node(node, depth) => expand_node(f, &mut stack, node, depth, alternate)?,
            }
        }
        Ok(())
    }
}

/// One pending piece of `Debug` output for a [`Node`].
enum NodeTok<'n, 'a> {
    Node(&'n Node<'a>, usize),
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

/// Write a node's flat fields and queue its children and slots.
///
/// The eight non-recursive fields are delegated to their own `Debug`, which is
/// what the derive would have called; only `children` and `slots` are walked.
fn expand_node<'n, 'a>(
    f: &mut std::fmt::Formatter<'_>,
    stack: &mut Vec<NodeTok<'n, 'a>>,
    node: &'n Node<'a>,
    depth: usize,
    alternate: bool,
) -> std::fmt::Result {
    /// Format a flat field the way the derive nests it.
    fn flat(value: &dyn std::fmt::Debug, depth: usize, alternate: bool) -> String {
        if alternate {
            indent_block(&format!("{value:#?}"), depth)
        } else {
            format!("{value:?}")
        }
    }

    let mut queued: Vec<NodeTok<'n, 'a>> = Vec::new();

    if alternate {
        let inner = depth + 1;
        f.write_str("Node {")?;
        for (name, rendered) in [
            ("node_type", flat(&node.node_type, inner, true)),
            ("tag", flat(&node.tag, inner, true)),
            ("attributes", flat(&node.attributes, inner, true)),
        ] {
            queued.push(NodeTok::Line(inner));
            queued.push(NodeTok::Owned(format!("{name}: {rendered},")));
        }

        queued.push(NodeTok::Line(inner));
        if node.children.is_empty() {
            queued.push(NodeTok::Text("children: [],"));
        } else {
            queued.push(NodeTok::Text("children: ["));
            for child in &node.children {
                queued.push(NodeTok::Line(inner + 1));
                queued.push(NodeTok::Node(child, inner + 1));
                queued.push(NodeTok::Text(","));
            }
            queued.push(NodeTok::Line(inner));
            queued.push(NodeTok::Text("],"));
        }

        queued.push(NodeTok::Line(inner));
        if node.slots.is_empty() {
            queued.push(NodeTok::Text("slots: {},"));
        } else {
            queued.push(NodeTok::Text("slots: {"));
            for (key, slot) in &node.slots {
                queued.push(NodeTok::Line(inner + 1));
                queued.push(NodeTok::Owned(format!("{key:?}: ")));
                queued.push(NodeTok::Node(slot, inner + 1));
                queued.push(NodeTok::Text(","));
            }
            queued.push(NodeTok::Line(inner));
            queued.push(NodeTok::Text("},"));
        }

        for (name, rendered) in [
            ("errors", flat(&node.errors, inner, true)),
            ("lines", flat(&node.lines, inner, true)),
            ("annotations", flat(&node.annotations, inner, true)),
            ("inline", flat(&node.inline, inner, true)),
            ("location", flat(&node.location, inner, true)),
        ] {
            queued.push(NodeTok::Line(inner));
            queued.push(NodeTok::Owned(format!("{name}: {rendered},")));
        }
        queued.push(NodeTok::Line(depth));
        queued.push(NodeTok::Text("}"));
    } else {
        write!(
            f,
            "Node {{ node_type: {}, tag: {}, attributes: {}, children: [",
            flat(&node.node_type, depth, false),
            flat(&node.tag, depth, false),
            flat(&node.attributes, depth, false),
        )?;
        for (index, child) in node.children.iter().enumerate() {
            if index > 0 {
                queued.push(NodeTok::Text(", "));
            }
            queued.push(NodeTok::Node(child, depth));
        }
        queued.push(NodeTok::Text("], slots: {"));
        for (index, (key, slot)) in node.slots.iter().enumerate() {
            if index > 0 {
                queued.push(NodeTok::Text(", "));
            }
            queued.push(NodeTok::Owned(format!("{key:?}: ")));
            queued.push(NodeTok::Node(slot, depth));
        }
        queued.push(NodeTok::Owned(format!(
            "}}, errors: {}, lines: {}, annotations: {}, inline: {}, location: {} }}",
            flat(&node.errors, depth, false),
            flat(&node.lines, depth, false),
            flat(&node.annotations, depth, false),
            flat(&node.inline, depth, false),
            flat(&node.location, depth, false),
        )));
    }

    stack.extend(queued.into_iter().rev());
    Ok(())
}

impl<'a> Node<'a> {
    /// A node of the given type, with no attributes, children or location.
    ///
    /// Every field is spelled out rather than filled from `Node::default()`:
    /// [`Node`] has a manual [`Drop`], and struct-update syntax moves out of the
    /// value it updates from, which a `Drop` type forbids. Naming the fields is
    /// also the thing that fails to compile when a field is added, which is
    /// what you want of a constructor.
    #[must_use]
    pub fn new(node_type: NodeType) -> Node<'a> {
        Node {
            node_type,
            tag: None,
            attributes: IndexMap::new(),
            children: Vec::new(),
            slots: IndexMap::new(),
            errors: Vec::new(),
            lines: Vec::new(),
            annotations: Vec::new(),
            inline: false,
            location: None,
        }
    }

    /// A node with attributes, children, and an optional tag name.
    ///
    /// The argument order is upstream's `new Node(type, attributes, children,
    /// tag)`, so a ported test reads next to the TypeScript it came from.
    #[must_use]
    pub fn with(
        node_type: NodeType,
        attributes: IndexMap<String, Value>,
        children: Vec<Node<'a>>,
        tag: Option<String>,
    ) -> Node<'a> {
        Node {
            node_type,
            tag,
            attributes,
            children,
            slots: IndexMap::new(),
            errors: Vec::new(),
            lines: Vec::new(),
            annotations: Vec::new(),
            inline: false,
            location: None,
        }
    }

    /// Appends a child.
    pub fn push(&mut self, node: Node<'a>) {
        self.children.push(node);
    }

    /// Sets an attribute, in authored order.
    ///
    /// A repeated name keeps its first position and takes the last value, which
    /// is what JavaScript object assignment does and therefore what upstream's
    /// output order is.
    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        self.attributes.insert(name.into(), value);
    }

    /// Reads an attribute.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    /// The name a diagnostic should call this node: its tag if it has one, its
    /// type otherwise.
    ///
    /// Upstream spells this `node.tag || node.type` at each site that needs it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.tag
            .as_deref()
            .unwrap_or_else(|| self.node_type.as_str())
    }

    /// Every descendant, depth first, slots before children.
    ///
    /// The order is upstream's `walk()` exactly, and it is load-bearing:
    /// `ast/node.test.ts` asserts the sequence for a document with slots, and a
    /// validator that reported errors in a different order would produce a
    /// different diff for the same file.
    #[must_use]
    pub fn walk(&self) -> Walk<'_, 'a> {
        Walk {
            stack: self.descendants_in_order(),
        }
    }

    /// The direct descendants in walk order, ready to be pushed onto a stack.
    ///
    /// Reversed, because [`Walk`] pops from the end.
    fn descendants_in_order(&self) -> Vec<&Node<'a>> {
        let mut out: Vec<&Node<'a>> = self.slots.values().chain(self.children.iter()).collect();
        out.reverse();
        out
    }
}

/// Dropping a tree is iterative, for the same reason walking it is.
///
/// Nesting depth is attacker-controlled -- `{% a %}` repeated is a nesting level
/// per line -- and the derived recursive drop turns a deep document into a stack
/// overflow, which aborts the process rather than raising anything a caller
/// could catch. Unlinking the tree onto the heap first bounds the recursion at
/// one level.
///
/// The cost, stated because it is invisible until someone hits it: a type with a
/// manual `Drop` cannot have a field moved out of it, so a consumer that wants
/// to take ownership of `children` uses [`std::mem::take`] rather than a partial
/// move. That is a small tax on the stages above, paid once, against an abort
/// that a document can trigger.
impl Drop for Node<'_> {
    fn drop(&mut self) {
        let mut pending: Vec<Node<'_>> = std::mem::take(&mut self.children);
        pending.extend(self.slots.drain(..).map(|(_, node)| node));
        while let Some(mut node) = pending.pop() {
            pending.append(&mut node.children);
            pending.extend(node.slots.drain(..).map(|(_, child)| child));
            // `node` is dropped here already emptied, so this recurses once.
        }
    }
}

/// A depth-first walk over a node's descendants.
///
/// Iterative rather than recursive: the input is arbitrary text, nesting depth
/// is attacker-controlled, and a recursive iterator would make tree depth a
/// stack-overflow budget.
pub struct Walk<'n, 'a> {
    stack: Vec<&'n Node<'a>>,
}

impl<'n, 'a> Iterator for Walk<'n, 'a> {
    type Item = &'n Node<'a>;

    fn next(&mut self) -> Option<&'n Node<'a>> {
        let node = self.stack.pop()?;
        self.stack.extend(node.descendants_in_order());
        Some(node)
    }
}

/// `Debug` output is observable, so the hand-written emitter is pinned against
/// the derive. Same pattern as [`Scalar`](crate::renderable::Scalar).
#[cfg(test)]
mod debug_parity {
    use super::*;

    mod mirror {
        // Every field exists to be formatted by the derive and is never read
        // otherwise -- that is the whole point of the type.
        #![allow(dead_code, clippy::struct_field_names)]

        use super::{Attribute, Location, NodeType, ValidationError, Value};
        use indexmap::IndexMap;

        #[derive(Debug)]
        pub struct Node<'a> {
            pub node_type: NodeType,
            pub tag: Option<String>,
            pub attributes: IndexMap<String, Value>,
            pub children: Vec<Node<'a>>,
            pub slots: IndexMap<String, Node<'a>>,
            pub errors: Vec<ValidationError<'a>>,
            pub lines: Vec<usize>,
            pub annotations: Vec<Attribute>,
            pub inline: bool,
            pub location: Option<Location<'a>>,
        }
    }

    fn to_mirror<'a>(node: &Node<'a>) -> mirror::Node<'a> {
        mirror::Node {
            node_type: node.node_type,
            tag: node.tag.clone(),
            attributes: node.attributes.clone(),
            children: node.children.iter().map(to_mirror).collect(),
            slots: node
                .slots
                .iter()
                .map(|(key, slot)| (key.clone(), to_mirror(slot)))
                .collect(),
            errors: node.errors.clone(),
            lines: node.lines.clone(),
            annotations: node.annotations.clone(),
            inline: node.inline,
            location: node.location,
        }
    }

    fn assert_parity(node: &Node<'_>) {
        let reference = to_mirror(node);
        assert_eq!(format!("{node:?}"), format!("{reference:?}"), "plain Debug");
        assert_eq!(
            format!("{node:#?}"),
            format!("{reference:#?}"),
            "alternate Debug"
        );
    }

    #[test]
    fn every_node_shape_formats_as_the_derive_would() {
        let mut bare = Node::new(NodeType::Paragraph);
        bare.lines = vec![1, 2];

        let mut attributed = Node::new(NodeType::Tag);
        attributed.tag = Some("callout".to_owned());
        attributed.set("level", Value::Number(2.0));
        attributed.set("title", Value::String("hi".to_owned()));
        attributed.inline = true;

        let nested = Node::with(
            NodeType::Document,
            IndexMap::new(),
            vec![Node::with(
                NodeType::Paragraph,
                IndexMap::new(),
                vec![Node::new(NodeType::Text)],
                None,
            )],
            None,
        );

        let mut slotted = Node::new(NodeType::Tag);
        slotted.tag = Some("card".to_owned());
        slotted
            .slots
            .insert("header".to_owned(), Node::new(NodeType::Paragraph));

        // A value deep enough to matter inside an attribute, which the node's
        // own walk hands to `Value`'s.
        let mut deep_attribute = Node::new(NodeType::Tag);
        deep_attribute.set(
            "data",
            Value::Array(vec![Value::Hash(
                [("k".to_owned(), Value::Null)].into_iter().collect(),
            )]),
        );

        for shape in &[bare, attributed, nested, slotted, deep_attribute] {
            assert_parity(shape);
        }
    }

    #[test]
    fn a_deep_node_survives_all_three_traversals() {
        // `{% a %}` repeated is one level per line, so this depth is
        // attacker-supplied rather than hypothetical.
        let mut node = Node::new(NodeType::Paragraph);
        for _ in 0..100_000 {
            node = Node::with(NodeType::Tag, IndexMap::new(), vec![node], Some("a".into()));
        }
        let copy = node.clone();
        assert!(copy == node, "an iterative clone must equal its source");
        assert!(format!("{node:?}").starts_with("Node { node_type: Tag"));
    }

    #[test]
    fn a_node_deep_through_slots_survives_all_three() {
        let mut node = Node::new(NodeType::Paragraph);
        for _ in 0..100_000 {
            let mut outer = Node::new(NodeType::Tag);
            outer.slots.insert("s".to_owned(), node);
            node = outer;
        }
        let copy = node.clone();
        assert!(copy == node);
    }

    #[test]
    fn cloning_preserves_child_and_slot_order() {
        let mut node = Node::with(
            NodeType::Document,
            IndexMap::new(),
            vec![Node::new(NodeType::Heading), Node::new(NodeType::Paragraph)],
            None,
        );
        node.slots.insert("z".to_owned(), Node::new(NodeType::Text));
        node.slots
            .insert("a".to_owned(), Node::new(NodeType::Fence));

        let copy = node.clone();
        assert_eq!(copy.children.len(), 2);
        assert_eq!(copy.children[0].node_type, NodeType::Heading);
        assert_eq!(copy.children[1].node_type, NodeType::Paragraph);
        assert_eq!(copy.slots.keys().collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(copy.slots["a"].node_type, NodeType::Fence);
        assert!(copy == node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content: &str) -> Node<'static> {
        let mut node = Node::new(NodeType::Text);
        node.set("content", Value::String(content.to_string()));
        node
    }

    fn block(node_type: NodeType, children: Vec<Node<'static>>) -> Node<'static> {
        Node::with(node_type, IndexMap::new(), children, None)
    }

    /// Ported from `ast/node.test.ts`, "traversal / with a simple document".
    #[test]
    fn walking_a_simple_document_visits_every_descendant() {
        let example = block(
            NodeType::Document,
            vec![
                block(
                    NodeType::Heading,
                    vec![block(NodeType::Inline, vec![text("This is a heading")])],
                ),
                block(
                    NodeType::Paragraph,
                    vec![block(NodeType::Inline, vec![text("This is a paragraph")])],
                ),
            ],
        );

        assert_eq!(example.walk().count(), 6);
    }

    #[test]
    fn walking_visits_slots_before_children() {
        // The order upstream's `ast/node.test.ts` asserts for a parsed document
        // with slots, built here by hand so the assertion does not also depend
        // on the segmenter.
        let mut tag = Node::with(
            NodeType::Tag,
            IndexMap::new(),
            Vec::new(),
            Some("example".into()),
        );
        tag.slots.insert(
            "foo".to_string(),
            block(
                NodeType::Paragraph,
                vec![block(NodeType::Inline, vec![text("baz")])],
            ),
        );
        tag.push(block(
            NodeType::Heading,
            vec![block(NodeType::Inline, vec![text("bar")])],
        ));
        let document = block(NodeType::Document, vec![tag]);

        let visited: Vec<String> = document
            .walk()
            .map(|node| node.name().to_string())
            .collect();
        assert_eq!(
            visited,
            [
                "example",
                "paragraph",
                "inline",
                "text",
                "heading",
                "inline",
                "text"
            ]
        );
    }

    #[test]
    fn walking_is_iterative_and_survives_deep_nesting() {
        // Nesting depth is attacker-controlled. A recursive walk would make
        // this a stack overflow rather than a count.
        let mut node = Node::new(NodeType::Document);
        for _ in 0..50_000 {
            node = block(NodeType::Tag, vec![node]);
        }
        assert_eq!(node.walk().count(), 50_000);
    }

    #[test]
    fn attribute_order_is_authored_order() {
        let mut node = Node::new(NodeType::Tag);
        node.set("z", Value::Number(1.0));
        node.set("a", Value::Number(2.0));
        node.set("z", Value::Number(3.0));
        let keys: Vec<&str> = node.attributes.keys().map(String::as_str).collect();
        assert_eq!(keys, ["z", "a"]);
        assert_eq!(node.get("z"), Some(&Value::Number(3.0)));
    }

    #[test]
    fn a_node_names_itself_by_tag_then_type() {
        assert_eq!(Node::new(NodeType::Paragraph).name(), "paragraph");
        let mut tagged = Node::new(NodeType::Tag);
        tagged.tag = Some("callout".to_string());
        assert_eq!(tagged.name(), "callout");
    }

    #[test]
    fn node_types_spell_themselves_as_upstream_does() {
        assert_eq!(NodeType::Fence.as_str(), "fence");
        assert_eq!(NodeType::Hardbreak.to_string(), "hardbreak");
        assert_eq!(NodeType::default(), NodeType::Node);
    }
}
