//! The bundled [`Tokenizer`], over pulldown-cmark.
//!
//! Behind the `pulldown-cmark-tokenizer` feature, which is on by default and
//! which a host that already pins a CommonMark parser turns off. See the crate
//! README for why that matters: a host pinning pulldown-cmark from git is
//! pinning a *different package* as far as Cargo is concerned, so leaving this
//! on would compile two CommonMark parsers into one binary and render some
//! documents through each.
//!
//! # Which extensions are on
//!
//! Tables and strikethrough, and nothing else.
//!
//! Both are in the conformance corpus -- upstream's markdown-it is constructed
//! with its GFM-ish defaults -- so switching them off would fail cases that
//! have nothing to do with the tag language.
//!
//! Heading attributes (`# Title {#id}`) are deliberately **off**, which is
//! worth stating because `DIVERGENCES.md` entry 5 mentions them. That entry is
//! a rule for the *host's* Markdown pipeline, where both spellings exist and
//! one has to win. Here there is only one: Markdoc's annotation syntax, which
//! the corpus's first three cases exercise. Turning pulldown-cmark's spelling
//! on as well would have it try to read `{% #asdf %}` as an attribute block and
//! produce a heading neither syntax describes.
//!
//! Metadata blocks are off for the same reason as `DIVERGENCES.md` entry 7: the
//! host parses frontmatter and removes it before this crate sees a document.

use std::borrow::Cow;

use pulldown_cmark::{CodeBlockKind, Options, Parser, Tag, TagEnd};

use crate::parse::tokenizer::{Alignment, Container, ContainerKind, Event, Spanned, Tokenizer};

/// A [`Tokenizer`] over pulldown-cmark.
#[derive(Clone, Copy, Debug, Default)]
pub struct PulldownTokenizer {
    _private: (),
}

impl PulldownTokenizer {
    /// The bundled tokenizer.
    #[must_use]
    pub const fn new() -> PulldownTokenizer {
        PulldownTokenizer { _private: () }
    }

    /// The extension set, as one place to read it.
    fn options() -> Options {
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
    }
}

impl Tokenizer for PulldownTokenizer {
    fn tokenize<'s>(&self, source: &'s str) -> Vec<Spanned<'s>> {
        Parser::new_ext(source, PulldownTokenizer::options())
            .into_offset_iter()
            .filter_map(|(event, range)| convert(event).map(|event| (event, range)))
            .collect()
    }
}

