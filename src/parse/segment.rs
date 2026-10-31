//! Splitting raw text into tags and runs of Markdown.
//!
//! This is the half of the port with no line-by-line source. Upstream adds a
//! block rule, an inline rule and a core pass to markdown-it, so its tag syntax
//! is resolved *inside* the CommonMark parse, in step with it. There is no
//! equivalent hook in a pull parser, and reimplementing markdown-it's ruler
//! architecture to get one would mean owning a CommonMark implementation --
//! which is the thing `DIVERGENCES.md` entry 2 exists to avoid.
//!
//! So tag syntax is resolved *before* the CommonMark parse instead, by scanning
//! raw text. Four jobs, in one pass over the lines:
//!
//! 1. **Block-level tags at the margin.** A line that begins with `{%`, and
//!    whose `%}` ends a line, is a block of its own. It splits the document,
//!    so the Markdown on either side is tokenized separately and a tag cannot
//!    be swallowed by a paragraph that started above it. No container holds
//!    such a line: a list item's content is indented, a quote's is marked, and
//!    upstream's rule interrupts the paragraph that would otherwise take it as
//!    a lazy continuation.
//! 2. **Block-level tags in containers.** The same tag behind indentation, a
//!    `>` or a list marker may belong to a blockquote or a list item, which only
//!    the CommonMark parse can tell. It is masked with a placeholder that the
//!    parse places; see below.
//! 3. **Fence interception.** Inside a fenced code block nothing is a tag.
//!    `DIVERGENCES.md` entry 1 inverts upstream's default here: fence content is
//!    literal unless the fence opts in.
//! 4. **Inline tags, by masking.** A tag inside a text run is found, recorded,
//!    and its interior overwritten with filler in a *copy* of the source that
//!    is what the tokenizer actually reads.
//!
//! # Why masking, rather than splitting text runs afterwards
//!
//! A tag's internals are not Markdown, and they routinely contain characters
//! that are: `{% foo bar="a*b*c" %}` has emphasis in it, `{% foo bar="`x`" %}`
//! has a code span. markdown-it never sees those, because its inline rule
//! consumes the whole tag at the `{` and moves the cursor past `%}` before the
//! emphasis rule reaches them. A pull parser has no cursor to move.
//!
//! Masking reproduces that property exactly and cheaply: the tokenizer is given
//! a buffer of identical length in which every tag's interior is `x`, so it
//! cannot find markup that is not there, and every byte range it reports still
//! indexes the original source. The parser above reads the real text back
//! through those ranges, and never takes text from the tokenizer where a tag
//! was.
//!
//! # Block tags inside containers
//!
//! markdown-it runs the tag rule inside its block ruler, after `list` and
//! `blockquote` have stripped their markers, so a tag written inside a list item
//! or a quote is a block tag *within* it. Knowing which container a line is in
//! means running CommonMark's container phase, and a second implementation of
//! that phase would have to agree with the tokenizer's forever.
//!
//! So the tokenizer runs it. A tag alone on its lines behind container markers
//! or indentation is masked with `<?` and `?>` in place of `{%` and `%}`. That
//! is a processing instruction, which CommonMark reads as an HTML block: one
//! that may interrupt a paragraph, as upstream's rule does, and that ends on
//! the line holding `?>`, which is the tag's last line. The tokenizer reports
//! the block inside whatever container holds it, and the parser turns it back
//! into the tag. Where an HTML block cannot start -- a paragraph continuation
//! line indented four columns, say -- the same placeholder is inline HTML. The
//! parser still splits the paragraph at it when it is alone on its line, as
//! upstream's corpus does (`DIVERGENCES.md` entry 13), and leaves the tag
//! inline when text precedes it.

use std::ops::Range;

use crate::parse::scan::{CLOSE, OPEN, find_tag_end};

