//! The value lattice: everything a tag attribute, a function parameter, an
//! array element, or a hash entry can hold.
//!
//! Mirrors upstream `src/ast/variable.ts` and `src/ast/function.ts`, plus the
//! untyped `any` that upstream's PEG grammar returns for literals. TypeScript
//! never has to name that set -- a value is "whatever the parser produced" --
//! but Rust does, so [`Value`] is the name, and the grammar in
//! [`crate::grammar`] is the only thing in this crate that builds one.
//!
//! # What is deliberately missing
//!
//! Upstream's `Variable` and `Function` each carry a `resolve(config)` that
//! reads variables and functions out of a `Config`. Resolution belongs to the
//! transform stage, which owns `Config`; porting it here would pull the whole
//! configuration surface into the leaf type before there is anything to
//! resolve against. What lands here is the data shape and its construction --
//! nothing that needs to look at the world.
//!
//! # Order
//!
//! Hashes and function parameters are [`IndexMap`], never `HashMap`. Attribute
//! and hash order is authored order, because rendered output has to be
//! byte-reproducible across runs and a hash-ordered map makes that a coin
//! flip.

use indexmap::IndexMap;

/// A value produced by the tag grammar.
///
/// The variants are upstream's `Value` alternation in
/// `src/grammar/tag.pegjs`, in the order the grammar tries them. That order is
/// load-bearing at parse time -- `null` is matched before an identifier could
/// be, and a function call is matched before a variable -- but here it is only
/// documentation, because by the time you hold a `Value` the choice is made.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A double-quoted string, unescaped: `\"`, `\\`, `\n`, `\r` and `\t` are
    /// the only sequences the grammar recognises.
    String(String),
    /// A number.
    ///
    /// There is one numeric type, and it is `f64`, because upstream parses
    /// every literal with `parseFloat`. An integer literal is not a distinct
    /// kind of value, so `1` and `1.0` are the same `Value` and any port that
    /// distinguishes them has already diverged.
    Number(f64),
    /// `[1, 2, 3]`.
    ///
    /// Also what an `@`-prefixed variable path parses to: upstream's grammar
    /// returns a bare JavaScript array of path steps for `@foo.bar`, not a
    /// [`Variable`], and a bare array of strings and numbers is exactly this.
    Array(Vec<Value>),
    /// `{key: "value"}`, in authored order.
    Hash(IndexMap<String, Value>),
    /// A function call, `f(1, x=2)`.
    Function(Function),
    /// A `$`-prefixed variable reference, `$foo.bar[0]`.
    Variable(Variable),
}

impl Value {
    /// Reports whether JavaScript would treat this value as truthy.
    ///
    /// The grammar needs this in exactly one place, and it is not cosmetic.
    /// Upstream's `TagOpen` action unshifts the primary attribute under a bare
    /// `if (primary)`, so a primary value of `null`, `false`, `0` or `""` is
    /// parsed and then dropped: `{% foo 0 %}` is a tag with no attributes at
    /// all. Port the action without its truthiness test and you invent an
    /// attribute upstream does not produce.
    ///
    /// Empty collections are truthy, as they are in JavaScript.
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Boolean(b) => *b,
            Value::String(s) => !s.is_empty(),
            // `-0.0 != 0.0` is false, so negative zero is falsy, as in
            // JavaScript. NaN is unreachable from the grammar, and falsy.
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::Array(_) | Value::Hash(_) | Value::Function(_) | Value::Variable(_) => true,
        }
    }
}

/// One step of a variable path.
///
/// `$foo.bar[0]` is `[Key("foo"), Key("bar"), Index(0.0)]`. Upstream stores
/// the same two shapes in one JavaScript array, `(string | number)[]`, and the
/// formatter reprints a step by asking which it is.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum PathSegment {
    /// A named step: `.name`, or the string form `["name"]`.
    ///
    /// Both spellings collapse to a string here, as they do upstream. Which
    /// one to reprint is the formatter's decision -- it emits `.name` when the
    /// key is a valid identifier and `["name"]` when it is not -- so keeping
    /// them apart in the AST would record a syntactic accident the formatter
    /// then has to ignore.
    Key(String),
    /// A numeric step: `[0]`.
    ///
    /// A number rather than an index type, because the grammar accepts
    /// `$a[1.5]` and `$a[-1]` and upstream stores whatever `parseFloat`
    /// returned. Narrowing it here would reject input upstream accepts.
    Index(f64),
}

