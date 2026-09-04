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
//! raw text. Three jobs, in one pass over the lines:
//!
//! 1. **Block-level tags.** A line whose first non-whitespace content is `{%`
//!    and whose `%}` ends a line is a block of its own. It splits the document,
//!    so the Markdown on either side is tokenized separately and a tag cannot
//!    be swallowed by a paragraph that started above it.
//! 2. **Fence interception.** Inside a fenced code block nothing is a tag.
//!    `DIVERGENCES.md` entry 1 inverts upstream's default here: fence content is
//!    literal unless the fence opts in.
//! 3. **Inline tags, by masking.** A tag inside a text run is found, recorded,
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
//! through those ranges.
//!
//! # What is deliberately not reproduced
//!
//! markdown-it runs the tag rule at a specific point in its block ruler --
//! after `list`, `heading` and `blockquote`, before `paragraph` -- so a tag
//! indented inside a list item is a block tag *within the item*. Here a line
//! that does not begin with `{%` is Markdown, so the same tag is inline. The
//! corpus does not distinguish the two, and reproducing the ordering means
//! reproducing the ruler.

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
    /// The source with every inline tag's interior overwritten, byte for byte.
    ///
    /// The tokenizer reads this. Nothing else does: every range it reports is
    /// resolved against the original source.
    pub masked: String,
    /// The document's blocks, in order.
    pub blocks: Vec<Block>,
    /// Every inline tag, in source order. Block tags are not here; they are in
    /// [`Segmentation::blocks`].
    pub inline_tags: Vec<TagSpan>,
}

/// Scan a document.
pub(crate) fn segment(source: &str) -> Segmentation {
    let lines = line_ranges(source);
    let mut state = Scan {
        source,
        masked: source.as_bytes().to_vec(),
        blocks: Vec::new(),
        inline_tags: Vec::new(),
        markdown_start: 0,
        fence: None,
    };

    let mut index = 0;
    while index < lines.len() {
        index = state.line(&lines, index);
    }
    state.flush_markdown(source.len());

    Segmentation {
        // Every replacement byte is ASCII `x`, so the buffer is still valid
        // UTF-8 by construction. The fallback keeps the promise anyway rather
        // than asserting it.
        masked: String::from_utf8(state.masked).unwrap_or_else(|_| source.to_string()),
        blocks: state.blocks,
        inline_tags: state.inline_tags,
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
    inline_tags: Vec<TagSpan>,
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

        let content_start = line.start + indent_of(text);
        if let Some(span) = self.block_tag(lines, index, content_start) {
            let next = span.lines[1];
            // Flush to the start of the line, not to the tag: a block tag's own
            // indentation belongs to the tag line, and carrying it into the
            // preceding Markdown run would leave a stray whitespace segment
            // that CommonMark could read as an indented block.
            self.flush_markdown(line.start);
            self.markdown_start = lines.get(next).map_or(self.source.len(), |line| line.start);
            self.blocks.push(Block::Tag(span));
            return next.max(index + 1);
        }

        self.mask_inline_tags(lines, index)
    }

    /// Whether the line at `index` is a block-level tag, and its span.
    ///
    /// Three conditions, all upstream's:
    ///
    /// - the line's first non-whitespace content is `{%`;
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
        })
    }

    /// Record and mask every tag on this line, and return the next line index.
    ///
    /// A tag may close on a later line, so this returns the line after the one
    /// the scan ended on rather than always `index + 1`.
    fn mask_inline_tags(&mut self, lines: &[Range<usize>], index: usize) -> usize {
        let Some(line) = lines.get(index) else {
            return index + 1;
        };
        let mut pos = line.start;
        let mut last = index;
        while pos < line.end {
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
            let inner = pos + OPEN.len()..tag_end;
            let outer_end = tag_end + CLOSE.len();
            last = line_of(lines, index, outer_end);
            self.mask(inner.clone());
            self.inline_tags.push(TagSpan {
                outer: pos..outer_end,
                inner,
                lines: [index, last + 1],
            });
            pos = outer_end;
        }
        last + 1
    }

    /// Overwrite a range with filler, preserving line structure.
    ///
    /// Newlines survive: a multi-line tag still occupies the lines it occupies,
    /// so the tokenizer's block structure around it is unchanged. Everything
    /// else becomes `x`, which is inert in CommonMark and, being ASCII, keeps
    /// the buffer the same length and still valid UTF-8.
    fn mask(&mut self, range: Range<usize>) {
        for index in range {
            if let Some(byte) = self.masked.get_mut(index)
                && *byte != b'\n'
            {
                *byte = b'x';
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

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
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
        assert_eq!(segment("{% foo %} trailing\n").inline_tags.len(), 1);
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
    fn an_indented_block_tag_is_still_a_block_tag() {
        assert_eq!(blocks("    {% foo %}\n"), ["tag \" foo \""]);
    }

    #[test]
    fn tags_inside_a_fence_are_left_alone() {
        let source = "```\n{% foo %}\n```\n";
        assert_eq!(blocks(source), [format!("markdown {source:?}")]);
        assert!(segment(source).inline_tags.is_empty());
    }

    #[test]
    fn a_tilde_fence_closes_only_on_tildes() {
        let source = "~~~\n{% foo %}\n```\n{% bar %}\n~~~\n";
        assert!(segment(source).inline_tags.is_empty());
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
        assert_eq!(segmentation.inline_tags.len(), 1);
        assert_eq!(
            &source[segmentation.inline_tags[0].inner.clone()],
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
        assert_eq!(segmentation.inline_tags.len(), 2);
        assert_eq!(segmentation.masked, "a {%xxxxx%}b{%xxxxxx%} c");
    }

    #[test]
    fn an_unclosed_tag_does_not_stall_the_scan() {
        let source = "hello {%\nworld\n";
        let segmentation = segment(source);
        assert!(segmentation.inline_tags.is_empty());
        assert_eq!(segmentation.masked, source);
    }

    #[test]
    fn an_empty_document_produces_nothing() {
        assert!(segment("").blocks.is_empty());
    }
}
