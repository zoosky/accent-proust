//! Printing an AST back to canonical Markdoc source.
//!
//! Mirrors upstream `src/formatter.ts`, the largest single file in the port at
//! 506 lines, and the hardest: it is the only layer whose correctness is
//! judged against the exact bytes it emits.
//!
//! It is also what makes tooling possible -- editing a document as a tree and
//! writing it back, mechanically migrating one syntax to another, showing an
//! author the canonical form of what they wrote.
//!
//! Two properties gate it, beyond the corpus:
//!
//! - `format(parse(s))` is idempotent.
//! - `parse(format(ast))` round-trips the AST.
//!
//! # Why the output is a list of chunks rather than a string
//!
//! Upstream is written as nested generators, and one case reads the *shape* of
//! what its children yielded rather than their concatenation. A `tr` yields a
//! JavaScript array of formatted cells; a `table` inside a `{% table %}` tag
//! walks the yielded items and prints an array as `- cell` lines and a string
//! as a line of its own. Concatenating as you go destroys that distinction, and
//! it also destroys the *boundaries* between strings, which the same loop reads:
//! each yielded string becomes its own line, so merging two of them merges two
//! lines.
//!
//! So [`Out`] keeps the yields as a `Vec` of [`Chunk`], one per `yield`, and
//! joins only at the end. That is the generator stream, made data.
//!
//! # Where upstream has no defined behaviour
//!
//! A few branches upstream reach only with a hand-built tree: an attribute that
//! is a plain object where a scalar is expected, a `title` that is not a
//! string, a `text` node whose `content` is neither a string nor an AST value.
//! JavaScript answers those with a `TypeError`, an unhandled `Error`, or
//! `"[object Object]"`. None of the three is a specification, and this crate
//! promises not to panic, so each prints the value in its Markdoc literal
//! spelling instead -- the spelling that re-parses. Nothing a parsed document
//! contains reaches any of them.
//!
//! # Bounds
//!
//! The walk is recursive, over a tree whose depth is attacker-controlled, so it
//! is bounded: see [`MAX_FORMAT_DEPTH`] and `DIVERGENCES.md` entry 15.

mod escape;
mod node;
mod value;

#[cfg(test)]
mod tests;

use crate::ast::{Node, Value};
use crate::parse::{CLOSE, OPEN};

/// A single space. Upstream's `SPACE`.
const SPACE: &str = " ";
/// The value separator inside arrays, hashes and parameter lists.
const SEP: &str = ", ";
/// A newline. Upstream's `NL`.
const NL: &str = "\n";
/// The default ordered-list marker. Upstream's `OL`.
const OL: &str = ".";
/// The default unordered-list marker. Upstream's `UL`.
const UL: &str = "-";

/// The node types whose text children escape `*`, `_` and `~`.
///
/// Upstream's `WRAPPING_TYPES`. Inside one of these, an unescaped marker
/// character would close the wrapper early.
const WRAPPING_TYPES: [crate::ast::NodeType; 3] = [
    crate::ast::NodeType::Strong,
    crate::ast::NodeType::Em,
    crate::ast::NodeType::S,
];

/// The width past which a block tag's opening is broken across lines.
///
/// Upstream's `MAX_TAG_OPENING_WIDTH`. Measured in UTF-16 code units, because
/// that is what `String.prototype.length` counts and the threshold is a
/// formatting decision upstream already made.
pub const MAX_TAG_OPENING_WIDTH: usize = 80;

/// How deep the formatter will walk before it stops.
///
/// The same argument as `grammar::MAX_VALUE_DEPTH` and the transform stage's
/// `MAX_TRANSFORM_DEPTH`, one layer further up: nesting depth is
/// attacker-controlled, and in Rust unbounded recursion over it is a stack
/// overflow, which aborts the process and cannot be caught. A node below this
/// depth formats to nothing; its ancestors print normally. See `DIVERGENCES.md`
/// entry 15.
///
/// 512 is far past anything a person writes and matches the transform stage, so
/// a document that transforms also formats.
pub const MAX_FORMAT_DEPTH: usize = 512;

/// The largest number of `#` characters a heading prints.
///
/// `level` is an ordinary attribute, so a host can set it to anything a
/// [`f64`](f64) holds, and `"#".repeat(n)` for a large `n` is an allocation
/// failure rather than a formatting bug. The bound is far above CommonMark's
/// six levels, so it never changes the output of a parsed document. See
/// `DIVERGENCES.md` entry 15.
const MAX_HEADING_LEVEL: usize = 1024;

