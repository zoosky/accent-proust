//! WebAssembly bindings for [`accent_proust`], for a browser or any other
//! JavaScript host.
//!
//! The library performs no I/O, reads no configuration and decides no HTML
//! policy; everything host-specific reaches it through a seam. A WebAssembly
//! ABI is one more host, so it lives in this crate rather than behind a feature
//! flag in the library: `crate-type` is not conditional, and a `cdylib` feature
//! would change how every native consumer links.
//!
//! # The pipeline, one call per stage
//!
//! ```js
//! import init, { validate, renderHtml, transform, format } from "accent-proust";
//! await init();
//!
//! const errors = validate(source);   // diagnostics, before anything renders
//! const html   = renderHtml(source); // the preview
//! const tree   = transform(source);  // for a React or Vue renderer
//! const clean  = format(source);     // canonical Markdoc source
//! ```
//!
//! Stages are separate because a preview pane needs the diagnostics, not only
//! the markup. Collapsing them into one `markdocToHtml` would throw away the
//! reason to use Markdoc over Markdown.
//!
//! # What crosses the boundary
//!
//! Values are built as JavaScript objects directly rather than serialised to
//! JSON and parsed back, so a number is formatted by the engine and this crate
//! does not reimplement ECMAScript's `ToString`. The two mappings that have a
//! shape to honour are documented where they live: [`tree`] for the renderable
//! tree, and [`diagnostics`] for validation errors.
//!
//! Every entry point parses from scratch. Nothing is cached between calls and
//! no value holds a borrow across the boundary, which is what makes each of
//! these a plain function rather than a handle the caller has to free.
//!
//! # Not here yet
//!
//! Variables, partials and host-defined tag schemas. Each needs a second
//! mapping -- JavaScript values inward rather than Rust values outward -- and
//! schemas additionally need a way to call a JavaScript hook from inside the
//! validator. Until then these entry points use the built-in configuration
//! from [`accent_proust::builtins::config`], which is upstream's default set of
//! nodes, tags and functions.

use wasm_bindgen::prelude::wasm_bindgen;

mod diagnostics;
mod tree;

/// Render a Markdoc document to HTML.
///
/// This is the whole pipeline: parse, transform against the built-in
/// configuration, then render. Validation errors do not stop it, because
/// upstream renders a document that has them and a preview pane that blanks on
/// the first mistake is worse than one that shows the mistake.
#[wasm_bindgen(js_name = renderHtml)]
#[must_use]
pub fn render_html(source: &str) -> String {
    let document = accent_proust::parse::parse(source);
    let config = accent_proust::builtins::config();
    let nodes = accent_proust::transform::transform(&document, &config).into_vec();
    accent_proust::render::render_all(&nodes)
}

/// Validate a Markdoc document.
///
/// Returns an array of upstream's `ValidateError`, empty when the document is
/// clean. The `error.id` of each is upstream's id unchanged: it is the field
/// external tooling binds to, so it is the field this crate is least free to
/// invent.
#[wasm_bindgen(js_name = validate)]
#[must_use]
pub fn validate(source: &str) -> js_sys::Array {
    let document = accent_proust::parse::parse(source);
    let config = accent_proust::builtins::config();
    diagnostics::errors(&accent_proust::validate::validate_tree(&document, &config))
}

/// Transform a Markdoc document into a renderable tree.
///
/// Returns an array of upstream's `RenderableTreeNode`: a tag is an object
/// carrying `$$mdtype: "Tag"`, and anything else is the scalar it renders to.
/// Use this rather than [`render_html`] when the host owns the markup -- a
/// React or Vue renderer maps tag names onto its own components from this tree.
#[wasm_bindgen(js_name = transform)]
#[must_use]
pub fn transform(source: &str) -> js_sys::Array {
    let document = accent_proust::parse::parse(source);
    let config = accent_proust::builtins::config();
    let nodes = accent_proust::transform::transform(&document, &config).into_vec();
    tree::nodes(&nodes)
}

/// Format a Markdoc document as canonical Markdoc source.
///
/// `format(format(s)) === format(s)`, and the document a formatted source
/// parses to is the document the original parsed to, so an editor can rewrite
/// a buffer in place without losing anything.
#[wasm_bindgen(js_name = format)]
#[must_use]
pub fn format(source: &str) -> String {
    let document = accent_proust::parse::parse(source);
    accent_proust::format::format(&document)
}
