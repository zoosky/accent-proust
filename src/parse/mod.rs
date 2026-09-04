//! Segmentation and document parsing: source text in, AST out.
//!
//! Mirrors upstream `src/parser.ts` and `src/tokenizer/`, and is the one part
//! of the port with no line-by-line source. Upstream hooks markdown-it's block
//! ruler, inline ruler, and a core pass; none of those exist here.
//!
//! The Rust equivalent is a segmenter over the raw text -- block-level `{% %}`
//! line detection, inline spans inside text runs, fence interception -- that
//! feeds each Markdown segment to a `Tokenizer` and lifts the resulting
//! events, with their source spans, into the AST under the current tag scope.
//! The behaviour to reproduce is the behaviour the conformance corpus fixes,
//! not markdown-it's mechanics.
//!
//! `Tokenizer` is the seam that keeps the CommonMark engine a detail. The
//! bundled implementation over pulldown-cmark sits behind the
//! `pulldown-cmark-tokenizer` feature; a host that already parses CommonMark
//! can implement the trait instead and avoid compiling a second parser.

mod annotate;
mod document;
mod scan;
mod segment;
mod tokenizer;

#[cfg(feature = "pulldown-cmark-tokenizer")]
mod pulldown;

pub use scan::{CLOSE, OPEN, contains_markdoc_tag_in_url, find_tag_end};
pub use tokenizer::{Alignment, Container, ContainerKind, Event, Spanned, Tokenizer};

#[cfg(feature = "pulldown-cmark-tokenizer")]
pub use pulldown::PulldownTokenizer;

use crate::ast::Node;
use crate::transform;

/// How to parse a document.
///
/// Upstream spreads these across two objects -- `new Tokenizer({allowComments,
/// allowLinkValidation, ...})` and `parse(tokens, {file, slots, location})` --
/// because its tokenizer is the CommonMark parser. Here the CommonMark parser
/// is behind a trait and the tag rules are the parser's own, so all of it is one
/// struct.
///
/// The defaults are upstream's library defaults, not the ones its conformance
/// runner uses. That distinction matters: `spec/marktest/index.ts` builds its
/// tokenizer with `allowIndentation: true, allowComments: true`, and neither is
/// a default anywhere.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ParseOptions<'s> {
    /// A name for the document, carried into every location.
    ///
    /// A label, never a path this crate opens: it performs no I/O.
    pub file: Option<&'s str>,
    /// Whether `{% slot "name" %}` is lifted into its parent's slot map.
    pub slots: bool,
    /// Whether nodes carry a [`Location`](crate::ast::Location).
    ///
    /// On by default, and worth switching off for a throwaway parse: a location
    /// borrows rather than copies, so the cost is the struct, not the text.
    pub location: bool,
    /// Whether an HTML comment becomes a `comment` node.
    ///
    /// Off by default, as upstream has it. On, a comment is a node the
    /// transform stage can drop; off, it is literal text, because Markdoc runs
    /// markdown-it with `html: false`.
    pub allow_comments: bool,
    /// URL schemes to reject a tag inside, or [`None`] to skip the check.
    ///
    /// Upstream spells this as two options, `allowLinkValidation` and
    /// `linkValidationOptions.validatedProtocols`. One option holding the list
    /// says the same thing without letting the two disagree. Upstream's default
    /// list, once switched on, is `["http", "https"]`.
    pub validated_protocols: Option<Vec<String>>,
    /// Tags allowed to wrap rows inside a `{% table %}`, or [`None`] for
    /// upstream's default of `["if"]`.
    ///
    /// The table rewrite runs at the end of the parse (see
    /// [`crate::transform::table`]), so this is a parse option
    /// even though the pass it configures is a transform. A tag not named here
    /// that appears between rows is reported as `table-syntax` rather than kept,
    /// because a component wrapping `<tr>` elements produces invalid HTML.
    pub conditional_tags: Option<Vec<String>>,
}

impl<'s> ParseOptions<'s> {
    /// Upstream's library defaults: locations on, everything else off.
    #[must_use]
    pub fn new() -> ParseOptions<'s> {
        ParseOptions {
            file: None,
            slots: false,
            location: true,
            allow_comments: false,
            validated_protocols: None,
            conditional_tags: None,
        }
    }

    /// Name the document.
    #[must_use]
    pub fn file(mut self, file: &'s str) -> ParseOptions<'s> {
        self.file = Some(file);
        self
    }

    /// Lift `{% slot %}` into its parent's slot map.
    #[must_use]
    pub fn slots(mut self, slots: bool) -> ParseOptions<'s> {
        self.slots = slots;
        self
    }

    /// Turn locations off.
    #[must_use]
    pub fn location(mut self, location: bool) -> ParseOptions<'s> {
        self.location = location;
        self
    }

    /// Turn HTML comments into `comment` nodes.
    #[must_use]
    pub fn allow_comments(mut self, allow: bool) -> ParseOptions<'s> {
        self.allow_comments = allow;
        self
    }

    /// Reject tags inside URLs using these schemes.
    #[must_use]
    pub fn validated_protocols(mut self, protocols: Vec<String>) -> ParseOptions<'s> {
        self.validated_protocols = Some(protocols);
        self
    }

    /// Name the tags allowed to wrap rows inside a `{% table %}`.
    #[must_use]
    pub fn conditional_tags(mut self, tags: Vec<String>) -> ParseOptions<'s> {
        self.conditional_tags = Some(tags);
        self
    }
}

/// Parse a document with the bundled tokenizer and default options.
///
/// ```
/// use accent_proust::ast::NodeType;
///
/// let document = accent_proust::parse::parse("# Title {% #intro %}\n");
/// let heading = &document.children[0];
/// assert_eq!(heading.node_type, NodeType::Heading);
/// assert_eq!(heading.get("id").and_then(|value| match value {
///     accent_proust::ast::Value::String(text) => Some(text.as_str()),
///     _ => None,
/// }), Some("intro"));
/// ```
#[cfg(feature = "pulldown-cmark-tokenizer")]
#[must_use]
pub fn parse(source: &str) -> Node<'_> {
    parse_with(source, &PulldownTokenizer::new(), &ParseOptions::new())
}

/// Parse a document.
///
/// The tokenizer is a parameter rather than a type parameter so that a host can
/// choose one at run time and so that this function stays object-safe to call
/// from a trait object. Parsing allocates a masked copy of `source`, which is
/// what keeps a tag's internals out of the CommonMark parse; see
/// [`segment`](self) for why that is the mechanism.
#[must_use]
pub fn parse_with<'s>(
    source: &'s str,
    tokenizer: &dyn Tokenizer,
    options: &ParseOptions<'s>,
) -> Node<'s> {
    let segmentation = segment::segment(source);
    let mut document = document::Builder::new(source, options).run(&segmentation, tokenizer);

    // Upstream runs its transform list here, at the end of `parser()`, and the
    // list has exactly one member. Running it inside the parse rather than
    // leaving it to the caller is what makes `{% table %}` a table for every
    // stage above -- the validator reports `table-syntax` on the rewritten tree,
    // and the formatter reprints one.
    let conditional: Vec<&str> = match &options.conditional_tags {
        Some(tags) => tags.iter().map(String::as_str).collect(),
        None => transform::table::DEFAULT_CONDITIONAL_TAGS.to_vec(),
    };
    transform::table::apply(&mut document, &conditional);
    document
}
