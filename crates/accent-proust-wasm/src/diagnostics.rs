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
//! `{ line, character, offset }`: `offset` is an absolute byte index and has no
//! upstream counterpart, but an editor that wants to place a marker needs it
//! and cannot recover it from a line and a column.
//!
//! Both `line` and `character` are **zero-based**, matching the library's
//! `Position`, and `character` is a **byte** column, not a UTF-16 code unit.
//! For ASCII the two agree; for anything else a host mapping these onto an
//! editor range has to convert. That is a real difference from what an LSP
//! client expects, and is called out here rather than papered over.

use accent_proust::ast::{Location, Position};
use accent_proust::validate::ValidateError;
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::JsValue;

/// Convert validation errors into a JavaScript array of `ValidateError`.
pub(crate) fn errors(list: &[ValidateError<'_>]) -> Array {
    let out = Array::new();
    for item in list {
        out.push(&error(item));
    }
    out
}

/// Convert one `ValidateError`.
fn error(item: &ValidateError<'_>) -> JsValue {
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
        set(&inner, "location", &location(&spot));
    }

    let object = Object::new();
    set(&object, "type", &JsValue::from_str(item.node_type.as_str()));
    set(&object, "lines", &lines);
    if let Some(spot) = item.location {
        set(&object, "location", &location(&spot));
    }
    set(&object, "error", &inner);
    object.into()
}

/// Convert a location, including the `file` label only when the caller set one.
fn location(spot: &Location<'_>) -> JsValue {
    let object = Object::new();
    if let Some(file) = spot.file {
        set(&object, "file", &JsValue::from_str(file));
    }
    set(&object, "start", &position(&spot.start));
    set(&object, "end", &position(&spot.end));
    object.into()
}

/// Convert one edge of a location.
fn position(edge: &Position) -> JsValue {
    let object = Object::new();
    set(&object, "line", &number(edge.line));
    set(&object, "character", &number(edge.column));
    set(&object, "offset", &number(edge.offset));
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