/// The span of one `{% ... %}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TagSpan {
    /// The whole tag, `{%` and `%}` included.
    pub outer: Range<usize>,
    /// What lies between the delimiters, untrimmed.
    pub inner: Range<usize>,
    /// The zero-based lines the tag spans, as `[first, last + 1]`, which is the
    /// half-open pair upstream records in `Node::lines`.
    pub lines: [usize; 2],
    /// The `>` markers of the quotes the tag is written in, on every line after
    /// its first, as byte offsets into the source.
    ///
    /// They are container syntax inside the tag's span, not tag syntax. Upstream
    /// reads a quoted tag through `state.getLines`, which has already stripped
    /// them, so the parser blanks them before the grammar sees the body.
    pub quote_markers: Vec<usize>,
    /// Whether the tag is alone on its lines behind container markers or
    /// indentation, and was masked as `<?...?>` so that the tokenizer can place
    /// it as a block inside its container.
    pub nested: bool,
}

/// One top-level piece of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    /// A run of ordinary Markdown, to be tokenized.
    Markdown(Range<usize>),
    /// A block-level tag, which the tokenizer never sees.
    Tag(TagSpan),
}

/// What one pass over the source produced.
pub(crate) struct Segmentation {
    /// The source with every masked tag's interior overwritten, byte for byte.
    ///
    /// The tokenizer reads this. The parser reads it only to recognise what the
    /// tokenizer made of a tag: every range is resolved against the original
    /// source.
    pub masked: String,
    /// The document's blocks, in order.
    pub blocks: Vec<Block>,
    /// Every masked tag, in source order: the inline ones and the
    /// [nested](TagSpan::nested) ones. Block tags at the margin are not here;
    /// they are in [`Segmentation::blocks`].
    pub tags: Vec<TagSpan>,
}

/// Scan a document.
pub(crate) fn segment(source: &str) -> Segmentation {
    let lines = line_ranges(source);
    let mut state = Scan {
        source,
        masked: source.as_bytes().to_vec(),
        blocks: Vec::new(),
        tags: Vec::new(),
        markdown_start: 0,
        fence: None,
    };

    let mut index = 0;
    while index < lines.len() {
        index = state.line(&lines, index);
    }
    state.flush_markdown(source.len());

    Segmentation {
        // Every replacement byte is ASCII, so the buffer is still valid UTF-8
        // by construction. The fallback keeps the promise anyway rather than
        // asserting it.
        masked: String::from_utf8(state.masked).unwrap_or_else(|_| source.to_string()),
        blocks: state.blocks,
        tags: state.tags,
    }
}

/// An open fenced code block.
struct Fence {
    marker: u8,
    length: usize,
}

struct Scan<'s> {
    source: &'s str,
    masked: Vec<u8>,
    blocks: Vec<Block>,
    tags: Vec<TagSpan>,
    markdown_start: usize,
    fence: Option<Fence>,
}

