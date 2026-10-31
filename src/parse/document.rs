//! Lifting segments and CommonMark events into the document tree.
//!
//! Ports upstream `src/parser.ts`. That file walks a flat markdown-it token
//! stream with a stack of open nodes; this one walks a flat [`Event`] stream
//! with the same stack, plus the block-level tags the segmenter took out first.
//! The node shapes are upstream's, because the conformance corpus and the
//! schemas above both name them.
//!
//! # Five places this is not a straight transliteration
//!
//! - **The `inline` node is synthesised, not received.** markdown-it emits an
//!   `inline` token inside every block that has inline content, and marks the
//!   paragraph tokens of a tight list item hidden so the item's child *is* that
//!   token. pulldown-cmark has no such event. So an `inline` node is opened
//!   lazily, on the first inline-level event inside a block, and closed when the
//!   block closes. One rule reproduces both shapes, `paragraph > inline > text`
//!   and `item > inline > text`. Inside a block tag in a tight item it opens a
//!   paragraph too, because markdown-it hides only the item's own paragraphs.
//! - **Markers are read back from the source.** markdown-it puts `*` or `**` in
//!   `token.markup`; no pulldown-cmark event carries it. Container ranges cover
//!   their delimiters, so the marker is the first byte or two of the span. This
//!   is the borrow in [`Location`](crate::ast::Location) earning its place.
//! - **Setext headings are rewritten.** Upstream disables markdown-it's
//!   `lheading` rule, so `Testing\n-------` is a paragraph followed by a
//!   thematic break. pulldown-cmark has no such switch, but a setext heading is
//!   recognisable after the fact -- its span does not begin with `#` -- and its
//!   underline is recoverable from that span. The sibling case, upstream's
//!   disabled `code` rule, is *not* recoverable that way, and is
//!   `DIVERGENCES.md` entry 11.
//! - **A fence's content is literal by default.** `DIVERGENCES.md` entry 1
//!   inverts upstream: a fence opts *in* to tag processing with
//!   `{% process=true %}` rather than out of it.
//! - **A block tag inside a container arrives as HTML.** The segmenter masks it
//!   as a processing instruction so that the tokenizer places it (see
//!   [`segment`](super::segment)). An HTML block, inline HTML, or a line of an
//!   indented code block that starts with one is turned back into the tag.

use std::borrow::Cow;
use std::ops::Range;

use crate::ast::{AttributeLocation, Lines, Location, Node, NodeType, ValidationError, Value};
use crate::grammar::{Attribute, AttributeSpan, TagItem, parse_tag_spanned};
use crate::parse::ParseOptions;
use crate::parse::annotate::annotate;
use crate::parse::scan::{CLOSE, OPEN, contains_markdoc_tag_in_url, find_tag_end};
use crate::parse::segment::{Block, Segmentation, TagSpan};
use crate::parse::tokenizer::{Alignment, Container, ContainerKind, Event, Tokenizer};

/// A fenced or indented code block being accumulated.
///
/// A fence node cannot be built when it opens: its `content` attribute is
/// everything between the delimiters, and whether it has children depends on an
/// annotation in its info string that is only meaningful once the content is
/// known. So the events are collected and the node is built at the close.
struct Fence {
    span: Range<usize>,
    info: Option<String>,
    content: String,
    content_start: Option<usize>,
    /// Where the last line with content ends.
    content_end: usize,
    /// Whether this is the part of an indented code block after a tag that
    /// split it, whose span is the lines it ends up holding.
    split: bool,
}

/// Builds one document.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag tracks a different structure the tokenizer does not \
              report: the table head, the synthesised `tbody`, a synthesised \
              paragraph and a split one; none is a state of another"
)]
pub(crate) struct Builder<'s, 'o> {
    source: &'s str,
    lines: Lines<'s>,
    options: &'o ParseOptions<'s>,
    /// Open nodes, innermost last. Never empty: the document is at the bottom.
    stack: Vec<Node<'s>>,
    /// Index in [`Builder::stack`] of the block that owns the open inline run.
    ///
    /// Upstream threads this through `handleToken` as `inlineParent`. It is what
    /// makes `# Title {% #id %}` set `id` on the heading rather than on the text
    /// beside it, and what makes an annotation with no inline run above it the
    /// `no-inline-annotations` error.
    inline_parent: Option<usize>,
    /// The source span of the open inline run, for link validation.
    inline_span: Option<Range<usize>>,
    /// The underline of the open setext heading, when the open block is one.
    setext: Option<Range<usize>>,
    fence: Option<Fence>,
    /// Consecutive block-level HTML events, waiting to be read as one thing.
    ///
    /// pulldown-cmark reports an HTML block a line at a time. A comment is only
    /// a comment as a whole, so the lines are gathered and decided together.
    html_block: Option<Range<usize>>,
    /// Column alignments of the innermost open table.
    alignments: Vec<Alignment>,
    /// Which column the next cell is.
    column: usize,
    /// Whether the open row is in the header.
    in_head: bool,
    /// Whether a synthesised `tbody` is open.
    in_body: bool,
    /// Whether the open inline run sits in a paragraph synthesised for it.
    wrapped: bool,
    /// Whether the tokenizer's open paragraph was split at a tag, so that what
    /// follows the tag goes into a paragraph of its own.
    resumed: bool,
    /// The end of the last tag emitted before its span was over.
    ///
    /// A tag that spans lines reaches the parser as several events: a text run
    /// or an HTML block, a line at a time. The first emits the tag; the rest
    /// carry its masked body, and every content event before this offset is
    /// dropped. Containers still open and close, so the stack stays balanced.
    skip_to: usize,
    /// Where the text of each open link or image ends so far, innermost last.
    ///
    /// A destination and a title follow the text, so only a tag past this
    /// point can be one of theirs.
    links: Vec<usize>,
    /// Where each open block container's span ends, innermost last.
    ///
    /// An inline tag that runs past the end of its block is not a tag: upstream
    /// parses inline content one block at a time, so its `{%` is text there.
    ends: Vec<usize>,
}

impl<'s, 'o> Builder<'s, 'o> {
    pub(crate) fn new(source: &'s str, options: &'o ParseOptions<'s>) -> Builder<'s, 'o> {
        Builder {
            source,
            lines: Lines::new(source),
            options,
            stack: vec![Node::new(NodeType::Document)],
            inline_parent: None,
            inline_span: None,
            setext: None,
            fence: None,
            html_block: None,
            alignments: Vec::new(),
            column: 0,
            in_head: false,
            in_body: false,
            wrapped: false,
            resumed: false,
            skip_to: 0,
            links: Vec::new(),
            ends: Vec::new(),
        }
    }

