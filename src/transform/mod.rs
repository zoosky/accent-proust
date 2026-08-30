//! Transformation: a validated AST becomes a renderable tree.
//!
//! Mirrors upstream `src/transformer.ts` and `src/transforms/`. This is where
//! variables resolve, functions evaluate, and each node is handed to its
//! schema's transform hook.
//!
//! Transform hooks here are synchronous. Upstream accepts a promise so a schema
//! can fetch during transform; this crate does no I/O by construction, so an
//! async hook would be a signature with no reachable implementation that
//! coloured every caller above it. Recorded in `DIVERGENCES.md`.
//!
//! # One pass runs earlier than the rest
//!
//! [`table`] is a transform in upstream's sense and a parse-stage pass in
//! practice: `parser()` applies it before it returns, so every stage above --
//! the validator, this one, the formatter -- sees a document in which
//! `{% table %}` has already become a `table` node. It lives here because that
//! is where upstream puts it and because the yearly upstream diff is worth more
//! than the tidier module map.
//!
//! # Resolution is lazy
//!
//! Upstream resolves the whole tree into a second tree and then transforms it.
//! Here each attribute is resolved at the moment the transform stage reads it,
//! which reaches the same answer -- see [`resolve`] for why the one case where
//! the configuration changes mid-tree, `{% partial %}`, agrees too.

mod node;
pub mod resolve;
pub mod table;

pub use node::{attributes, children, find_schema, global_attributes, node, MAX_TRANSFORM_DEPTH};
pub use resolve::{resolve, resolve_variable, MAX_RESOLVE_DEPTH};

pub(crate) use node::scalar;

use crate::ast::Node;
use crate::renderable::RenderableTreeNodes;
use crate::validate::Config;

/// Transform a parsed document into a renderable tree.
///
/// Upstream's `Markdoc.transform(node, config)`, minus the `mergeConfig` step:
/// the built-ins are already in [`builtins::config`](crate::builtins::config),
/// so a caller who wants them has them and a caller who does not starts from
/// [`Config::new`].
///
/// ```
/// use proust::renderable::{RenderableTreeNode, RenderableTreeNodes};
///
/// let document = proust::parse::parse("# Title\n");
/// let config = proust::builtins::config();
/// let RenderableTreeNodes::One(RenderableTreeNode::Tag(article)) =
///     proust::transform::transform(&document, &config)
/// else {
///     panic!("a document renders one element");
/// };
/// assert_eq!(article.name, "article");
/// ```
#[must_use]
pub fn transform<'a>(document: &'a Node<'a>, config: &Config<'a>) -> RenderableTreeNodes {
    node(document, config)
}
