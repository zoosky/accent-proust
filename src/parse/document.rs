//! Lifting segments and CommonMark events into the document tree.
//!
//! Ports upstream `src/parser.ts`. That file walks a flat markdown-it token
//! stream with a stack of open nodes; this one walks a flat [`Event`] stream
//! with the same stack, plus the block-level tags the segmenter took out first.
//! The node shapes are upstream's, because the conformance corpus and the
//! schemas above both name them.
//!
//! # Four places this is not a straight transliteration
//!
//! - **The `inline` node is synthesised, not received.** markdown-it emits an
//!   `inline` token inside every block that has inline content, and marks the
//!   paragraph tokens of a tight list item hidden so the item's child *is* that
//!   token. pulldown-cmark has no such event. So an `inline` node is opened
//!   lazily, on the first inline-level event inside a block, and closed when the
//!   block closes. One rule reproduces both shapes, `paragraph > inline > text`
//!   and `item > inline > text`.
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

use std::ops::Range;

use crate::ast::{Lines, Location, Node, NodeType, ValidationError, Value};
use crate::grammar::{parse_tag, Attribute, TagItem};
use crate::parse::annotate::annotate;
use crate::parse::scan::{contains_markdoc_tag_in_url, find_tag_end, CLOSE, OPEN};
use crate::parse::segment::{Block, Segmentation, TagSpan};
use crate::parse::tokenizer::{Alignment, Container, ContainerKind, Event, Tokenizer};
use crate::parse::ParseOptions;

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
}

