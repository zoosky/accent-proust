//! The shape a configuration arrives in, abstracted just far enough to walk.
//!
//! A host's configuration is a `JsValue` in the browser and a YAML document
//! on the command line. Converting either wholesale into a
//! [`Value`](accent_proust::ast::Value) before checking it would mean
//! converting a JavaScript function written where a hook is not allowed --
//! and failing on the conversion, with a message about functions, rather than
//! on the key, with a message about hooks. So the walker asks for keys first
//! and converts values only where the vocabulary expects one: an attribute's
//! `default`, and `variables`. Seven methods, and the host owns its reading.

use accent_proust::ast::Value;

use crate::error::{Error, ErrorKind};
use crate::path::Path;

/// What kind of thing a declared value is, for dispatch and for messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Shape {
    /// `null`, or absent.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A number.
    Number,
    /// A string.
    String,
    /// An ordered list.
    List,
    /// A keyed object, in authored order.
    Object,
    /// Something with no counterpart, named by the host: `"function"`.
    Other(&'static str),
}

impl Shape {
    /// The shape as prose, with its article, for "expected X, not Y".
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Shape::Null => "null".to_owned(),
            Shape::Boolean => "a boolean".to_owned(),
            Shape::Number => "a number".to_owned(),
            Shape::String => "a string".to_owned(),
            Shape::List => "a list".to_owned(),
            Shape::Object => "an object".to_owned(),
            Shape::Other(what) => format!("a {what}"),
        }
    }
}

/// A configuration value, as the host holds it.
///
/// Implemented once per host. [`Value`] implements it too, so a configuration
/// already in the library's own lattice -- a test's, or one a host assembled
/// itself -- can be declared without a detour.
pub trait Declaration: Sized {
    /// What this value is.
    ///
    /// An object is anything read key by key: a plain object, a class
    /// instance, a proxy. Whether it would survive conversion to a [`Value`]
    /// is [`to_value`](Declaration::to_value)'s question, asked only where
    /// the vocabulary carries a value through.
    fn shape(&self) -> Shape;

    /// The value as a boolean, if it is one.
    fn as_bool(&self) -> Option<bool>;

    /// The value as a string, if it is one. Owned, because a `JsValue`'s
    /// string is a copy either way.
    fn as_str(&self) -> Option<String>;

    /// An object's keys, in authored order. Empty for anything else.
    fn keys(&self) -> Vec<String>;

    /// One property of an object, or `None` when absent. A JavaScript
    /// property that is explicitly `undefined` is absent too.
    ///
    /// `at` is the property's own path, for the error.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Unreadable`] when the property exists and cannot be read
    /// -- a getter that throws, a proxy trap that refuses. Absent and
    /// unreadable are different answers: the first is a key the author did
    /// not write, the second is one that was written and then lost, and a
    /// configuration that loses a block silently is the failure this crate
    /// exists to refuse.
    fn get(&self, key: &str, at: &Path) -> Result<Option<Self>, Error>;

    /// A list's elements, in order. Empty for anything else.
    fn items(&self) -> Vec<Self>;

    /// The whole value as a [`Value`], for the places the vocabulary carries
    /// one through unchanged.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NoCounterpart`] at the offending path, for anything
    /// Markdoc has no value for.
    fn to_value(&self, at: &Path) -> Result<Value, Error>;
}

impl Declaration for Value {
    fn shape(&self) -> Shape {
        match self {
            Value::Null => Shape::Null,
            Value::Boolean(_) => Shape::Boolean,
            Value::Number(_) => Shape::Number,
            Value::String(_) => Shape::String,
            Value::Array(_) => Shape::List,
            Value::Hash(_) => Shape::Object,
            Value::Function(_) => Shape::Other("function call"),
            Value::Variable(_) => Shape::Other("variable reference"),
            _ => Shape::Other("value of this kind"),
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(flag) => Some(*flag),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<String> {
        match self {
            Value::String(text) => Some(text.clone()),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<String> {
        match self {
            Value::Hash(map) => map.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    fn get(&self, key: &str, _at: &Path) -> Result<Option<Value>, Error> {
        Ok(match self {
            Value::Hash(map) => map.get(key).cloned(),
            _ => None,
        })
    }

    fn items(&self) -> Vec<Value> {
        match self {
            Value::Array(items) => items.clone(),
            _ => Vec::new(),
        }
    }

    fn to_value(&self, at: &Path) -> Result<Value, Error> {
        match self.shape() {
            Shape::Other(what) => Err(Error::new(
                at.clone(),
                ErrorKind::NoCounterpart(what.to_owned()),
            )),
            _ => Ok(self.clone()),
        }
    }
}
