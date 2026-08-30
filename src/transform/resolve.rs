//! Resolving a parsed [`Value`]: variables read, functions called.
//!
//! Ports the `resolve` that upstream splits across three files: `ast/base.ts`
//! (the recursive walk), `ast/variable.ts` (`$foo.bar` against
//! `config.variables`), and `ast/function.ts` (`f(1, x=2)` against
//! `config.functions`). Goal A deliberately left all three out of the AST,
//! because each needs a `Config` and the AST is below the stage that owns one.
//!
//! # Resolution stays in the value lattice
//!
//! A resolved value is still a [`Value`] -- with every [`Value::Variable`] and
//! [`Value::Function`] replaced by what it stands for. Turning it into a
//! [`Scalar`](crate::renderable::Scalar) happens once, at the attribute
//! boundary, because that is where an attribute type may rewrite it first.
//! `Scalar::from_value` is the conversion, and it answers [`None`] for anything
//! still unresolved, which is what makes "this never resolved" impossible to
//! mistake for data.
//!
//! # Why `Option<Value>` and not `Value`
//!
//! JavaScript has two empty values and Markdoc uses both. `null` is a value: an
//! attribute resolving to it is rendered. `undefined` is the absence of one: an
//! attribute resolving to it falls back to the schema's default and is then
//! dropped, `default($a, "x")` returns `"x"` for it and not for `null`, and
//! `{% if %}` treats both as false but for different reasons. Collapsing them
//! would be a silent behaviour change at four separate sites, so [`None`] is
//! `undefined` throughout this module.
//!
//! # When resolution happens
//!
//! Upstream resolves the whole tree in one pass and then transforms it
//! (`index.ts`: `transform` calls `resolve(nodes, config)` before
//! `content.transform(config)`). Here resolution is lazy: each attribute is
//! resolved at the moment the transform stage reads it.
//!
//! The two agree, because upstream resolves every node exactly once under the
//! configuration in force for it, and so does this -- including the one case
//! where the configuration changes mid-tree, `{% partial %}`, which upstream
//! handles by re-resolving the partial's own AST under a scoped config. What
//! lazy resolution avoids is building a second copy of the tree in order to
//! throw it away, and a `Node` whose attributes are sometimes resolved and
//! sometimes not, which is a type that lies about itself.

use indexmap::IndexMap;

use crate::ast::{PathSegment, Value, Variable};
use crate::validate::Config;

/// How deep a value may nest before resolution stops descending.
///
/// The same bound and the same reason as the grammar's `MAX_VALUE_DEPTH`
/// (`DIVERGENCES.md` entry 9): a stack overflow in Rust aborts the process and
/// cannot be caught, so a promise of panic-freedom over arbitrary input needs a
/// limit rather than a hope. The check is repeated here rather than inherited
/// from parse time, because a variable resolves to a value the *host* supplied
/// and the grammar never saw it.
pub const MAX_RESOLVE_DEPTH: usize = 64;

/// Resolve one value against a configuration.
///
/// [`None`] is JavaScript's `undefined`: an unset variable, a path that leads
/// nowhere, or a call to a function that is not registered.
#[must_use]
pub fn resolve(value: &Value, config: &Config<'_>) -> Option<Value> {
    resolve_at(value, config, 0)
}