/// Map one pulldown-cmark event, or drop it.
///
/// [`None`] drops an event that has no counterpart in this crate's vocabulary.
/// Only three kinds reach that arm, and each is dropped rather than
/// approximated:
///
/// - **Footnotes and definition lists.** Their extensions are off, so the
///   parser does not produce them.
/// - **Math and wikilinks.** Likewise off.
/// - **Task-list markers.** The extension is off, and Markdoc has no node for
///   one.
///
/// Dropping is safe only because each dropped event is a leaf. A dropped
/// container would unbalance the stream, so anything with a `Start`/`End` pair
/// is mapped rather than filtered -- including the ones the options never
/// switch on, so that a future option change cannot silently unbalance it.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per event kind; splitting it would only move the arms"
)]
fn convert(event: pulldown_cmark::Event<'_>) -> Option<Event<'_>> {
    use pulldown_cmark::Event as P;
    Some(match event {
        P::Start(tag) => Event::Start(container(tag)?),
        P::End(tag) => Event::End(kind(tag)?),
        // Math has no Markdoc node, and its extension is off; if it were ever
        // switched on, literal text is the honest reading of `$x$`.
        P::Text(text) | P::InlineMath(text) | P::DisplayMath(text) => Event::Text(cow(text)),
        P::Code(text) => Event::Code(cow(text)),
        P::Html(html) => Event::Html(cow(html)),
        P::InlineHtml(html) => Event::InlineHtml(cow(html)),
        P::SoftBreak => Event::SoftBreak,
        P::HardBreak => Event::HardBreak,
        P::Rule => Event::Rule,
        P::FootnoteReference(_) | P::TaskListMarker(_) => return None,
    })
}

fn container(tag: Tag<'_>) -> Option<Container<'_>> {
    Some(match tag {
        Tag::Paragraph => Container::Paragraph,
        Tag::Heading { level, .. } => Container::Heading {
            level: heading_level(level),
        },
        Tag::BlockQuote(_) => Container::Blockquote,
        Tag::CodeBlock(CodeBlockKind::Fenced(info)) => Container::CodeBlock {
            info: Some(cow(info)),
        },
        Tag::CodeBlock(CodeBlockKind::Indented) => Container::CodeBlock { info: None },
        Tag::List(start) => Container::List {
            ordered: start.is_some(),
            start,
        },
        Tag::Item => Container::Item,
        Tag::Table(alignments) => Container::Table {
            alignments: alignments.into_iter().map(alignment).collect(),
        },
        Tag::TableHead => Container::TableHead,
        Tag::TableRow => Container::TableRow,
        Tag::TableCell => Container::TableCell,
        Tag::Emphasis => Container::Emphasis,
        Tag::Strong => Container::Strong,
        Tag::Strikethrough => Container::Strikethrough,
        Tag::Link {
            dest_url, title, ..
        } => Container::Link {
            destination: cow(dest_url),
            title: cow(title),
        },
        Tag::Image {
            dest_url, title, ..
        } => Container::Image {
            destination: cow(dest_url),
            title: cow(title),
        },
        // `HtmlBlock` is dropped for a different reason from the rest: its
        // content still arrives, as `Event::Html`, so the wrapper carries
        // nothing. Mapping it to a paragraph would wrap literal markup in a node
        // upstream does not produce. The others are extensions that are off.
        Tag::HtmlBlock
        | Tag::FootnoteDefinition(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::Superscript
        | Tag::Subscript
        | Tag::MetadataBlock(_) => return None,
    })
}

fn kind(tag: TagEnd) -> Option<ContainerKind> {
    Some(match tag {
        TagEnd::Paragraph => ContainerKind::Paragraph,
        TagEnd::Heading(_) => ContainerKind::Heading,
        TagEnd::BlockQuote(_) => ContainerKind::Blockquote,
        TagEnd::CodeBlock => ContainerKind::CodeBlock,
        TagEnd::List(_) => ContainerKind::List,
        TagEnd::Item => ContainerKind::Item,
        TagEnd::Table => ContainerKind::Table,
        TagEnd::TableHead => ContainerKind::TableHead,
        TagEnd::TableRow => ContainerKind::TableRow,
        TagEnd::TableCell => ContainerKind::TableCell,
        TagEnd::Emphasis => ContainerKind::Emphasis,
        TagEnd::Strong => ContainerKind::Strong,
        TagEnd::Strikethrough => ContainerKind::Strikethrough,
        TagEnd::Link => ContainerKind::Link,
        TagEnd::Image => ContainerKind::Image,
        TagEnd::HtmlBlock
        | TagEnd::FootnoteDefinition
        | TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition
        | TagEnd::Superscript
        | TagEnd::Subscript
        | TagEnd::MetadataBlock(_) => return None,
    })
}

fn heading_level(level: pulldown_cmark::HeadingLevel) -> u8 {
    use pulldown_cmark::HeadingLevel as H;
    match level {
        H::H1 => 1,
        H::H2 => 2,
        H::H3 => 3,
        H::H4 => 4,
        H::H5 => 5,
        H::H6 => 6,
    }
}

fn alignment(alignment: pulldown_cmark::Alignment) -> Alignment {
    use pulldown_cmark::Alignment as A;
    match alignment {
        A::None => Alignment::None,
        A::Left => Alignment::Left,
        A::Center => Alignment::Center,
        A::Right => Alignment::Right,
    }
}

/// pulldown-cmark's `CowStr` carries a small inline variant, so it is not
/// `Cow<str>` and cannot be transmuted into one. Borrowed stays borrowed;
/// everything else is copied once, here, rather than at every read site.
fn cow(text: pulldown_cmark::CowStr<'_>) -> Cow<'_, str> {
    match text {
        pulldown_cmark::CowStr::Borrowed(text) => Cow::Borrowed(text),
        other => Cow::Owned(other.into_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(source: &str) -> Vec<Event<'_>> {
        PulldownTokenizer::new()
            .tokenize(source)
            .into_iter()
            .map(|(event, _)| event)
            .collect()
    }

    #[test]
    fn a_paragraph_is_one_container_around_its_text() {
        assert_eq!(
            events("hello"),
            [
                Event::Start(Container::Paragraph),
                Event::Text(Cow::Borrowed("hello")),
                Event::End(ContainerKind::Paragraph),
            ]
        );
    }

    #[test]
    fn ranges_cover_delimiters_so_markers_can_be_read_back() {
        let source = "*a*";
        let spans = PulldownTokenizer::new().tokenize(source);
        let (_, range) = spans
            .iter()
            .find(|(event, _)| matches!(event, Event::Start(Container::Emphasis)))
            .expect("emphasis is produced");
        assert_eq!(source.get(range.clone()), Some("*a*"));
    }

    #[test]
    fn tables_and_strikethrough_are_on_and_nothing_else_is() {
        assert!(events("| a |\n| - |\n| b |")
            .iter()
            .any(|event| matches!(event, Event::Start(Container::Table { .. }))));
        assert!(events("~~x~~")
            .iter()
            .any(|event| matches!(event, Event::Start(Container::Strikethrough))));
        // Heading attributes stay off: the braces are text, not an id.
        assert!(events("# t {#id}")
            .iter()
            .any(|event| matches!(event, Event::Text(text) if text.contains("{#id}"))));
    }

    #[test]
    fn a_fence_carries_its_info_string_verbatim() {
        let found = events("```ruby this is a test\nx\n```")
            .into_iter()
            .find_map(|event| match event {
                Event::Start(Container::CodeBlock { info }) => info,
                _ => None,
            });
        assert_eq!(found.as_deref(), Some("ruby this is a test"));
    }

    #[test]
    fn every_start_is_balanced_by_an_end() {
        let source =
            "# h\n\n> q\n\n* i\n\n| a |\n| - |\n| b |\n\n`c` *e* **s** ~~d~~ [l](/u) ![i](/u)";
        let mut depth = 0i32;
        for event in events(source) {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0, "an end arrived with nothing open");
        }
        assert_eq!(depth, 0, "a container was left open");
    }
}
