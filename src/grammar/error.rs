//! Syntax errors, in upstream's words.
//!
//! Upstream generates its parser with peggy, and peggy's `SyntaxError` message
//! is not decoration you may replace: three cases in the conformance corpus
//! assert the exact string, character for character.
//!
//! ```text
//! Expected "}", identifier, string, or whitespace but "," found.
//! Expected ",", "]", or whitespace but "2" found.
//! ```
//!
//! So this module reproduces peggy's algorithm rather than inventing a message
//! of its own. The parser records what it was willing to accept at each
//! position it reached; the furthest such position wins; and the message is
//! that set, described, sorted, de-duplicated and joined. `Cursor` in
//! `cursor.rs` is the half that records, this is the half that prints.
//!
//! # Positions are byte offsets, and there are no line and column numbers
//!
//! Upstream's `SyntaxError` carries `{offset, line, column}` for the start and
//! end of the failure. Its offsets are UTF-16 code-unit offsets into the tag
//! body; ours are byte offsets into the same body, which is what the rest of a
//! Rust program can slice with.
//!
//! The line and column are dropped rather than ported. They are relative to
//! the tag body, not to the document, so they are wrong for every tag that
//! does not start at the beginning of a line -- which is why upstream's own
//! caller reads only `offset` and recomputes the rest against the document
//! (`src/tokenizer/plugins/annotations.ts`). Reporting a plausible but
//! document-relative-looking number is worse than reporting none.

use std::fmt::Write as _;

/// Something the parser was willing to accept at a given position.
///
/// The three shapes correspond to peggy's `literalExpectation`,
/// `otherExpectation` and `endExpectation`. Character classes never appear:
/// every rule in the grammar that matches one -- `Identifier`, `_`,
/// `ValueNumber`, `ValueString`, `Variable` -- is a *named* rule, and peggy
/// reports a named rule by its name and suppresses everything inside it. That
/// is why the corpus messages say "identifier" rather than `[a-zA-Z0-9_-]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expectation {
    /// A literal string, printed quoted: `","`.
    Literal(&'static str),
    /// A named rule, printed as its name: `identifier`.
    Named(&'static str),
    /// The end of the input, printed as `end of input`.
    EndOfInput,
}

impl Expectation {
    /// Describes this expectation the way peggy's `describeExpectation` does.
    fn describe(self) -> String {
        match self {
            Expectation::Literal(text) => format!("\"{}\"", escape(text)),
            Expectation::Named(name) => name.to_string(),
            Expectation::EndOfInput => "end of input".to_string(),
        }
    }
}

/// Escapes a string for a message, as peggy's `literalEscape` does.
///
/// Control characters become `\xNN` with uppercase hex, which is why this is
/// not `char::escape_debug`: that spells the same characters `\u{1f}` and the
/// corpus strings would stop matching.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{1}'..='\u{f}' => {
                // Writing to a String cannot fail; the result is discarded
                // deliberately rather than unwrapped.
                let _ = write!(out, "\\x0{:X}", ch as u32);
            }
            '\u{10}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                let _ = write!(out, "\\x{:X}", ch as u32);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Builds peggy's message from the expectation set and the offending
/// character.
///
/// Sorting is byte order here and UTF-16 code-unit order there. Every
/// description this grammar can produce is ASCII -- the named rules are ASCII
/// and the only literals that survive into a message are single ASCII
/// punctuation marks -- so the two orders agree.
pub(crate) fn build_message(variants: &[Expectation], found: Option<char>) -> String {
    let mut descriptions: Vec<String> = variants.iter().map(|e| e.describe()).collect();
    descriptions.sort();
    descriptions.dedup();

    let expected = match descriptions.as_slice() {
        // Unreachable: every alternative of `Top` records an expectation at
        // offset 0 before it can fail. The arm keeps the function total.
        [] => Expectation::EndOfInput.describe(),
        [only] => only.clone(),
        [first, second] => format!("{first} or {second}"),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
    };

    let found = match found {
        Some(ch) => format!("\"{}\"", escape(&ch.to_string())),
        None => "end of input".to_string(),
    };

    format!("Expected {expected} but {found} found.")
}

/// A tag body that is not a tag.
///
/// The message is upstream's, byte for byte, because tooling and the
/// conformance corpus read it. The offsets are this crate's: byte offsets into
/// the tag body you passed to `parse_tag`, which a host adds its own document
/// offset to.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{message}")]
pub struct TagError {
    message: String,
    start: usize,
    end: usize,
}

impl TagError {
    /// Creates an error at a byte range of the tag body.
    pub(crate) fn new(message: String, start: usize, end: usize) -> Self {
        Self {
            message,
            start,
            end,
        }
    }

    /// Returns the message, which is upstream's message for the same input.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the byte offset where the parse failed.
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Returns the byte offset just past the offending character.
    ///
    /// This is `start` plus the UTF-8 length of the character found there, or
    /// `start` itself at the end of the input. Both ends are on character
    /// boundaries, so slicing the tag body with them is safe.
    #[must_use]
    pub fn end(&self) -> usize {
        self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_expectation_reads_as_a_bare_description() {
        let message = build_message(&[Expectation::EndOfInput], Some('['));
        assert_eq!(message, "Expected end of input but \"[\" found.");
    }

    #[test]
    fn two_expectations_are_joined_with_or_and_no_comma() {
        let message = build_message(
            &[Expectation::Literal("]"), Expectation::Named("whitespace")],
            None,
        );
        assert_eq!(
            message,
            "Expected \"]\" or whitespace but end of input found."
        );
    }

    #[test]
    fn many_expectations_are_sorted_and_de_duplicated() {
        let message = build_message(
            &[
                Expectation::Named("whitespace"),
                Expectation::Literal("]"),
                Expectation::Named("whitespace"),
                Expectation::Literal(","),
            ],
            Some('2'),
        );
        assert_eq!(
            message,
            "Expected \",\", \"]\", or whitespace but \"2\" found."
        );
    }

    #[test]
    fn control_characters_are_escaped_the_way_peggy_escapes_them() {
        assert_eq!(escape("\u{1}"), "\\x01");
        assert_eq!(escape("\u{1f}"), "\\x1F");
        assert_eq!(escape("\u{7f}"), "\\x7F");
        assert_eq!(escape("\0\t\n\r\\\""), "\\0\\t\\n\\r\\\\\\\"");
    }
}
