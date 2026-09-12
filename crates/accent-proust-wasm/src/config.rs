//! A host's schema configuration, from JavaScript.
//!
//! The bindings default to [`accent_proust::builtins::config`] -- Markdoc's own
//! nodes, tags and functions. That is the right default and the wrong one to be
//! stuck with: pointed at a document written for a host that defines its own
//! components, it reports `tag-undefined` for every one of them, which is
//! correct and useless.
//!
//! This module takes the host's declarations and merges them over the built-ins,
//! which is what upstream does with a user config and is why `{% if %}` and
//! `{% partial %}` keep working after a host adds a tag.
//!
//! # Where the vocabulary lives
//!
//! Not here. Which keys a declaration may carry, what each means, and the
//! refusal of an unknown one with the path to it are
//! [`accent_proust_schema_config`]'s, shared with the command-line host so
//! that the two cannot drift apart. This file is what is JavaScript's: how a
//! `JsValue` answers the vocabulary's seven questions, and the reasons this
//! host adds to a refusal that the vocabulary states without one.
//!
//! # What crosses, and what cannot
//!
//! A schema is data: a name, a list of allowed children, typed attributes, a
//! render policy. All of that crosses.
//!
//! A hook is code. `transform`, `validate`, a custom attribute type and a
//! `RegExp` in `matches` are Rust or JavaScript that has to run inside the
//! validator, and none of it crosses a WebAssembly boundary as a value. So the
//! browser sees what a tag declares and does not see hook-level checking, which
//! means **the editor is never stricter than the server, only faster**. The
//! authority stays where the whole document is.
//!
//! None of that is silently dropped. A configuration carrying a key this crate
//! cannot honour is refused, with the path to it -- a schema that half arrives
//! is worse than one that does not, because the half that is missing is
//! invisible until an author trips over it. The vocabulary refuses the key;
//! [`explain`] adds why this host in particular cannot take it.

use std::sync::Arc;

use accent_proust::ast::Value;
use accent_proust::builtins;
use accent_proust::validate::{Config, MapSchemaSource};
use accent_proust_schema_config::{Declaration, Error, ErrorKind, Path, Shape, declare};
use js_sys::{Array, Reflect};
use wasm_bindgen::JsValue;

use crate::value;

/// Build a validator configuration from a host's declarations.
///
/// # Errors
///
/// Returns a message naming the path to the first problem. The configuration
/// is a document a person wrote, so the path is the actionable half of the
/// message and is never omitted.
pub(crate) fn build(value: &JsValue) -> Result<Config<'static>, String> {
    let declared = declare(&Js(value.clone())).map_err(|error| explain(&error))?;
    // Merged over the built-ins into one source, built once, and the config
    // assembled around it.
    let mut schemas = MapSchemaSource::builtin();
    let variables = declared.apply(&mut schemas);
    let mut config = builtins::config_with(Arc::new(schemas));
    config.variables = variables;
    Ok(config)
}

/// A JavaScript value, read as a declaration.
///
/// A newtype because the trait and the value are both foreign here. Holding
/// the `JsValue` by value costs nothing: it is a handle, and cloning one is a
/// reference-count bump on the JavaScript side.
struct Js(JsValue);

impl Declaration for Js {
    fn shape(&self) -> Shape {
        let item = &self.0;
        if item.is_null() || item.is_undefined() {
            Shape::Null
        } else if item.as_bool().is_some() {
            Shape::Boolean
        } else if item.as_f64().is_some() {
            Shape::Number
        } else if item.is_string() {
            Shape::String
        } else if Array::is_array(item) {
            Shape::List
        } else if value::plain_object(item).is_some() {
            Shape::Object
        } else {
            Shape::Other(value::describe(item))
        }
    }

    fn as_bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    fn as_str(&self) -> Option<String> {
        self.0.as_string()
    }

    fn keys(&self) -> Vec<String> {
        value::plain_object(&self.0)
            .map(|object| value::keys(&object))
            .unwrap_or_default()
    }

    fn get(&self, key: &str) -> Option<Js> {
        let object = value::plain_object(&self.0)?;
        let property = Reflect::get(&object, &JsValue::from_str(key)).ok()?;
        // Explicitly `undefined` is absent, as the trait says.
        if property.is_undefined() {
            None
        } else {
            Some(Js(property))
        }
    }

    fn items(&self) -> Vec<Js> {
        if Array::is_array(&self.0) {
            Array::from(&self.0).iter().map(Js).collect()
        } else {
            Vec::new()
        }
    }

    fn to_value(&self, at: &Path) -> Result<Value, Error> {
        value::value(&self.0, at)
    }
}

/// The vocabulary's message, with this host's reason where it has one.
///
/// Three keys are refused everywhere and for a reason that is particular to a
/// WebAssembly boundary. The vocabulary says "unrecognised key"; this says why
/// a browser cannot take it, so that an author porting a server-side schema
/// learns what to leave behind rather than what to misspell.
fn explain(error: &Error) -> String {
    if let ErrorKind::UnknownKey { key, expected } = &error.kind {
        let why = match key.as_str() {
            "transform" | "validate" => Some(
                "a hook is code, and code does not cross into WebAssembly. Declare what you \
                 can and leave the rest to the server, which sees the whole document",
            ),
            "functions" => Some(
                "a function is code. Markdoc's own are already present; a host's own cannot \
                 cross",
            ),
            "partials" => Some(
                "partials are not supported yet: a parsed partial borrows its source, and \
                 holding both across the boundary needs a design this does not have",
            ),
            _ => None,
        };
        if let Some(why) = why {
            return format!(
                "{}: unrecognised key -- {why}. Expected one of {}",
                error.path,
                expected.join(", ")
            );
        }
    }
    error.to_string()
}