    /// Walk the segmentation and produce the document node.
    pub(crate) fn run(
        mut self,
        segmentation: &Segmentation,
        tokenizer: &dyn Tokenizer,
    ) -> Node<'s> {
        for block in &segmentation.blocks {
            match block {
                Block::Markdown(range) => {
                    let text = segmentation.masked.get(range.clone()).unwrap_or("");
                    for (event, span) in tokenizer.tokenize(text) {
                        let span = range.start + span.start..range.start + span.end;
                        self.event(&event, span, segmentation);
                    }
                }
                Block::Tag(span) => {
                    // A raw-HTML block that ends right before a tag belongs
                    // where it was written. Flushed only at the next markdown
                    // event, it would attach to whichever node is open then:
                    // the tag that follows, the parent after a closing tag,
                    // the branch after `{% else /%}`.
                    self.flush_html_block();
                    self.close_inline();
                    self.tag(span, false);
                }
            }
        }
        self.finish()
    }

    /// Close every node still open, marking each as missing its closing tag.
    fn finish(mut self) -> Node<'s> {
        self.flush_html_block();
        self.close_inline();
        while self.stack.len() > 1 {
            let mut node = self.pop();
            let name = node.name().to_string();
            node.errors.push(ValidationError::missing_closing(&name));
            self.attach(node);
        }
        self.stack
            .pop()
            .unwrap_or_else(|| Node::new(NodeType::Document))
    }

    // ---- the stack -------------------------------------------------------

    fn top(&mut self) -> &mut Node<'s> {
        // The document is never popped, so this is never empty. Re-establishing
        // the invariant rather than indexing blindly keeps the promise that no
        // path in this crate panics on arbitrary input.
        if self.stack.is_empty() {
            self.stack.push(Node::new(NodeType::Document));
        }
        let last = self.stack.len() - 1;
        match self.stack.get_mut(last) {
            Some(node) => node,
            // Unreachable: the stack was just made non-empty. Returning a
            // scratch node would need a place to put it, so this leans on the
            // push above instead.
            None => unreachable!("the stack is non-empty"),
        }
    }

    fn open(&mut self, node: Node<'s>) {
        self.stack.push(node);
    }

    fn pop(&mut self) -> Node<'s> {
        if self.stack.len() > 1 {
            self.stack
                .pop()
                .unwrap_or_else(|| Node::new(NodeType::Node))
        } else {
            Node::new(NodeType::Node)
        }
    }

    /// Attach a finished node to whatever is open above it.
    ///
    /// A `{% slot "name" %}` goes into the parent's slot map instead of its
    /// children, which keeps a tag's ordinary content separable from its named
    /// regions. Upstream decides this when the node is created; here nodes are
    /// attached when they close, so the decision moves with them.
    fn attach(&mut self, node: Node<'s>) {
        if self.merge_text(&node) {
            return;
        }
        if self.options.slots
            && node.tag.as_deref() == Some("slot")
            && let Some(Value::String(name)) = node.get("primary")
        {
            let name = name.clone();
            self.top().slots.insert(name, node);
            return;
        }
        self.top().push(node);
    }

    /// Fold a text node into the text node before it, if there is one.
    ///
    /// markdown-it does this twice: the inline rule `fragments_join` merges
    /// adjacent text after emphasis is resolved, and the core rule `text_join`
    /// turns escapes and entities (`text_special`) into text and merges them
    /// with their neighbours. Before markdown-it 13 both were one inline rule,
    /// `text_collapse`. The result is observable: the corpus compares
    /// renderable trees, so two text children where upstream has one is a
    /// failed case even though the rendered HTML is identical. pulldown-cmark splits a run at every backslash escape,
    /// entity and unrecognised `<`, so without this a document with `\*` in it
    /// has a different tree shape from upstream's.
    ///
    /// Only plain strings merge. A text node whose `content` is a variable came
    /// from a tag, not from a text run, and upstream does not merge those either
    /// -- its token is a `variable` until the parser renames it, and both
    /// rules have already run by then.
    fn merge_text(&mut self, node: &Node<'s>) -> bool {
        if node.node_type != NodeType::Text
            || !node.errors.is_empty()
            || !matches!(node.get("content"), Some(Value::String(_)))
        {
            return false;
        }
        let Some(Value::String(addition)) = node.get("content") else {
            return false;
        };
        let addition = addition.clone();
        let end = node.location.map(|location| location.end);
        let text_end = node.location.map(|location| location.span().end);
        let source = self.source;
        let previous = self.top().children.last_mut();
        let Some(previous) = previous else {
            return false;
        };
        if previous.node_type != NodeType::Text || previous.inline != node.inline {
            return false;
        }
        let Some(Value::String(existing)) = previous.attributes.get_mut("content") else {
            return false;
        };
        existing.push_str(&addition);
        if let (Some(location), Some(end), Some(text_end)) = (&mut previous.location, end, text_end)
        {
            location.end = end;
            location.text = source.get(location.start.offset..text_end).unwrap_or("");
        }
        true
    }

    // ---- inline runs -----------------------------------------------------

    /// Open the synthesised `inline` node, unless one is already open.
    ///
    /// Inline content directly inside a block tag gets a paragraph around it
    /// as well. The tokenizer leaves the paragraph out because the list item
    /// the tag sits in is tight; markdown-it hides only the paragraphs that are
    /// the item's own children, so one inside the tag stays.
    fn open_inline(&mut self, span: &Range<usize>) {
        self.extend_inline_span(span);
        if self.inline_parent.is_some() {
            return;
        }
        let in_tag = {
            let top = self.top();
            top.node_type == NodeType::Tag && !top.inline
        };
        if in_tag || self.resumed {
            let paragraph = self.node(NodeType::Paragraph, span.clone());
            self.open(paragraph);
            self.wrapped = true;
        }
        let owner = self.stack.len() - 1;
        let mut node = Node::new(NodeType::Inline);
        node.lines = self.top().lines.clone();
        self.open(node);
        self.inline_parent = Some(owner);
    }

    fn extend_inline_span(&mut self, span: &Range<usize>) {
        self.inline_span = Some(match self.inline_span.take() {
            Some(open) => open.start.min(span.start)..open.end.max(span.end),
            None => span.clone(),
        });
    }

    /// Close the inline run, and everything opened inside it.
    ///
    /// An inline tag left unclosed at the end of a paragraph unwinds here rather
    /// than leaking into the next block, which is what markdown-it's separate
    /// inline pass gives upstream for free.
    fn close_inline(&mut self) {
        let span = self.inline_span.take();
        if self.inline_parent.take().is_none() {
            return;
        }
        while self.stack.len() > 1 {
            let is_inline = self.top().node_type == NodeType::Inline;
            let mut node = self.pop();
            if is_inline {
                if let Some(span) = &span {
                    self.validate_links(&mut node, span);
                }
                if std::mem::take(&mut self.wrapped) {
                    let mut paragraph = self.pop();
                    if let Some(span) = span {
                        paragraph.lines = self.line_pair(&span);
                        node.lines.clone_from(&paragraph.lines);
                        if self.options.location {
                            paragraph.location = Some(self.locate(span));
                        }
                    }
                    paragraph.push(node);
                    self.attach(paragraph);
                    return;
                }
                self.attach(node);
                return;
            }
            let name = node.name().to_string();
            node.errors.push(ValidationError::missing_closing(&name));
            self.attach(node);
        }
    }

    /// Upstream's link plugin, which is off by default.
    ///
    /// It reads the raw text of an inline run rather than a parsed link, because
    /// the interesting cases are the ones that do not parse as a link: a space
    /// inside `{% ... %}` ends a destination, so `[a](https://{% $x %})` is not a
    /// link at all, and it is still a URL with a variable in it.
    fn validate_links(&self, node: &mut Node<'s>, span: &Range<usize>) {
        let Some(protocols) = &self.options.validated_protocols else {
            return;
        };
        let protocols: Vec<&str> = protocols.iter().map(String::as_str).collect();
        if contains_markdoc_tag_in_url(self.slice(span), &protocols) {
            node.errors.push(ValidationError::href_format_invalid());
        }
    }

    // ---- node construction -----------------------------------------------

    /// A node of `node_type` covering `span`, located and line-mapped.
    fn node(&mut self, node_type: NodeType, span: Range<usize>) -> Node<'s> {
        let mut node = Node::new(node_type);
        node.inline = self.inline_parent.is_some();
        node.lines = self.line_pair(&span);
        if self.options.location {
            node.location = Some(self.locate(span));
        }
        node
    }

    fn locate(&self, span: Range<usize>) -> Location<'s> {
        self.lines.locate(span, self.options.file)
    }

    /// Translate attribute spans out of tag-body coordinates into the document's.
    ///
    /// `base` is where the parsed body starts in the source. This is the same
    /// translation a parse error gets a few lines below: the grammar counts from
    /// the trimmed body it was handed and knows nothing of the document around
    /// it.
    ///
    /// Empty when locations are switched off, so the result is either empty or
    /// one entry per span -- which is the invariant
    /// [`Node::annotation_locations`](crate::ast::Node::annotation_locations)
    /// promises its readers.
    fn attribute_locations(
        &self,
        spans: &[AttributeSpan],
        base: usize,
    ) -> Vec<AttributeLocation<'s>> {
        if !self.options.location {
            return Vec::new();
        }
        spans
            .iter()
            .map(|span| AttributeLocation {
                all: self.locate(base + span.all.start..base + span.all.end),
                value: span
                    .value
                    .as_ref()
                    .map(|value| self.locate(base + value.start..base + value.end)),
            })
            .collect()
    }

    /// Upstream's half-open `[first, last + 1]` line pair.
    fn line_pair(&self, span: &Range<usize>) -> Vec<usize> {
        let first = self.lines.position(span.start).line;
        let last = self
            .lines
            .position(span.end.saturating_sub(1).max(span.start))
            .line;
        vec![first, last + 1]
    }

    fn slice(&self, span: &Range<usize>) -> &'s str {
        self.source.get(span.clone()).unwrap_or("")
    }

    fn text_node(&mut self, content: String, span: Range<usize>) -> Node<'s> {
        let mut node = self.node(NodeType::Text, span);
        node.set("content", Value::String(content));
        node
    }

    // ---- tags ------------------------------------------------------------

    /// Turn one `{% ... %}` into whatever it means.
    ///
    /// `inline` says where it was found. It decides the `inline` flag and
    /// nothing else: the four things a tag body can be are the same in both
    /// positions.
    fn tag(&mut self, span: &TagSpan, inline: bool) {
        let body = self.body(span);
        let trimmed = body.trim();
        // The grammar is handed a trimmed body, as upstream's tokenizer hands
        // it one. Its error offsets are relative to that, so translating them
        // back to the document costs the leading whitespace that was cut.
        let body_start = span.inner.start + (body.len() - body.trim_start().len());

        match parse_tag_spanned(trimmed) {
            Ok((
                TagItem::TagOpen {
                    name,
                    attributes,
                    self_closing,
                },
                spans,
            )) => {
                let mut node = self.node(NodeType::Tag, span.outer.clone());
                node.inline = inline;
                node.tag = Some(name);
                node.lines = span.lines.to_vec();
                let locations = self.attribute_locations(&spans, body_start);
                annotate(&mut node, &attributes);
                node.annotation_locations = locations;
                if self_closing {
                    self.attach(node);
                } else {
                    self.open(node);
                }
            }
            Ok((TagItem::TagClose { name }, _)) => self.close_tag(&name, span, inline),
            Ok((TagItem::Annotation { attributes }, spans)) => {
                self.annotation(&attributes, &spans, body_start, span);
            }
            Ok((TagItem::Variable(value), _)) => {
                // Upstream maps the `variable` token type onto `text`, whose
                // `content` attribute then holds a value rather than a string.
                let mut node = self.node(NodeType::Text, span.outer.clone());
                node.inline = inline;
                node.set("content", value);
                self.attach(node);
            }
            Err(error) => {
                let mut node = self.node(NodeType::Error, span.outer.clone());
                node.inline = inline;
                let start = body_start + error.start();
                let end = (body_start + error.end()).max(start);
                let location = self.locate(start..end);
                node.errors
                    .push(ValidationError::parse_error(error.message()).at(location));
                self.attach(node);
            }
        }
    }

    /// A tag's body as the grammar reads it: the source, with the quote markers
    /// of its continuation lines blanked.
    ///
    /// Blanking rather than removing keeps every offset in the body an offset
    /// into the document, which is what error and attribute locations are
    /// computed from.
    fn body(&self, span: &TagSpan) -> Cow<'s, str> {
        let raw = self.slice(&span.inner);
        if span.quote_markers.is_empty() {
            return Cow::Borrowed(raw);
        }
        let mut bytes = raw.as_bytes().to_vec();
        for marker in &span.quote_markers {
            if let Some(byte) = marker
                .checked_sub(span.inner.start)
                .and_then(|offset| bytes.get_mut(offset))
            {
                *byte = b' ';
            }
        }
        // Only ASCII `>` was replaced, and by an ASCII space.
        Cow::Owned(String::from_utf8(bytes).unwrap_or_else(|_| raw.to_string()))
    }

    /// Emit a tag that the tokenizer may report over several events, and drop
    /// the rest of them.
    fn emit_spanning(&mut self, tag: &TagSpan, inline: bool) {
        self.tag(tag, inline);
        self.skip_to = self.skip_to.max(tag.outer.end);
    }

    /// Put back the tags in a link's or an image's destination and title.
    ///
    /// The tokenizer built both from masked text. They follow the link text, so
    /// only the tags between its end and the end of the link are theirs. An
    /// autolink, `<...>`, is the exception: its destination *is* its text.
    ///
    /// The node is the innermost open link or image, which is not always the top
    /// of the stack: an inline tag left open in the link text is above it.
    fn restore_destination(&mut self, span: &Range<usize>, segmentation: &Segmentation) {
        let text_end = self.links.pop().unwrap_or(span.start);
        let after = if self.slice(span).starts_with('<') {
            span.clone()
        } else {
            text_end.min(span.end)..span.end
        };
        if !overlaps(&segmentation.tags, &after) {
            return;
        }
        let Some(index) = self
            .stack
            .iter()
            .rposition(|node| matches!(node.node_type, NodeType::Link | NodeType::Image))
        else {
            return;
        };
        let Some(node) = self.stack.get(index) else {
            return;
        };
        // In source order: the destination is written before the title.
        let (names, mut values): (Vec<&str>, Vec<String>) = ["href", "src", "title"]
            .into_iter()
            .filter_map(|name| match node.get(name) {
                Some(Value::String(value)) => Some((name, value.clone())),
                _ => None,
            })
            .unzip();
        self.unmask(&mut values, &after, segmentation);
        if let Some(node) = self.stack.get_mut(index) {
            for (name, value) in names.into_iter().zip(values) {
                node.set(name, Value::String(value));
            }
        }
    }

    /// Put every tag in `span` back into strings the tokenizer built from
    /// masked text.
    ///
    /// A link destination, a title or a fence's info string is not a text run:
    /// the tokenizer builds it, resolving escapes, from text it read masked.
    /// Each tag is found by its masked form one line at a time, because a value
    /// may hold only some lines of a tag that spans several, and a line after
    /// the first without the markers and indentation the tokenizer strips.
    ///
    /// `values` are in source order, and the search only moves forward through
    /// them, so two tags that mask alike each go back where they were written.
    fn unmask(&self, values: &mut [String], span: &Range<usize>, segmentation: &Segmentation) {
        let tags = &segmentation.tags;
        let first = tags.partition_point(|tag| tag.outer.end <= span.start);
        // Where the next line of a tag is looked for: a value, and an offset
        // into it.
        let (mut field, mut from) = (0, 0);
        for tag in tags
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(|tag| tag.outer.start < span.end)
        {
            let end = tag.outer.end.min(span.end);
            let mut start = tag.outer.start.max(span.start);
            while start < end {
                let stop = self
                    .source
                    .get(start..end)
                    .and_then(|rest| rest.find('\n'))
                    .map_or(end, |offset| start + offset);
                // Masking keeps a continuation line's container prefix as
                // written, and only there is a leading `>` or space unmasked.
                let lead = if start > tag.outer.start {
                    segmentation.masked.get(start..stop).map_or(0, |line| {
                        line.len() - line.trim_start_matches([' ', '\t', '>']).len()
                    })
                } else {
                    0
                };
                if let (Some(masked), Some(written)) = (
                    segmentation.masked.get(start + lead..stop),
                    self.source.get(start + lead..stop),
                ) && masked != written
                    && let Some((index, at)) = values
                        .get(field)
                        .and_then(|value| find_masked(value, from, masked))
                        .map(|at| (field, at))
                        .or_else(|| {
                            (field + 1..values.len()).find_map(|index| {
                                values
                                    .get(index)
                                    .and_then(|value| find_masked(value, 0, masked))
                                    .map(|at| (index, at))
                            })
                        })
                    && let Some(value) = values.get_mut(index)
                {
                    value.replace_range(at..at + masked.len(), written);
                    (field, from) = (index, at + written.len());
                }
                start = stop + 1;
            }
        }
    }

    /// Close the innermost open tag of this name, or record that there is none.
    fn close_tag(&mut self, name: &str, span: &TagSpan, inline: bool) {
        let matches = {
            let top = self.top();
            top.node_type == NodeType::Tag && top.tag.as_deref() == Some(name)
        };
        if matches {
            let mut node = self.pop();
            // Upstream appends the closing token's line pair to the node's, so
            // `lines` ends up `[open_first, open_last, close_first, close_last]`.
            node.lines.extend_from_slice(&span.lines);
            if let Some(open) = node.location {
                node.location = Some(Location {
                    end: self.lines.position(span.outer.end),
                    text: self.slice(&(open.start.offset..span.outer.end)),
                    ..open
                });
            }
            self.attach(node);
            return;
        }
        let mut node = self.node(NodeType::Tag, span.outer.clone());
        node.inline = inline;
        node.tag = Some(name.to_string());
        node.errors.push(ValidationError::missing_opening(name));
        self.attach(node);
    }

    /// Apply a bare `{% #id .cls %}` to the block that owns the inline run.
    ///
    /// The locations are built before the owner is borrowed mutably, because
    /// they read the line index that the borrow would otherwise lock out. They
    /// extend rather than replace: a block can carry more than one annotation,
    /// and every annotation reaching a block is positioned.
    fn annotation(
        &mut self,
        attributes: &[Attribute],
        spans: &[AttributeSpan],
        body_start: usize,
        span: &TagSpan,
    ) {
        // Only an annotation that lands on a block keeps its locations. The
        // error path below never reads them, so a bare annotation with no inline
        // run above it does not pay for the line-table lookups.
        let locations = if self.inline_parent.is_some() {
            self.attribute_locations(spans, body_start)
        } else {
            Vec::new()
        };
        if let Some(owner) = self.inline_parent
            && let Some(node) = self.stack.get_mut(owner)
        {
            annotate(node, attributes);
            node.annotation_locations.extend(locations);
            return;
        }
        let location = self.locate(span.outer.clone());
        let name = self.top().name().to_string();
        self.top()
            .errors
            .push(ValidationError::no_inline_annotations(&name).at(location));
    }

    // ---- events ----------------------------------------------------------

    fn event(&mut self, event: &Event<'_>, span: Range<usize>, segmentation: &Segmentation) {
        match event {
            Event::Start(container) if !is_inline(container.kind()) => self.ends.push(span.end),
            Event::End(kind) if !is_inline(*kind) => {
                self.ends.pop();
            }
            _ => {}
        }
        if self.inside_emitted_tag(event, &span) {
            return;
        }
        if !matches!(
            event,
            Event::End(ContainerKind::Link | ContainerKind::Image)
        ) {
            for end in &mut self.links {
                *end = (*end).max(span.end);
            }
        }
        if !matches!(event, Event::Html(_)) {
            self.flush_html_block();
        }
        // A code block swallows its own text: the content is one attribute, and
        // whether it also has children is decided at the close.
        if self.fence.is_some() {
            match event {
                Event::Text(text) => self.code_text(text, span, &segmentation.tags),
                Event::End(ContainerKind::CodeBlock) => self.close_fence(),
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(container) => self.start(container, span, segmentation),
            Event::End(kind) => self.end(*kind, span, segmentation),
            Event::Text(text) => self.text(text, span, &segmentation.tags),
            Event::Code(_) => {
                self.open_inline(&span);
                let content = code_span_content(self.slice(&span));
                let mut node = self.node(NodeType::Code, span);
                node.set("content", Value::String(content));
                self.attach(node);
            }
            Event::Html(_) => {
                if let Some(tag) = nested_at(self.source, &segmentation.tags, &span) {
                    self.flush_html_block();
                    self.close_inline();
                    self.emit_spanning(tag, false);
                    return;
                }
                self.close_inline();
                match self.html_block.take() {
                    // A tokenizer reports an HTML block one line at a time, so
                    // the lines are gathered back into one node. Where one block
                    // ends and the next begins has to be read off the spans:
                    // the lines of a block are contiguous, and two blocks are
                    // separated by at least the blank line between them. The
                    // block wrapper itself carries nothing and is dropped (see
                    // `pulldown::container`), so it cannot say instead -- and
                    // without this test two `<!-- ... -->` comments a blank line
                    // apart merge into one comment whose content contains the
                    // first one's terminator.
                    Some(open) if open.end == span.start => {
                        self.html_block = Some(open.start..span.end);
                    }
                    Some(open) => {
                        self.html_block = Some(open);
                        self.flush_html_block();
                        self.html_block = Some(span);
                    }
                    None => self.html_block = Some(span),
                }
            }
            Event::InlineHtml(_) => {
                if let Some(tag) = nested_at(self.source, &segmentation.tags, &span) {
                    if self.begins_line(tag) && self.splittable() {
                        self.split_paragraph(tag);
                    } else {
                        self.open_inline(&span);
                        self.emit_spanning(tag, true);
                    }
                    return;
                }
                self.open_inline(&span);
                let node = self.html_node(span);
                self.attach(node);
            }
            // A run never opens with a soft break. One that would has lost what
            // came before it on its line: the body of a tag, or a tag split
            // off. A hard break can open one, as `\` alone on a paragraph's
            // first line, so it is dropped only at the end of a tag's line.
            Event::SoftBreak if self.inline_parent.is_none() => {}
            Event::HardBreak if self.inline_parent.is_none() && self.ends_tag_line(&span) => {}
            Event::SoftBreak => {
                self.open_inline(&span);
                let node = self.node(NodeType::Softbreak, span);
                self.attach(node);
            }
            Event::HardBreak => {
                self.open_inline(&span);
                let node = self.node(NodeType::Hardbreak, span);
                self.attach(node);
            }
            Event::Rule => {
                self.close_inline();
                let node = self.node(NodeType::Hr, span);
                self.attach(node);
            }
        }
    }

    /// Turn the gathered HTML block into nodes.
    ///
    /// A comment becomes a `comment` node when comments are enabled. Anything
    /// else -- a comment when they are not, or any other raw HTML -- is what
    /// markdown-it with `html: false` makes of the same lines: it has no HTML
    /// block rule, so they are paragraphs of literal text, one per run of
    /// non-blank lines, each line a text node and the lines joined by soft
    /// breaks.
    ///
    /// This used to attach the raw markup as one bare `text` node in the
    /// block position, a shape upstream never produces and one the `document`
    /// schema rejects: every document with an HTML comment and a tag failed
    /// validation with "Can't nest 'text' in 'document'".
    fn flush_html_block(&mut self) {
        let Some(span) = self.html_block.take() else {
            return;
        };
        if let Some(comment) = self.comment_node(&span) {
            self.attach(comment);
            return;
        }
        for lines in paragraph_runs(self.source, &span) {
            if let Some(node) = self.literal_paragraph(&lines) {
                self.attach(node);
            }
        }
    }

    /// A paragraph of literal text, one text node per line, as markdown-it
    /// builds a paragraph from lines it does not parse as anything else.
    /// `None` for no lines, which `paragraph_runs` never yields.
    fn literal_paragraph(&mut self, lines: &[Range<usize>]) -> Option<Node<'s>> {
        let span = lines.first()?.start..lines.last()?.end;
        let mut paragraph = self.node(NodeType::Paragraph, span);
        let mut inline = Node::new(NodeType::Inline);
        inline.lines.clone_from(&paragraph.lines);
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                let mut breaker = Node::new(NodeType::Softbreak);
                breaker.inline = true;
                inline.push(breaker);
            }
            let content = self.slice(line).to_string();
            let mut text = self.text_node(content, line.clone());
            text.inline = true;
            inline.push(text);
        }
        paragraph.push(inline);
        Some(paragraph)
    }

    /// The `comment` node for `span`, when comments are enabled and the
    /// markup is one comment.
    fn comment_node(&mut self, span: &Range<usize>) -> Option<Node<'s>> {
        if !self.options.allow_comments {
            return None;
        }
        let inner = self
            .slice(span)
            .trim()
            .strip_prefix("<!--")
            .and_then(|rest| rest.strip_suffix("-->"))?;
        let content = inner.trim().to_string();
        let mut node = self.node(NodeType::Comment, span.clone());
        node.set("content", Value::String(content));
        Some(node)
    }

    /// Inline HTML: a comment, or the literal text of something that is not
    /// one.
    ///
    /// Upstream runs markdown-it with `html: false`, so raw HTML is text there,
    /// and adds a comment rule when `allowComments` is on. Both are reproduced:
    /// a comment becomes a `comment` node when comments are enabled, and
    /// anything else -- including a comment when they are not -- becomes text
    /// carrying the markup verbatim, inside the paragraph it sits in.
    fn html_node(&mut self, span: Range<usize>) -> Node<'s> {
        if let Some(comment) = self.comment_node(&span) {
            return comment;
        }
        let raw = self.slice(&span).to_string();
        self.text_node(raw, span)
    }

    fn start(
        &mut self,
        container: &Container<'_>,
        span: Range<usize>,
        segmentation: &Segmentation,
    ) {
        match container {
            Container::Emphasis
            | Container::Strong
            | Container::Strikethrough
            | Container::Link { .. }
            | Container::Image { .. } => self.open_inline(&span),
            _ => self.close_inline(),
        }

        match container {
            Container::Paragraph => {
                let node = self.node(NodeType::Paragraph, span);
                self.open(node);
            }
            Container::Heading { level } => self.open_heading(*level, span),
            Container::Blockquote => {
                let node = self.node(NodeType::Blockquote, span);
                self.open(node);
            }
            Container::List { ordered, start } => self.open_list(*ordered, *start, span),
            Container::Item => {
                let node = self.node(NodeType::Item, span);
                self.open(node);
            }
            Container::CodeBlock { info } => {
                // The info string is on the opening line.
                let opening = span.start
                    ..self
                        .slice(&span)
                        .find('\n')
                        .map_or(span.end, |offset| span.start + offset);
                let info = info.as_ref().map(|info| {
                    let mut values = [info.to_string()];
                    self.unmask(&mut values, &opening, segmentation);
                    let [info] = values;
                    info
                });
                self.fence = Some(Fence {
                    content_end: span.start,
                    span,
                    info,
                    content: String::new(),
                    content_start: None,
                    split: false,
                });
            }
            Container::Emphasis => {
                let marker = marker(self.slice(&span), 1);
                let mut node = self.node(NodeType::Em, span);
                node.set("marker", Value::String(marker));
                self.open(node);
            }
            Container::Strong => {
                let marker = marker(self.slice(&span), 2);
                let mut node = self.node(NodeType::Strong, span);
                node.set("marker", Value::String(marker));
                self.open(node);
            }
            Container::Strikethrough => {
                let node = self.node(NodeType::S, span);
                self.open(node);
            }
            Container::Link { destination, title } => {
                self.links.push(span.start);
                let mut node = self.node(NodeType::Link, span);
                node.set("href", Value::String(destination.to_string()));
                if !title.is_empty() {
                    node.set("title", Value::String(title.to_string()));
                }
                self.open(node);
            }
            Container::Image { destination, title } => {
                self.links.push(span.start);
                let mut node = self.node(NodeType::Image, span);
                node.set("alt", Value::String(String::new()));
                node.set("src", Value::String(destination.to_string()));
                if !title.is_empty() {
                    node.set("title", Value::String(title.to_string()));
                }
                self.open(node);
            }
            Container::Table { .. }
            | Container::TableHead
            | Container::TableRow
            | Container::TableCell => self.start_table(container, span),
        }
    }

    /// The table containers, which need two wrappers markdown-it has and
    /// pulldown-cmark does not.
    ///
    /// markdown-it puts a `tr` inside the `thead` and a `tbody` around the body
    /// rows. pulldown-cmark's `TableHead` *is* the header row, and its body rows
    /// are direct children of the table. Both wrappers are synthesised here, so
    /// a table schema sees the shape upstream documents.
    fn start_table(&mut self, container: &Container<'_>, span: Range<usize>) {
        match container {
            Container::Table { alignments } => {
                self.alignments.clone_from(alignments);
                let node = self.node(NodeType::Table, span);
                self.open(node);
            }
            Container::TableHead => {
                self.in_head = true;
                let node = self.node(NodeType::Thead, span);
                self.open(node);
                let row = self.node(NodeType::Tr, self.span_of_top());
                self.open(row);
                self.column = 0;
            }
            Container::TableRow => {
                if !self.in_body {
                    let body = self.node(NodeType::Tbody, span.clone());
                    self.open(body);
                    self.in_body = true;
                }
                let node = self.node(NodeType::Tr, span);
                self.open(node);
                self.column = 0;
            }
            _ => {
                let node_type = if self.in_head {
                    NodeType::Th
                } else {
                    NodeType::Td
                };
                let align = self
                    .alignments
                    .get(self.column)
                    .copied()
                    .unwrap_or(Alignment::None)
                    .as_str();
                let mut node = self.node(node_type, span);
                if let Some(align) = align {
                    node.set("align", Value::String(align.to_string()));
                }
                self.column += 1;
                self.open(node);
            }
        }
    }

    /// The span of the node on top of the stack, for a synthesised sibling.
    fn span_of_top(&self) -> Range<usize> {
        self.stack
            .last()
            .and_then(|node| node.location.map(|location| location.span()))
            .unwrap_or(0..0)
    }

    fn end(&mut self, kind: ContainerKind, span: Range<usize>, segmentation: &Segmentation) {
        match kind {
            ContainerKind::Emphasis | ContainerKind::Strong | ContainerKind::Strikethrough => {
                let node = self.pop();
                self.attach(node);
                return;
            }
            ContainerKind::Link => {
                self.restore_destination(&span, segmentation);
                let node = self.pop();
                self.attach(node);
                return;
            }
            ContainerKind::Image => {
                self.restore_destination(&span, segmentation);
                // Upstream treats an image as a leaf: its children become the
                // `alt` text and are then dropped.
                let mut node = self.pop();
                let alt = collect_text(&node);
                node.children.clear();
                node.set("alt", Value::String(alt));
                self.attach(node);
                return;
            }
            _ => self.close_inline(),
        }

        match kind {
            ContainerKind::Heading => self.close_heading(),
            ContainerKind::TableHead => {
                // Close the synthesised `tr` as well as the `thead`.
                self.in_head = false;
                let row = self.pop();
                self.attach(row);
                let node = self.pop();
                self.attach(node);
            }
            ContainerKind::Table => {
                if self.in_body {
                    self.in_body = false;
                    let body = self.pop();
                    self.attach(body);
                }
                let node = self.pop();
                self.attach(node);
            }
            // The tokenizer's paragraph was closed where a tag split it.
            ContainerKind::Paragraph if self.resumed => self.resumed = false,
            _ => {
                self.close_block_tags();
                let node = self.pop();
                // A container the tokenizer opened inside a tag's span held only
                // the tag's masked body, which was dropped.
                if node.children.is_empty() && span.start < self.skip_to {
                    return;
                }
                self.attach(node);
            }
        }
    }

    /// Whether nothing but indentation and quote markers precede a tag on its
    /// line.
    ///
    /// A `>` the tokenizer read as text rather than as a marker, past four
    /// columns of indentation or on a lazy line, is a text node before the tag.
    fn begins_line(&self, tag: &TagSpan) -> bool {
        if self.inline_parent.is_some()
            && self
                .stack
                .last()
                .and_then(|top| top.children.last())
                .is_some_and(|child| {
                    !matches!(child.node_type, NodeType::Softbreak | NodeType::Hardbreak)
                })
        {
            return false;
        }
        let head = self.source.get(..tag.outer.start).unwrap_or("");
        let line = head
            .rfind('\n')
            .map_or(head, |newline| head.get(newline + 1..).unwrap_or(""));
        line.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'>'))
    }

    /// Whether the open inline run belongs to a paragraph the parser can split,
    /// or to a tight list item, which has no paragraph to split.
    ///
    /// A heading's run cannot be split, and neither can a paragraph that is
    /// really a setext heading: the tokenizer ends either with a heading, not a
    /// paragraph, so there would be no paragraph end to resume from. Nor can a
    /// run with an emphasis or a link open across the tag's line, whose end the
    /// tokenizer has already matched past it.
    fn splittable(&self) -> bool {
        let Some(owner) = self.inline_parent.and_then(|index| self.stack.get(index)) else {
            return true;
        };
        if self
            .stack
            .last()
            .is_none_or(|top| top.node_type != NodeType::Inline)
        {
            return false;
        }
        match owner.node_type {
            NodeType::Paragraph => self.setext.is_none(),
            NodeType::Item => true,
            _ => false,
        }
    }

    /// Split the open paragraph at a tag alone on one of its continuation
    /// lines.
    ///
    /// The tokenizer reads a line indented four columns past a paragraph as
    /// part of it, and so does stock markdown-it. Upstream's corpus runs with
    /// `allowIndentation` on (`DIVERGENCES.md` entry 8), which lets the tag
    /// interrupt the paragraph there, and the segmenter this replaced always
    /// split at such a line. So the paragraph ends before the tag, and what
    /// follows the tag is a paragraph of its own.
    fn split_paragraph(&mut self, tag: &TagSpan) {
        // The break before the tag's line ends the paragraph instead. No hard
        // break ends a paragraph: two spaces go with it, and a backslash is
        // text, as at the end of any paragraph.
        let top = self.top();
        let hard = top.node_type == NodeType::Inline
            && top
                .children
                .pop_if(|child| {
                    matches!(child.node_type, NodeType::Softbreak | NodeType::Hardbreak)
                })
                .is_some_and(|child| child.node_type == NodeType::Hardbreak);
        let head = self.slice(&(0..tag.outer.start));
        let previous = head.get(..head.rfind('\n').unwrap_or(0)).unwrap_or("");
        let previous = previous.strip_suffix('\r').unwrap_or(previous);
        if hard && previous.ends_with('\\') {
            let slash = previous.len() - 1;
            let node = self.text_node("\\".to_string(), slash..previous.len());
            self.attach(node);
        }
        self.close_inline();
        // In a tight list item there is no paragraph to close, and no paragraph
        // end will come to resume from.
        if !self.resumed && self.top().node_type == NodeType::Paragraph {
            let node = self.pop();
            self.attach(node);
            self.resumed = true;
        }
        self.emit_spanning(tag, false);
    }

    /// Whether an event carries nothing but the body of a tag already emitted.
    ///
    /// An HTML block line runs on past the tag to its newline, so only what
    /// follows the tag decides. Containers never qualify: dropping one would
    /// unbalance the stack.
    fn inside_emitted_tag(&self, event: &Event<'_>, span: &Range<usize>) -> bool {
        span.start < self.skip_to
            && !matches!(event, Event::Start(_) | Event::End(_))
            && self
                .source
                .get(self.skip_to.min(span.end)..span.end)
                .is_none_or(|rest| rest.trim().is_empty())
    }

    /// Whether a break follows the last tag emitted, with nothing but spaces
    /// between them: the tag ends its line, and the break ends that.
    fn ends_tag_line(&self, span: &Range<usize>) -> bool {
        self.skip_to > 0
            && self
                .source
                .get(self.skip_to..span.start)
                .is_some_and(|gap| gap.bytes().all(|byte| matches!(byte, b' ' | b'\t')))
    }

    /// Close every block tag still open inside the container that is closing.
    ///
    /// Upstream's parser closes only the node on top of its stack, so there the
    /// container's closing token becomes a `missing-opening` error, the
    /// container stays open, and the rest of the document lands inside the tag.
    /// Here the tag closes with its container and reports `missing-closing`, as
    /// an inline tag left open at the end of its paragraph does.
    fn close_block_tags(&mut self) {
        while self.stack.len() > 1 && self.top().node_type == NodeType::Tag {
            let mut node = self.pop();
            let name = node.name().to_string();
            node.errors.push(ValidationError::missing_closing(&name));
            self.attach(node);
        }
    }

    // ---- headings --------------------------------------------------------

    /// Open a heading, or a paragraph if it is really a setext heading.
    ///
    /// Upstream disables markdown-it's `lheading` rule, so `Testing\n---` is a
    /// paragraph and a thematic break. A setext heading is the one pulldown-cmark
    /// node whose rule can be undone after the fact: nothing else can produce a
    /// heading whose span does not begin with `#`.
    fn open_heading(&mut self, level: u8, span: Range<usize>) {
        if self.slice(&span).trim_start().starts_with('#') {
            let mut node = self.node(NodeType::Heading, span);
            node.set("level", Value::Number(f64::from(level)));
            self.open(node);
            return;
        }
        let underline = last_line(self.source, &span);
        let node = self.node(NodeType::Paragraph, span);
        self.open(node);
        self.setext = Some(underline);
    }

    fn close_heading(&mut self) {
        let Some(underline) = self.setext.take() else {
            let node = self.pop();
            self.attach(node);
            return;
        };
        let text = self.slice(&underline);
        let mut node = self.pop();
        if text.trim_start().starts_with('=') {
            // `Testing\n===` is one paragraph: the underline is a continuation
            // line, joined to it by a soft break.
            let mut breaker = Node::new(NodeType::Softbreak);
            breaker.inline = true;
            let mut trailing = Node::new(NodeType::Text);
            trailing.inline = true;
            trailing.set("content", Value::String(text.trim_end().to_string()));
            if let Some(inline) = node
                .children
                .iter_mut()
                .find(|child| child.node_type == NodeType::Inline)
            {
                inline.push(breaker);
                inline.push(trailing);
            }
            self.attach(node);
            return;
        }
        // `Testing\n---` is a paragraph and a thematic break.
        self.attach(node);
        let rule = self.node(NodeType::Hr, underline);
        self.attach(rule);
    }

    // ---- lists -----------------------------------------------------------

    fn open_list(&mut self, ordered: bool, start: Option<u64>, span: Range<usize>) {
        let marker = list_marker(self.slice(&span), ordered);
        let mut node = self.node(NodeType::List, span);
        node.set("ordered", Value::Boolean(ordered));
        // Upstream reports `start` only when markdown-it does, and markdown-it
        // omits the attribute for a list beginning at 1. `parser.test.ts` asserts
        // exactly that.
        if let Some(start) = start.filter(|&start| ordered && start != 1) {
            #[allow(
                clippy::cast_precision_loss,
                reason = "the grammar's one numeric type is f64; a list ordinal \
                          large enough to lose precision is not a document"
            )]
            node.set("start", Value::Number(start as f64));
        }
        node.set("marker", Value::String(marker));
        self.open(node);
    }

    // ---- fences ----------------------------------------------------------

    /// Add one line of a code block's content.
    ///
    /// The tokenizer's text is used, as everywhere, unless a tag overlaps the
    /// line. Then the line is read from the source: the tokenizer saw the tag
    /// masked, and a code block's content is literal.
    fn code_text(&mut self, text: &str, span: Range<usize>, tags: &[TagSpan]) {
        let indented = self
            .fence
            .as_ref()
            .is_some_and(|fence| fence.info.is_none());
        if indented && let Some(tag) = nested_at(self.source, tags, &span) {
            self.split_code(tag);
            return;
        }
        let piece = if overlaps(tags, &span) {
            self.slice(&span)
        } else {
            text
        };
        let Some(fence) = &mut self.fence else {
            return;
        };
        let blank = piece.trim().is_empty();
        // A part split off after a tag starts at its first line with content, as
        // any indented code block does.
        if fence.split && fence.content.is_empty() && blank {
            return;
        }
        if fence.content_start.is_none() {
            fence.content_start = Some(span.start);
            if fence.split {
                fence.span.start = span.start;
            }
        }
        fence.content.push_str(piece);
        if !blank {
            fence.content_end = span.end;
        }
    }

    /// Split an indented code block at a tag alone on one of its lines.
    ///
    /// Upstream has no indented code block -- it disables markdown-it's `code`
    /// rule -- so a tag alone on its line is a block tag at any indentation.
    /// pulldown-cmark reads four columns in as code (`DIVERGENCES.md` entry 11).
    /// The lines around the tag stay code, and the tag is a tag.
    fn split_code(&mut self, tag: &TagSpan) {
        let Some(mut before) = self.fence.take() else {
            return;
        };
        let end = before.span.end;
        trim_blank_lines(&mut before.content);
        if !before.content.is_empty() {
            before.span.end = before.content_end;
            self.fence = Some(before);
            self.close_fence();
        }
        self.emit_spanning(tag, false);
        self.fence = Some(Fence {
            span: tag.outer.end..end,
            info: None,
            content: String::new(),
            content_start: None,
            content_end: tag.outer.end,
            split: true,
        });
    }

    /// Build the code-block node now that its content is known.
    fn close_fence(&mut self) {
        let Some(mut fence) = self.fence.take() else {
            return;
        };
        if fence.split {
            trim_blank_lines(&mut fence.content);
            fence.span.end = fence.content_end;
        }
        // An indented code block is never empty as the tokenizer reports it. One
        // that is empty here held nothing but the masked body of a tag, or
        // nothing after a tag that split it.
        if fence.info.is_none() && fence.content.is_empty() {
            return;
        }
        let mut node = self.node(NodeType::Fence, fence.span.clone());
        node.set("content", Value::String(fence.content.clone()));

        let info = fence.info.unwrap_or_default();
        if let Some(language) = info.split(' ').next()
            && !language.is_empty()
            && language != OPEN
        {
            node.set("language", Value::String(language.to_string()));
        }

        // The info string may carry an annotation: ```` ```js {% #id %} ````.
        // Upstream parses it in a core pass over the token stream; here it is
        // read straight off the fence line, which is the same information.
        if let Some(start) = info.find(OPEN)
            && let Some(end) = find_tag_end(&info, start)
        {
            let body = info.get(start + OPEN.len()..end).unwrap_or("").trim();
            // The spans are dropped here, and the node keeps no attribute
            // locations. `info` is the tokenizer's reconstruction of the info
            // string rather than a slice of the document, so there is no base
            // offset to translate them against and no honest position to report.
            match parse_tag_spanned(body) {
                Ok((
                    TagItem::Annotation { attributes } | TagItem::TagOpen { attributes, .. },
                    _,
                )) => {
                    annotate(&mut node, &attributes);
                }
                Ok(_) => {}
                Err(error) => node.errors.push(ValidationError::new(
                    "fence-tag-error",
                    crate::ast::ErrorLevel::Error,
                    format!("Syntax error in fence tag: {}", error.message()),
                )),
            }
        }

        let process = matches!(node.get("process"), Some(Value::Boolean(true)));
        if !process {
            self.attach(node);
            return;
        }

        // `DIVERGENCES.md` entry 1: this is the opt-in path, not the default.
        let base = fence.content_start.unwrap_or(fence.span.start);
        let content = fence.content.clone();
        self.open(node);
        self.fence_children(&content, base);
        let node = self.pop();
        self.attach(node);
    }

    /// Upstream's `parseTags`: split fence content into text and tags.
    ///
    /// The one subtlety is upstream's whitespace rule. A tag that is alone on
    /// its line absorbs the newline before it, so `foo\n{% x /%}\nbar` yields
    /// `"foo\n"`, the tag, `"\nbar"` rather than an empty line either side.
    fn fence_children(&mut self, content: &str, base: usize) {
        let mut cursor = 0;
        let mut pos = 0;
        while pos < content.len() {
            if content
                .get(pos..)
                .is_none_or(|rest| !rest.starts_with(OPEN))
            {
                pos += 1;
                continue;
            }
            let Some(end) = find_tag_end(content, pos) else {
                pos += OPEN.len();
                continue;
            };
            let outer = pos..end + CLOSE.len();
            let line_end = content
                .get(end..)
                .and_then(|rest| rest.find('\n'))
                .map_or(content.len(), |offset| end + offset);
            let preceding_end = match content.get(..pos).and_then(|head| head.rfind('\n')) {
                Some(line_start)
                    if content.get(line_start..line_end).is_some_and(|line| {
                        line.trim() == content.get(outer.clone()).unwrap_or("")
                    }) =>
                {
                    line_start
                }
                _ => pos,
            };
            self.push_fence_text(content, cursor..preceding_end, base);
            let document_span = base + outer.start..base + outer.end;
            let pair = self.line_pair(&document_span);
            let span = TagSpan {
                outer: document_span,
                inner: base + pos + OPEN.len()..base + end,
                lines: [
                    pair.first().copied().unwrap_or(0),
                    pair.get(1).copied().unwrap_or(0),
                ],
                quote_markers: Vec::new(),
                nested: false,
            };
            self.tag(&span, false);
            cursor = outer.end;
            pos = outer.end;
        }
        self.push_fence_text(content, cursor..content.len(), base);
    }

    fn push_fence_text(&mut self, content: &str, range: Range<usize>, base: usize) {
        let Some(text) = content.get(range.clone()) else {
            return;
        };
        if text.is_empty() {
            // Upstream emits the empty text token and `handleToken` drops it.
            return;
        }
        let node = self.text_node(text.to_string(), base + range.start..base + range.end);
        self.attach(node);
    }

    // ---- text ------------------------------------------------------------

    /// Emit a text run, splitting it around any inline tags it contains.
    ///
    /// With no tag in it the tokenizer's own string is used, so backslash
    /// escapes and character entities are already resolved. With any part of a
    /// tag in it the pieces come from the original source instead -- the
    /// tokenizer only ever saw filler there -- and carry a backslash unescape of
    /// their own. Entity references inside such a run are left as written, which
    /// is the one place the two paths differ and is why the fast path is the
    /// default rather than an optimisation.
    ///
    /// A tag that spans lines reaches here in pieces, one per line. It is
    /// emitted from the run it starts in, and the pieces after it are dropped.
    fn text(&mut self, text: &str, span: Range<usize>, tags: &[TagSpan]) {
        // A run that starts inside a tag already emitted carries the tag's
        // masked tail first; only what follows the tag is text.
        let tail = span.start < self.skip_to;
        let span = if tail { self.skip_to..span.end } else { span };
        self.open_inline(&span);
        if !tail && !overlaps(tags, &span) {
            if !text.is_empty() {
                let node = self.text_node(text.to_string(), span);
                self.attach(node);
            }
            return;
        }

        let limit = self.ends.last().copied().unwrap_or(self.source.len());
        let first = tags.partition_point(|tag| tag.outer.end <= span.start);
        let mut cursor = span.start;
        for tag in tags
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(|tag| tag.outer.start < span.end)
        {
            // A tag that began before this run, or that runs past the end of the
            // block, was never one: its text is literal.
            if tag.outer.start < span.start || tag.outer.end > limit {
                continue;
            }
            self.push_source_text(cursor..tag.outer.start);
            if tag.outer.end > span.end {
                self.emit_spanning(tag, true);
                return;
            }
            self.tag(tag, true);
            cursor = tag.outer.end;
        }
        self.push_source_text(cursor..span.end);
    }

    fn push_source_text(&mut self, span: Range<usize>) {
        let text = unescape(self.slice(&span));
        if text.is_empty() {
            return;
        }
        let node = self.text_node(text, span);
        self.attach(node);
    }
}