impl Scan<'_> {
    /// Handle one line, and return the index of the next one to handle.
    ///
    /// Returning the index rather than advancing by one is what lets a tag or a
    /// fence consume more than the line it started on.
    fn line(&mut self, lines: &[Range<usize>], index: usize) -> usize {
        let Some(line) = lines.get(index) else {
            return index + 1;
        };
        let text = self.source.get(line.clone()).unwrap_or("");

        if let Some(fence) = &self.fence {
            if closes_fence(text, fence) {
                self.fence = None;
            }
            return index + 1;
        }
        if let Some(fence) = opens_fence(text) {
            self.fence = Some(fence);
            return index + 1;
        }

        let prefix = container_prefix(text);
        if let Some(span) = self.block_tag(lines, index, line.start + prefix) {
            let next = span.lines[1].max(index + 1);
            if prefix > 0 {
                let depth = quote_depth(text.get(..prefix).unwrap_or(""));
                self.nest(lines, span, depth);
                return next;
            }
            self.flush_markdown(line.start);
            self.markdown_start = lines.get(next).map_or(self.source.len(), |line| line.start);
            self.blocks.push(Block::Tag(span));
            return next;
        }

        self.mask_inline_tags(lines, index)
    }

    /// Whether the tag at `content_start` is a block-level tag, and its span.
    ///
    /// Three conditions, all upstream's:
    ///
    /// - the line's first content after its container markers is `{%`;
    /// - the tag closes somewhere (it may be several lines down);
    /// - nothing but whitespace follows `%}` on the line that closes it.
    ///
    /// Plus upstream's one exclusion: a tag whose content starts with `$` is
    /// *not* a block tag. `{% $foo %}` alone on a line is a paragraph
    /// containing a variable, not a block-level node, and the corpus fixes
    /// that.
    fn block_tag(
        &self,
        lines: &[Range<usize>],
        index: usize,
        content_start: usize,
    ) -> Option<TagSpan> {
        if !self.source.get(content_start..)?.starts_with(OPEN) {
            return None;
        }
        let tag_end = find_tag_end(self.source, content_start)?;
        let inner = content_start + OPEN.len()..tag_end;
        if self
            .source
            .get(inner.clone())?
            .trim_start()
            .starts_with('$')
        {
            return None;
        }

        let outer_end = tag_end + CLOSE.len();
        let last = line_of(lines, index, outer_end);
        let rest = self.source.get(outer_end..lines.get(last)?.end)?;
        if !rest.trim().is_empty() {
            return None;
        }

        Some(TagSpan {
            outer: content_start..outer_end,
            inner,
            lines: [index, last + 1],
            quote_markers: Vec::new(),
            nested: false,
        })
    }

    /// Mask a block-level tag inside a container with a placeholder, and record
    /// it.
    ///
    /// `depth` is how many quotes the tag's first line opens with. The
    /// placeholder keeps the delimiters' length, so every offset survives.
    fn nest(&mut self, lines: &[Range<usize>], mut span: TagSpan, depth: usize) {
        span.quote_markers = self.mask_body(lines, &span, depth);
        self.overwrite(span.outer.start, b"<?");
        self.overwrite(span.inner.end, b"?>");
        span.nested = true;
        self.tags.push(span);
    }

    /// Record and mask every tag on this line, and return the next line index.
    ///
    /// A tag may close on a later line. The scan then carries on to the end of
    /// that line, since another tag may follow on it, and returns the line
    /// after the one it ended on.
    ///
    /// A tag that would span a blank line is not one: an inline tag lives in a
    /// paragraph, and a blank line ends the paragraph first. Its `{%` is
    /// ordinary text, as upstream's inline rule leaves it.
    fn mask_inline_tags(&mut self, lines: &[Range<usize>], index: usize) -> usize {
        let Some(line) = lines.get(index) else {
            return index + 1;
        };
        let text = self.source.get(line.clone()).unwrap_or("");
        let depth = quote_depth(text.get(..container_prefix(text)).unwrap_or(""));
        let mut pos = line.start;
        let mut end = line.end;
        let mut last = index;
        while pos < end {
            if self
                .source
                .get(pos..)
                .is_none_or(|rest| !rest.starts_with(OPEN))
            {
                pos += 1;
                continue;
            }
            let Some(tag_end) = find_tag_end(self.source, pos) else {
                // An unclosed `{%` is ordinary text. Step past the delimiter so
                // the scan cannot stall on it.
                pos += OPEN.len();
                continue;
            };
            let outer_end = tag_end + CLOSE.len();
            let closing = line_of(lines, last, outer_end);
            let mut span = TagSpan {
                outer: pos..outer_end,
                inner: pos + OPEN.len()..tag_end,
                lines: [last, closing + 1],
                quote_markers: Vec::new(),
                nested: false,
            };
            if self.spans_blank_line(lines, &span, depth) {
                pos += OPEN.len();
                continue;
            }
            span.quote_markers = self.mask_body(lines, &span, depth);
            self.tags.push(span);
            last = closing;
            end = lines.get(closing).map_or(end, |line| line.end);
            pos = outer_end;
        }
        last + 1
    }

    /// Whether a line between the tag's first and last is blank once its
    /// container markers are set aside.
    fn spans_blank_line(&self, lines: &[Range<usize>], span: &TagSpan, depth: usize) -> bool {
        let [first, end] = span.lines;
        (first + 1..end.saturating_sub(1)).any(|index| {
            lines.get(index).is_some_and(|line| {
                let (content, _) = continuation_prefix(self.source, line.clone(), depth);
                self.source
                    .get(content..line.end)
                    .is_none_or(|rest| rest.trim().is_empty())
            })
        })
    }

    /// Overwrite a tag's body with filler, and return the offsets of the quote
    /// markers it kept.
    ///
    /// Newlines survive: a multi-line tag still occupies the lines it occupies.
    /// So does the start of every line after the first, up to `depth` `>`
    /// markers and the whitespace around them: a tag written inside a quote or
    /// a list item continues on lines that carry the quote's markers and the
    /// item's indentation, and without them the tokenizer would end the
    /// container halfway through the tag. Everything else becomes `x`, which is
    /// inert in CommonMark and, being ASCII, keeps the buffer the same length
    /// and still valid UTF-8.
    fn mask_body(&mut self, lines: &[Range<usize>], span: &TagSpan, depth: usize) -> Vec<usize> {
        let mut markers = Vec::new();
        let [first, end] = span.lines;
        for index in first..end {
            let Some(line) = lines.get(index) else {
                break;
            };
            let stop = line.end.min(span.inner.end);
            let mut start = line.start.max(span.inner.start);
            if index > first {
                let (content, quotes) = continuation_prefix(self.source, line.start..stop, depth);
                markers.extend(quotes);
                start = content;
            }
            for offset in start..stop {
                if let Some(byte) = self.masked.get_mut(offset) {
                    *byte = b'x';
                }
            }
        }
        markers
    }

    /// Write `bytes` into the masked buffer at `offset`.
    fn overwrite(&mut self, offset: usize, bytes: &[u8]) {
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(slot) = self.masked.get_mut(offset + index) {
                *slot = byte;
            }
        }
    }

    fn flush_markdown(&mut self, end: usize) {
        if end > self.markdown_start {
            self.blocks.push(Block::Markdown(
                self.markdown_start..end.min(self.source.len()),
            ));
        }
    }
}