/// Builds one document.
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
                        self.event(&event, span, &segmentation.inline_tags);
                    }
                }
                Block::Tag(span) => {
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
        if self.options.slots && node.tag.as_deref() == Some("slot") {
            if let Some(Value::String(name)) = node.get("primary") {
                let name = name.clone();
                self.top().slots.insert(name, node);
                return;
            }
        }
        self.top().push(node);
    }

    /// Fold a text node into the text node before it, if there is one.
    ///
    /// markdown-it does this in a core rule called `text_collapse`, and the
    /// result is observable: the corpus compares renderable trees, so two text
    /// children where upstream has one is a failed case even though the rendered
    /// HTML is identical. pulldown-cmark splits a run at every backslash escape,
    /// entity and unrecognised `<`, so without this a document with `\*` in it
    /// has a different tree shape from upstream's.
    ///
    /// Only plain strings merge. A text node whose `content` is a variable came
    /// from a tag, not from a text run, and upstream does not merge those either
    /// -- its token is a `variable` until the parser renames it, and
    /// `text_collapse` has already run by then.
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
    fn open_inline(&mut self, span: &Range<usize>) {
        self.extend_inline_span(span);
        if self.inline_parent.is_some() {
            return;
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
        let body = self.slice(&span.inner);
        let trimmed = body.trim();
        // The grammar is handed a trimmed body, as upstream's tokenizer hands
        // it one. Its error offsets are relative to that, so translating them
        // back to the document costs the leading whitespace that was cut.
        let body_start = span.inner.start + (body.len() - body.trim_start().len());

        match parse_tag(trimmed) {
            Ok(TagItem::TagOpen {
                name,
                attributes,
                self_closing,
            }) => {
                let mut node = self.node(NodeType::Tag, span.outer.clone());
                node.inline = inline;
                node.tag = Some(name);
                node.lines = span.lines.to_vec();
                annotate(&mut node, &attributes);
                if self_closing {
                    self.attach(node);
                } else {
                    self.open(node);
                }
            }
            Ok(TagItem::TagClose { name }) => self.close_tag(&name, span, inline),
            Ok(TagItem::Annotation { attributes }) => self.annotation(&attributes, span),
            Ok(TagItem::Variable(value)) => {
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
    fn annotation(&mut self, attributes: &[Attribute], span: &TagSpan) {
        if let Some(owner) = self.inline_parent {
            if let Some(node) = self.stack.get_mut(owner) {
                annotate(node, attributes);
                return;
            }
        }
        let location = self.locate(span.outer.clone());
        let name = self.top().name().to_string();
        self.top()
            .errors
            .push(ValidationError::no_inline_annotations(&name).at(location));
    }

    // ---- events ----------------------------------------------------------

    fn event(&mut self, event: &Event<'_>, span: Range<usize>, inline_tags: &[TagSpan]) {
        if !matches!(event, Event::Html(_)) {
            self.flush_html_block();
        }
        // A code block swallows its own text: the content is one attribute, and
        // whether it also has children is decided at the close.
        if let Some(fence) = &mut self.fence {
            match event {
                Event::Text(text) => {
                    if fence.content_start.is_none() {
                        fence.content_start = Some(span.start);
                    }
                    fence.content.push_str(text);
                    return;
                }
                Event::End(ContainerKind::CodeBlock) => {
                    self.close_fence();
                    return;
                }
                _ => return,
            }
        }

        match event {
            Event::Start(container) => self.start(container, span),
            Event::End(kind) => self.end(*kind, span),
            Event::Text(text) => self.text(text, span, inline_tags),
            Event::Code(_) => {
                self.open_inline(&span);
                let content = code_span_content(self.slice(&span));
                let mut node = self.node(NodeType::Code, span);
                node.set("content", Value::String(content));
                self.attach(node);
            }
            Event::Html(_) => {
                self.close_inline();
                self.html_block = Some(match self.html_block.take() {
                    Some(open) => open.start..span.end,
                    None => span,
                });
            }
            Event::InlineHtml(_) => {
                self.open_inline(&span);
                let node = self.html_node(span);
                self.attach(node);
            }
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

    /// Turn the gathered HTML block into one node.
    fn flush_html_block(&mut self) {
        let Some(span) = self.html_block.take() else {
            return;
        };
        let node = self.html_node(span);
        self.attach(node);
    }

    /// A comment, or the literal text of something that is not one.
    ///
    /// Upstream runs markdown-it with `html: false`, so raw HTML is text there,
    /// and adds a comment rule when `allowComments` is on. Both are reproduced:
    /// a comment becomes a `comment` node when comments are enabled, and
    /// anything else -- including a comment when they are not -- becomes text
    /// carrying the markup verbatim.
    fn html_node(&mut self, span: Range<usize>) -> Node<'s> {
        let raw = self.slice(&span);
        let trimmed = raw.trim();
        if self.options.allow_comments {
            if let Some(inner) = trimmed
                .strip_prefix("<!--")
                .and_then(|rest| rest.strip_suffix("-->"))
            {
                let content = inner.trim().to_string();
                let mut node = self.node(NodeType::Comment, span);
                node.set("content", Value::String(content));
                return node;
            }
        }
        self.text_node(raw.to_string(), span)
    }

    fn start(&mut self, container: &Container<'_>, span: Range<usize>) {
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
                self.fence = Some(Fence {
                    span,
                    info: info.as_ref().map(ToString::to_string),
                    content: String::new(),
                    content_start: None,
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
                let mut node = self.node(NodeType::Link, span);
                node.set("href", Value::String(destination.to_string()));
                if !title.is_empty() {
                    node.set("title", Value::String(title.to_string()));
                }
                self.open(node);
            }
            Container::Image { destination, title } => {
                let mut node = self.node(NodeType::Image, span);
                node.set("alt", Value::String(String::new()));
                node.set("src", Value::String(destination.to_string()));
                if !title.is_empty() {
                    node.set("title", Value::String(title.to_string()));
                }
                self.open(node);
            }
            Container::Table { alignments } => {
                self.alignments.clone_from(alignments);
                let node = self.node(NodeType::Table, span);
                self.open(node);
            }
            Container::TableHead => {
                self.in_head = true;
                let node = self.node(NodeType::Thead, span);
                self.open(node);
                // markdown-it wraps the header row in a `tr` inside the `thead`;
                // pulldown-cmark's TableHead *is* the row. Synthesising the `tr`
                // here keeps the two-deep shape every table schema expects.
                let row = self.node(NodeType::Tr, self.span_of_top());
                self.open(row);
                self.column = 0;
            }
            Container::TableRow => {
                // markdown-it wraps body rows in a `tbody`; pulldown-cmark emits
                // them as direct children of the table. Both wrappers are
                // synthesised here so a table schema sees the shape upstream
                // documents.
                if !self.in_body {
                    let body = self.node(NodeType::Tbody, span.clone());
                    self.open(body);
                    self.in_body = true;
                }
                let node = self.node(NodeType::Tr, span);
                self.open(node);
                self.column = 0;
            }
            Container::TableCell => {
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

    fn end(&mut self, kind: ContainerKind, span: Range<usize>) {
        match kind {
            ContainerKind::Emphasis
            | ContainerKind::Strong
            | ContainerKind::Strikethrough
            | ContainerKind::Link => {
                let node = self.pop();
                self.attach(node);
                return;
            }
            ContainerKind::Image => {
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
            _ => {
                let node = self.pop();
                self.attach(node);
            }
        }
        let _ = span;
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

    /// Build the code-block node now that its content is known.
    fn close_fence(&mut self) {
        let Some(fence) = self.fence.take() else {
            return;
        };
        let mut node = self.node(NodeType::Fence, fence.span.clone());
        node.set("content", Value::String(fence.content.clone()));

        let info = fence.info.unwrap_or_default();
        if let Some(language) = info.split(' ').next() {
            if !language.is_empty() && language != OPEN {
                node.set("language", Value::String(language.to_string()));
            }
        }

        // The info string may carry an annotation: ```` ```js {% #id %} ````.
        // Upstream parses it in a core pass over the token stream; here it is
        // read straight off the fence line, which is the same information.
        if let Some(start) = info.find(OPEN) {
            if let Some(end) = find_tag_end(&info, start) {
                let body = info.get(start + OPEN.len()..end).unwrap_or("").trim();
                match parse_tag(body) {
                    Ok(TagItem::Annotation { attributes })
                    | Ok(TagItem::TagOpen { attributes, .. }) => annotate(&mut node, &attributes),
                    Ok(_) => {}
                    Err(error) => node.errors.push(ValidationError::new(
                        "fence-tag-error",
                        crate::ast::ErrorLevel::Error,
                        format!("Syntax error in fence tag: {}", error.message()),
                    )),
                }
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
    /// escapes and character entities are already resolved. With a tag in it the
    /// pieces come from the original source instead -- the tokenizer only ever
    /// saw filler there -- and carry a backslash unescape of their own. Entity
    /// references inside such a run are left as written, which is the one place
    /// the two paths differ and is why the fast path is the default rather than
    /// an optimisation.
    fn text(&mut self, text: &str, span: Range<usize>, inline_tags: &[TagSpan]) {
        self.open_inline(&span);
        let tags: Vec<&TagSpan> = inline_tags
            .iter()
            .filter(|tag| tag.outer.start >= span.start && tag.outer.end <= span.end)
            .collect();
        if tags.is_empty() {
            if !text.is_empty() {
                let node = self.text_node(text.to_string(), span);
                self.attach(node);
            }
            return;
        }

        let mut cursor = span.start;
        for tag in tags {
            self.push_source_text(cursor..tag.outer.start);
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
        .map(String::from)
        .unwrap_or_else(|| ".".to_string())
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
