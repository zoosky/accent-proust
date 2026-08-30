//! The JavaScript coercions the port cannot avoid.
//!
//! Upstream writes an attribute as `escapeHtml(String(v))`, where `v` is
//! whatever the attribute map holds -- a [`Scalar`], or the transformed nodes of
//! a rendered slot. `String(v)` is not a formatting choice; it is ECMAScript's
//! `ToString`, and the conformance corpus grades one of its cases on it:
//! "Rendering HTML with an array attribute" expects `foo="1,2,3"` from the
//! Markdoc source `foo=[1,2,3]`. Rust's `Debug` gives `[1.0, 2.0, 3.0]` and its
//! `Display` gives nothing at all for a sequence, so the coercion has to be
//! written out.
//!
//! Two pieces of it are non-obvious and are the reason this module exists
//! rather than a `format!` at the call site:
//!
//! - **A number is not `{}`.** ECMAScript switches to exponent notation
//!   outside `1e21 > |n| >= 1e-6`; Rust's `Display` never does. `1e21` prints
//!   as `1e+21` there and `1000000000000000000000` here, and `1e-7` prints as
//!   `1e-7` there and `0.0000001` here. [`number`] implements the actual
//!   algorithm.
//! - **`null` inside an array is not `null`.** `String(null)` is `"null"`, but
//!   `Array.prototype.join` renders a `null` element as the empty string.
//!   `String([null])` is `""`, not `"null"`.
//!
//! # Depth
//!
//! Both walks are iterative. A renderable tree is produced by the transform
//! stage from an attacker-controlled document, and this crate promises
//! panic-freedom; a recursive `join` over a nested array would trade that
//! promise for four fewer lines. This is the same reasoning that gave
//! `crate::ast::Node` and [`Tag`](crate::renderable::Tag) their manual
//! iterative `Drop`.

use crate::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar};

/// The string JavaScript gives for any object without its own `toString`.
///
/// Upstream reaches this for a tag in an attribute, which is what a rendered
/// slot is. It is a useless value; it is also the value, and inventing a better
/// one -- JSON, say -- would change bytes upstream emits.
const OBJECT: &str = "[object Object]";

/// ECMAScript `String(value)` for an attribute value.
///
/// An attribute holds a whole subtree, not a scalar, because a rendered slot is
/// stored in the attribute map as its transformed nodes. Upstream types the map
/// as `Record<string, any>` and coerces whatever it finds:
///
/// - one scalar coerces as [`string`];
/// - one tag is an object with no `toString`, so it is `[object Object]`;
/// - a list is a JavaScript array, so it joins with commas under the same rules
///   as [`string`] on an array -- `null` elements contribute nothing.
#[must_use]
pub(crate) fn string_nodes(value: &RenderableTreeNodes) -> String {
    match value {
        RenderableTreeNodes::One(RenderableTreeNode::Scalar(scalar)) => string(scalar),
        RenderableTreeNodes::One(RenderableTreeNode::Tag(_)) => OBJECT.to_owned(),
        RenderableTreeNodes::Many(nodes) => join_nodes(nodes),
    }
}

/// `Array.prototype.join(',')` over a list of nodes.
///
/// Flat: a node is a tag or a scalar, and neither is a list of nodes, so the
/// only nesting is inside a [`Scalar::Array`], which [`join`] handles.
fn join_nodes(nodes: &[RenderableTreeNode]) -> String {
    let mut out = String::new();
    for (index, node) in nodes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match node {
            // `join` renders a `null` element as the empty string.
            RenderableTreeNode::Scalar(Scalar::Null) => {}
            RenderableTreeNode::Scalar(Scalar::Array(nested)) => out.push_str(&join(nested)),
            RenderableTreeNode::Scalar(scalar) => out.push_str(&string(scalar)),
            RenderableTreeNode::Tag(_) => out.push_str(OBJECT),
        }
    }
    out
}

/// ECMAScript `String(value)` for a renderable-tree scalar.
///
/// - `null` becomes `"null"`, `true` becomes `"true"`.
/// - A number goes through [`number`].
/// - An array is joined with commas, with `null` elements contributing nothing
///   and nested arrays flattened, which is `Array.prototype.join`.
/// - An object becomes [`OBJECT`].
#[must_use]
pub(crate) fn string(value: &Scalar) -> String {
    match value {
        Scalar::Null => "null".to_owned(),
        Scalar::Boolean(true) => "true".to_owned(),
        Scalar::Boolean(false) => "false".to_owned(),
        Scalar::Number(n) => number(*n),
        Scalar::String(s) => s.clone(),
        Scalar::Array(items) => join(items),
        Scalar::Object(_) => OBJECT.to_owned(),
    }
}

/// One step of the iterative array join.
enum Step<'a> {
    /// A value to stringify into the output.
    Element(&'a Scalar),
    /// A separator between two elements of the same array.
    Comma,
}

