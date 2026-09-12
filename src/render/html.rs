//! The HTML renderer, transliterated from `reference/src/renderers/html.ts`.
//!
//! Forty-eight lines upstream, and the last stage before bytes. It takes a
//! renderable tree and writes markup: no schema, no config, no policy. Whether
//! a tag is allowed to exist was decided by the validator; what it is called
//! was decided by the transform. This layer only spells it.
//!
//! Upstream's one function is two things here. The **walk** owns the tree's
//! shape -- which nodes are visited, in what order, and which of them are
//! markup at all -- and is [`render_into`]. The **markup** is [`Html`], behind
//! the [`TagRenderer`] seam. [`render`] and [`render_all`] are the walk with
//! `Html` plugged in; [`render_with`] and [`render_all_with`] are the walk with
//! anything else.
//!
//! # The four early-outs, in upstream's order
//!
//! The order is load-bearing, because the checks overlap. All four belong to
//! the walk; only the escaping inside the first belongs to the markup, which
//! is the split the seam makes:
//!
//! 1. A string or a number is escaped and emitted. Nothing else is. The walk
//!    picks them out: a string goes to [`TagRenderer::text`], which escapes,
//!    and a number to [`TagRenderer::number`], whose default spells it as
//!    ECMAScript does and passes that to `text`.
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
//!    The walk takes this one too, so a renderer is never asked to open a tag
//!    it has no name for.
//!
//! # What `Html` decides, and does not
//!
//! - **Attribute order is authored order.** `IndexMap`, never a hash map, so
//!   two runs over one document produce identical bytes.
//! - **Attribute names are lowercased on output**, values are not. `colSpan`
//!   becomes `colspan`; `Data` stays `Data`.
//! - **An attribute value is coerced, not rendered.** It holds a whole subtree,
//!   because a rendered slot is stored there as its transformed nodes, and
//!   upstream writes `String(v)` over it rather than recursing. A tag in an
//!   attribute is therefore `[object Object]`, which is upstream's answer and
//!   not a good one. [`attribute_value`] is that coercion.
//! - **The void-element list is the HTML standard's fourteen**, hard-coded
//!   upstream and hard-coded here. See [`VOID_ELEMENTS`].
//! - **Escaping is markdown-it's**, exactly. See [`super::escape_html`].

use crate::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};

use super::escape::escape_html_into;
use super::js;
use super::renderer::{Children, TagRenderer};

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

/// ECMAScript `String(v)` over an attribute value: what upstream writes
/// between the quotes.
///
/// An attribute holds a whole subtree rather than a scalar, because a rendered
/// slot is stored in the attribute map as its transformed nodes, and upstream
/// coerces whatever it finds there instead of rendering it. The rules are
/// ECMAScript's, not a formatting choice -- `[1, 2, 3]` is `1,2,3`, a `null`
/// element of an array contributes nothing, a number outside `1e-6..1e21`
/// switches to exponent notation, and a tag is `[object Object]` -- and the
/// conformance corpus grades on them.
///
/// Public so that a [`TagRenderer`] that writes attributes differently can
/// still write their values the way upstream does. The result is not escaped:
/// escaping is the renderer's policy, and [`Html`] applies
/// [`escape_html_into`](super::escape_html_into) to it afterwards.
///
/// # Examples
///
/// ```
/// use accent_proust::render::attribute_value;
/// use accent_proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar};
///
/// let list = Scalar::Array(vec![Scalar::Number(1.0), Scalar::Null, Scalar::Number(3.0)]);
/// let value = RenderableTreeNodes::One(RenderableTreeNode::Scalar(list));
/// assert_eq!(attribute_value(&value), "1,,3");
/// ```
#[must_use]
pub fn attribute_value(value: &RenderableTreeNodes) -> String {
    js::string_nodes(value)
}

/// Upstream's HTML renderer, as a [`TagRenderer`].
///
/// The default for [`render`] and [`render_all`], and the reference for what a
/// different implementation is departing from. It makes exactly upstream's
/// choices: attribute names lowercased, values coerced with
/// [`attribute_value`] and escaped, the HTML standard's [`VOID_ELEMENTS`]
/// closed by nothing, and text escaped with markdown-it's four replacements.
///
/// A unit struct because it holds no policy a caller could vary. A renderer
/// that does -- a different void list, say -- is its own type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Html;

impl TagRenderer for Html {
    fn open(&self, out: &mut String, tag: &Tag) -> Children {
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
            escape_html_into(out, &attribute_value(value));
            out.push('"');
        }
        out.push('>');

        // A void element has no children and no closing tag, even when the tree
        // gives it children. Upstream returns before either.
        if is_void_element(&tag.name) {
            Children::Skip
        } else {
            Children::Render
        }
    }

    fn close(&self, out: &mut String, tag: &Tag) {
        out.push_str("</");
        out.push_str(&tag.name);
        out.push('>');
    }

    fn text(&self, out: &mut String, text: &str) {
        escape_html_into(out, text);
    }
}

