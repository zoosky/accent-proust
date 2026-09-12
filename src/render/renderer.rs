//! The seam between the renderable tree and its markup: [`TagRenderer`] and
//! the [`Children`] answer its `open` gives back.
//!
//! Private module; both items are re-exported from [`crate::render`], and the
//! reasoning that shapes the trait is on the trait itself, where a reader
//! meets it.

use crate::renderable::Tag;

/// Whether a tag's children are rendered after its opening markup.
///
/// Returned by [`TagRenderer::open`]. [`Html`](super::Html) answers [`Skip`]
/// for a void element, which is upstream's
/// `if (VOID_ELEMENTS.has(name)) return`: the children are dropped, not
/// deferred, and no closing markup follows.
///
/// [`Skip`]: Children::Skip
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Children {
    /// Render the children, then call [`TagRenderer::close`].
    Render,
    /// Render nothing further for this tag: no children, no closing markup.
    Skip,
}

/// Turns renderable tags into markup.
///
/// Upstream's `renderers/html.ts` is one function that does two things at
/// once: it decides *which nodes* to visit and in what order, and it decides
/// *what bytes* a visited node becomes. Only the second is HTML. This trait is
/// the second decision on its own; the first stays in this crate, in
/// [`render_with`](super::render_with), for a reason that shapes the trait.
///
/// # Why three methods and not one
///
/// The obvious seam hands a host one tag and a callback that renders the
/// children:
///
/// ```text
/// fn render_tag(&self, out: &mut String, tag: &Tag, children: impl FnOnce(&mut String));
/// ```
///
/// That is recursion with the host's frame in the loop, once per level of
/// nesting. Nesting depth comes from the document, which is
/// attacker-controlled, and a stack overflow in Rust aborts the process rather
/// than unwinding into anything a caller could catch. The renderer walks an
/// explicit, heap-allocated stack for exactly this reason, and a callback
/// would hand that stack back to the host one frame at a time, in code this
/// crate cannot see.
///
/// So the trait never asks for children to be rendered. [`open`] writes what
/// precedes them and says whether they follow; the crate walks them;
/// [`close`] writes what comes after. Every method is called for one node,
/// writes, and returns. An implementation needs no depth limit of its own and
/// cannot be handed a tree deep enough to overflow, whatever the document did.
///
/// # What the crate decides, and what the host decides
///
/// The crate owns the shape of the walk: document order, that an array child
/// renders element by element, that `null` and a boolean render as nothing,
/// and that a tag with no name is a wrapper whose children render in place.
/// None of those reaches an implementation, because none of them is markup.
/// They are upstream's tree semantics, and a host that changed one would be
/// rendering a different tree.
///
/// The host owns everything that is markup: what a tag opens and closes with,
/// which attributes are written and how, which elements are void, and how text
/// is escaped. [`Html`](super::Html) is upstream's answer to each, and three
/// pieces of it are public so that an implementation can keep the parts it
/// wants:
///
/// - [`escape_html_into`](super::escape_html_into) is markdown-it's escaper,
///   the four replacements upstream makes and no others.
/// - [`attribute_value`](super::attribute_value) is ECMAScript's `String(v)`
///   over an attribute, which is what upstream writes between the quotes.
/// - [`is_void_element`](super::is_void_element) is the HTML standard's list.
///
/// Every method writes into `out` rather than returning a `String`, so a
/// document renders into one allocation however many tags it holds.
///
/// # Examples
///
/// A renderer that emits an S-expression instead of HTML:
///
/// ```
/// use accent_proust::render::{Children, TagRenderer, attribute_value, render_with};
/// use accent_proust::renderable::{RenderableTreeNode, Tag};
/// use indexmap::IndexMap;
///
/// struct Sexp;
///
/// impl TagRenderer for Sexp {
///     fn open(&self, out: &mut String, tag: &Tag) -> Children {
///         out.push('(');
///         out.push_str(&tag.name);
///         for (key, value) in &tag.attributes {
///             out.push(' ');
///             out.push_str(key);
///             out.push('=');
///             out.push_str(&attribute_value(value));
///         }
///         Children::Render
///     }
///
///     fn close(&self, out: &mut String, _tag: &Tag) {
///         out.push(')');
///     }
///
///     fn text(&self, out: &mut String, text: &str) {
///         out.push_str(" \"");
///         out.push_str(text);
///         out.push('"');
///     }
/// }
///
/// let heading = Tag::with("h1", IndexMap::new(), vec![RenderableTreeNode::text("hi")]);
/// assert_eq!(render_with(&RenderableTreeNode::tag(heading), &Sexp), r#"(h1 "hi")"#);
/// ```
///
/// [`open`]: TagRenderer::open
/// [`close`]: TagRenderer::close
pub trait TagRenderer {
    /// Write the markup that opens `tag`, and say whether its children follow.
    ///
    /// Never called for a tag whose name is empty. Upstream treats one as a
    /// wrapper -- `if (!name) return render(children)` -- and so does the
    /// walk, which renders the children in place without consulting the
    /// renderer. That decision is the tree's, not the markup's, and taking it
    /// here would make every implementation repeat it.
    ///
    /// Attributes arrive as authored: names in their original case, values as
    /// the subtree the transform stored. Lowercasing the names and coercing
    /// the values are policy, and [`Html`](super::Html) is where upstream's
    /// policy lives.
    fn open(&self, out: &mut String, tag: &Tag) -> Children;

    /// Write the markup that closes `tag`.
    ///
    /// Called once the children have rendered, and only when [`open`] returned
    /// [`Children::Render`] for the same tag. It receives the tag rather than
    /// only its name so that whatever [`open`] derived from the attributes --
    /// an element name chosen by a `level`, say -- can be derived again the
    /// same way.
    ///
    /// [`open`]: TagRenderer::open
    fn close(&self, out: &mut String, tag: &Tag);

    /// Write `text`, escaped to the implementation's policy.
    ///
    /// Reached for a string child and for a number child. A number arrives
    /// already formatted by ECMAScript's rules -- `1e21` as `1e+21`, `-0` as
    /// `0` -- because that formatting is upstream's arithmetic rather than its
    /// markup, and it is the same for every renderer. What remains here is
    /// escaping, which is not.
    fn text(&self, out: &mut String, text: &str);
}
