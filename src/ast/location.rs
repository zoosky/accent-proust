//! Where a node came from in the source it was parsed from.
//!
//! Upstream carries `{file?, start: {line, character?}, end: {line, character?}}`
//! and treats `character` as an offset that is frequently absent, because
//! markdown-it only supplies one for the tokens its own plugins create. That is
//! enough to point an error message at a line and not enough to reprint a
//! document.
//!
//! Here a location is always complete: a byte range into the source buffer, the
//! line and column of each edge, and the text itself, borrowed. The formatter
//! reprints canonical source, so it needs the bytes it is reprinting rather than
//! a line number it would have to go looking up; and a diagnostic that can
//! quote the offending span reads better than one that names a coordinate.
//!
//! # Why the text borrows
//!
//! [`Location`] carries `&'a str`, which makes [`Node`](super::Node) carry a
//! lifetime too. That is the deliberate trade. Owning each node's text would
//! copy the whole document into the tree a second time, and it would still not
//! give byte fidelity for anything the tree does not model -- the exact spelling
//! of an emphasis marker, the whitespace inside a tag. Borrowing gives both for
//! nothing, and the lifetime stops at the AST: transform produces an owned
//! renderable tree, so nothing above this layer inherits it.
//!
//! # Coordinates
//!
//! Lines and columns are **zero-based**, matching upstream (`parser.test.ts`
//! asserts `start: {line: 0}` for the first line of a document). Columns count
//! bytes, not characters or display cells: a column is only ever used to slice
//! the source or to report a position, and both want the same unit the range
//! does.

use std::ops::Range;

/// One edge of a [`Location`].
///
/// Zero-based, and byte-denominated. `offset` is the absolute byte index into
/// the source; `line` and `column` are the same point expressed for a human.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// Zero-based line.
    pub line: usize,
    /// Zero-based byte column within the line.
    pub column: usize,
    /// Absolute byte offset into the source.
    pub offset: usize,
}

impl Position {
    /// A position at the very start of a source buffer.
    #[must_use]
    pub const fn start() -> Position {
        Position {
            line: 0,
            column: 0,
            offset: 0,
        }
    }
}

/// A byte range in a source document, and the text it covers.
///
/// `file` is whatever name the caller passed to
/// [`parse_with`](crate::parse::parse_with). This crate performs no I/O, so it
/// is a label, never a path that is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location<'a> {
    /// The caller's name for the document. A label, not a path.
    pub file: Option<&'a str>,
    /// The first byte of the span.
    pub start: Position,
    /// One past the last byte of the span.
    pub end: Position,
    /// The source text of the span, borrowed.
    pub text: &'a str,
}

impl Location<'_> {
    /// The byte range this location covers.
    #[must_use]
    pub const fn span(&self) -> Range<usize> {
        self.start.offset..self.end.offset
    }

    /// How many bytes the span covers.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end.offset.saturating_sub(self.start.offset)
    }

    /// Whether the span is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Turns byte offsets into line and column, for one source buffer.
///
/// Built once per parse and consulted per node. The alternative -- counting
/// newlines from the start of the document for every span -- is quadratic in
/// the number of nodes, which is the shape a documentation site hits first.
///
/// Offsets are resolved by binary search over the line-start table, so an offset
/// past the end of the source lands on the last line rather than panicking.
/// Nothing in the parser produces such an offset today; accepting one is
/// cheaper than an invariant every caller has to remember.
#[derive(Debug)]
pub struct Lines<'a> {
    source: &'a str,
    /// Byte offset of the first character of each line. Always starts with 0.
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    /// Index a source buffer.
    #[must_use]
    pub fn new(source: &'a str) -> Lines<'a> {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|&(_, byte)| byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        Lines { source, starts }
    }

    /// The source this index was built over.
    #[must_use]
    pub const fn source(&self) -> &'a str {
        self.source
    }

    /// The zero-based line and column of a byte offset.
    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        // `partition_point` gives the number of line starts at or before the
        // offset, which is the one-based line; subtracting one makes it the
        // zero-based index into `starts`, and it is never zero because
        // `starts[0]` is 0 and every offset is >= 0.
        let line = self.starts.partition_point(|&start| start <= offset);
        let index = line.saturating_sub(1);
        let start = self.starts.get(index).copied().unwrap_or(0);
        Position {
            line: index,
            column: offset.saturating_sub(start),
            offset,
        }
    }

    /// Build a [`Location`] for a byte range.
    ///
    /// A range whose end runs past the source is clamped, and a reversed range
    /// is treated as empty at its start. Both are defensive: the segmenter
    /// produces neither, and a panic in a parser is a promise broken.
    #[must_use]
    pub fn locate(&self, range: Range<usize>, file: Option<&'a str>) -> Location<'a> {
        let start = range.start.min(self.source.len());
        let end = range.end.clamp(start, self.source.len());
        let text = self.source.get(start..end).unwrap_or("");
        Location {
            file,
            start: self.position(start),
            end: self.position(end),
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_zero_based() {
        let lines = Lines::new("ab\ncd\n");
        assert_eq!(
            lines.position(0),
            Position {
                line: 0,
                column: 0,
                offset: 0
            }
        );
        assert_eq!(
            lines.position(1),
            Position {
                line: 0,
                column: 1,
                offset: 1
            }
        );
        assert_eq!(
            lines.position(3),
            Position {
                line: 1,
                column: 0,
                offset: 3
            }
        );
        assert_eq!(
            lines.position(5),
            Position {
                line: 1,
                column: 2,
                offset: 5
            }
        );
    }

    #[test]
    fn a_trailing_newline_opens_a_line() {
        let lines = Lines::new("ab\n");
        assert_eq!(lines.position(3).line, 1);
    }

    #[test]
    fn locate_borrows_the_span() {
        let lines = Lines::new("# heading\n");
        let location = lines.locate(2..9, Some("foo.md"));
        assert_eq!(location.text, "heading");
        assert_eq!(location.file, Some("foo.md"));
        assert_eq!(location.span(), 2..9);
        assert_eq!(location.len(), 7);
        assert!(!location.is_empty());
    }

    #[test]
    fn out_of_range_spans_are_clamped_rather_than_panicking() {
        let lines = Lines::new("abc");
        assert_eq!(lines.locate(1..99, None).text, "bc");
        // Deliberately reversed, which is why it is built rather than written
        // as a literal: `9..1` as a literal is a lint, and the point is that the
        // parser cannot be made to panic by one.
        let reversed = std::ops::Range { start: 9, end: 1 };
        assert_eq!(lines.locate(reversed, None).text, "");
        assert_eq!(lines.position(99).line, 0);
    }

    #[test]
    fn multibyte_columns_count_bytes() {
        // The column is a byte column on purpose: it is used to slice the
        // source, and a character column cannot do that without a second scan.
        let lines = Lines::new("\u{e9}x\n");
        assert_eq!(
            lines.position(2),
            Position {
                line: 0,
                column: 2,
                offset: 2
            }
        );
    }
}
