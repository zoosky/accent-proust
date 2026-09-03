//! The tag-internals grammar: what appears between `{%` and `%}`.
//!
//! Mirrors upstream `src/grammar/tag.pegjs` -- a 176-line PEG covering tag
//! names, attributes, values, variables, function calls, and annotations. Here
//! it becomes a hand-written recursive-descent parser over the same grammar.
//!
//! This is the crate's outermost attack surface: it is fed arbitrary text from
//! arbitrary documents. It must never panic, and a property test asserts that
//! against generated input rather than trusting review.
//!
//! # How the port is arranged
//!
//! | File | Upstream |
//! |---|---|
//! | `tag.rs` | `Top`, `Annotation`, `TagOpen`, `TagClose`, the attribute list |
//! | `value.rs` | `Value` and everything it reaches: literals, arrays, hashes, `Function`, `Variable` |
//! | `cursor.rs` | peggy's position, backtracking and expectation bookkeeping |
//! | `error.rs` | peggy's `SyntaxError` message algorithm |
//!
//! One function per production, named after it, in declaration order. Read a
//! rule beside the `.pegjs` and the two should say the same thing.
//!
//! # Fidelity notes
//!
//! Three behaviours are easy to mistake for bugs and are none of them. They
//! have tests, and the tests say why.
//!
//! - **The start rule must consume the whole body.** A PEG start rule that
//!   matches and leaves text behind is an error, and no other alternative is
//!   tried. `foo=1a` does not fall back to a tag named `foo`; it fails.
//! - **A falsy primary value is parsed and dropped.** `{% foo 0 %}` is a tag
//!   with no attributes, because upstream unshifts the primary attribute under
//!   a JavaScript truthiness test. See [`Value::is_truthy`].
//! - **A `$$mdtype` hash key is discarded.** It is upstream's runtime type tag,
//!   and dropping the guard would let authored content forge one.
//!
//! [`Value::is_truthy`]: crate::ast::Value::is_truthy

mod cursor;
mod error;
mod tag;
mod value;

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;

pub use cursor::MAX_VALUE_DEPTH;
pub use error::TagError;

use crate::ast::Value;
use cursor::Cursor;

/// Upstream's `Top` production: the four things that can appear between `{%`
/// and `%}`.
///
/// Upstream returns a markdown-it token whose `type` and `nesting` encode the
/// same four cases, and this is the mapping the tokenizer above reverses:
///
/// | Here | `type` | `nesting` |
/// |---|---|---|
/// | [`TagItem::Variable`] | `variable` | 0 |
/// | [`TagItem::Annotation`] | `annotation` | 0 |
/// | [`TagItem::TagOpen`] with `self_closing: false` | `tag_open` | 1 |
/// | [`TagItem::TagOpen`] with `self_closing: true` | `tag` | 0 |
/// | [`TagItem::TagClose`] | `tag_close` | -1 |
///
/// The `self_closing` flag replaces upstream's `type`/`nesting` pair because
/// the pair is one decision spelled twice: `['tag', 0]` and `['tag_open', 1]`
/// differ in exactly the presence of the trailing `/`. Nesting is a property of
/// the token stream, not of the tag body, so it belongs to the layer that
/// builds the stream.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TagItem {
    /// A bare value: `{% $foo %}` or `{% equals(1, 2) %}`.
    ///
    /// Upstream calls this a variable even when it holds a function call,
    /// because both resolve to a value at transform time rather than naming a
    /// tag.
    Variable(Value),
    /// Attributes with no tag name: `{% #id .class key="value" %}`.
    Annotation {
        /// The attributes, in authored order.
        attributes: Vec<Attribute>,
    },
    /// An opening or self-closing tag: `{% callout type="note" %}`.
    TagOpen {
        /// The tag name.
        name: String,
        /// The attributes, in authored order.
        ///
        /// A primary value -- the unnamed one that may follow the tag name --
        /// arrives here as an attribute named `primary`, first in the list.
        attributes: Vec<Attribute>,
        /// Whether the tag closed itself with a trailing `/`.
        self_closing: bool,
    },
    /// A closing tag: `{% /callout %}`.
    TagClose {
        /// The tag name.
        name: String,
    },
}

/// One entry of a tag's attribute list.
///
/// The `#id` shortcut is not a variant: upstream expands it to an ordinary
/// attribute named `id` with a string value, and a consumer that special-cased
/// it would be handling a syntax that no longer exists by this point. The
/// `.class` shortcut *is* a variant, because a node collects classes into a set
/// rather than overwriting one attribute.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Attribute {
    /// `key=value`, or the `#id` shortcut.
    Attribute {
        /// The attribute name.
        name: String,
        /// The attribute value.
        value: Value,
    },
    /// The `.class` shortcut.
    ///
    /// There is no value field. Upstream carries `value: true` here and never
    /// reads anything else out of it, so the value is the variant.
    Class {
        /// The class name.
        name: String,
    },
}

/// Parses the internals of a tag: everything between `{%` and `%}`.
///
/// Pass the body with the delimiters removed and both ends trimmed, which is
/// what upstream's tokenizer passes (`content.trim()` in
/// `src/tokenizer/plugins/annotations.ts`). The grammar has no leading- or
/// trailing-whitespace rule of its own beyond an annotation's trailing `_*`, so
/// untrimmed input is a syntax error rather than a lenient parse. Trimming here
/// instead would be a divergence, and a silent one.
///
/// ```
/// use accent_proust::ast::Value;
/// use accent_proust::grammar::{parse_tag, Attribute, TagItem};
///
/// let item = parse_tag(r#"callout type="note" /"#)?;
/// assert_eq!(
///     item,
///     TagItem::TagOpen {
///         name: "callout".to_string(),
///         attributes: vec![Attribute::Attribute {
///             name: "type".to_string(),
///             value: Value::String("note".to_string()),
///         }],
///         self_closing: true,
///     }
/// );
/// # Ok::<(), accent_proust::grammar::TagError>(())
/// ```
///
/// # Errors
///
/// Returns a [`TagError`] when the body is not a well-formed tag, and also when
/// it *is* one followed by anything else: the start rule has to consume the
/// whole body, so `foo=1 bar` fails rather than parsing the part it
/// understands. The message is upstream's message for the same input, and the
/// offsets are byte offsets into `input`.
pub fn parse_tag(input: &str) -> Result<TagItem, TagError> {
    let mut cursor = Cursor::new(input);
    match cursor.top() {
        Some(item) if cursor.at_end() => Ok(item),
        Some(_) => {
            cursor.expect_end_of_input();
            Err(cursor.into_error())
        }
        None => Err(cursor.into_error()),
    }
}