/// How far a line's container markers reach: its indentation, any blockquote
/// `>`, and any list bullet or ordinal, in any order and with the whitespace
/// after them.
///
/// This is a reading of the line's shape, not CommonMark's container phase. It
/// only has to say whether a tag after it *could* be a block inside a
/// container; the tokenizer decides whether it is, and where.
fn container_prefix(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut pos = 0;
    loop {
        while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
            pos += 1;
        }
        let marker = match bytes.get(pos) {
            Some(b'>' | b'-' | b'+' | b'*') => 1,
            Some(byte) if byte.is_ascii_digit() => {
                let digits = bytes.get(pos..).map_or(0, |rest| {
                    rest.iter().take_while(|byte| byte.is_ascii_digit()).count()
                });
                if digits > 9 || !matches!(bytes.get(pos + digits), Some(b'.' | b')')) {
                    return pos;
                }
                digits + 1
            }
            _ => return pos,
        };
        // A list marker needs whitespace after it; `-{%` is text.
        let is_quote = bytes.get(pos) == Some(&b'>');
        if !is_quote && !matches!(bytes.get(pos + marker), Some(b' ' | b'\t')) {
            return pos;
        }
        pos += marker;
    }
}

/// How many blockquotes a container prefix opens.
fn quote_depth(prefix: &str) -> usize {
    prefix.bytes().filter(|&byte| byte == b'>').count()
}

/// Where the content of a tag's continuation line starts, past its
/// indentation and up to `depth` `>` markers, and the markers' offsets.
///
/// `line` is a byte range into `source` and the result never leaves it.
fn continuation_prefix(source: &str, line: Range<usize>, depth: usize) -> (usize, Vec<usize>) {
    let bytes = source.as_bytes();
    let mut pos = line.start;
    let mut markers = Vec::new();
    let skip_whitespace = |pos: &mut usize| {
        while *pos < line.end && matches!(bytes.get(*pos), Some(b' ' | b'\t')) {
            *pos += 1;
        }
    };
    skip_whitespace(&mut pos);
    while markers.len() < depth && pos < line.end && bytes.get(pos) == Some(&b'>') {
        markers.push(pos);
        pos += 1;
        skip_whitespace(&mut pos);
    }
    (pos, markers)
}

/// The line index containing `offset`, searching forward from `from`.
///
/// Forward-only because every caller already knows the line the span started
/// on, and a span never runs backwards. Falls back to the last line for an
/// offset past the end, which the scan does not produce but which costs
/// nothing to survive.
fn line_of(lines: &[Range<usize>], from: usize, offset: usize) -> usize {
    for (index, line) in lines.iter().enumerate().skip(from) {
        if offset <= line.end {
            return index;
        }
    }
    lines.len().saturating_sub(1).max(from)
}

