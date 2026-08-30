//! The built-in functions available in tag attributes and annotations.
//!
//! Mirrors upstream `src/functions/index.ts`: `and`, `or`, `not`, `equals`,
//! `default`, `debug`.
//!
//! Functions are pure and total. They take resolved values and return one; they
//! cannot fail, cannot observe anything outside their arguments, and have no
//! access to the host.
//!
//! # Two things about the arguments
//!
//! **Positional arguments are keyed by index**, as decimal strings, because
//! upstream builds one object for named and positional parameters alike
//! (`parameters[name || index] = value`). `not($x)` reads `parameters["0"]`.
//! Use [`Function::positional_key`](crate::ast::Function::positional_key)
//! rather than spelling the coercion out.
//!
//! **[`None`] is `undefined`, and it is not `null`.** An argument that resolved
//! to nothing keeps its key with a `None` value, because `and`, `or` and
//! `equals` read every argument and an absent key would change how many they
//! see. `default` reads the difference the other way round: `default(null, 1)`
//! is `null` and `default($missing, 1)` is `1`.
//!
//! # Truthiness here is Markdoc's, not JavaScript's
//!
//! `and`, `or` and `not` all call [`truthy`](crate::tags::truthy), which is
//! `value !== false && value !== undefined && value !== null`. `and(0, "")` is
//! **true**. The corpus fixes this in "Truthy things are not false", and using
//! JavaScript's `Boolean()` instead would invert five of its assertions.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::ast::{Function, Value};
use crate::tags::truthy;
use crate::validate::{Config, ConfigFunction};

/// Every built-in function, by the name a document writes.
#[must_use]
pub fn builtin() -> IndexMap<String, ConfigFunction> {
    let mut functions = IndexMap::new();
    functions.insert("and".to_string(), pure(and));
    functions.insert("or".to_string(), pure(or));
    functions.insert("not".to_string(), pure(not));
    functions.insert("equals".to_string(), pure(equals));
    functions.insert("default".to_string(), pure(default));
    functions.insert("debug".to_string(), pure(debug));
    functions
}

