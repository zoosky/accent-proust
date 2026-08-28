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
