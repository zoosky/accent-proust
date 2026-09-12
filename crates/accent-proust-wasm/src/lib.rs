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
//! # Host schemas
//!
//! The functions above use Markdoc's built-in configuration, which reports
//! `tag-undefined` for a tag the host defines. To teach them a host's own
//! components, build a [`Config`] once and call the same stages on it:
//!
//! ```js
//! const config = new Config({
//!   tags: {
//!     callout: {
//!       render: "Callout",
//!       attributes: { type: { type: "String", matches: ["note", "warning"] } },
//!     },
//!   },
//!   variables: { flags: { beta: true } },
//! });
//!
//! config.validate(source);
//! config.renderHtml(source);
//! ```
//!
//! Declarations merge over the built-ins, so `{% if %}` and `{% partial %}`
//! keep working. What a schema declares crosses; a `transform` or `validate`
//! hook is code and does not, so the browser is never *stricter* than the
//! server -- only faster. See [`config`] for the full contract.
//!
//! # Not here yet
//!
//! Partials and host-defined functions. A parsed partial borrows its source,
//! so holding both across the boundary needs a design this does not have, and
//! a function is code. Both are refused by name rather than ignored.

use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

mod config;
mod diagnostics;
mod tree;
mod utf16;
mod value;

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
    let errors = accent_proust::validate::validate_tree(&document, &config);
    diagnostics::errors(source, &errors)
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

/// A host's schema configuration, built once and reused.
///
/// Constructing this parses and checks the declarations; the stage methods on
/// it then cost no more than the free functions do. That matters for the
/// surface this exists for -- an editor revalidating on every keystroke should
/// not re-read a schema registry each time.
///
/// Call `free()` when done, as with any wasm-bindgen object.
#[wasm_bindgen(js_name = Config)]
pub struct HostConfig {
    inner: accent_proust::validate::Config<'static>,
}

#[wasm_bindgen(js_class = Config)]
impl HostConfig {
    /// Build a configuration from a host's declarations.
    ///
    /// Declarations merge over Markdoc's built-ins. `null` or `undefined`
    /// gives the built-ins alone, which is what the free functions use.
    ///
    /// # Errors
    ///
    /// Throws with the path to the first problem -- `config.tags.callout.
    /// attributes.type.type`, not "invalid schema". A configuration is a
    /// document a person wrote, so the path is the actionable half.
    #[wasm_bindgen(constructor)]
    pub fn new(declarations: &JsValue) -> Result<HostConfig, JsValue> {
        config::build(declarations)
            .map(|inner| HostConfig { inner })
            .map_err(|message| js_sys::Error::new(&message).into())
    }

    /// Render a Markdoc document to HTML against this configuration.
    ///
    /// See [`render_html`], which this is the configured form of.
    #[wasm_bindgen(js_name = renderHtml)]
    #[must_use]
    pub fn render_html(&self, source: &str) -> String {
        let document = accent_proust::parse::parse(source);
        let nodes = accent_proust::transform::transform(&document, &self.inner).into_vec();
        accent_proust::render::render_all(&nodes)
    }

    /// Validate a Markdoc document against this configuration.
    ///
    /// See [`validate`], which this is the configured form of. A tag the
    /// configuration declares no longer reports `tag-undefined`; its attributes
    /// are checked against what it declares.
    #[wasm_bindgen(js_name = validate)]
    #[must_use]
    pub fn validate(&self, source: &str) -> js_sys::Array {
        let document = accent_proust::parse::parse(source);
        let errors = accent_proust::validate::validate_tree(&document, &self.inner);
        diagnostics::errors(source, &errors)
    }

    /// Transform a Markdoc document into a renderable tree against this
    /// configuration.
    ///
    /// See [`transform`], which this is the configured form of.
    #[wasm_bindgen(js_name = transform)]
    #[must_use]
    pub fn transform(&self, source: &str) -> js_sys::Array {
        let document = accent_proust::parse::parse(source);
        let nodes = accent_proust::transform::transform(&document, &self.inner).into_vec();
        tree::nodes(&nodes)
    }
}