/// A built-in: a transform, and no declared parameters or return type.
///
/// `parameters: None` rather than `Some(empty)` on purpose -- see
/// [`ConfigFunction::parameters`]. `Some` of an empty map would reject every
/// argument, and these take any number.
fn pure(transform: fn(&Parameters, &Config<'_>) -> Option<Value>) -> ConfigFunction {
    ConfigFunction {
        transform: Some(Arc::new(transform)),
        ..ConfigFunction::default()
    }
}

/// The parameters, in the order they were written.
///
/// Every argument the call site wrote has a key; [`None`] is the one that
/// resolved to `undefined`.
type Parameters = IndexMap<String, Option<Value>>;

/// The value of the argument at `index`, or [`None`] for `undefined` and for an
/// argument that was never written.
fn positional(parameters: &Parameters, index: usize) -> Option<&Value> {
    parameters.get(&Function::positional_key(index))?.as_ref()
}

/// `and(...)`: every argument is truthy.
///
/// An empty call is `true`, which is what `Array.every` returns for an empty
/// array.
fn and(parameters: &Parameters, _config: &Config<'_>) -> Option<Value> {
    Some(Value::Boolean(
        parameters.values().all(|value| truthy(value.as_ref())),
    ))
}

/// `or(...)`: some argument is truthy.
fn or(parameters: &Parameters, _config: &Config<'_>) -> Option<Value> {
    Some(Value::Boolean(
        parameters.values().any(|value| truthy(value.as_ref())),
    ))
}

/// `not(x)`: the first argument is not truthy.
fn not(parameters: &Parameters, _config: &Config<'_>) -> Option<Value> {
    Some(Value::Boolean(!truthy(positional(parameters, 0))))
}

/// `equals(...)`: every argument equals the first.
///
/// Upstream compares with `===`, so two containers are equal only when they are
/// the same object; this compares them structurally. See `DIVERGENCES.md` --
/// object identity does not survive resolution into owned values, and
/// structural equality is what an author writing `equals($a, [1])` means.
fn equals(parameters: &Parameters, _config: &Config<'_>) -> Option<Value> {
    let mut values = parameters.values();
    let Some(first) = values.next() else {
        // `[].every(...)` is `true`.
        return Some(Value::Boolean(true));
    };
    Some(Value::Boolean(values.all(|value| value == first)))
}

/// `default(value, fallback)`: `fallback` when `value` is `undefined`.
///
/// `null` is a value and is returned as one. Only `undefined` -- an unset
/// variable, a path that leads nowhere -- falls through.
fn default(parameters: &Parameters, _config: &Config<'_>) -> Option<Value> {
    match positional(parameters, 0) {
        Some(value) => Some(value.clone()),
        None => positional(parameters, 1).cloned(),
    }
}

/// `debug(value)`: the first argument as indented JSON.
///
/// Upstream's `JSON.stringify(parameters[0], null, 2)`, which returns
/// `undefined` rather than a string when there is nothing to print -- so an
/// empty `debug()` renders nothing rather than the word "undefined".
fn debug(parameters: &Parameters, _config: &Config<'_>) -> Option<Value> {
    // `JSON.stringify(undefined, null, 2)` is `undefined`, not the string
    // "undefined", so an argument that resolved to nothing prints nothing.
    let value = positional(parameters, 0)?;
    let mut out = String::new();
    write_json(value, 0, &mut out);
    Some(Value::String(out))
}

/// `JSON.stringify(value, null, 2)`, which is what `debug` promises.
fn write_json(value: &Value, indent: usize, out: &mut String) {
    use std::fmt::Write as _;

    let pad = |out: &mut String, level: usize| {
        for _ in 0..level * 2 {
            out.push(' ');
        }
    };
    match value {
        Value::Null => out.push_str("null"),
        Value::Boolean(boolean) => out.push_str(if *boolean { "true" } else { "false" }),
        Value::Number(number) => {
            // `JSON.stringify` writes a non-finite number as `null`, and prints
            // an integral float without a fractional part -- which is what
            // Rust's `Display` for `f64` does too.
            if number.is_finite() {
                let _ = write!(out, "{number}");
            } else {
                out.push_str("null");
            }
        }
        Value::String(text) => write_json_string(text, out),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (position, item) in items.iter().enumerate() {
                if position > 0 {
                    out.push_str(",\n");
                }
                pad(out, indent + 1);
                write_json(item, indent + 1, out);
            }
            out.push('\n');
            pad(out, indent);
            out.push(']');
        }
        Value::Hash(entries) if entries.is_empty() => out.push_str("{}"),
        Value::Hash(entries) => {
            out.push_str("{\n");
            for (position, (key, value)) in entries.iter().enumerate() {
                if position > 0 {
                    out.push_str(",\n");
                }
                pad(out, indent + 1);
                write_json_string(key, out);
                out.push_str(": ");
                write_json(value, indent + 1, out);
            }
            out.push('\n');
            pad(out, indent);
            out.push('}');
        }
        // A reference reaching `debug` has not been resolved, which means the
        // caller skipped resolution rather than that the value is a reference.
        // `JSON.stringify` of an object with no `toJSON` prints its own fields;
        // printing `null` says "there is nothing here" instead of inventing a
        // spelling for a bug.
        Value::Variable(_) | Value::Function(_) => out.push_str("null"),
    }
}