/// Byte ranges of each line, newline excluded.
fn line_ranges(source: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            out.push(start..index);
            start = index + 1;
        }
    }
    if start <= source.len() {
        out.push(start..source.len());
    }
    out
}

/// Whether a line opens a fenced code block, and with what.
///
/// CommonMark allows up to three spaces of indentation before a fence; more
/// makes it an indented code block. The distinction does not matter here --
/// either way the content is not scanned for tags -- so this accepts any
/// indentation, which is the cheaper and safer direction to be wrong in.
fn opens_fence(line: &str) -> Option<Fence> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let marker = trimmed.bytes().next()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = trimmed.bytes().take_while(|&byte| byte == marker).count();
    if length < 3 {
        return None;
    }
    // A backtick fence's info string may not contain a backtick, which is what
    // keeps `` `a` `` from opening one. A tilde fence has no such rule.
    if marker == b'`' && trimmed.get(length..).is_some_and(|info| info.contains('`')) {
        return None;
    }
    Some(Fence { marker, length })
}

fn closes_fence(line: &str, fence: &Fence) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let length = trimmed
        .bytes()
        .take_while(|&byte| byte == fence.marker)
        .count();
    length >= fence.length
        && trimmed
            .get(length..)
            .is_some_and(|rest| rest.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(source: &str) -> Vec<String> {
        segment(source)
            .blocks
            .iter()
            .map(|block| match block {
                Block::Markdown(range) => format!("markdown {:?}", &source[range.clone()]),
                Block::Tag(span) => format!("tag {:?}", &source[span.inner.clone()]),
            })
            .collect()
    }

    #[test]
    fn a_tag_on_its_own_line_splits_the_document() {
        assert_eq!(
            blocks("{% foo %}\nThis is a test\n{% /foo %}\n"),
            [
                "tag \" foo \"",
                "markdown \"This is a test\\n\"",
                "tag \" /foo \"",
            ]
        );
    }

    #[test]
    fn a_tag_with_text_after_it_is_not_a_block() {
        assert_eq!(
            blocks("{% foo %} trailing\n"),
            ["markdown \"{% foo %} trailing\\n\""]
        );
        assert_eq!(segment("{% foo %} trailing\n").tags.len(), 1);
    }

    /// Upstream's one exclusion: `content[0] === '$'` is not a block tag.
    #[test]
    fn a_bare_variable_is_not_a_block_tag() {
        assert_eq!(blocks("{% $test %}\n"), ["markdown \"{% $test %}\\n\""]);
    }

    #[test]
    fn a_block_tag_may_span_lines() {
        let source = "{%\nfoo\n#bar\n%}\nThis is a test\n";
        assert_eq!(
            blocks(source),
            ["tag \"\\nfoo\\n#bar\\n\"", "markdown \"This is a test\\n\""]
        );
        let Block::Tag(span) = &segment(source).blocks[0] else {
            panic!("expected a tag");
        };
        assert_eq!(span.lines, [0, 4]);
    }

    #[test]
    fn a_block_tag_may_close_on_a_line_with_its_own_content() {
        assert_eq!(
            blocks("{% foo\n#bar %}\nThis is a test\n"),
            ["tag \" foo\\n#bar \"", "markdown \"This is a test\\n\""]
        );
    }

    #[test]
    fn an_indented_block_tag_is_left_to_the_tokenizer_to_place() {
        let source = "    {% foo %}\n";
        let segmentation = segment(source);
        assert_eq!(blocks(source), [format!("markdown {source:?}")]);
        assert_eq!(segmentation.masked, "    <?xxxxx?>\n");
        assert!(segmentation.tags.iter().all(|tag| tag.nested));
    }

    #[test]
    fn a_block_tag_behind_container_markers_is_nested() {
        for source in [
            "> {% foo %}\n",
            ">{% foo %}\n",
            "- {% foo %}\n",
            "12) {% foo %}\n",
            "> - > {% foo %}\n",
        ] {
            let segmentation = segment(source);
            assert_eq!(segmentation.tags.len(), 1, "{source:?}");
            assert!(segmentation.tags[0].nested, "{source:?}");
            assert_eq!(segmentation.masked.len(), source.len());
            assert!(segmentation.masked.contains("<?xxxxx?>"), "{source:?}");
        }
    }

    #[test]
    fn text_before_a_tag_keeps_it_inline() {
        for source in [
            "foo {% bar %}\n",
            "-{% bar %}\n",
            "# {% bar %}\n",
            "1.{% bar %}\n",
        ] {
            let segmentation = segment(source);
            assert_eq!(segmentation.tags.len(), 1, "{source:?}");
            assert!(!segmentation.tags[0].nested, "{source:?}");
        }
    }

    #[test]
    fn a_nested_tag_keeps_its_quote_markers_and_indentation() {
        let source = "> - {% foo\n>     bar=1 %}\n";
        let segmentation = segment(source);
        assert_eq!(segmentation.masked, "> - <?xxxx\n>     xxxxxx?>\n");
        assert_eq!(segmentation.tags[0].quote_markers, [11]);
        assert_eq!(&source[11..12], ">");
    }

    #[test]
    fn a_tag_outside_a_quote_masks_a_leading_marker() {
        let source = "foo {% bar\n> baz %} qux\n";
        let segmentation = segment(source);
        assert_eq!(segmentation.masked, "foo {%xxxx\nxxxxxx%} qux\n");
        assert_eq!(segmentation.tags[0].quote_markers, []);
    }

    #[test]
    fn an_inline_tag_cannot_span_a_blank_line() {
        for source in ["foo {% bar\n\nbaz %}\n", "> foo {% bar\n>\n> baz %}\n"] {
            let segmentation = segment(source);
            assert_eq!(segmentation.tags, [], "{source:?}");
            assert_eq!(segmentation.masked, source);
        }
    }

    #[test]
    fn a_tag_after_a_multi_line_tag_on_its_closing_line_is_found() {
        let source = "a {% foo\nbar %} b {% /foo %} c\n";
        let segmentation = segment(source);
        assert_eq!(segmentation.tags.len(), 2);
        assert_eq!(segmentation.tags[1].lines, [1, 2]);
        assert_eq!(segmentation.masked, "a {%xxxx\nxxxx%} b {%xxxxxx%} c\n");
    }

    #[test]
    fn tags_inside_a_fence_are_left_alone() {
        let source = "```\n{% foo %}\n```\n";
        assert_eq!(blocks(source), [format!("markdown {source:?}")]);
        assert_eq!(segment(source).tags, []);
    }

    #[test]
    fn a_tilde_fence_closes_only_on_tildes() {
        let source = "~~~\n{% foo %}\n```\n{% bar %}\n~~~\n";
        assert_eq!(segment(source).tags, []);
    }

    #[test]
    fn an_inline_code_span_does_not_open_a_fence() {
        // ``` on a line with a closing backtick is a code span, not a fence.
        let source = "a ```b``` c\n{% foo %}\n";
        assert!(
            segment(source)
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Tag(_)))
        );
    }

    #[test]
    fn masking_hides_markdown_inside_a_tag_and_keeps_every_offset() {
        let source = "Example {% foo bar=\"a*b*c\" %} baz";
        let segmentation = segment(source);
        assert_eq!(segmentation.masked.len(), source.len());
        assert_eq!(segmentation.masked, "Example {%xxxxxxxxxxxxxxxxx%} baz");
        assert_eq!(segmentation.tags.len(), 1);
        assert_eq!(
            &source[segmentation.tags[0].inner.clone()],
            " foo bar=\"a*b*c\" "
        );
    }

    #[test]
    fn masking_preserves_newlines_so_block_structure_survives() {
        let source = "Example {% foo\n#bar %} baz";
        assert_eq!(segment(source).masked, "Example {%xxxx\nxxxxx%} baz");
    }

    #[test]
    fn two_tags_in_succession_are_both_recorded() {
        let source = "a {% foo %}b{% /foo %} c";
        let segmentation = segment(source);
        assert_eq!(segmentation.tags.len(), 2);
        assert_eq!(segmentation.masked, "a {%xxxxx%}b{%xxxxxx%} c");
    }

    #[test]
    fn an_unclosed_tag_does_not_stall_the_scan() {
        let source = "hello {%\nworld\n";
        let segmentation = segment(source);
        assert_eq!(segmentation.tags, []);
        assert_eq!(segmentation.masked, source);
    }

    #[test]
    fn an_empty_document_produces_nothing() {
        assert_eq!(segment("").blocks, []);
    }
}
