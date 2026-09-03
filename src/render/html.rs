//! The HTML renderer, transliterated from `reference/src/renderers/html.ts`.
//!
//! Forty-eight lines upstream, and the last stage before bytes. It takes a
//! renderable tree and writes markup: no schema, no config, no policy. Whether
//! a tag is allowed to exist was decided by the validator; what it is called
//! was decided by the transform. This layer only spells it.
//!
//! # The four early-outs, in upstream's order
//!
//! The order is load-bearing, because the checks overlap:
//!
//! 1. A string or a number is escaped and emitted. Nothing else is.
//! 2. An array is rendered element by element and concatenated. This is checked
//!    *before* the tag check, so a [`Scalar::Array`] child renders its
//!    elements: `[1, 2, 3]` as a child is `123`, while the same array as an
//!    *attribute* is `1,2,3`, because an attribute goes through ECMAScript's
//!    `String` and a child does not.
//! 3. Anything that is not a tag renders as the empty string. Upstream reaches
//!    this with `null`, a boolean, an object, or any value failing
//!    `Tag.isTag`; here it is the remaining [`Scalar`] variants. Silently, on
//!    purpose: the renderer is not a validator, and a tree that got this far
//!    has already been graded.
//! 4. A tag with **no name** renders its children with no wrapper. Upstream
//!    writes `if (!name) return render(children)`, and the transform relies on
//!    it -- an unnamed tag is how a schema says "these children, no element".
//!
//! # What the renderer does not decide
//!
//! - **Attribute order is authored order.** `IndexMap`, never a hash map, so
//!   two runs over one document produce identical bytes.
//! - **Attribute names are lowercased on output**, values are not. `colSpan`
//!   becomes `colspan`; `Data` stays `Data`.
//! - **An attribute value is coerced, not rendered.** It holds a whole subtree,
//!   because a rendered slot is stored there as its transformed nodes, and
//!   upstream writes `String(v)` over it rather than recursing. A tag in an
//!   attribute is therefore `[object Object]`, which is upstream's answer and
//!   not a good one.
//! - **The void-element list is the HTML standard's fourteen**, hard-coded
//!   upstream and hard-coded here. See [`VOID_ELEMENTS`].
//! - **Escaping is markdown-it's**, exactly. See [`super::escape_html`].

use crate::renderable::{RenderableTreeNode, Scalar, Tag};

use super::escape::escape_html_into;
use super::js;

/// The HTML elements that have no closing tag.
///
/// Upstream hard-codes this list from
/// [the HTML standard](https://html.spec.whatwg.org/#void-elements), and so
/// does this port. Substituting a crate's notion of void elements would make
/// the rendered output depend on that crate's reading of the spec and on its
/// release cadence; the list is fourteen strings and has not changed in years.
///
/// Matched against the tag name **as authored**. Only attribute names are
/// lowercased, so a tag named `HR` is not void here, exactly as upstream.
pub const VOID_ELEMENTS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Reports whether `name` is one of the [`VOID_ELEMENTS`].
#[must_use]
pub fn is_void_element(name: &str) -> bool {
    VOID_ELEMENTS.contains(&name)
}

/// Render one node of the renderable tree to HTML.
///
/// Upstream's `render` takes `RenderableTreeNodes`, a TypeScript union of "one
/// node or an array of them". Rust has no such union without inventing a type
/// for it, and inventing one buys nothing: the two arms are two functions, and
/// which one you want is known at the call site. Use [`render_all`] for a
/// document's children.
///
/// # Examples
///
/// ```
/// use indexmap::IndexMap;
/// use accent_proust::render::render;
/// use accent_proust::renderable::{RenderableTreeNode, Tag};
///
/// let heading = Tag::with("h1", IndexMap::new(), vec![RenderableTreeNode::text("test")]);
/// assert_eq!(render(&RenderableTreeNode::tag(heading)), "<h1>test</h1>");
/// ```
#[must_use]
pub fn render(node: &RenderableTreeNode) -> String {
    let mut out = String::new();
    render_into(&mut out, std::slice::from_ref(node));
    out
}