/// `Array.prototype.join(',')`, iteratively.
fn join(items: &[Scalar]) -> String {
    let mut out = String::new();
    let mut stack: Vec<Step<'_>> = Vec::new();
    push_elements(&mut stack, items);

    while let Some(step) = stack.pop() {
        match step {
            Step::Comma => out.push(','),
            // `join` renders `null` and `undefined` as the empty string. This
            // is the arm that makes `String([null])` differ from
            // `String(null)`.
            Step::Element(Scalar::Null) => {}
            // A nested array contributes its own join with no brackets, so its
            // elements go on the same stack rather than into a sub-call.
            Step::Element(Scalar::Array(nested)) => push_elements(&mut stack, nested),
            // Every remaining variant is a leaf, so `string` returns here
            // without reaching `join` again.
            Step::Element(leaf) => out.push_str(&string(leaf)),
        }
    }

    out
}

/// Push `items` so that they pop in order, separated by commas.
fn push_elements<'a>(stack: &mut Vec<Step<'a>>, items: &'a [Scalar]) {
    for (index, item) in items.iter().enumerate().rev() {
        stack.push(Step::Element(item));
        if index > 0 {
            stack.push(Step::Comma);
        }
    }
}

/// ECMAScript `Number::toString` with radix 10.
///
/// Rust's `Display` for `f64` and ECMAScript's `ToString` agree on the digits
/// -- both emit the shortest decimal that round-trips -- and disagree on when
/// to use exponent notation and on what to call the non-finite values. This
/// implements the spec's case analysis (ECMA-262, `Number::toString`, step 5)
/// on top of Rust's `{:e}`, which hands over exactly the `s` and `n` the spec
/// asks for: the shortest digit string, and the decimal exponent.
///
/// The thresholds are `1e21` and `1e-6`; inside them the notation is plain, and
/// outside it is exponential. Every number the tag grammar can write as a
/// literal without an exponent falls inside, which is why the difference is
/// invisible until someone writes a very large or very small one -- and why
/// leaving it to `Display` would have been a divergence nobody noticed.
#[must_use]
pub(crate) fn number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    // Covers negative zero, whose sign ECMAScript drops: `String(-0)` is `"0"`.
    if value == 0.0 {
        return "0".to_owned();
    }
    if value < 0.0 {
        return format!("-{}", number(-value));
    }
    if value.is_infinite() {
        return "Infinity".to_owned();
    }

    // `value` is finite and strictly positive from here, so `{:e}` is always
    // `d[.ddd]e<exp>` with no sign on the mantissa.
    let scientific = format!("{value:e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        // `{:e}` always emits an exponent, so this is unreachable. Returning
        // the plain rendering rather than asserting keeps the promise that this
        // function cannot panic on any input.
        return format!("{value}");
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return format!("{value}");
    };

    // `digits` is the spec's `s`, `k` its length, and `n` its decimal point
    // position: s * 10^(n - k) == value.
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let Ok(k) = i32::try_from(digits.len()) else {
        return format!("{value}");
    };
    let n = exponent + 1;

    // Spec step 5, case 1: `k <= n <= 21`. Integral, no exponent needed, so
    // pad the digits with zeros.
    if (k..=21).contains(&n) {
        let Ok(padding) = usize::try_from(n - k) else {
            return format!("{value}");
        };
        return digits + &"0".repeat(padding);
    }

    // Case 2: `0 < n <= 21`. A decimal point inside the digits. `n < k` also
    // holds, because case 1 took `k <= n`, so the split is always in range.
    if (1..=21).contains(&n) {
        let Ok(point) = usize::try_from(n) else {
            return format!("{value}");
        };
        let Some((whole, fraction)) = digits.split_at_checked(point) else {
            return format!("{value}");
        };
        return format!("{whole}.{fraction}");
    }

    // Case 3: `-6 < n <= 0`. A leading `0.` and up to five zeros.
    if (-5..=0).contains(&n) {
        let Ok(zeros) = usize::try_from(-n) else {
            return format!("{value}");
        };
        return format!("0.{}{digits}", "0".repeat(zeros));
    }

    // Outside both windows: exponent notation, with the exponent written
    // relative to the first digit and always carrying its sign.
    let power = n - 1;
    let sign = if power < 0 { '-' } else { '+' };
    let magnitude = power.unsigned_abs();
    if k == 1 {
        return format!("{digits}e{sign}{magnitude}");
    }
    let Some((first, rest)) = digits.split_at_checked(1) else {
        return format!("{value}");
    };
    format!("{first}.{rest}e{sign}{magnitude}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn integers_print_without_a_decimal_point() {
        assert_eq!(number(0.0), "0");
        assert_eq!(number(1.0), "1");
        assert_eq!(number(2.0), "2");
        assert_eq!(number(42.0), "42");
        assert_eq!(number(100.0), "100");
        assert_eq!(number(1234.0), "1234");
    }

    #[test]
    fn negative_zero_loses_its_sign() {
        assert_eq!(number(-0.0), "0");
    }

    #[test]
    fn negatives_carry_one_minus() {
        assert_eq!(number(-1.0), "-1");
        assert_eq!(number(-1.5), "-1.5");
        assert_eq!(number(-0.1), "-0.1");
    }

    #[test]
    fn fractions_print_their_shortest_round_trip() {
        assert_eq!(number(1.5), "1.5");
        assert_eq!(number(0.1), "0.1");
        assert_eq!(number(123.456), "123.456");
        assert_eq!(number(0.300_000_000_000_000_04), "0.30000000000000004");
    }

    #[test]
    fn the_upper_threshold_is_1e21_and_it_is_exclusive() {
        // `1e20` is the largest that stays plain: 21 digits, `n == 21`.
        assert_eq!(number(1e20), "100000000000000000000");
        assert_eq!(number(1e21), "1e+21");
        assert_eq!(number(1.5e21), "1.5e+21");
        assert_eq!(number(1e100), "1e+100");
    }

    #[test]
    fn the_lower_threshold_is_1e_minus_6_and_it_is_inclusive() {
        assert_eq!(number(1e-6), "0.000001");
        assert_eq!(number(1.5e-6), "0.0000015");
        assert_eq!(number(1e-7), "1e-7");
        assert_eq!(number(1.5e-7), "1.5e-7");
        assert_eq!(number(5e-324), "5e-324");
    }

    #[test]
    fn non_finite_values_use_javascript_names() {
        // Rust would say `NaN`, `inf` and `-inf`.
        assert_eq!(number(f64::NAN), "NaN");
        assert_eq!(number(f64::INFINITY), "Infinity");
        assert_eq!(number(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn the_extremes_of_f64_match_javascript() {
        assert_eq!(number(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(number(f64::MIN_POSITIVE), "2.2250738585072014e-308");
        // `Number.MAX_SAFE_INTEGER`.
        assert_eq!(number(9_007_199_254_740_991.0), "9007199254740991");
    }

    #[test]
    fn scalars_coerce_the_way_string_does() {
        assert_eq!(string(&Scalar::Null), "null");
        assert_eq!(string(&Scalar::Boolean(true)), "true");
        assert_eq!(string(&Scalar::Boolean(false)), "false");
        assert_eq!(string(&Scalar::Number(42.0)), "42");
        assert_eq!(string(&Scalar::String("hi".to_owned())), "hi");
    }

    #[test]
    fn an_object_is_the_useless_string_upstream_writes() {
        let mut object = IndexMap::new();
        object.insert("foo".to_owned(), Scalar::String("bar".to_owned()));
        assert_eq!(string(&Scalar::Object(object)), "[object Object]");
    }

    #[test]
    fn an_array_joins_with_commas() {
        // The corpus case: `foo=[1,2,3]` renders as `foo="1,2,3"`.
        let array = Scalar::Array(vec![
            Scalar::Number(1.0),
            Scalar::Number(2.0),
            Scalar::Number(3.0),
        ]);
        assert_eq!(string(&array), "1,2,3");
    }

    #[test]
    fn an_empty_array_is_the_empty_string() {
        assert_eq!(string(&Scalar::Array(vec![])), "");
        assert_eq!(string(&Scalar::Array(vec![Scalar::Array(vec![])])), "");
    }

    #[test]
    fn null_inside_an_array_contributes_nothing() {
        // The asymmetry that makes this module necessary.
        assert_eq!(string(&Scalar::Null), "null");
        assert_eq!(string(&Scalar::Array(vec![Scalar::Null])), "");
        assert_eq!(
            string(&Scalar::Array(vec![
                Scalar::Null,
                Scalar::Number(1.0),
                Scalar::Null
            ])),
            ",1,"
        );
    }

    #[test]
    fn nested_arrays_flatten() {
        let array = Scalar::Array(vec![
            Scalar::Number(1.0),
            Scalar::Array(vec![Scalar::Number(2.0), Scalar::Number(3.0)]),
            Scalar::Number(4.0),
        ]);
        assert_eq!(string(&array), "1,2,3,4");
    }

    #[test]
    fn an_object_inside_an_array_is_still_useless() {
        let array = Scalar::Array(vec![
            Scalar::String("a".to_owned()),
            Scalar::Object(IndexMap::new()),
        ]);
        assert_eq!(string(&array), "a,[object Object]");
    }

    #[test]
    fn a_deeply_nested_array_does_not_overflow_the_stack() {
        // The reason `join` is iterative. A recursive one dies here.
        let mut value = Scalar::Array(vec![Scalar::Number(1.0)]);
        for _ in 0..100_000 {
            value = Scalar::Array(vec![value]);
        }
        assert_eq!(string(&value), "1");
        dismantle(value);
    }

    /// Take a nested value apart iteratively, so the fixture's own cleanup is
    /// not what fails.
    ///
    /// [`Tag`](crate::renderable::Tag) carries a manual iterative `Drop`, so a
    /// deep *tree* cleans itself up. [`Scalar`] deliberately does not -- it is
    /// the leaf type, and scalars drop where they stand -- so this fixture, and
    /// only this one, has to unwind itself. Without it the test passes and then
    /// aborts with SIGABRT while dropping, which reads as a failure in the code
    /// under test rather than in the fixture. Verified by deleting it.
    fn dismantle(value: Scalar) {
        let mut current = value;
        loop {
            let Scalar::Array(mut items) = current else {
                break;
            };
            match items.pop() {
                Some(inner) => current = inner,
                None => break,
            }
        }
    }
}