/// A JSON string literal, escaped as `JSON.stringify` escapes one.
fn write_json_string(text: &str, out: &mut String) {
    use std::fmt::Write as _;

    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Call a built-in with positional arguments. [`None`] is `undefined`, and
    /// it keeps its key, which is what resolution produces.
    fn call(name: &str, arguments: &[Option<Value>]) -> Option<Value> {
        let config = crate::builtins::config();
        let mut parameters = Parameters::new();
        for (index, argument) in arguments.iter().enumerate() {
            parameters.insert(Function::positional_key(index), argument.clone());
        }
        let function = config.functions.get(name).expect("a built-in function");
        let transform = function.transform.as_ref().expect("a transform");
        transform(&parameters, &config)
    }

    fn some(value: Value) -> Option<Value> {
        Some(value)
    }

    fn string(text: &str) -> Option<Value> {
        Some(Value::String(text.to_string()))
    }

    #[test]
    fn every_upstream_function_is_registered() {
        let functions = builtin();
        let names: Vec<&str> = functions.keys().map(String::as_str).collect();
        assert_eq!(names, ["and", "or", "not", "equals", "default", "debug"]);
    }

    /// The corpus's "Truthy things are not false" in miniature: `0` and `""`
    /// are truthy to Markdoc, whatever JavaScript's `Boolean()` says.
    #[test]
    fn truthiness_is_markdocs_rather_than_javascripts() {
        assert_eq!(
            call("and", &[some(Value::Number(0.0)), string("")]),
            some(Value::Boolean(true))
        );
        assert_eq!(
            call(
                "or",
                &[some(Value::Boolean(false)), some(Value::Number(0.0))]
            ),
            some(Value::Boolean(true))
        );
        assert_eq!(call("not", &[string("")]), some(Value::Boolean(false)));
    }

    #[test]
    fn null_and_undefined_are_both_untruthy() {
        assert_eq!(
            call("and", &[some(Value::Null), some(Value::Boolean(true))]),
            some(Value::Boolean(false))
        );
        // An argument that resolved to nothing keeps its key, so `and` still
        // sees two arguments and one of them is untruthy.
        assert_eq!(
            call("and", &[None, some(Value::Boolean(true))]),
            some(Value::Boolean(false))
        );
        assert_eq!(call("not", &[None]), some(Value::Boolean(true)));
    }

    #[test]
    fn an_empty_call_follows_javascript_array_semantics() {
        assert_eq!(call("and", &[]), some(Value::Boolean(true)));
        assert_eq!(call("or", &[]), some(Value::Boolean(false)));
        assert_eq!(call("equals", &[]), some(Value::Boolean(true)));
    }

    #[test]
    fn equals_compares_every_argument_against_the_first() {
        assert_eq!(
            call("equals", &[string("a"), string("a"), string("a")]),
            some(Value::Boolean(true))
        );
        assert_eq!(
            call("equals", &[string("a"), string("b")]),
            some(Value::Boolean(false))
        );
        // `undefined` equals `undefined` and does not equal `null`. The corpus
        // depends on the second half: "Conditional with equals and an undefined
        // variable" is `equals($foo.bar, "test")` with nothing defined, and it
        // must be false rather than a one-argument call that trivially holds.
        assert_eq!(call("equals", &[None, None]), some(Value::Boolean(true)));
        assert_eq!(
            call("equals", &[None, some(Value::Null)]),
            some(Value::Boolean(false))
        );
        assert_eq!(
            call("equals", &[None, string("test")]),
            some(Value::Boolean(false))
        );
    }

    #[test]
    fn default_falls_through_undefined_but_not_null() {
        assert_eq!(call("default", &[None, string("x")]), string("x"));
        assert_eq!(
            call("default", &[some(Value::Null), string("x")]),
            some(Value::Null)
        );
        assert_eq!(call("default", &[string("a"), string("x")]), string("a"));
        // Nothing to fall through to is `undefined`, not null, so the attribute
        // it feeds disappears rather than rendering.
        assert_eq!(call("default", &[]), None);
    }

    #[test]
    fn debug_prints_indented_json() {
        let value = Value::Hash(IndexMap::from([
            ("a".to_string(), Value::Number(1.0)),
            (
                "b".to_string(),
                Value::Array(vec![Value::Boolean(true), Value::Null]),
            ),
        ]));
        assert_eq!(
            call("debug", &[Some(value)]),
            string("{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null\n  ]\n}")
        );
        assert_eq!(call("debug", &[]), None);
        assert_eq!(call("debug", &[string("x\"y")]), string("\"x\\\"y\""));
    }
}