/// Render a sequence of nodes to HTML, concatenated with no separator.
///
/// This is upstream's `node.map(render).join('')` arm.
///
/// # Examples
///
/// ```
/// use indexmap::IndexMap;
/// use accent_proust::render::render_all;
/// use accent_proust::renderable::{RenderableTreeNode, Tag};
///
/// let paragraph = |text: &str| {
///     RenderableTreeNode::tag(Tag::with(
///         "p",
///         IndexMap::new(),
///         vec![RenderableTreeNode::text(text)],
///     ))
/// };
/// assert_eq!(
///     render_all(&[paragraph("foo"), paragraph("bar")]),
///     "<p>foo</p><p>bar</p>"
/// );
/// ```
#[must_use]
pub fn render_all(nodes: &[RenderableTreeNode]) -> String {
    let mut out = String::new();
    render_into(&mut out, nodes);
    out
}

/// One item of the renderer's work stack.
enum Step<'a> {
    /// A node still to be rendered.
    Node(&'a RenderableTreeNode),
    /// A leaf value still to be rendered.
    Leaf(&'a Scalar),
    /// A closing tag, queued under the children it closes.
    Close(&'a str),
}

/// Render `nodes` into `out`.
///
/// Iterative, with an explicit stack, where upstream recurses. Nesting depth in
/// a renderable tree comes from the document that produced it, which is
/// attacker-controlled, and a stack overflow in Rust aborts the process rather
/// than raising something a caller could catch. That makes recursion here
/// incompatible with the crate's panic-freedom promise, for the same reason
/// `crate::ast::Node` and [`Tag`](crate::renderable::Tag) both carry a manual
/// iterative `Drop`.
///
/// The stack holds children in reverse so they pop in document order, with the
/// closing tag pushed underneath them.
fn render_into(out: &mut String, nodes: &[RenderableTreeNode]) {
    let mut stack: Vec<Step<'_>> = nodes.iter().rev().map(Step::Node).collect();

    while let Some(step) = stack.pop() {
        match step {
            Step::Node(RenderableTreeNode::Scalar(scalar)) => stack.push(Step::Leaf(scalar)),
            Step::Node(RenderableTreeNode::Tag(tag)) => open_tag(out, &mut stack, tag),
            Step::Leaf(scalar) => match scalar {
                Scalar::String(text) => escape_html_into(out, text),
                Scalar::Number(value) => escape_html_into(out, &js::number(*value)),
                // Upstream's `Array.isArray` arm, which sits above the tag
                // check: the elements are rendered, not joined with commas.
                Scalar::Array(items) => stack.extend(items.iter().rev().map(Step::Leaf)),
                // Everything `Tag.isTag` rejects renders as nothing.
                Scalar::Null | Scalar::Boolean(_) | Scalar::Object(_) => {}
            },
            Step::Close(name) => {
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
    }
}

/// Write a tag's opening markup and queue what follows it.
fn open_tag<'a>(out: &mut String, stack: &mut Vec<Step<'a>>, tag: &'a Tag) {
    // `if (!name) return render(children)`. An unnamed tag is a wrapper the
    // transform asked for and does not want printed.
    if tag.name.is_empty() {
        stack.extend(tag.children.iter().rev().map(Step::Node));
        return;
    }

    out.push('<');
    out.push_str(&tag.name);
    for (key, value) in &tag.attributes {
        out.push(' ');
        // Names are lowercased, values are not. Upstream calls
        // `String.prototype.toLowerCase`, which is Unicode's locale-independent
        // mapping, and so is Rust's. The tag grammar only admits
        // `[a-zA-Z0-9_-]+` as an attribute name, so in practice this is ASCII.
        out.push_str(&key.to_lowercase());
        out.push_str("=\"");
        escape_html_into(out, &js::string_nodes(value));
        out.push('"');
    }
    out.push('>');

    // A void element has no children and no closing tag, even when the tree
    // gives it children. Upstream returns before either.
    if is_void_element(&tag.name) {
        return;
    }

    stack.push(Step::Close(&tag.name));
    stack.extend(tag.children.iter().rev().map(Step::Node));
}
