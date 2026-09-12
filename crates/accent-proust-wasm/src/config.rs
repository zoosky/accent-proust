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
//! render policy. All of that crosses. So does any object read key by key -- a
//! plain object, a class instance, a proxy -- because a schema is read that
//! way; only a value carried through whole, an attribute's `default` or a
//! variable, has to be a plain one, and [`crate::value`] says why.
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
//! invisible until an author trips over it. The vocabulary refuses the key, or
//! the function where a value was wanted, or the pattern where a list was;
//! [`explain`] adds why this host in particular cannot take it. A property
//! whose getter throws is refused too, as unreadable, rather than read as
//! absent: a block that was written and then lost is the failure above in
//! another form.

use std::sync::Arc;

use accent_proust::ast::Value;
use accent_proust::builtins;
use accent_proust::validate::{Config, MapSchemaSource};
use accent_proust_schema_config::{Declaration, Error, ErrorKind, Path, Shape, declare};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

use crate::value;

/// Build a validator configuration from a host's declarations.
///
/// # Errors
///
/// Returns a message naming the path to the first problem. The configuration
/// is a document a person wrote, so the path is the actionable half of the
/// message and is never omitted.
pub(crate) fn build(value: &JsValue) -> Result<Config<'static>, String> {
    let declared = declare(&Js(value.clone())).map_err(explain)?;
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

impl Js {
    /// The value as an object to read key by key, if it is one.
    ///
    /// Any object but an array or a function: a class instance and a proxy
    /// both have keys and properties, and a schema is read as nothing else.
    /// The stricter question -- would this survive as a `Value`? -- is
    /// [`value::plain_object`]'s, asked only by [`Declaration::to_value`].
    fn object(&self) -> Option<Object> {
        let item = &self.0;
        if item.is_object() && !Array::is_array(item) && !item.is_function() {
            Some(item.clone().unchecked_into())
        } else {
            None
        }
    }
}

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
        } else if self.object().is_some() {
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
        self.object()
            .map(|object| value::keys(&object))
            .unwrap_or_default()
    }

    fn get(&self, key: &str, at: &Path) -> Result<Option<Js>, Error> {
        let Some(object) = self.object() else {
            return Ok(None);
        };
        // A getter that throws, or a proxy trap that refuses: the property
        // exists and was lost, which is not the same as absent.
        let property = Reflect::get(&object, &JsValue::from_str(key))
            .map_err(|_| Error::new(at.clone(), ErrorKind::Unreadable))?;
        // Explicitly `undefined` is absent, as the trait says.
        Ok(if property.is_undefined() {
            None
        } else {
            Some(Js(property))
        })
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
/// The vocabulary says what is true everywhere: "unrecognised key", "expected
/// a string, not a function", "a regular expression is not supported". This
/// says why a browser in particular cannot take it, so that an author porting
/// a server-side schema learns what to leave behind rather than what to
/// misspell. The sentence stays the vocabulary's; only the reason is added.
fn explain(error: Error) -> String {
    let why = match &error.kind {
        ErrorKind::UnknownKey { key, .. } => match key.as_str() {
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
        },
        // A function where a value was wanted: a custom attribute type, a
        // `matches` predicate, a computed default. All code, none of it
        // crossing.
        ErrorKind::Expected {
            got: Shape::Other("function"),
            ..
        } => Some(
            "a function is code, and code does not cross into WebAssembly; declare what you \
             can and leave the custom check to the server",
        ),
        ErrorKind::MatchesNotAList => {
            Some("and a host pattern is code that cannot cross into WebAssembly")
        }
        _ => None,
    };
    match why {
        Some(why) => error.explained(why).to_string(),
        None => error.to_string(),
    }
}