/// The delimiter a span opens with, up to `len` bytes.
///
/// `*a*` opens with `*`, `**a**` with `**`. markdown-it reports this as
/// `token.markup`; here it is the head of the container's span, which is why
/// container ranges have to cover their delimiters.
fn marker(span: &str, len: usize) -> String {
    span.chars().take(len).collect()
}

/// The bullet or delimiter a list is written with.
///
/// markdown-it's `token.markup` is the bullet for an unordered list and the `.`
/// or `)` for an ordered one.
fn list_marker(span: &str, ordered: bool) -> String {
    let trimmed = span.trim_start();
    if !ordered {
        return trimmed.chars().next().map(String::from).unwrap_or_default();
    }
    trimmed
        .chars()
        .find(|character| *character == '.' || *character == ')')
        .map_or_else(|| ".".to_string(), String::from)
}

/// The range of the last line of `span`.
fn last_line(source: &str, span: &Range<usize>) -> Range<usize> {
    let end = span.end;
    let start = source
        .get(span.start..end)
        .and_then(|text| text.trim_end().rfind('\n'))
        .map_or(span.start, |offset| span.start + offset + 1);
    start..end
}

/// Every text descendant of a node, concatenated.
///
/// Used for an image's `alt`, which upstream takes from `token.content` -- the
/// raw inline text of the image's children.
fn collect_text(node: &Node<'_>) -> String {
    let mut out = String::new();
    for child in node.walk() {
        if let Some(Value::String(text)) = child.get("content") {
            out.push_str(text);
        }
    }
    out
}

