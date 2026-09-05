//! Validation errors, in the shape upstream's `ValidateError` has.
//!
//! `reference/src/types.ts:142-154` is the contract: a `ValidateError` carries
//! `type`, `lines`, an optional `location`, and a nested `error` holding `id`,
//! `level`, `message` and its own optional `location`. Error ids are the one
//! place divergence is disallowed outright (`AGENT.md`), because external
//! tooling binds to them -- so they cross the boundary unchanged.
//!
//! # Where the location shape differs
//!
//! Upstream's location edge is `{ line, character? }`. This crate emits
//! `{ line, character, offset, byteOffset }`. The last two have no upstream
//! counterpart and are there because an editor placing a marker needs an
//! absolute position and cannot recover one from a line and a column.
//!
//! **Everything except `byteOffset` is counted in UTF-16 code units**, which is
//! what JavaScript means by a string index and what a CodeMirror position, a
//! Monaco position and an LSP `character` all are. The library measures in
//! UTF-8 bytes, because that is what a `&str` is, and the conversion happens
//! here -- see [`crate::utf16`] for why it happens here rather than in each
//! host.
//!
//! - `line` is zero-based, as the library has it.
//! - `character` is UTF-16 code units from the start of the line.
//! - `offset` is UTF-16 code units from the start of the document, so it can be
//!   used as a position directly.
//! - `byteOffset` is the library's own unit, for a host that wants to index
//!   back into the source as bytes.

use crate::utf16::Utf16Index;
use accent_proust::ast::{Location, Position};
use accent_proust::validate::ValidateError;
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::JsValue;

/// Convert validation errors into a JavaScript array of `ValidateError`.
///
/// `source` is the document the errors were produced from, and is needed to
/// turn the library's byte offsets into the code-unit offsets JavaScript
/// counts in. It is indexed once for the whole batch rather than once per
/// error.
pub(crate) fn errors(source: &str, list: &[ValidateError<'_>]) -> Array {
    let index = Utf16Index::new(source);
    let out = Array::new();
    for item in list {
        out.push(&error(&index, item));
    }
    out
}

/// Convert one `ValidateError`.
fn error(index: &Utf16Index, item: &ValidateError<'_>) -> JsValue {
    let lines = Array::new();
    for line in &item.lines {
        lines.push(&number(*line));
    }

    let inner = Object::new();
    set(&inner, "id", &JsValue::from_str(item.error.id));
    set(
        &inner,
        "level",
        &JsValue::from_str(item.error.level.as_str()),
    );
    set(&inner, "message", &JsValue::from_str(&item.error.message));
    if let Some(spot) = item.error.location {
        set(&inner, "location", &location(index, &spot));
    }

    let object = Object::new();
    set(&object, "type", &JsValue::from_str(item.node_type.as_str()));
    set(&object, "lines", &lines);
    if let Some(spot) = item.location {
        set(&object, "location", &location(index, &spot));
    }
    set(&object, "error", &inner);
    object.into()
}

/// Convert a location, including the `file` label only when the caller set one.
fn location(index: &Utf16Index, spot: &Location<'_>) -> JsValue {
    let object = Object::new();
    if let Some(file) = spot.file {
        set(&object, "file", &JsValue::from_str(file));
    }
    set(&object, "start", &position(index, &spot.start));
    set(&object, "end", &position(index, &spot.end));
    object.into()
}

/// Convert one edge of a location.
fn position(index: &Utf16Index, edge: &Position) -> JsValue {
    let offset = index.at(edge.offset);
    // `column` is a byte count from the start of the line, so the line's own
    // start has to be converted too: the difference of two absolute code-unit
    // offsets is the column in code units, and subtracting the byte column
    // from the byte offset is how the line start is found.
    let line_start = index.at(edge.offset.saturating_sub(edge.column));

    let object = Object::new();
    set(&object, "line", &number(edge.line));
    set(
        &object,
        "character",
        &number(offset.saturating_sub(line_start)),
    );
    set(&object, "offset", &number(offset));
    set(&object, "byteOffset", &number(edge.offset));
    object.into()
}

/// A `usize` as a JavaScript number.
///
/// `usize` is 32 bits on `wasm32`, so every value this crate produces is
/// exactly representable. The saturating conversion is for the `rlib` built on
/// a 64-bit host, where a document large enough to exceed `u32::MAX` would
/// already have exhausted the address space wasm gives it.
fn number(value: usize) -> JsValue {
    JsValue::from_f64(f64::from(u32::try_from(value).unwrap_or(u32::MAX)))
}

/// Set a property, discarding the result. See `tree::set`.
fn set(target: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(target, &JsValue::from_str(key), value);
}
