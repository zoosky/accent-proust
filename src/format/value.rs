//! Values, attributes and annotations: everything printed between `{%` and
//! `%}`.
//!
//! Upstream's `formatValue`, `formatScalar`, `formatVariable`, `formatFunction`,
//! `formatAnnotationValue`, `formatAttributes` and `formatAnnotations`. Two of
//! them look alike and are not, which is the thing to know before reading:
//!
//! - **`format_value` prints a value as content.** A string prints as its
//!   characters. It is what a `text` node's variable and an image's `alt` go
//!   through.
//! - **`format_scalar` prints a value as a literal.** A string prints quoted. It
//!   is what an attribute's right-hand side goes through, so that reparsing the
//!   output produces the value again.
//!
//! # Annotations are reprinted, not reconstructed
//!
//! `.foo` is stored twice: folded into the `class` attribute as
//! `{foo: true}`, and kept verbatim in [`Node::annotations`]. The formatter
//! reads the second, because the first cannot say whether the author wrote
//! `.foo` or `class={foo: true}` and the two are different documents to a
//! reader.
//!
//! [`Node::annotations`]: crate::ast::Node::annotations

use std::fmt::Write as _;

use super::{Ctx, Formatter, Out, MAX_FORMAT_DEPTH, SEP};
use crate::ast::{Node, PathSegment, Value};
use crate::grammar::Attribute;
use crate::render::js;

/// Whether a name can be written without quotes.
///
/// Upstream's `IDENTIFIER_REGEX`, `/^[a-zA-Z0-9_-]+$/`. It decides three
/// things: whether a hash key is quoted, whether an `id` attribute contracts to
/// `#id`, and whether a class contracts to `.class`.
pub(super) fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Which of the two shapes an attribute has, for
/// [`Formatter::annotation_value`].
///
/// Upstream's `AttributeValue.type`. A class is not merely an attribute named
/// `class`: it contracts to `.name`, and its value is the presence of the name
/// rather than anything printable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttributeKind {
    /// `name=value`, or one of its contractions.
    Attribute,
    /// A class, which contracts to `.name`.
    Class,
}