/// A `$`-prefixed variable reference: `$foo`, `$foo.bar`, `$foo[0].bar`.
///
/// Mirrors upstream `src/ast/variable.ts`, minus `resolve` (see the module
/// docs). An `@`-prefixed path is *not* one of these: the grammar returns it
/// as a plain [`Value::Array`], which is what upstream does too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Variable {
    /// The path, in source order.
    pub path: Vec<PathSegment>,
}

impl Variable {
    /// Creates a variable reference from its path.
    #[must_use]
    pub fn new(path: Vec<PathSegment>) -> Self {
        Self { path }
    }
}

/// A function call: a name and its parameters.
///
/// Mirrors upstream `src/ast/function.ts`, minus `resolve` (see the module
/// docs).
///
/// # How parameters are keyed
///
/// Upstream builds one JavaScript object for both named and positional
/// parameters: `parameters[name || index] = value`. A positional parameter
/// therefore keys on its index in the argument list, coerced to a string, and
/// this port keeps that -- `f(1, x=2)` is `{"0": 1, "x": 2}`. Two consequences
/// are worth stating rather than leaving to be found:
///
/// - The index is the position in the *whole* argument list, not among the
///   unnamed ones. `f(x=1, 2)` is `{"x": 1, "1": 2}`.
/// - Because upstream's key space is strings, a parameter literally named `0`
///   collides with the first positional one. `Identifier` accepts digits, so
///   `f(0=1)` is legal input and reaches the same key as `f(1)`. Splitting the
///   two into an enum would fix a collision upstream has, which is a
///   divergence dressed as a tidy-up: any built-in function that reads
///   `parameters["0"]` would then miss a value upstream finds there.
///
/// Use [`Function::positional_key`] rather than spelling the coercion out at
/// each call site.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// The function name, as written.
    pub name: String,
    /// The parameters, in authored order.
    ///
    /// Upstream's order is JavaScript object order, which hoists integer-like
    /// keys ahead of named ones; this is authored order instead. See
    /// `DIVERGENCES.md`.
    pub parameters: IndexMap<String, Value>,
}

impl Function {
    /// Creates a function call from its name and parameters.
    #[must_use]
    pub fn new(name: String, parameters: IndexMap<String, Value>) -> Self {
        Self { name, parameters }
    }

    /// Returns the parameter key for a positional argument at `index`.
    ///
    /// This is the string coercion JavaScript performs when upstream writes
    /// `parameters[index] = value`.
    #[must_use]
    pub fn positional_key(index: usize) -> String {
        index.to_string()
    }
}

/// Dropping a value is iterative, for the reason dropping a
/// [`Node`](crate::ast::Node) is.
///
/// [`Node`](crate::ast::Node) and [`Tag`](crate::renderable::Tag) carry manual
/// `Drop` implementations because their nesting is attacker-controlled. This
/// type's nesting is not: every value the crate itself builds comes out of the
/// value grammar, which is bounded at
/// [`MAX_VALUE_DEPTH`](crate::grammar::MAX_VALUE_DEPTH) (`DIVERGENCES.md` entry
/// 9), so no document can produce a value deep enough to overflow a recursive
/// drop.
///
/// **That bound covers construction inside the crate and does not bind a
/// caller.** `Value` is public and three of its variants are recursive --
/// [`Value::Array`], [`Value::Hash`] and the parameters inside
/// [`Value::Function`] -- so a host can assemble one of any depth through the
/// public API. A derived drop would then abort the process, and an abort cannot
/// be caught, which makes the crate's panic-freedom promise untrue for input the
/// crate never parsed. Guarding here is what keeps that promise a property of
/// the type rather than of one code path.
///
/// The cost, stated because it is invisible until someone hits it: a type with a
/// manual `Drop` cannot have a field moved out of it, so taking ownership of a
/// variant's contents needs [`std::mem::take`] or [`std::mem::replace`] rather
/// than a partial move. That is the same tax [`Node`](crate::ast::Node) charges,
/// paid for the same reason.
impl Drop for Value {
    fn drop(&mut self) {
        let mut pending: Vec<Value> = Vec::new();
        unlink(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            unlink(&mut value, &mut pending);
            // `value` is dropped here already emptied, so this recurses once.
        }
    }
}

