//! Rendering a renderable tree to output.
//!
//! Mirrors upstream `src/renderers/html.ts` -- 48 lines, and the smallest layer
//! in the crate.
//!
//! Upstream's React renderers are not ported. The renderable tree is public, so
//! a renderer for any other target can live outside this crate; shipping an
//! untested one here for symmetry would not.
//!
//! # What is here
//!
//! - [`render`] and [`render_all`] turn a renderable tree into HTML. They are
//!   the two arms of upstream's `RenderableTreeNodes` union, which TypeScript
//!   spells as one type and Rust does not need to.
//! - [`escape_html`] is markdown-it's escaper, reproduced character for
//!   character rather than approximated, because a host writing its own
//!   `TagRenderer` needs the same four replacements and no others.
//! - [`VOID_ELEMENTS`] is the HTML standard's list, hard-coded as upstream
//!   hard-codes it.
//!
//! # What is deliberately absent
//!
//! No sanitising, no attribute allow-list, no URL scheme check. The renderer
//! writes the tree it is given. Deciding which tags may exist is the
//! validator's job, and deciding whether a host trusts an author is the host's;
//! a policy hidden in the last stage would be one neither of them could see.

mod escape;
mod html;
// `pub(crate)` for the formatter, which needs the same ECMAScript number
// rendering this module wrote for the HTML renderer. Two copies of that
// algorithm would be two places for it to drift.
pub(crate) mod js;

pub use escape::escape_html;
pub use html::{is_void_element, render, render_all, VOID_ELEMENTS};
