//! Rendering a renderable tree to output.
//!
//! Mirrors upstream `src/renderers/html.ts` -- 48 lines, and the smallest layer
//! in the crate.
//!
//! Upstream's React renderers are not ported. The renderable tree is public, so
//! a renderer for any other target can live outside this crate; shipping an
//! untested one here for symmetry would not. [`TagRenderer`] is how one does.
//!
//! # What is here
//!
//! - [`render`] and [`render_all`] turn a renderable tree into HTML. They are
//!   the two arms of upstream's `RenderableTreeNodes` union, which TypeScript
//!   spells as one type and Rust does not need to.
//! - [`TagRenderer`] is the seam between the tree and its markup, and [`Html`]
//!   is upstream's implementation of it. [`render_with`] and
//!   [`render_all_with`] take any other, and the pairing mirrors
//!   `format`/`format_with`. The walk over the tree stays here whichever
//!   renderer is plugged in, which is what keeps the document's depth off the
//!   host's stack; the trait's documentation says why that shapes it.
//! - [`escape_html`] and [`escape_html_into`] are markdown-it's escaper,
//!   reproduced character for character rather than approximated, because a
//!   host writing its own `TagRenderer` needs the same four replacements and
//!   no others.
//! - [`attribute_value`] is ECMAScript's `String(v)` over an attribute value,
//!   for the same host: what upstream writes between the quotes, before
//!   escaping.
//! - [`VOID_ELEMENTS`] is the HTML standard's list, hard-coded as upstream
//!   hard-codes it.
//!
//! # What is deliberately absent
//!
//! No sanitising, no attribute allow-list, no URL scheme check. The renderer
//! writes the tree it is given. Deciding which tags may exist is the
//! validator's job, and deciding whether a host trusts an author is the host's;
//! a policy hidden in the last stage would be one neither of them could see.
//! A host that wants one puts it in its own [`TagRenderer`], where it is the
//! host's decision in the host's code rather than this crate's in the dark.

mod escape;
mod html;
// `pub(crate)` for the formatter, which needs the same ECMAScript number
// rendering this module wrote for the HTML renderer. Two copies of that
// algorithm would be two places for it to drift.
pub(crate) mod js;
mod renderer;

pub use escape::{escape_html, escape_html_into};
pub use html::{
    Html, VOID_ELEMENTS, attribute_value, is_void_element, render, render_all, render_all_with,
    render_with,
};
pub use renderer::{Children, TagRenderer};
