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

pub mod table;
