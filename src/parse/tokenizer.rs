//! The seam that keeps the CommonMark engine a detail.
//!
//! Upstream has no equivalent. It hooks markdown-it's block ruler, inline ruler
//! and a core pass, which means the parser and the CommonMark implementation
//! are the same object. This port cannot do that -- pulldown-cmark is a pull
//! parser with no rulers to hook -- and would not want to: a host that already
//! parses CommonMark should not compile a second parser into its binary just to
//! read Markdoc tags.
//!
//! So the shape is inverted. The segmenter owns tag syntax and hands each run
//! of ordinary Markdown to a [`Tokenizer`], which returns a flat stream of
//! [`Event`]s with byte ranges. Everything Markdoc-specific happens above that
//! line; everything CommonMark-specific happens below it.
//!
//! # Whether this trait is exported
//!
//! Deliberately open until publication. Exporting it semver-freezes the event
//! vocabulary, and the vocabulary is the part most likely to grow -- a
//! CommonMark extension the host enables is a variant here. It is `pub` today
//! because the crate's own bundled implementation and its tests live in
//! different modules, and because a host with its own parser needs it. The call
//! at publication is whether to keep it in the public API or move it behind a
//! narrower re-export.
//!
//! # Why the vocabulary is this crate's own
//!
//! It would be less code to hand pulldown-cmark's `Event` straight through. It
//! would also make pulldown-cmark's public API part of this crate's, so a host
//! implementing the trait would have to depend on the parser it is trying to
//! avoid, and a pulldown-cmark major release would be a major release here.
//! The vocabulary below is the subset the AST can represent, and nothing else.

use std::borrow::Cow;
use std::ops::Range;

/// A CommonMark event with the byte range it covers.
///
/// Ranges are relative to the string passed to [`Tokenizer::tokenize`], and
/// they must be exact: the segmenter reads the original source back through
/// them to recover text the tokenizer may have transformed, so a range that is
/// merely close produces a document that is merely close.
pub type Spanned<'s> = (Event<'s>, Range<usize>);

/// How a table column is aligned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Alignment {
    /// No alignment was specified.
    #[default]
    None,
    /// `:---`
    Left,
    /// `:---:`
    Center,
    /// `---:`
    Right,
}

impl Alignment {
    /// The value Markdoc puts in a cell's `align` attribute, or [`None`] when
    /// the column carries no alignment and the attribute is omitted.
    #[must_use]
    pub const fn as_str(self) -> Option<&'static str> {
        match self {
            Alignment::None => None,
            Alignment::Left => Some("left"),
            Alignment::Center => Some("center"),
            Alignment::Right => Some("right"),
        }
    }
}

/// A container that opens and closes.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Container<'s> {
    /// A paragraph.
    Paragraph,
    /// An ATX or setext heading, level 1 to 6.
    Heading {
        /// The heading level.
        level: u8,
    },
    /// `> quoted`.
    Blockquote,
    /// A list. `start` is the first ordinal of an ordered list.
    List {
        /// Whether the list is ordered.
        ordered: bool,
        /// The first ordinal, for an ordered list.
        start: Option<u64>,
    },
    /// A list item.
    Item,
    /// A code block. `info` is the fence's info string; [`None`] means the
    /// block was indented rather than fenced.
    CodeBlock {
        /// The info string, verbatim.
        info: Option<Cow<'s, str>>,
    },
    /// `*emphasis*`.
    Emphasis,
    /// `**strong**`.
    Strong,
    /// `~~strikethrough~~`.
    Strikethrough,
    /// `[text](destination "title")`.
    Link {
        /// The destination, as written.
        destination: Cow<'s, str>,
        /// The title, empty when there is none.
        title: Cow<'s, str>,
    },
    /// `![alt](destination "title")`.
    Image {
        /// The destination, as written.
        destination: Cow<'s, str>,
        /// The title, empty when there is none.
        title: Cow<'s, str>,
    },
    /// A table, with one alignment per column.
    Table {
        /// Column alignments, in column order.
        alignments: Vec<Alignment>,
    },
    /// A table's header row group.
    TableHead,
    /// A table row.
    TableRow,
    /// A table cell.
    TableCell,
}