fn resolve_at(value: &Value, config: &Config<'_>, depth: usize) -> Option<Value> {
    if depth > MAX_RESOLVE_DEPTH {
        return None;
    }
    match value {
        Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) => {
            Some(value.clone())
        }
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                // An element resolving to `undefined` becomes `null`. That is
                // what upstream's output shows: the element stays `undefined` in
                // the array, and every consumer of the tree -- JSON, the React
                // shim, the HTML renderer -- writes it as null or as nothing.
                .map(|item| resolve_at(item, config, depth + 1).unwrap_or(Value::Null))
                .collect(),
        )),
        Value::Hash(entries) => {
            let mut out = IndexMap::with_capacity(entries.len());
            for (key, value) in entries {
                // An entry resolving to `undefined` is dropped, which is what
                // `JSON.stringify` does to it and therefore what upstream's
                // graded output contains.
                if let Some(resolved) = resolve_at(value, config, depth + 1) {
                    out.insert(key.clone(), resolved);
                }
            }
            Some(Value::Hash(out))
        }
        Value::Variable(variable) => resolve_variable(variable, config),
        Value::Function(function) => {
            let declared = config.functions.get(function.name.as_str())?;
            let transform = declared.transform.as_ref()?;
            let mut parameters = IndexMap::with_capacity(function.parameters.len());
            for (key, value) in &function.parameters {
                // Every parameter keeps its key, whether or not it resolved.
                // Upstream's grammar always sets one and `resolve` maps over the
                // entries, so `and`, `or` and `equals` -- which read
                // `Object.values(parameters)` -- count an undefined argument.
                // Dropping the key would silently change the arity they see.
                parameters.insert(key.clone(), resolve_at(value, config, depth + 1));
            }
            transform(&parameters, config)
        }
    }
}

/// Resolve `$foo.bar[0]` against `config.variables`.
///
/// Upstream folds the path with `obj[key]`, which is JavaScript property
/// access: it reads object keys and array indices, and -- because everything
/// there is an object -- string lengths and prototype members besides. This
/// reads objects by key and arrays by index and answers `undefined` for
/// anything else, which `DIVERGENCES.md` declares.
#[must_use]
pub fn resolve_variable(variable: &Variable, config: &Config<'_>) -> Option<Value> {
    let variables = config.variables.as_ref()?;
    let mut current: Option<&Value> = None;
    for (position, segment) in variable.path.iter().enumerate() {
        current = match (position, current) {
            // The first step indexes the variables map itself, which upstream
            // spells as the `variables` object.
            (0, _) => match segment {
                PathSegment::Key(key) => variables.get(key.as_str()),
                PathSegment::Index(index) => variables.get(&js_key(*index)),
            },
            (_, Some(value)) => step(value, segment),
            (_, None) => return None,
        };
    }
    // A variable may itself hold a reference -- a host is free to put one there
    // -- so what it points at is resolved in turn, bounded like everything else.
    current.and_then(|value| resolve_at(value, config, 1))
}

/// One property access into a resolved value.
fn step<'v>(value: &'v Value, segment: &PathSegment) -> Option<&'v Value> {
    match (value, segment) {
        (Value::Hash(entries), PathSegment::Key(key)) => entries.get(key.as_str()),
        (Value::Hash(entries), PathSegment::Index(index)) => entries.get(&js_key(*index)),
        (Value::Array(items), PathSegment::Index(index)) => {
            index_of(*index).and_then(|index| items.get(index))
        }
        // `$a.0` on an array: the grammar reads a bare `0` after a dot as an
        // identifier, and JavaScript coerces it back to an index.
        (Value::Array(items), PathSegment::Key(key)) => {
            key.parse::<usize>().ok().and_then(|index| items.get(index))
        }
        _ => None,
    }
}

/// A numeric path step as an array index, or [`None`] if it is not one.
fn index_of(number: f64) -> Option<usize> {
    if number < 0.0 || number.fract() != 0.0 || !number.is_finite() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "guarded above: finite, non-negative, integral"
    )]
    Some(number as usize)
}