/// Whether a numbered list reprints its numbers or repeats the first one.
///
/// Upstream's `orderedListMode`. The default is [`OrderedListMode::Repeat`],
/// which writes `1.` for every item after the first and lets the Markdown
/// renderer number them -- the form that survives inserting an item in the
/// middle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OrderedListMode {
    /// Print the list's start number on the first item and `1` on the rest.
    #[default]
    Repeat,
    /// Print consecutive numbers counting up from the list's start number.
    Increment,
}

/// How to print a tree.
///
/// Upstream's `Options`, minus two fields. `parent` and `indent` are internal
/// bookkeeping rather than caller-facing choices, so they live in the walk;
/// `allowIndentation` does not exist here at all, because the tokenizer option
/// it mirrors does not (`DIVERGENCES.md` entry 8).
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct FormatOptions {
    /// The width past which a block tag's opening breaks across lines.
    ///
    /// Upstream accepts `Infinity` to switch the behaviour off; [`usize::MAX`]
    /// is the same instruction.
    pub max_tag_opening_width: usize,
    /// Whether a numbered list reprints its numbers.
    pub ordered_list_mode: OrderedListMode,
}

impl Default for FormatOptions {
    fn default() -> FormatOptions {
        FormatOptions::new()
    }
}

impl FormatOptions {
    /// Upstream's defaults: break a tag opening at 80 columns, repeat list
    /// numbers.
    #[must_use]
    pub const fn new() -> FormatOptions {
        FormatOptions {
            max_tag_opening_width: MAX_TAG_OPENING_WIDTH,
            ordered_list_mode: OrderedListMode::Repeat,
        }
    }

    /// Set the width past which a block tag's opening breaks across lines.
    #[must_use]
    pub const fn max_tag_opening_width(mut self, width: usize) -> FormatOptions {
        self.max_tag_opening_width = width;
        self
    }

    /// Set whether a numbered list reprints its numbers.
    #[must_use]
    pub const fn ordered_list_mode(mut self, mode: OrderedListMode) -> FormatOptions {
        self.ordered_list_mode = mode;
        self
    }
}

/// Print a tree as canonical Markdoc source, with upstream's default options.
///
/// Canonical means the spacing inside a tag is normalised while the author's
/// own spellings are not: an annotation reprints as it was written, and
/// `__bold__` does not become `**bold**`.
///
/// ```
/// let document = proust::parse::parse("{% callout   type=\"note\" %}\nBody\n{% /callout %}\n");
/// assert_eq!(
///     proust::format::format(&document),
///     "{% callout type=\"note\" %}\nBody\n{% /callout %}\n"
/// );
/// ```
#[must_use]
pub fn format(node: &Node<'_>) -> String {
    format_with(node, &FormatOptions::new())
}

/// Print a tree as canonical Markdoc source.
///
/// ```
/// use proust::format::{format_with, FormatOptions, OrderedListMode};
///
/// let document = proust::parse::parse("1. one\n1. two\n1. three\n");
/// let options = FormatOptions::new().ordered_list_mode(OrderedListMode::Increment);
/// assert_eq!(format_with(&document, &options), "1. one\n2. two\n3. three\n");
/// ```
#[must_use]
pub fn format_with(node: &Node<'_>, options: &FormatOptions) -> String {
    let mut formatter = Formatter {
        options,
        depth: 0,
        stack: 0,
    };
    let mut out = Out::default();
    formatter.node(node, Ctx::default(), &mut out);
    trim_start_owned(out.joined())
}

/// Print a value -- a variable, a function call, a literal -- as it would be
/// written inside a tag.
///
/// Upstream's `format` takes `Value | Value[]`, because a `Node` is one of its
/// values. Here it is not, so the two entry points are separate: this one is
/// what upstream's `format(null)` and `format($x)` reach.
///
/// ```
/// use proust::ast::{PathSegment, Value, Variable};
///
/// let value = Value::Variable(Variable::new(vec![
///     PathSegment::Key("user".into()),
///     PathSegment::Key("name".into()),
/// ]));
/// assert_eq!(proust::format::format_value(&value), "$user.name");
/// assert_eq!(proust::format::format_value(&Value::Null), "");
/// ```
#[must_use]
pub fn format_value(value: &Value) -> String {
    format_value_with(value, &FormatOptions::new())
}