impl Container<'_> {
    /// Which container this is, without its payload.
    #[must_use]
    pub const fn kind(&self) -> ContainerKind {
        match self {
            Container::Paragraph => ContainerKind::Paragraph,
            Container::Heading { .. } => ContainerKind::Heading,
            Container::Blockquote => ContainerKind::Blockquote,
            Container::List { .. } => ContainerKind::List,
            Container::Item => ContainerKind::Item,
            Container::CodeBlock { .. } => ContainerKind::CodeBlock,
            Container::Emphasis => ContainerKind::Emphasis,
            Container::Strong => ContainerKind::Strong,
            Container::Strikethrough => ContainerKind::Strikethrough,
            Container::Link { .. } => ContainerKind::Link,
            Container::Image { .. } => ContainerKind::Image,
            Container::Table { .. } => ContainerKind::Table,
            Container::TableHead => ContainerKind::TableHead,
            Container::TableRow => ContainerKind::TableRow,
            Container::TableCell => ContainerKind::TableCell,
        }
    }
}

/// A [`Container`] with its payload dropped, for the closing event.
///
/// A close carries no data because a well-formed stream pairs it with an open
/// the consumer already has. Carrying the payload twice would let the two
/// disagree, and a consumer would have no rule for which to believe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContainerKind {
    /// Closes [`Container::Paragraph`].
    Paragraph,
    /// Closes [`Container::Heading`].
    Heading,
    /// Closes [`Container::Blockquote`].
    Blockquote,
    /// Closes [`Container::List`].
    List,
    /// Closes [`Container::Item`].
    Item,
    /// Closes [`Container::CodeBlock`].
    CodeBlock,
    /// Closes [`Container::Emphasis`].
    Emphasis,
    /// Closes [`Container::Strong`].
    Strong,
    /// Closes [`Container::Strikethrough`].
    Strikethrough,
    /// Closes [`Container::Link`].
    Link,
    /// Closes [`Container::Image`].
    Image,
    /// Closes [`Container::Table`].
    Table,
    /// Closes [`Container::TableHead`].
    TableHead,
    /// Closes [`Container::TableRow`].
    TableRow,
    /// Closes [`Container::TableCell`].
    TableCell,
}

/// One CommonMark event.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event<'s> {
    /// A container opens.
    Start(Container<'s>),
    /// A container closes.
    End(ContainerKind),
    /// Literal text, with escapes and entities already resolved.
    Text(Cow<'s, str>),
    /// Inline code, `` `x` ``.
    Code(Cow<'s, str>),
    /// Raw HTML, block or inline.
    ///
    /// Markdoc runs markdown-it with `html: false`, so upstream never produces
    /// an HTML node -- raw HTML is literal text there. This port keeps the
    /// event because a comment arrives through it, and treats everything else
    /// that comes this way as text, which is the same outcome.
    Html(Cow<'s, str>),
    /// A newline inside a block.
    SoftBreak,
    /// A line break written as two trailing spaces or a backslash.
    HardBreak,
    /// A thematic break.
    Rule,
}

/// Turns CommonMark into events.
///
/// The bundled implementation is
/// [`PulldownTokenizer`](crate::parse::PulldownTokenizer), behind the
/// `pulldown-cmark-tokenizer` feature. A host that already parses CommonMark
/// implements this over the parser it already pins, which is the supported
/// configuration and the reason the seam exists.
///
/// # Contract
///
/// - Every [`Event::Start`] is matched by an [`Event::End`] with the same
///   [`ContainerKind`], properly nested.
/// - Ranges are byte ranges into `source`, non-decreasing in start order, and
///   on character boundaries.
/// - A container's range covers its delimiters as well as its content, because
///   the parser above reads markers back out of the source: an emphasis node's
///   `marker` attribute is the `*` or `_` at the start of its span, not
///   something the event carries.
pub trait Tokenizer {
    /// Tokenize a run of CommonMark.
    fn tokenize<'s>(&self, source: &'s str) -> Vec<Spanned<'s>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_names_its_own_kind() {
        assert_eq!(Container::Paragraph.kind(), ContainerKind::Paragraph);
        assert_eq!(
            Container::Heading { level: 3 }.kind(),
            ContainerKind::Heading
        );
        assert_eq!(
            Container::Link {
                destination: Cow::Borrowed("/x"),
                title: Cow::Borrowed(""),
            }
            .kind(),
            ContainerKind::Link
        );
    }

    #[test]
    fn alignments_spell_themselves_as_markdoc_does() {
        assert_eq!(Alignment::None.as_str(), None);
        assert_eq!(Alignment::Left.as_str(), Some("left"));
        assert_eq!(Alignment::Center.as_str(), Some("center"));
        assert_eq!(Alignment::Right.as_str(), Some("right"));
    }
}
