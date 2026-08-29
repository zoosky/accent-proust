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
#[derive(Clone, Debug, Default, PartialEq)]
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