/// Move every value directly inside `value` onto `pending`, leaving it empty.
///
/// [`Value::Variable`] is not walked: a [`PathSegment`] holds a string or a
/// number and never another value, so a variable path is flat however long it
/// is.
fn unlink(value: &mut Value, pending: &mut Vec<Value>) {
    match value {
        Value::Array(items) => pending.append(items),
        Value::Hash(entries) => pending.extend(entries.drain(..).map(|(_, value)| value)),
        Value::Function(function) => {
            pending.extend(function.parameters.drain(..).map(|(_, value)| value));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javascript_truthiness_of_primary_values() {
        assert!(!Value::Null.is_truthy());
        assert!(!Value::Boolean(false).is_truthy());
        assert!(Value::Boolean(true).is_truthy());
        assert!(!Value::String(String::new()).is_truthy());
        assert!(Value::String("0".to_string()).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(!Value::Number(-0.0).is_truthy());
        assert!(Value::Number(1.0).is_truthy());
        assert!(Value::Array(Vec::new()).is_truthy());
        assert!(Value::Hash(IndexMap::new()).is_truthy());
        assert!(Value::Variable(Variable::default()).is_truthy());
    }

    #[test]
    fn dropping_a_deep_array_does_not_abort() {
        // The value grammar bounds nesting at MAX_VALUE_DEPTH, so the crate
        // never parses one this deep. `Value` is public and `Array` is
        // recursive, so a host can build it anyway, and a derived drop aborts
        // -- which no caller can catch.
        let mut value = Value::Null;
        for _ in 0..100_000 {
            value = Value::Array(vec![value]);
        }
        drop(value);
    }

    #[test]
    fn dropping_a_deep_hash_does_not_abort() {
        let mut value = Value::Null;
        for _ in 0..100_000 {
            let mut hash = IndexMap::new();
            hash.insert("k".to_string(), value);
            value = Value::Hash(hash);
        }
        drop(value);
    }

    #[test]
    fn dropping_deeply_nested_function_parameters_does_not_abort() {
        // A function's parameters are values, so depth is reachable through a
        // call with no array or hash in it at all.
        let mut value = Value::Null;
        for _ in 0..100_000 {
            let mut parameters = IndexMap::new();
            parameters.insert("0".to_string(), value);
            value = Value::Function(Function::new("f".to_string(), parameters));
        }
        drop(value);
    }

    #[test]
    fn hash_order_is_authored_order() {
        let mut hash = IndexMap::new();
        hash.insert("z".to_string(), Value::Number(1.0));
        hash.insert("a".to_string(), Value::Number(2.0));
        let keys: Vec<&str> = hash.keys().map(String::as_str).collect();
        assert_eq!(keys, ["z", "a"]);
    }

    #[test]
    fn a_repeated_hash_key_keeps_its_first_position_and_last_value() {
        // JavaScript object assignment behaves this way, and `IndexMap::insert`
        // matches it. The grammar merges hash entries with `Object.assign`, so
        // any difference here would show up as reordered output.
        let mut hash = IndexMap::new();
        hash.insert("a".to_string(), Value::Number(1.0));
        hash.insert("b".to_string(), Value::Number(2.0));
        hash.insert("a".to_string(), Value::Number(3.0));
        let entries: Vec<(&str, &Value)> = hash.iter().map(|(k, v)| (k.as_str(), v)).collect();
        assert_eq!(
            entries,
            [("a", &Value::Number(3.0)), ("b", &Value::Number(2.0))]
        );
    }

    #[test]
    fn positional_keys_are_decimal_strings() {
        assert_eq!(Function::positional_key(0), "0");
        assert_eq!(Function::positional_key(12), "12");
    }
}
