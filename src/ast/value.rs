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

/// # Why the three traversals are written out rather than derived
///
/// The same reason [`Drop`] is, and the reasoning is set out once on
/// [`Scalar`](crate::renderable::Scalar): a derived `Clone`, `PartialEq` or
/// `Debug` recurses per level, [`Value::Array`], [`Value::Hash`] and the
/// parameters inside [`Value::Function`] are public and recursive, and a stack
/// overflow aborts rather than panics.
///
/// [`Variable`] is delegated to its own derive on purpose: a
/// [`PathSegment`] holds a string or a number and never another value, so a
/// path is flat however long it is. [`Function`] is **not** delegated, even
/// though it derives the three: its parameters are values, so calling its
/// derive would re-enter this type once per nested call.
impl Clone for Value {
    fn clone(&self) -> Self {
        let mut plan = vec![ValueStep::Open(self)];
        let mut done: Vec<Value> = Vec::new();

        while let Some(step) = plan.pop() {
            match step {
                ValueStep::Open(value) => match value {
                    Value::Array(items) => {
                        plan.push(ValueStep::Close(value));
                        for item in items.iter().rev() {
                            plan.push(ValueStep::Open(item));
                        }
                    }
                    Value::Hash(entries) => {
                        plan.push(ValueStep::Close(value));
                        for (_, nested) in entries.iter().rev() {
                            plan.push(ValueStep::Open(nested));
                        }
                    }
                    Value::Function(function) => {
                        plan.push(ValueStep::Close(value));
                        for (_, nested) in function.parameters.iter().rev() {
                            plan.push(ValueStep::Open(nested));
                        }
                    }
                    Value::Null => done.push(Value::Null),
                    Value::Boolean(inner) => done.push(Value::Boolean(*inner)),
                    Value::Number(inner) => done.push(Value::Number(*inner)),
                    Value::String(inner) => done.push(Value::String(inner.clone())),
                    Value::Variable(inner) => done.push(Value::Variable(inner.clone())),
                },
                ValueStep::Close(value) => match value {
                    Value::Array(items) => {
                        let start = done.len().saturating_sub(items.len());
                        let children = done.split_off(start);
                        done.push(Value::Array(children));
                    }
                    Value::Hash(entries) => {
                        let start = done.len().saturating_sub(entries.len());
                        let values = done.split_off(start);
                        done.push(Value::Hash(entries.keys().cloned().zip(values).collect()));
                    }
                    Value::Function(function) => {
                        let start = done.len().saturating_sub(function.parameters.len());
                        let values = done.split_off(start);
                        done.push(Value::Function(Function::new(
                            function.name.clone(),
                            function.parameters.keys().cloned().zip(values).collect(),
                        )));
                    }
                    _ => {}
                },
            }
        }

        done.pop().unwrap_or(Value::Null)
    }
}