/// Render one node of the renderable tree to HTML.
///
/// Upstream's `render` takes `RenderableTreeNodes`, a TypeScript union of "one
/// node or an array of them". Rust has no such union without inventing a type
/// for it, and inventing one buys nothing: the two arms are two functions, and
/// which one you want is known at the call site. Use [`render_all`] for a
/// document's children.
///
/// This is [`render_with`] with [`Html`], byte for byte.
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
    render_with(node, &Html)
}

/// Render a sequence of nodes to HTML, concatenated with no separator.
///
/// This is upstream's `node.map(render).join('')` arm, and
/// [`render_all_with`] with [`Html`].
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
    render_all_with(nodes, &Html)
}

/// Render one node through `renderer`.
///
/// [`render`] with the markup decided by the caller. `renderer` may be a
/// concrete type or a `dyn TagRenderer`; the walk is the same either way and
/// is this crate's, so the depth of the tree is never on the renderer's stack.
///
/// # Examples
///
/// ```
/// use indexmap::IndexMap;
/// use accent_proust::render::{Html, render, render_with};
/// use accent_proust::renderable::{RenderableTreeNode, Tag};
///
/// let heading = Tag::with("h1", IndexMap::new(), vec![RenderableTreeNode::text("test")]);
/// let heading = RenderableTreeNode::tag(heading);
/// assert_eq!(render_with(&heading, &Html), render(&heading));
/// ```
#[must_use]
pub fn render_with<R: TagRenderer + ?Sized>(node: &RenderableTreeNode, renderer: &R) -> String {
    let mut out = String::new();
    render_into(&mut out, std::slice::from_ref(node), renderer);
    out
}

/// Render a sequence of nodes through `renderer`, concatenated with no
/// separator.
///
/// [`render_all`] with the markup decided by the caller.
#[must_use]
pub fn render_all_with<R: TagRenderer + ?Sized>(
    nodes: &[RenderableTreeNode],
    renderer: &R,
) -> String {
    let mut out = String::new();
    render_into(&mut out, nodes, renderer);
    out
}

/// One item of the renderer's work stack.
enum Step<'a> {
    /// A node still to be rendered.
    Node(&'a RenderableTreeNode),
    /// A leaf value still to be rendered.
    Leaf(&'a Scalar),
    /// A closing tag, queued under the children it closes. The whole tag,
    /// because [`TagRenderer::close`] receives the same tag `open` saw.
    Close(&'a Tag),
}

/// Render `nodes` into `out` through `renderer`.
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
///
/// The renderer is called once per step and never re-entered. That is the
/// property [`TagRenderer`] promises its implementations, and the reason the
/// trait has no "render the children" method: the only stack in play is this
/// one, and it is on the heap.
fn render_into<R: TagRenderer + ?Sized>(
    out: &mut String,
    nodes: &[RenderableTreeNode],
    renderer: &R,
) {
    let mut stack: Vec<Step<'_>> = nodes.iter().rev().map(Step::Node).collect();

    while let Some(step) = stack.pop() {
        match step {
            Step::Node(RenderableTreeNode::Scalar(scalar)) => stack.push(Step::Leaf(scalar)),
            Step::Node(RenderableTreeNode::Tag(tag)) => open_tag(out, &mut stack, tag, renderer),
            Step::Leaf(scalar) => match scalar {
                Scalar::String(text) => renderer.text(out, text),
                // Through `number`, whose default spells the value as
                // ECMAScript does and hands it to `text`.
                Scalar::Number(value) => renderer.number(out, *value),
                // Upstream's `Array.isArray` arm, which sits above the tag
                // check: the elements are rendered, not joined with commas.
                Scalar::Array(items) => stack.extend(items.iter().rev().map(Step::Leaf)),
                // Everything `Tag.isTag` rejects renders as nothing.
                Scalar::Null | Scalar::Boolean(_) | Scalar::Object(_) => {}
            },
            Step::Close(tag) => renderer.close(out, tag),
        }
    }
}

/// Open `tag` through `renderer` and queue what follows it.
fn open_tag<'a, R: TagRenderer + ?Sized>(
    out: &mut String,
    stack: &mut Vec<Step<'a>>,
    tag: &'a Tag,
    renderer: &R,
) {
    // `if (!name) return render(children)`. An unnamed tag is a wrapper the
    // transform asked for and does not want printed. The walk's decision, not
    // the renderer's, so no implementation has to repeat it.
    if tag.name.is_empty() {
        stack.extend(tag.children.iter().rev().map(Step::Node));
        return;
    }

    match renderer.open(out, tag) {
        Children::Skip => {}
        Children::Render => {
            stack.push(Step::Close(tag));
            stack.extend(tag.children.iter().rev().map(Step::Node));
        }
    }
}