/// Whether a container kind is inline.
const fn is_inline(kind: ContainerKind) -> bool {
    matches!(
        kind,
        ContainerKind::Emphasis
            | ContainerKind::Strong
            | ContainerKind::Strikethrough
            | ContainerKind::Link
            | ContainerKind::Image
    )
}

/// The nested tag whose placeholder starts the content of `span`, if one does.
///
/// An HTML block line or a code line may begin with indentation the tokenizer
/// kept, so whitespace before the placeholder is allowed.
fn nested_at<'t>(source: &str, tags: &'t [TagSpan], span: &Range<usize>) -> Option<&'t TagSpan> {
    let tag = tags.get(tags.partition_point(|tag| tag.outer.start < span.start))?;
    let lead = source.get(span.start..tag.outer.start)?;
    (tag.nested && tag.outer.start < span.end && lead.trim().is_empty()).then_some(tag)
}

/// Whether any tag overlaps `span`.
fn overlaps(tags: &[TagSpan], span: &Range<usize>) -> bool {
    tags.get(tags.partition_point(|tag| tag.outer.end <= span.start))
        .is_some_and(|tag| tag.outer.start < span.end)
}

/// Where `masked`, one line of a masked tag, occurs in `text` at or after
/// `from`.
///
/// A run of filler is as long as the tag's body on that line, so an occurrence
/// that runs on into more filler is part of a longer tag, not this one.
fn find_masked(text: &str, from: usize, masked: &str) -> Option<usize> {
    let mut start = from;
    while let Some(offset) = text.get(start..).and_then(|rest| rest.find(masked)) {
        let at = start + offset;
        let end = at + masked.len();
        let before = text.get(..at).and_then(|head| head.bytes().last());
        let after = text.get(end..).and_then(|tail| tail.bytes().next());
        let joined = (masked.starts_with('x') && before == Some(b'x'))
            || (masked.ends_with('x') && after == Some(b'x'));
        if !joined {
            return Some(at);
        }
        start = at + 1;
    }
    None
}

