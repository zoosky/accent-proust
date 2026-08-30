//! The three attribute types Markdoc itself defines.
//!
//! Mirrors upstream `src/schema-types/`. They are *types*, not schemas, which
//! is why they live with the validator rather than with the built-in node and
//! tag schemas that use them: a host writing its own schema reaches for [`Id`]
//! or [`Class`] the same way the built-ins do.
//!
//! Two of the three exist because JavaScript's `value.constructor === type`
//! check cannot express what Markdoc needs. [`Class`] accepts a string or an
//! object, because `class="a b"` and `class={a: true, b: false}` are both
//! valid; [`Conditional`] accepts a boolean, an object, or nothing at all,
//! because `{% if $undefined %}` must be a legal document rather than a type
//! error. Both are unions that a single constructor cannot name.
//!
//! [`Id`] is different: it is a *value* check wearing a type's clothes, and it
//! reports `attribute-value-invalid` rather than `attribute-type-invalid`
//! because of it.

use crate::ast::{ErrorLevel, ValidationError, Value};
use crate::renderable::Scalar;
use crate::validate::{AttributeType, Config};

/// `class`: a string, or an object whose truthy keys are the class names.
///
/// Mirrors upstream `schema-types/class.ts`. The object form is what makes
/// conditional classes expressible in an annotation:
/// `{% class={active: $isActive} %}`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Class;

impl AttributeType for Class {
    fn name(&self) -> &'static str {
        "Class"
    }

    fn validate<'a>(
        &self,
        value: &Value,
        _config: &Config<'a>,
        name: &str,
    ) -> Option<Vec<ValidationError<'a>>> {
        // Upstream tests `typeof value === 'string' || typeof value ===
        // 'object'`, and `typeof null` is `'object'` in JavaScript -- a wart,
        // but one the check inherits, so `class=null` is accepted.
        match value {
            Value::String(_)
            | Value::Hash(_)
            | Value::Array(_)
            | Value::Null
            | Value::Function(_)
            | Value::Variable(_) => Some(Vec::new()),
            Value::Boolean(_) | Value::Number(_) => Some(vec![ValidationError::new(
                "attribute-type-invalid",
                ErrorLevel::Error,
                format!("Attribute '{name}' must be type 'string | object'"),
            )]),
        }
    }

    fn transform(&self, value: Option<&Value>, _config: &Config<'_>) -> Option<Scalar> {
        match value {
            // Upstream returns the value unchanged when it is falsy or a
            // string, which covers `undefined`, `null`, `""` and `class="a b"`.
            None => None,
            Some(value @ (Value::String(_) | Value::Null)) => Scalar::from_value(value),
            Some(Value::Hash(entries)) => {
                let names: Vec<&str> = entries
                    .iter()
                    .filter(|(_, value)| value.is_truthy())
                    .map(|(key, _)| key.as_str())
                    .collect();
                Some(Scalar::String(names.join(" ")))
            }
            // Anything else is falsy-or-passthrough in upstream's `if (!value ||
            // typeof value === 'string') return value`, and `Object.entries` of
            // a non-object yields nothing, so the join is empty.
            Some(value) if !value.is_truthy() => Scalar::from_value(value),
            Some(_) => Some(Scalar::String(String::new())),
        }
    }
}

/// `id`: a string starting with a letter.
///
/// Mirrors upstream `schema-types/id.ts`. The restriction is not decoration: an
/// id becomes an HTML fragment target and a CSS selector, and a leading digit
/// makes the selector invalid without making the document look wrong.
#[derive(Clone, Copy, Debug, Default)]
pub struct Id;

impl AttributeType for Id {
    fn name(&self) -> &'static str {
        "Id"
    }

    fn validate<'a>(
        &self,
        value: &Value,
        _config: &Config<'a>,
        _name: &str,
    ) -> Option<Vec<ValidationError<'a>>> {
        // Upstream's `value.match(/^[a-zA-Z]/)`, which is ASCII-only and
        // anchored at the start. Written out rather than reached through a
        // regular expression engine this crate does not carry.
        let valid = match value {
            Value::String(text) => text
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic()),
            _ => false,
        };
        if valid {
            return Some(Vec::new());
        }
        Some(vec![ValidationError::new(
            "attribute-value-invalid",
            ErrorLevel::Error,
            "The 'id' attribute must start with a letter",
        )])
    }
}