impl Formatter<'_> {
    /// Upstream's `formatValue`: a value printed as content.
    ///
    /// Bounded at [`MAX_FORMAT_DEPTH`] for the reason [`Formatter::scalar`] is:
    /// an array holds arrays, and how deep is the caller's choice.
    pub(super) fn value(&mut self, value: &Value, ctx: Ctx, out: &mut Out) {
        if self.stack >= MAX_FORMAT_DEPTH {
            return;
        }
        self.stack += 1;
        self.value_inner(value, ctx, out);
        self.stack -= 1;
    }

    fn value_inner(&mut self, value: &Value, ctx: Ctx, out: &mut Out) {
        match value {
            // `case 'undefined'` and `if (v === null) break` -- both print
            // nothing at all, which is what makes `format(null)` the empty
            // string.
            Value::Null => {}
            Value::Boolean(boolean) => out.text(boolean.to_string()),
            Value::Number(number) => out.text(js::number(*number)),
            Value::String(text) => out.text(text.clone()),
            Value::Array(items) => {
                for item in items {
                    self.value(item, ctx, out);
                }
            }
            Value::Function(_) | Value::Variable(_) => out.text(self.scalar(value)),
            // Upstream throws `Unimplemented: "undefined"` here: a plain object
            // carries no `$$mdtype`, so its switch falls through. A hash in
            // content position is unreachable from a parsed document, and the
            // literal spelling is the one that re-parses, so print that rather
            // than reproduce an uncaught host error.
            hash @ Value::Hash(_) => out.text(self.scalar(hash)),
        }
    }

    /// Upstream's `formatScalar`: a value printed as a literal.
    ///
    /// Returns a `String` rather than upstream's `string | undefined`. The
    /// `undefined` arm exists to skip an attribute whose value is missing, and
    /// a missing value in Rust is a key that is not in the map.
    pub(super) fn scalar(&mut self, value: &Value) -> String {
        if self.stack >= MAX_FORMAT_DEPTH {
            return String::new();
        }
        self.stack += 1;
        let out = self.scalar_inner(value);
        self.stack -= 1;
        out
    }

    fn scalar_inner(&mut self, value: &Value) -> String {
        match value {
            Value::Variable(variable) => variable_literal(variable),
            Value::Function(function) => self.function_literal(function),
            Value::Null => "null".to_owned(),
            Value::Array(items) => {
                let items: Vec<String> = items.iter().map(|item| self.scalar(item)).collect();
                format!("[{}]", items.join(SEP))
            }
            Value::Hash(entries) => {
                let entries: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = if is_identifier(key) {
                            key.clone()
                        } else {
                            // Upstream wraps the key in quotes rather than
                            // JSON-encoding it, so a key containing a quote
                            // prints broken output. The grammar cannot produce
                            // one; reproducing the spelling keeps the two in
                            // step if it ever can.
                            format!("\"{key}\"")
                        };
                        format!("{key}: {}", self.scalar(value))
                    })
                    .collect();
                format!("{{{}}}", entries.join(SEP))
            }
            // `JSON.stringify` for the three remaining kinds.
            Value::Boolean(boolean) => boolean.to_string(),
            Value::Number(number) => json_number(*number),
            Value::String(text) => json_string(text),
        }
    }

    /// Upstream's `formatFunction`: `name(arg, arg)`.
    ///
    /// Parameter *names* are dropped, as upstream drops them -- it prints
    /// `Object.values` -- so `f(x=1)` reprints as `f(1)`. The order is authored
    /// order rather than JavaScript's object order; see `DIVERGENCES.md` entry
    /// 10.
    fn function_literal(&mut self, function: &crate::ast::Function) -> String {
        let parameters: Vec<String> = function
            .parameters
            .values()
            .map(|value| self.scalar(value))
            .collect();
        format!("{}({})", function.name, parameters.join(SEP))
    }

    /// Upstream's `formatAnnotationValue`: one attribute, in its shortest
    /// spelling.
    ///
    /// Four spellings, in upstream's order. A `primary` attribute prints as a
    /// bare value, `id="x"` contracts to `#x`, a class contracts to `.x`, and
    /// everything else is `name=value`. The order matters: a class *named* `id`
    /// takes the class arm, because the `id` arm requires a string value and a
    /// class does not have one.
    pub(super) fn annotation_value(
        &mut self,
        kind: AttributeKind,
        name: &str,
        value: &Value,
    ) -> String {
        let formatted = self.scalar(value);

        if name == "primary" {
            return formatted;
        }
        if name == "id" {
            if let Value::String(text) = value {
                if is_identifier(text) {
                    return format!("#{text}");
                }
            }
        }
        if kind == AttributeKind::Class && is_identifier(name) {
            return format!(".{name}");
        }
        format!("{name}={formatted}")
    }

    /// Upstream's `formatAttributes`: a tag's attribute map, in authored order.
    ///
    /// The `class` attribute is expanded back into one `.name` per class. What
    /// upstream passes as that entry's *value* is the whole class hash, not the
    /// entry's own value -- which only shows when a class name is not an
    /// identifier, and then prints `name={a: true}`. It is ported as written,
    /// because the alternative is inventing a spelling upstream does not emit.
    pub(super) fn attributes(&mut self, node: &Node<'_>) -> Vec<String> {
        let mut out = Vec::new();
        for (key, value) in &node.attributes {
            match value {
                Value::Hash(classes) if key == "class" => {
                    for name in classes.keys() {
                        let name = name.clone();
                        out.push(self.annotation_value(AttributeKind::Class, &name, value));
                    }
                }
                _ => out.push(self.annotation_value(AttributeKind::Attribute, key, value)),
            }
        }
        out
    }

    /// Upstream's `formatAnnotations`: `{% #id .cls key=value %}`, or nothing.
    ///
    /// Three chunks, not one, because upstream yields three -- and the
    /// `{% table %}` branch counts chunks.
    pub(super) fn annotations(&mut self, node: &Node<'_>, out: &mut Out) {
        if node.annotations.is_empty() {
            return;
        }
        let values: Vec<String> = node
            .annotations
            .iter()
            .map(|annotation| match annotation {
                Attribute::Attribute { name, value } => {
                    self.annotation_value(AttributeKind::Attribute, name, value)
                }
                Attribute::Class { name } => {
                    // A class carries no value upstream either; `value: true`
                    // is a placeholder its formatter never prints.
                    self.annotation_value(AttributeKind::Class, name, &Value::Boolean(true))
                }
            })
            .collect();
        out.text(format!("{} ", super::OPEN));
        out.text(values.join(super::SPACE));
        out.text(format!(" {}", super::CLOSE));
    }
}

/// Upstream's `formatVariable`: `$a.b[0]["c d"]`.
///
/// The first step is printed bare and every later one is punctuated: a name
/// that is an identifier as `.name`, a number as `[0]`, and anything else as
/// `["name"]`. That last spelling is why [`PathSegment::Key`] does not record
/// which form the author wrote -- the formatter re-derives it, so the two
/// spellings of one path print identically.
fn variable_literal(variable: &crate::ast::Variable) -> String {
    let mut out = "$".to_owned();
    for (index, segment) in variable.path.iter().enumerate() {
        match segment {
            PathSegment::Key(key) if index == 0 => out.push_str(key),
            PathSegment::Index(number) if index == 0 => out.push_str(&js::number(*number)),
            PathSegment::Key(key) if is_identifier(key) => {
                out.push('.');
                out.push_str(key);
            }
            PathSegment::Key(key) => {
                out.push('[');
                out.push('"');
                out.push_str(key);
                out.push('"');
                out.push(']');
            }
            PathSegment::Index(number) => {
                out.push('[');
                out.push_str(&js::number(*number));
                out.push(']');
            }
        }
    }
    out
}

/// `JSON.stringify` for a number.
///
/// The same digits as [`js::number`], except that the non-finite values -- which
/// JSON cannot express -- become `null`.
fn json_number(number: f64) -> String {
    if number.is_finite() {
        js::number(number)
    } else {
        "null".to_owned()
    }
}

/// `JSON.stringify` for a string.
///
/// The escapes are the specification's: quote, backslash, the five named
/// control characters, and `\u00XX` for the rest of C0. Nothing above U+001F is
/// escaped, so the output is UTF-8 rather than the `\uXXXX` soup a `JSON.parse`
/// round-trip would also accept.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
