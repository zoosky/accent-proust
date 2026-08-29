//! The document tree: nodes, tags, variables, functions, and source locations.
//!
//! Mirrors upstream `src/ast/`. Lands with the tokenizer, because the two are
//! one design decision: the AST shape is what the segmenter has to be able to
//! produce.
//!
//! Two properties are fixed here and depended on everywhere above:
//!
//! - **Spans borrow.** A location is a byte range plus line and column, with
//!   text borrowed from the source buffer. The formatter needs byte fidelity to
//!   reprint canonical source, and copying every node's text to own it forfeits
//!   that for no gain.
//! - **Attribute order is authored order.** Not hash order. Rendered output has
//!   to be reproducible byte-for-byte across runs.

mod value;

pub use value::{Function, PathSegment, Value, Variable};