/// The condition of an `{% if %}` or `{% else %}` tag.
///
/// Mirrors upstream `schema-types/conditional.ts`, whose class is named
/// `ConditionalAttributeType`. Accepts a boolean, an object, or nothing --
/// "nothing" being the point: `{% if $notSet %}` has to be a document that
/// renders its else branch, not a document that fails validation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditional;

impl AttributeType for Conditional {
    fn name(&self) -> &'static str {
        "ConditionalAttributeType"
    }

    fn validate<'a>(
        &self,
        value: &Value,
        _config: &Config<'a>,
        name: &str,
    ) -> Option<Vec<ValidationError<'a>>> {
        // `typeof null === 'object'`, so upstream's three-way test accepts
        // null twice over. Arrays, functions and variables are objects too.
        match value {
            Value::Boolean(_)
            | Value::Null
            | Value::Hash(_)
            | Value::Array(_)
            | Value::Function(_)
            | Value::Variable(_) => Some(Vec::new()),
            Value::String(_) | Value::Number(_) => Some(vec![ValidationError::new(
                "attribute-type-invalid",
                ErrorLevel::Error,
                format!(
                    "Attribute '{name}' must be type 'boolean | object' \
                     (null or undefined are also allowed)"
                ),
            )]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn errors(result: Option<Vec<ValidationError<'_>>>) -> Vec<ValidationError<'_>> {
        result.expect("these types all declare validation")
    }

    #[test]
    fn class_accepts_a_string_or_an_object() {
        let config = Config::new();
        assert!(errors(Class.validate(&Value::String("a b".into()), &config, "class")).is_empty());
        assert!(errors(Class.validate(&Value::Hash(IndexMap::new()), &config, "class")).is_empty());
        let rejected = errors(Class.validate(&Value::Number(1.0), &config, "class"));
        assert_eq!(
            rejected.first().map(|e| e.id),
            Some("attribute-type-invalid")
        );
        assert_eq!(
            rejected.first().map(|e| e.message.as_str()),
            Some("Attribute 'class' must be type 'string | object'")
        );
    }

    #[test]
    fn class_joins_the_truthy_keys_of_an_object() {
        let config = Config::new();
        let mut hash = IndexMap::new();
        hash.insert("active".to_string(), Value::Boolean(true));
        hash.insert("hidden".to_string(), Value::Boolean(false));
        hash.insert("large".to_string(), Value::Number(1.0));
        assert_eq!(
            Class.transform(Some(&Value::Hash(hash)), &config),
            Some(Scalar::String("active large".to_string()))
        );
        // A string passes through, which is the common case.
        assert_eq!(
            Class.transform(Some(&Value::String("a b".into())), &config),
            Some(Scalar::String("a b".to_string()))
        );
    }

    #[test]
    fn an_id_must_start_with_an_ascii_letter() {
        let config = Config::new();
        assert!(errors(Id.validate(&Value::String("bar".into()), &config, "id")).is_empty());
        for rejected in ["1bar", "#bar", "", "\u{e9}bar"] {
            let found = errors(Id.validate(&Value::String(rejected.into()), &config, "id"));
            assert_eq!(
                found.first().map(|e| e.id),
                Some("attribute-value-invalid"),
                "{rejected:?} should be rejected"
            );
            assert_eq!(
                found.first().map(|e| e.message.as_str()),
                Some("The 'id' attribute must start with a letter")
            );
        }
    }

    #[test]
    fn a_condition_may_be_absent_without_being_wrong() {
        let config = Config::new();
        assert!(errors(Conditional.validate(&Value::Null, &config, "primary")).is_empty());
        assert!(
            errors(Conditional.validate(&Value::Boolean(false), &config, "primary")).is_empty()
        );
        let rejected =
            errors(Conditional.validate(&Value::String("yes".into()), &config, "primary"));
        assert_eq!(
            rejected.first().map(|e| e.message.as_str()),
            Some(
                "Attribute 'primary' must be type 'boolean | object' \
                 (null or undefined are also allowed)"
            )
        );
    }
}
