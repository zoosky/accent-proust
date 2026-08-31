//! A Rust implementation of the [Markdoc](https://markdoc.dev) language.
//!
//! Markdoc is CommonMark plus a tag syntax that turns documents into
//! structured, validatable content instead of pre-rendered HTML:
//!
//! ```markdown
//! {% callout type="note" %}
//! Tags nest, take typed attributes, and are validated against a schema.
//! {% /callout %}
//! ```
//!
//! This crate implements that language as a pipeline of pure stages:
//!
//! ```text
//! parse  ->  AST  ->  validate  ->  transform  ->  renderable tree  ->  format
//! ```
//!
//! # What this crate does not do
//!
//! It performs no I/O, reads no configuration, and decides no HTML policy. It
//! has no concept of a file, a theme, a template, or a plugin. Everything
//! host-specific arrives as data the caller passes in, or through a trait the
//! caller implements:
//!
//! - [`Tokenizer`](parse) segments Markdown. A default implementation over
//!   pulldown-cmark ships behind the `pulldown-cmark-tokenizer` feature, so a
//!   host that already owns a CommonMark parser can supply its own rather than
//!   compile a second one.
//! - `SchemaSource` answers "what is the schema for this tag name?". Whether
//!   that answer comes from a file, a constant, or a sandboxed guest is the
//!   host's business, not this crate's.
//! - `TagRenderer` turns a validated tag plus its rendered children into
//!   markup. Escaping, template lookup, and HTML policy live there.
//!
//! That boundary is deliberate and is enforced by a CI job that builds and
//! tests this crate with nothing else present.
//!
//! # Compatibility
//!
//! Ported from upstream Markdoc at revision `afee1a4` (v0.5.9). The tag
//! language and the validation error ids are the contract; CommonMark edge
//! behaviour is not, because upstream is built on markdown-it and this crate is
//! built on pulldown-cmark. Every deliberate difference is recorded in
//! `DIVERGENCES.md` at the repository root, which is normative rather than a
//! changelog.
//!
//! # Conventions this crate commits to
//!
//! - **Public enums are `#[non_exhaustive]`.** Markdoc gained node types across
//!   its 0.5.x line; spelling them exhaustively would turn each new one into a
//!   breaking release.
//! - **Validation errors are data, not failures.** The validator returns a
//!   `Vec` of them. `Result::Err` is reserved for internal invariants.
//! - **Output is deterministic.** Attribute order is authored order, never hash
//!   order, so two runs over the same input produce identical bytes.
//! - **Panic-freedom is a promise.** Property tests assert the parser never
//!   panics on arbitrary input, and fuzzing precedes publication. An open
//!   parser is a claim about its attack surface.
//!
//!   *Destroying* a value is covered for values a **caller** builds as well as
//!   for documents this crate parses: every public recursive type --
//!   [`ast::Node`], [`ast::Value`], [`renderable::Tag`] and
//!   [`renderable::Scalar`] -- carries a manual iterative [`Drop`], because a
//!   derived one aborts on a deep value and an abort cannot be caught. The
//!   visible cost is that a variant's contents are taken with
//!   [`std::mem::take`] rather than moved out.
//!
//!   *Traversing* one is not, and the line is worth stating rather than
//!   leaving to be discovered: `Clone`, `PartialEq` and `Debug` are derived on
//!   those types and are therefore recursive, so a caller that assembles a
//!   value thousands of levels deep can still overflow by cloning or comparing
//!   it. Nothing this crate parses reaches that depth -- the value grammar is
//!   bounded at [`grammar::MAX_VALUE_DEPTH`] (`DIVERGENCES.md` entry 9) -- so
//!   the exposure is a caller's own construction, and closing it means
//!   hand-writing those three the way [`Drop`] is hand-written.

pub mod ast;
pub mod builtins;
pub mod format;
pub mod functions;
pub mod grammar;
pub mod parse;
pub mod render;
pub mod renderable;
pub mod tags;
pub mod transform;
pub mod validate;
