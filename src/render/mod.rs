//! Rendering a renderable tree to output.
//!
//! Mirrors upstream `src/renderers/html.ts` -- 48 lines, and the smallest layer
//! in the crate.
//!
//! Upstream's React renderers are not ported. The renderable tree is public, so
//! a renderer for any other target can live outside this crate; shipping an
//! untested one here for symmetry would not.