/// A number as JavaScript spells it when it is used as an object key.
fn js_key(number: f64) -> String {
    if number.fract() == 0.0 && number.is_finite() {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "integral and finite, checked immediately above"
        )]
        return (number as i64).to_string();
    }
    number.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Function;
    use crate::builtins;

    fn config() -> Config<'static> {
        let mut config = builtins::config();
        config.variables = Some(IndexMap::from([
            ("foo".to_string(), Value::String("bar".to_string())),
            (
                "nested".to_string(),
                Value::Hash(IndexMap::from([(
                    "this is a test".to_string(),
                    Value::String("bar".to_string()),
                )])),
            ),
            (
                "list".to_string(),
                Value::Array(vec![Value::Number(1.0), Value::Number(2.0)]),
            ),
        ]));
        config
    }

    fn variable(path: &[&str]) -> Value {
        Value::Variable(Variable::new(
            path.iter()
                .map(|step| PathSegment::Key((*step).to_string()))
                .collect(),
        ))
    }

    #[test]
    fn a_variable_resolves_to_its_value() {
        assert_eq!(
            resolve(&variable(&["foo"]), &config()),
            Some(Value::String("bar".to_string()))
        );
    }

    #[test]
    fn a_missing_variable_is_undefined_rather_than_null() {
        assert_eq!(resolve(&variable(&["nope"]), &config()), None);
    }

    /// With no `variables` at all, every reference is `undefined` rather than an
    /// error: upstream's `path.reduce((obj = {}, key) => obj[key], undefined)`
    /// defaults its way to the same answer.
    #[test]
    fn no_variables_at_all_resolves_to_undefined() {
        assert_eq!(resolve(&variable(&["foo"]), &Config::new()), None);
    }

    #[test]
    fn a_string_key_indexes_a_hash() {
        assert_eq!(
            resolve(&variable(&["nested", "this is a test"]), &config()),
            Some(Value::String("bar".to_string()))
        );
    }

    #[test]
    fn a_numeric_step_indexes_an_array() {
        let path = Value::Variable(Variable::new(vec![
            PathSegment::Key("list".to_string()),
            PathSegment::Index(1.0),
        ]));
        assert_eq!(resolve(&path, &config()), Some(Value::Number(2.0)));
        let past_end = Value::Variable(Variable::new(vec![
            PathSegment::Key("list".to_string()),
            PathSegment::Index(9.0),
        ]));
        assert_eq!(resolve(&past_end, &config()), None);
    }

    /// `DIVERGENCES.md`: upstream throws a `TypeError` here and takes the whole
    /// transform with it. `undefined` is the same answer an author gets for any
    /// other path that leads nowhere.
    #[test]
    fn a_path_through_a_string_is_undefined_rather_than_a_crash() {
        assert_eq!(resolve(&variable(&["foo", "length"]), &config()), None);
    }

    /// Ported from `functions/index.test.ts`, "equals function".
    #[test]
    fn equals_matches_a_variable_against_a_literal() {
        let call = |value: &str| {
            let mut config = builtins::config();
            config.variables = Some(IndexMap::from([(
                "foo".to_string(),
                Value::String(value.to_string()),
            )]));
            let mut parameters = IndexMap::new();
            parameters.insert(
                Function::positional_key(0),
                Value::Variable(Variable::new(vec![PathSegment::Key("foo".to_string())])),
            );
            parameters.insert(
                Function::positional_key(1),
                Value::String("bar".to_string()),
            );
            resolve(
                &Value::Function(Function::new("equals".to_string(), parameters)),
                &config,
            )
        };
        assert_eq!(call("bar"), Some(Value::Boolean(true)));
        assert_eq!(call("baz"), Some(Value::Boolean(false)));
    }

    #[test]
    fn an_unregistered_function_is_undefined() {
        let call = Value::Function(Function::new("nope".to_string(), IndexMap::new()));
        assert_eq!(resolve(&call, &builtins::config()), None);
    }

    #[test]
    fn deep_nesting_terminates_rather_than_overflowing() {
        let mut value = Value::Number(1.0);
        for _ in 0..500 {
            value = Value::Array(vec![value]);
        }
        // The answer past the bound is `null`, not a crash and not a truncated
        // tree that claims to be complete.
        assert!(resolve(&value, &Config::new()).is_some());
    }
}