/// Drop the blank lines at the end of a code block's content.
fn trim_blank_lines(content: &mut String) {
    let end = content.trim_end().len();
    if end == 0 {
        content.clear();
        return;
    }
    let keep = content
        .get(end..)
        .and_then(|rest| rest.find('\n'))
        .map_or(content.len(), |offset| end + offset + 1);
    content.truncate(keep);
}

/// Strip a code span's backticks and apply CommonMark's one-space rule.
///
/// Read from the source rather than taken from the tokenizer, because a tag
/// inside a code span was masked and must come back literal -- which is also
/// what markdown-it does, since its code rule consumes the span before its tag
/// rule can look inside.
fn code_span_content(span: &str) -> String {
    let ticks = span.bytes().take_while(|&byte| byte == b'`').count();
    let inner = span
        .get(ticks..span.len().saturating_sub(ticks))
        .unwrap_or(span)
        .replace('\n', " ");
    if inner.len() >= 2
        && inner.starts_with(' ')
        && inner.ends_with(' ')
        && !inner.trim().is_empty()
    {
        return inner
            .get(1..inner.len().saturating_sub(1))
            .unwrap_or(&inner)
            .to_string();
    }
    inner
}

/// Resolve CommonMark backslash escapes.
///
/// Only needed on the path that reads text back out of the source, which is the
/// path a run containing a tag takes.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some(next) if next.is_ascii_punctuation() => out.push(next),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The runs of non-blank lines in `span`, each line trimmed of surrounding
/// whitespace, as byte ranges into `source`.
///
/// markdown-it ends a paragraph at a blank line and strips each line's
/// leading and trailing whitespace, so an HTML block that spans a blank line
/// -- a comment written across one, say -- is two paragraphs there.
fn paragraph_runs(source: &str, span: &Range<usize>) -> Vec<Vec<Range<usize>>> {
    let text = source.get(span.clone()).unwrap_or("");
    let mut runs: Vec<Vec<Range<usize>>> = Vec::new();
    let mut current: Vec<Range<usize>> = Vec::new();
    let mut offset = span.start;
    for line in text.split_inclusive('\n') {
        let start = offset + (line.len() - line.trim_start().len());
        let end = offset + line.trim_end().len();
        offset += line.len();
        if start >= end {
            if !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(start..end);
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}
