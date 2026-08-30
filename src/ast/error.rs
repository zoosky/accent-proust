//! Problems found in a document, as data.
//!
//! Mirrors upstream's `ValidationError` in `types.ts`. It lives in the AST
//! rather than in the validator because the parser produces them first: a tag
//! whose internals do not parse, a closing tag with no opening, an annotation
//! with nowhere to attach. Those are attached to the node they concern and
//! collected later, exactly as upstream does.
//!
//! # The ids never diverge
//!
//! [`ValidationError::id`] is the part external tooling binds to, so it is
//! copied from upstream verbatim and renaming one is itself a divergence
//! (`DIVERGENCES.md`, rule 2). The ids this module's constructors produce are
//! the five the parser can raise; the validator adds its own with the same
//! rule.
//!
//! # Errors are not failures
//!
//! Nothing here is a Rust error type. A document with a broken tag still parses
//! into a tree -- an editor wants the rest of the file, and the formatter has to
//! reprint what it was given. `Result::Err` is reserved for invariants this
//! crate would have broken itself.

use crate::ast::Location;

/// How serious a problem is.
///
/// The five upstream levels, spelled the same way. `Critical` is the parser's
/// own vocabulary for "this document does not say what it appears to say":
/// a missing closing tag changes the shape of everything after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ErrorLevel {
    /// Diagnostic noise, off by default in every consumer.
    Debug,
    /// Worth saying, never worth stopping for.
    Info,
    /// The document works but the author probably did not mean this.
    Warning,
    /// The document is wrong in a way that changes its output.
    Error,
    /// The document's structure is wrong; what follows may be misread.
    Critical,
}

impl ErrorLevel {
    /// Upstream's spelling, which is what tooling reads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorLevel::Debug => "debug",
            ErrorLevel::Info => "info",
            ErrorLevel::Warning => "warning",
            ErrorLevel::Error => "error",
            ErrorLevel::Critical => "critical",
        }
    }
}

impl std::fmt::Display for ErrorLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One problem, attached to the node it concerns.
///
/// The `id` is a stable, upstream-compatible string. The `message` is prose and
/// may be reworded; the `id` may not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError<'a> {
    /// The stable upstream id, such as `missing-closing`.
    pub id: &'static str,
    /// How serious it is.
    pub level: ErrorLevel,
    /// A human-readable description.
    pub message: String,
    /// Where in the source it was found, when the producer knows.
    pub location: Option<Location<'a>>,
}

impl<'a> ValidationError<'a> {
    /// A problem with an id, a level, and a message.
    #[must_use]
    pub fn new(id: &'static str, level: ErrorLevel, message: impl Into<String>) -> Self {
        ValidationError {
            id,
            level,
            message: message.into(),
            location: None,
        }
    }

    /// Attach a source location.
    #[must_use]
    pub fn at(mut self, location: Location<'a>) -> Self {
        self.location = Some(location);
        self
    }

    /// `parse-error`: the text between `{%` and `%}` is not a tag.
    ///
    /// The message is the grammar's own, verbatim, because upstream's corpus
    /// compares it character for character.
    #[must_use]
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new("parse-error", ErrorLevel::Critical, message)
    }

    /// `missing-opening`: a closing tag with nothing to close.
    #[must_use]
    pub fn missing_opening(name: &str) -> Self {
        Self::new(
            "missing-opening",
            ErrorLevel::Critical,
            format!("Node '{name}' is missing opening"),
        )
    }

    /// `missing-closing`: a tag still open at the end of the document.
    #[must_use]
    pub fn missing_closing(name: &str) -> Self {
        Self::new(
            "missing-closing",
            ErrorLevel::Critical,
            format!("Node '{name}' is missing closing"),
        )
    }

    /// `duplicate-attribute`: an annotation set an attribute that was already
    /// set.
    ///
    /// A warning, not an error: the last value wins and the document still
    /// renders, which is what upstream does.
    #[must_use]
    pub fn duplicate_attribute(name: &str) -> Self {
        Self::new(
            "duplicate-attribute",
            ErrorLevel::Warning,
            format!("Attribute '{name}' already set"),
        )
    }

    /// `no-inline-annotations`: an annotation appeared where there is no inline
    /// content to annotate, so there is no node it could belong to.
    #[must_use]
    pub fn no_inline_annotations(parent: &str) -> Self {
        Self::new(
            "no-inline-annotations",
            ErrorLevel::Error,
            format!("Can't apply inline annotations to '{parent}'"),
        )
    }

    /// `href-format-invalid`: a URL contains a tag or a variable.
    ///
    /// Raised only when link validation is switched on, which is off by
    /// default, as upstream has it.
    #[must_use]
    pub fn href_format_invalid() -> Self {
        Self::new(
            "href-format-invalid",
            ErrorLevel::Error,
            "The 'href' format cannot contain Markdoc tag or variable. \
             URLs must be static strings.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_upstream() {
        assert_eq!(ValidationError::parse_error("x").id, "parse-error");
        assert_eq!(
            ValidationError::missing_opening("foo").id,
            "missing-opening"
        );
        assert_eq!(
            ValidationError::missing_closing("foo").id,
            "missing-closing"
        );
        assert_eq!(
            ValidationError::duplicate_attribute("bar").id,
            "duplicate-attribute"
        );
        assert_eq!(
            ValidationError::no_inline_annotations("fence").id,
            "no-inline-annotations"
        );
        assert_eq!(
            ValidationError::href_format_invalid().id,
            "href-format-invalid"
        );
    }

    #[test]
    fn messages_match_upstream() {
        assert_eq!(
            ValidationError::missing_closing("foo").message,
            "Node 'foo' is missing closing"
        );
        assert_eq!(
            ValidationError::duplicate_attribute("bar").message,
            "Attribute 'bar' already set"
        );
        assert_eq!(
            ValidationError::no_inline_annotations("fence").message,
            "Can't apply inline annotations to 'fence'"
        );
        assert_eq!(
            ValidationError::href_format_invalid().message,
            "The 'href' format cannot contain Markdoc tag or variable. \
             URLs must be static strings."
        );
    }

    #[test]
    fn levels_print_as_upstream_spells_them() {
        assert_eq!(ErrorLevel::Warning.to_string(), "warning");
        assert_eq!(ErrorLevel::Critical.to_string(), "critical");
        assert!(ErrorLevel::Critical > ErrorLevel::Error);
    }
}
