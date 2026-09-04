//! The types an attribute value may be required to have.
//!
//! Mirrors upstream's `ValidationType` and `CustomAttributeTypeInterface` in
//! `src/types.ts`. Upstream's union is five JavaScript constructors, the five
//! strings naming them, and any class implementing the custom interface --
//! three spellings of two ideas. Here there are two: [`ValidationType`] names a
//! built-in, and [`ValidationType::Custom`] carries a host implementation of
//! [`AttributeType`].
//!
//! # Unions
//!
//! Upstream writes a union as an array, `type: [String, Number]`, and checks it
//! by recursion. [`ValidationType::Union`] is the same thing with a name. The
//! nesting is not flattened, because [`type_to_string`] joins with `" | "` and
//! the joined string is part of an error message that tooling reads.

use std::sync::Arc;

use crate::ast::{ValidationError, Value};
use crate::renderable::Scalar;
use crate::validate::Config;

/// A type an attribute value may be required to have.
///
/// The five built-ins are upstream's `String`, `Number`, `Boolean`, `Object`
/// and `Array`, and they check what upstream's `value.constructor === type`
/// checks: the shape of the parsed value, not what it could be coerced to. A
/// numeric string is not a `Number`.
#[derive(Clone)]
#[non_exhaustive]
pub enum ValidationType {
    /// A string literal.
    String,
    /// A number literal.
    Number,
    /// `true` or `false`.
    Boolean,
    /// A `{key: value}` hash.
    Object,
    /// A `[1, 2]` array.
    Array,
    /// A host-defined type.
    Custom(Arc<dyn AttributeType + Send + Sync + 'static>),
    /// Any one of several types. Upstream's array form.
    Union(Vec<ValidationType>),
}

impl ValidationType {
    /// Whether this is the same type as `other`, as upstream's `===` decides
    /// it.
    ///
    /// Used for one thing only: checking a function's declared return type
    /// against the attribute it feeds. Upstream compares constructor identity,
    /// which has two consequences worth reproducing rather than tidying.
    ///
    /// A **union is never equal to anything**, including an identical union: in
    /// JavaScript the operands there are an array and an element, and an array
    /// is never one of its own elements. A schema whose attribute type is a
    /// union and whose function returns the same union therefore fails the
    /// check upstream, and fails it here.
    ///
    /// A **custom type is equal only to itself**, by pointer, because upstream
    /// compares class identity rather than structure. Two separately
    /// constructed instances of one host type are different types, exactly as
    /// two separately declared JavaScript classes would be.
    #[must_use]
    pub fn is_same_type(&self, other: &ValidationType) -> bool {
        match (self, other) {
            (ValidationType::String, ValidationType::String)
            | (ValidationType::Number, ValidationType::Number)
            | (ValidationType::Boolean, ValidationType::Boolean)
            | (ValidationType::Object, ValidationType::Object)
            | (ValidationType::Array, ValidationType::Array) => true,
            (ValidationType::Custom(a), ValidationType::Custom(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Whether a parsed value has this type, ignoring unions and custom types.
    ///
    /// This is upstream's final line, `value != null && value.constructor ===
    /// type`. `null` has no constructor, so it satisfies no type at all --
    /// which is why the [`Conditional`](crate::validate::schema_types::Conditional)
    /// attribute type exists rather than a `Boolean` that also accepts `null`.
    #[must_use]
    pub fn accepts_shape(&self, value: &Value) -> bool {
        matches!(
            (self, value),
            (ValidationType::String, Value::String(_))
                | (ValidationType::Number, Value::Number(_))
                | (ValidationType::Boolean, Value::Boolean(_))
                | (ValidationType::Object, Value::Hash(_))
                | (ValidationType::Array, Value::Array(_))
        )
    }
}

/// How a type is spelled in an error message.
///
/// Upstream's `typeToString`: a built-in prints its constructor name, a union
/// joins its members with `" | "`, and a custom type prints the class name.
/// The strings are part of the error vocabulary, so they are copied rather than
/// reworded.
#[must_use]
pub fn type_to_string(value_type: &ValidationType) -> String {
    match value_type {
        ValidationType::String => "String".to_string(),
        ValidationType::Number => "Number".to_string(),
        ValidationType::Boolean => "Boolean".to_string(),
        ValidationType::Object => "Object".to_string(),
        ValidationType::Array => "Array".to_string(),
        ValidationType::Custom(custom) => custom.name().to_string(),
        ValidationType::Union(members) => members
            .iter()
            .map(type_to_string)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

/// A host-defined attribute type.
///
/// Mirrors upstream's `CustomAttributeTypeInterface`, which is a class with two
/// optional methods. The optionality is observable, and both methods keep it in
/// a different way.
///
/// # `validate` returning [`None`] is not "no errors"
///
/// Upstream checks whether the instance *has* a `validate` method: if it does,
/// its result is the answer; if it does not, the check falls through to
/// `value.constructor === type`, which compares the value against the class
/// itself and is therefore false for every value a document can produce. So
/// [`None`] here means "this type declares no validation and accepts nothing",
/// and an empty `Some(vec![])` means "valid". Collapsing the two would silently
/// turn a broken schema into a permissive one.
pub trait AttributeType {
    /// The name the error message quotes, standing in for the class name.
    ///
    /// `'static` because upstream reads it off the class, which cannot vary per
    /// instance. Tying it to `&self` would let a host build a name at run time
    /// and put it in an error id's neighbourhood, which is the one part of the
    /// vocabulary that must stay predictable.
    fn name(&self) -> &'static str;

    /// Whether the value is acceptable, or [`None`] if this type declares no
    /// validation at all.
    ///
    /// The default is [`None`], matching a class with no `validate` method.
    fn validate<'a>(
        &self,
        value: &Value,
        config: &Config<'a>,
        name: &str,
    ) -> Option<Vec<ValidationError<'a>>> {
        let _ = (value, config, name);
        None
    }

    /// The value as it should reach the rendered output.
    ///
    /// Upstream's optional `transform`, used by the transformer rather than by
    /// the validator. The default is the identity, which is what a class with no
    /// `transform` method produces: the value passes through unchanged, and an
    /// unresolved reference has no scalar form and drops out.
    fn transform(&self, value: Option<&Value>, config: &Config<'_>) -> Option<Scalar> {
        let _ = config;
        value.and_then(Scalar::from_value)
    }
}

impl std::fmt::Debug for ValidationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&type_to_string(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn built_in_types_check_the_parsed_shape() {
        assert!(ValidationType::String.accepts_shape(&Value::String("x".into())));
        assert!(!ValidationType::Number.accepts_shape(&Value::String("1".into())));
        assert!(ValidationType::Object.accepts_shape(&Value::Hash(IndexMap::new())));
        assert!(ValidationType::Array.accepts_shape(&Value::Array(Vec::new())));
        // `null` has no constructor, so it is of no type.
        assert!(!ValidationType::Boolean.accepts_shape(&Value::Null));
    }

    #[test]
    fn a_union_prints_as_upstream_joins_it() {
        let union = ValidationType::Union(vec![ValidationType::String, ValidationType::Number]);
        assert_eq!(type_to_string(&union), "String | Number");
    }

    #[test]
    fn identity_is_javascript_identity() {
        assert!(ValidationType::String.is_same_type(&ValidationType::String));
        assert!(!ValidationType::String.is_same_type(&ValidationType::Number));

        // Two identical unions are still not the same type, because upstream
        // compares an array against an element.
        let a = ValidationType::Union(vec![ValidationType::String]);
        let b = ValidationType::Union(vec![ValidationType::String]);
        assert!(!a.is_same_type(&b));
    }

    #[test]
    fn a_custom_type_is_equal_only_to_itself() {
        struct Link;
        impl AttributeType for Link {
            fn name(&self) -> &'static str {
                "Link"
            }
        }

        let one: Arc<dyn AttributeType + Send + Sync> = Arc::new(Link);
        let same = ValidationType::Custom(Arc::clone(&one));
        let other = ValidationType::Custom(Arc::new(Link));
        assert!(ValidationType::Custom(one).is_same_type(&same));
        assert!(!same.is_same_type(&other));
        assert_eq!(type_to_string(&same), "Link");
    }

    #[test]
    fn a_type_with_no_validate_method_accepts_nothing() {
        struct Bare;
        impl AttributeType for Bare {
            fn name(&self) -> &'static str {
                "Bare"
            }
        }
        let config = Config::new();
        assert!(
            Bare.validate(&Value::String("x".into()), &config, "k")
                .is_none()
        );
    }
}