/// Print a value as it would be written inside a tag.
///
/// See [`format_value`]. The options reach nothing a bare value can contain and
/// are taken for symmetry, so a caller threading one set of options does not
/// have to know which entry point ignores them.
#[must_use]
pub fn format_value_with(value: &Value, options: &FormatOptions) -> String {
    let mut formatter = Formatter {
        options,
        depth: 0,
        stack: 0,
    };
    let mut out = Out::default();
    formatter.value(value, Ctx::default(), &mut out);
    trim_start_owned(out.joined())
}

/// `String::trim_start` without a second allocation.
fn trim_start_owned(mut text: String) -> String {
    let trimmed = text.trim_start();
    if trimmed.len() != text.len() {
        text = trimmed.to_owned();
    }
    text
}

/// One `yield` of upstream's generator.
///
/// [`Chunk::Row`] is the array a `tr` yields. Keeping it apart from text is
/// what lets the `{% table %}` branch tell a row from a tag written between
/// rows, which is the one place in the file where the difference is read.
#[derive(Clone, Debug)]
enum Chunk {
    /// A run of output.
    Text(String),
    /// One table row, as its formatted cells.
    Row(Vec<String>),
}

/// The chunks yielded so far.
#[derive(Debug, Default)]
struct Out {
    chunks: Vec<Chunk>,
}

impl Out {
    /// Yield a run of output.
    ///
    /// Empty runs are kept. Upstream yields `indent` unconditionally, and the
    /// `{% table %}` branch skips a blank yield by testing it -- so dropping it
    /// here would change which chunk that loop sees first.
    fn text(&mut self, text: impl Into<String>) {
        self.chunks.push(Chunk::Text(text.into()));
    }

    /// Yield a table row.
    fn row(&mut self, cells: Vec<String>) {
        self.chunks.push(Chunk::Row(cells));
    }

    /// Yield everything another stream produced.
    fn append(&mut self, other: Out) {
        self.chunks.extend(other.chunks);
    }

    /// The concatenation upstream's `format` performs at the end.
    ///
    /// A row concatenates as JavaScript concatenates an array inside `join('')`
    /// -- its elements, comma-separated. No branch that joins can contain one;
    /// the coercion is written out so that a future one does not silently lose
    /// cells.
    fn joined(&self) -> String {
        let mut out = String::new();
        for chunk in &self.chunks {
            match chunk {
                Chunk::Text(text) => out.push_str(text),
                Chunk::Row(cells) => out.push_str(&cells.join(",")),
            }
        }
        out
    }

    /// Upstream's `trimStart` generator: drop leading chunks that are entirely
    /// whitespace, and left-trim the first that is not.
    fn trim_start(self) -> Out {
        let mut chunks = self.chunks.into_iter();
        let mut out = Out::default();
        for chunk in chunks.by_ref() {
            match chunk {
                Chunk::Text(text) => {
                    let trimmed = text.trim_start();
                    if !trimmed.is_empty() {
                        out.text(trimmed.to_owned());
                        break;
                    }
                }
                // A row has no `trimStart`; upstream would throw. No caller of
                // this can produce one, and passing it through is the reading
                // that loses nothing.
                row => {
                    out.chunks.push(row);
                    break;
                }
            }
        }
        out.chunks.extend(chunks);
        out
    }
}

/// What a node needs to know about where it sits.
///
/// Upstream carries the parent node itself in its options and reads exactly two
/// facts off it: whether it wraps its text (`strong`, `em`, `s`), and whether it
/// is the `{% table %}` tag. Carrying the two answers instead of the node keeps
/// the walk free of the AST's lifetime, which is otherwise threaded through
/// every helper for no gain.
#[derive(Clone, Copy, Debug, Default)]
struct Ctx {
    /// Columns of leading indentation.
    indent: usize,
    /// Whether the parent is `strong`, `em` or `s`.
    parent_wraps: bool,
    /// Whether the parent is the `{% table %}` tag.
    parent_is_table_tag: bool,
}

impl Ctx {
    /// Upstream's `increment(o, n)`.
    const fn increment(self, by: usize) -> Ctx {
        Ctx {
            indent: self.indent.saturating_add(by),
            ..self
        }
    }
}

/// The walk.
///
/// `depth` counts nodes, `stack` counts nested values; both are bounded at
/// [`MAX_FORMAT_DEPTH`] because both recurse over structure a document
/// controls.
struct Formatter<'o> {
    options: &'o FormatOptions,
    depth: usize,
    stack: usize,
}