/// One step of the post-order clone plan: see a value, then rebuild it.
enum ValueStep<'v> {
    Open(&'v Value),
    Close(&'v Value),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        let mut work: Vec<(&Value, &Value)> = vec![(self, other)];
        while let Some((left, right)) = work.pop() {
            match (left, right) {
                (Value::Null, Value::Null) => {}
                (Value::Boolean(a), Value::Boolean(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (Value::Number(a), Value::Number(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (Value::String(a), Value::String(b)) => {
                    if a != b {
                        return false;
                    }
                }
                // Flat: a path segment never holds a value.
                (Value::Variable(a), Value::Variable(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (Value::Array(a), Value::Array(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    work.extend(a.iter().zip(b.iter()));
                }
                (Value::Hash(a), Value::Hash(b)) => {
                    // Unordered, because that is what `IndexMap::eq` does.
                    if a.len() != b.len() {
                        return false;
                    }
                    for (key, value) in a {
                        match b.get(key) {
                            Some(nested) => work.push((value, nested)),
                            None => return false,
                        }
                    }
                }
                (Value::Function(a), Value::Function(b)) => {
                    if a.name != b.name || a.parameters.len() != b.parameters.len() {
                        return false;
                    }
                    for (key, value) in &a.parameters {
                        match b.parameters.get(key) {
                            Some(nested) => work.push((value, nested)),
                            None => return false,
                        }
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alternate = f.alternate();
        let mut stack: Vec<ValueTok<'_>> = vec![ValueTok::Node(self, 0)];

        while let Some(token) = stack.pop() {
            match token {
                ValueTok::Text(text) => f.write_str(text)?,
                ValueTok::Owned(text) => f.write_str(&text)?,
                ValueTok::Line(depth) => {
                    f.write_str("\n")?;
                    for _ in 0..depth {
                        f.write_str("    ")?;
                    }
                }
                ValueTok::Node(value, depth) => {
                    expand_value(f, &mut stack, value, depth, alternate)?;
                }
            }
        }
        Ok(())
    }
}

/// One pending piece of `Debug` output for a [`Value`].
enum ValueTok<'v> {
    Node(&'v Value, usize),
    Text(&'static str),
    Owned(String),
    Line(usize),
}

/// The derive expands a tuple variant's field onto its own line under `{:#?}`,
/// even when the field cannot nest.
fn value_leaf(
    f: &mut std::fmt::Formatter<'_>,
    name: &str,
    body: &str,
    depth: usize,
    alternate: bool,
) -> std::fmt::Result {
    if alternate {
        let pad = "    ".repeat(depth);
        write!(f, "{name}(\n{pad}    {body},\n{pad})")
    } else {
        write!(f, "{name}({body})")
    }
}

/// Re-pad every line after the first, so a block formatted at column zero can
/// be spliced in at `depth`.
fn indent_block(body: &str, depth: usize) -> String {
    let pad = "    ".repeat(depth);
    body.replace('\n', &format!("\n{pad}"))
}

/// Write a value's opening text and queue the rest of it.
#[allow(clippy::too_many_lines)]
fn expand_value<'v>(
    f: &mut std::fmt::Formatter<'_>,
    stack: &mut Vec<ValueTok<'v>>,
    value: &'v Value,
    depth: usize,
    alternate: bool,
) -> std::fmt::Result {
    match value {
        Value::Null => f.write_str("Null"),
        Value::Boolean(inner) => value_leaf(f, "Boolean", &format!("{inner:?}"), depth, alternate),
        Value::Number(inner) => value_leaf(f, "Number", &format!("{inner:?}"), depth, alternate),
        Value::String(inner) => value_leaf(f, "String", &format!("{inner:?}"), depth, alternate),
        // Flat -- a path segment never holds a value -- so its own derive is
        // safe here and is exactly what the outer derive would have called.
        //
        // The delegated block has to be re-indented, though: `{:#?}` formats it
        // as if it started at column zero, and it is being spliced in at
        // `depth + 1`. Without this its inner lines keep the wrong padding,
        // which the parity test catches.
        Value::Variable(inner) => {
            if alternate {
                let block = indent_block(&format!("{inner:#?}"), depth + 1);
                value_leaf(f, "Variable", &block, depth, alternate)
            } else {
                value_leaf(f, "Variable", &format!("{inner:?}"), depth, alternate)
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                return value_leaf(f, "Array", "[]", depth, alternate);
            }
            f.write_str("Array(")?;
            let mut queued: Vec<ValueTok<'v>> = Vec::new();
            if alternate {
                queued.push(ValueTok::Line(depth + 1));
                queued.push(ValueTok::Text("["));
                for item in items {
                    queued.push(ValueTok::Line(depth + 2));
                    queued.push(ValueTok::Node(item, depth + 2));
                    queued.push(ValueTok::Text(","));
                }
                queued.push(ValueTok::Line(depth + 1));
                queued.push(ValueTok::Text("],"));
                queued.push(ValueTok::Line(depth));
                queued.push(ValueTok::Text(")"));
            } else {
                queued.push(ValueTok::Text("["));
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        queued.push(ValueTok::Text(", "));
                    }
                    queued.push(ValueTok::Node(item, depth));
                }
                queued.push(ValueTok::Text("])"));
            }
            stack.extend(queued.into_iter().rev());
            Ok(())
        }
        Value::Hash(entries) => {
            if entries.is_empty() {
                return value_leaf(f, "Hash", "{}", depth, alternate);
            }
            f.write_str("Hash(")?;
            let mut queued: Vec<ValueTok<'v>> = Vec::new();
            if alternate {
                queued.push(ValueTok::Line(depth + 1));
                queued.push(ValueTok::Text("{"));
                for (key, nested) in entries {
                    queued.push(ValueTok::Line(depth + 2));
                    queued.push(ValueTok::Owned(format!("{key:?}: ")));
                    queued.push(ValueTok::Node(nested, depth + 2));
                    queued.push(ValueTok::Text(","));
                }
                queued.push(ValueTok::Line(depth + 1));
                queued.push(ValueTok::Text("},"));
                queued.push(ValueTok::Line(depth));
                queued.push(ValueTok::Text(")"));
            } else {
                queued.push(ValueTok::Text("{"));
                for (index, (key, nested)) in entries.iter().enumerate() {
                    if index > 0 {
                        queued.push(ValueTok::Text(", "));
                    }
                    queued.push(ValueTok::Owned(format!("{key:?}: ")));
                    queued.push(ValueTok::Node(nested, depth));
                }
                queued.push(ValueTok::Text("})"));
            }
            stack.extend(queued.into_iter().rev());
            Ok(())
        }
        // `Function(Function { name: .., parameters: .. })` -- a tuple variant
        // wrapping a struct, so both layers are written out here. Delegating to
        // `Function`'s own derive would re-enter this type per nested call.
        Value::Function(function) => {
            let mut queued: Vec<ValueTok<'v>> = Vec::new();
            let name = format!("{:?}", function.name);
            if alternate {
                f.write_str("Function(")?;
                queued.push(ValueTok::Line(depth + 1));
                queued.push(ValueTok::Text("Function {"));
                queued.push(ValueTok::Line(depth + 2));
                queued.push(ValueTok::Owned(format!("name: {name},")));
                queued.push(ValueTok::Line(depth + 2));
                if function.parameters.is_empty() {
                    queued.push(ValueTok::Text("parameters: {},"));
                } else {
                    queued.push(ValueTok::Text("parameters: {"));
                    for (key, nested) in &function.parameters {
                        queued.push(ValueTok::Line(depth + 3));
                        queued.push(ValueTok::Owned(format!("{key:?}: ")));
                        queued.push(ValueTok::Node(nested, depth + 3));
                        queued.push(ValueTok::Text(","));
                    }
                    queued.push(ValueTok::Line(depth + 2));
                    queued.push(ValueTok::Text("},"));
                }
                queued.push(ValueTok::Line(depth + 1));
                queued.push(ValueTok::Text("},"));
                queued.push(ValueTok::Line(depth));
                queued.push(ValueTok::Text(")"));
            } else {
                write!(f, "Function(Function {{ name: {name}, parameters: ")?;
                if function.parameters.is_empty() {
                    queued.push(ValueTok::Text("{}"));
                } else {
                    queued.push(ValueTok::Text("{"));
                    for (index, (key, nested)) in function.parameters.iter().enumerate() {
                        if index > 0 {
                            queued.push(ValueTok::Text(", "));
                        }
                        queued.push(ValueTok::Owned(format!("{key:?}: ")));
                        queued.push(ValueTok::Node(nested, depth));
                    }
                    queued.push(ValueTok::Text("}"));
                }
                queued.push(ValueTok::Text(" })"));
            }
            stack.extend(queued.into_iter().rev());
            Ok(())
        }
    }
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

/// `Debug` output is observable, so the hand-written emitter is pinned against
/// the derive rather than against a reading of it. See the same pattern on
/// [`Scalar`](crate::renderable::Scalar).
#[cfg(test)]
mod debug_parity {
    use super::*;

    mod mirror {
        // Every field exists to be formatted by the derive and is never read
        // otherwise -- that is the whole point of the type.
        #![allow(dead_code)]

        use indexmap::IndexMap;

        #[derive(Debug)]
        pub struct Function {
            pub name: String,
            pub parameters: IndexMap<String, Value>,
        }

        #[derive(Debug)]
        pub enum Value {
            Null,
            Boolean(bool),
            String(String),
            Number(f64),
            Array(Vec<Value>),
            Hash(IndexMap<String, Value>),
            Function(Function),
            Variable(crate::ast::Variable),
        }
    }

    fn to_mirror(value: &Value) -> mirror::Value {
        match value {
            Value::Null => mirror::Value::Null,
            Value::Boolean(inner) => mirror::Value::Boolean(*inner),
            Value::String(inner) => mirror::Value::String(inner.clone()),
            Value::Number(inner) => mirror::Value::Number(*inner),
            Value::Variable(inner) => mirror::Value::Variable(inner.clone()),
            Value::Array(items) => mirror::Value::Array(items.iter().map(to_mirror).collect()),
            Value::Hash(entries) => mirror::Value::Hash(
                entries
                    .iter()
                    .map(|(key, nested)| (key.clone(), to_mirror(nested)))
                    .collect(),
            ),
            Value::Function(function) => mirror::Value::Function(mirror::Function {
                name: function.name.clone(),
                parameters: function
                    .parameters
                    .iter()
                    .map(|(key, nested)| (key.clone(), to_mirror(nested)))
                    .collect(),
            }),
        }
    }

    fn assert_parity(value: &Value) {
        let reference = to_mirror(value);
        assert_eq!(
            format!("{value:?}"),
            format!("{reference:?}"),
            "plain Debug"
        );
        assert_eq!(
            format!("{value:#?}"),
            format!("{reference:#?}"),
            "alternate Debug"
        );
    }

    fn hash(pairs: Vec<(&str, Value)>) -> Value {
        Value::Hash(
            pairs
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    fn call(name: &str, pairs: Vec<(&str, Value)>) -> Value {
        Value::Function(Function::new(
            name.to_owned(),
            pairs
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        ))
    }

    #[test]
    fn every_value_shape_formats_as_the_derive_would() {
        let shapes = vec![
            Value::Null,
            Value::Boolean(true),
            Value::Number(1.0),
            Value::String("hi".to_owned()),
            Value::String("a \"quote\" and a \\ and a \n".to_owned()),
            Value::Variable(Variable::default()),
            Value::Variable(Variable::new(vec![
                PathSegment::Key("a".to_owned()),
                PathSegment::Index(1.0),
            ])),
            Value::Array(Vec::new()),
            hash(Vec::new()),
            call("f", Vec::new()),
            Value::Array(vec![Value::Null, Value::Boolean(false)]),
            hash(vec![("a", Value::Null), ("b", Value::Number(2.0))]),
            call("f", vec![("0", Value::Null)]),
            call(
                "g",
                vec![("0", Value::Null), ("1", Value::String("x".into()))],
            ),
            // Every recursive edge nested inside another.
            Value::Array(vec![
                hash(vec![("k", call("inner", vec![("0", Value::Null)]))]),
                Value::Array(vec![Value::Array(Vec::new())]),
            ]),
            call(
                "outer",
                vec![("0", call("inner", vec![("0", Value::Null)]))],
            ),
        ];
        for shape in &shapes {
            assert_parity(shape);
        }
    }

    #[test]
    fn a_deep_value_formats_clones_and_compares_without_aborting() {
        // The reason all three are hand-written. A recursive mirror is not
        // built here: it would overflow before the assertions could run.
        let mut value = Value::Null;
        for _ in 0..100_000 {
            value = Value::Array(vec![value]);
        }
        let copy = value.clone();
        assert!(copy == value);
        assert!(format!("{value:?}").starts_with("Array([Array("));
    }

    #[test]
    fn a_deep_value_through_function_parameters_survives_all_three() {
        // Depth reachable with no array or hash in it at all.
        let mut value = Value::Null;
        for _ in 0..100_000 {
            value = call("f", vec![("0", value)]);
        }
        let copy = value.clone();
        assert!(copy == value);
        assert!(format!("{value:?}").starts_with("Function(Function { name: \"f\""));
    }

    #[test]
    fn cloning_preserves_key_order() {
        let original = hash(vec![("z", Value::Null), ("a", Value::Number(1.0))]);
        let copy = original.clone();
        let Value::Hash(entries) = &copy else {
            panic!("expected a hash")
        };
        assert_eq!(entries.keys().collect::<Vec<_>>(), ["z", "a"]);
    }

    #[test]
    fn equality_ignores_hash_order_as_indexmap_does() {
        let left = hash(vec![("a", Value::Null), ("b", Value::Number(1.0))]);
        let right = hash(vec![("b", Value::Number(1.0)), ("a", Value::Null)]);
        assert!(left == right);
        assert!(left != hash(vec![("a", Value::Null)]));
        assert!(call("f", vec![("0", Value::Null)]) != call("g", vec![("0", Value::Null)]));
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
