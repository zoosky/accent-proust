//! JavaScript values, inward.
//!
//! Everything else in this crate converts Rust to JavaScript. A host
//! configuration goes the other way: an attribute default and a variable are
//! authored in JavaScript and have to become an [`accent_proust::ast::Value`].
//!
//! # Why this walk is iterative too
//!
//! For the reason the outward walks are. A configuration is data the host
//! assembles, and in a content management system it is assembled from theme
//! and plugin manifests -- files, in other words, and not necessarily files the
//! operator wrote. A recursive conversion would turn a deeply nested manifest
//! into a stack overflow, which in WebAssembly is a trap that poisons the
//! instance rather than an error anyone can catch.

use accent_proust::ast::Value;
use accent_proust_schema_config::{Error, ErrorKind, Path};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

/// One unit of work for [`value`].
enum Step {
    /// Convert this value.
    Read(JsValue, Path),
    /// Close an array over the given number of converted elements.
    Array(usize),
    /// Close an object over one value per key, in key order.
    Object(Vec<String>),
}

/// Convert a JavaScript value into an [`accent_proust::ast::Value`].
///
/// # Errors
///
/// [`ErrorKind::NoCounterpart`] at the path of anything with no counterpart --
/// a function, a symbol, a `BigInt`. Silence there would mean a default the
/// host wrote and this crate discarded.
pub(crate) fn value(root: &JsValue, at: &Path) -> Result<Value, Error> {
    let mut steps = vec![Step::Read(root.clone(), at.clone())];
    let mut values: Vec<Value> = Vec::new();

    while let Some(step) = steps.pop() {
        match step {
            Step::Read(item, path) => read(&mut steps, &mut values, &item, &path)?,
            Step::Array(len) => {
                let items = take(&mut values, len);
                values.push(Value::Array(items));
            }
            Step::Object(keys) => {
                let items = take(&mut values, keys.len());
                values.push(Value::Hash(keys.into_iter().zip(items).collect()));
            }
        }
    }

    Ok(values.pop().unwrap_or(Value::Null))
}

/// Convert a leaf in place, or schedule the contents of a container.
fn read(
    steps: &mut Vec<Step>,
    values: &mut Vec<Value>,
    item: &JsValue,
    path: &Path,
) -> Result<(), Error> {
    if item.is_null() || item.is_undefined() {
        values.push(Value::Null);
    } else if let Some(flag) = item.as_bool() {
        values.push(Value::Boolean(flag));
    } else if let Some(number) = item.as_f64() {
        values.push(Value::Number(number));
    } else if let Some(text) = item.as_string() {
        values.push(Value::String(text));
    } else if Array::is_array(item) {
        let array = Array::from(item);
        let len = usize::try_from(array.length()).unwrap_or(0);
        steps.push(Step::Array(len));
        for index in (0..array.length()).rev() {
            let position = usize::try_from(index).unwrap_or(usize::MAX);
            steps.push(Step::Read(array.get(index), path.index(position)));
        }
    } else if let Some(object) = plain_object(item) {
        let names = keys(&object);
        // The closing step goes on first so it comes off last, after every
        // child has left its value behind -- the same order the array branch
        // above uses, and the whole reason this is a worklist rather than a
        // recursion. Children are then pushed in reverse so they pop in key
        // order, which is insertion order for a JavaScript object, and authored
        // order is what this crate preserves.
        steps.push(Step::Object(names.clone()));
        for key in names.iter().rev() {
            let child = Reflect::get(&object, &JsValue::from_str(key)).map_err(|_| {
                Error::new(path.child(key), ErrorKind::Expected("a readable property"))
            })?;
            steps.push(Step::Read(child, path.child(key)));
        }
    } else {
        return Err(Error::new(
            path.clone(),
            ErrorKind::NoCounterpart(describe(item).to_owned()),
        ));
    }
    Ok(())
}

/// The value as a plain object, or [`None`] for anything else.
///
/// A `Date`, a `Map` and a class instance all pass `typeof x === "object"` and
/// none of them survives the trip, so they are refused by name rather than
/// silently flattened to `{}`.
pub(crate) fn plain_object(item: &JsValue) -> Option<Object> {
    if !item.is_object() {
        return None;
    }
    let object = item.clone().unchecked_into::<Object>();
    let prototype: JsValue = Object::get_prototype_of(item).into();
    // `Object.create(null)` has no prototype and is as plain as it gets.
    if prototype.is_null() {
        return Some(object);
    }
    // For `{}` the prototype is `Object.prototype`, whose own prototype is
    // null. For a `Date`, a `Map` or a class instance there is another link in
    // the chain. Walking one step is cheaper than reaching for the global
    // `Object.prototype` to compare against, and says the same thing.
    let grandparent: JsValue = Object::get_prototype_of(&prototype).into();
    if grandparent.is_null() {
        Some(object)
    } else {
        None
    }
}

/// A short name for an unconvertible value, for the error message.
pub(crate) fn describe(item: &JsValue) -> &'static str {
    if item.is_function() {
        "function"
    } else if item.is_symbol() {
        "symbol"
    } else if item.is_bigint() {
        "BigInt"
    } else if item.is_object() {
        "class instance or built-in object"
    } else {
        "value of this type"
    }
}

/// An object's own enumerable string keys, in insertion order.
pub(crate) fn keys(object: &Object) -> Vec<String> {
    Object::keys(object)
        .iter()
        .filter_map(|key| key.as_string())
        .collect()
}

/// Take the last `count` values, oldest first. See `tree::take`.
fn take(values: &mut Vec<Value>, count: usize) -> Vec<Value> {
    match values.len().checked_sub(count) {
        Some(start) => values.split_off(start),
        None => std::mem::take(values),
    }
}
