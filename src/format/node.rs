//! The node switch: upstream's `formatNode`, one arm per node type.
//!
//! Read it beside `reference/src/formatter.ts`. The arms are in upstream's
//! order, and each one yields the same chunks in the same sequence, because the
//! `{% table %}` arm reads chunk boundaries and merging two yields merges two
//! lines.
//!
//! # The three shapes that are not just "print the children"
//!
//! - **`blockquote` and `list` re-enter at the top.** They call the whole
//!   formatter on each child and paste the result behind a prefix, rather than
//!   streaming the child's chunks into their own. That is what lets them
//!   left-trim a child independently of its siblings.
//! - **`table` reads chunks, not text.** A `tr` yields a row; a tag written
//!   between rows yields strings. Inside a `{% table %}` the two print
//!   differently, which is the only reason [`Chunk`](super::Chunk) exists.
//! - **`text` escapes differently depending on its parent.** Inside `strong`,
//!   `em` or `s` it escapes the marker characters; anywhere else it escapes the
//!   one leading character that would turn the line into a different block.

use super::escape::{escape_markdown, Escape};
use super::{
    Chunk, Ctx, Formatter, OrderedListMode, Out, CLOSE, MAX_FORMAT_DEPTH, MAX_HEADING_LEVEL, NL,
    OL, OPEN, SPACE, UL, WRAPPING_TYPES,
};
use crate::ast::{Node, NodeType, Value};
use crate::render::js;

impl Formatter<'_> {
    /// Format one node, bounded at [`MAX_FORMAT_DEPTH`].
    ///
    /// Past the bound the node prints as nothing and its ancestors print
    /// normally. See `DIVERGENCES.md` entry 15 for why a bound rather than an
    /// iterative rewrite: the walk re-enters at the top for two node types, so
    /// unrolling it onto a stack means unrolling the public entry point too.
    pub(super) fn node(&mut self, node: &Node<'_>, ctx: Ctx, out: &mut Out) {
        if self.depth >= MAX_FORMAT_DEPTH {
            return;
        }
        self.depth += 1;
        self.node_inner(node, ctx, out);
        self.depth -= 1;
    }

    /// Every child, in order.
    fn children(&mut self, node: &Node<'_>, ctx: Ctx, out: &mut Out) {
        for child in &node.children {
            self.node(child, ctx, out);
        }
    }

    /// Every child, into a stream of its own.
    fn collect_children(&mut self, node: &Node<'_>, ctx: Ctx) -> Out {
        let mut out = Out::default();
        self.children(node, ctx, &mut out);
        out
    }

    /// Upstream's `formatInline`: the children, joined and trimmed, as one
    /// chunk.
    fn inline(&mut self, node: &Node<'_>, ctx: Ctx) -> String {
        self.collect_children(node, ctx).joined().trim().to_owned()
    }

    /// Upstream's recursive `format(child, options)`: a subtree as a string,
    /// left-trimmed, independent of what surrounds it.
    fn subtree(&mut self, node: &Node<'_>, ctx: Ctx) -> String {
        let mut out = Out::default();
        self.node(node, ctx, &mut out);
        super::trim_start_owned(out.joined())
    }

    /// A value where upstream interpolates it into a template literal, or hands
    /// it to a string method.
    ///
    /// A string is itself. Anything else upstream coerces with JavaScript's
    /// rules -- `"[object Object]"` for a hash, a `TypeError` for a `.replace`
    /// -- and none of those is behaviour to reproduce, so it prints in the
    /// Markdoc literal spelling: the one that re-parses. Nothing a parsed
    /// document contains reaches this.
    fn text_of(&mut self, value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => self.scalar(other),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "upstream's `formatNode` is one switch over the node types, and \
                  splitting it by arm would separate cases that have to be read \
                  against each other -- `table` against `tr`, `strong` against \
                  `text`"
    )]
    fn node_inner(&mut self, node: &Node<'_>, ctx: Ctx, out: &mut Out) {
        // Upstream's `no`: the same options, with this node recorded as the
        // parent its children will see.
        let no = child_ctx(node, ctx);
        let indent = SPACE.repeat(ctx.indent);

        match node.node_type {
            NodeType::Document => {
                // The parser never sets `frontmatter`: metadata blocks belong to
                // the host (`DIVERGENCES.md` entry 7). The branch is ported
                // anyway, so that a host carrying its own frontmatter in that
                // attribute gets it back.
                if let Some(Value::String(frontmatter)) = node.get("frontmatter") {
                    if !frontmatter.is_empty() {
                        out.text(format!("---{NL}{frontmatter}{NL}---{NL}{NL}"));
                    }
                }
                let children = self.collect_children(node, no).trim_start();
                out.append(children);
            }
            NodeType::Heading => {
                out.text(NL);
                out.text(indent);
                out.text("#".repeat(heading_level(node)));
                out.text(SPACE);
                let children = self.collect_children(node, no).trim_start();
                out.append(children);
                self.annotations(node, out);
                out.text(NL);
            }
            NodeType::Paragraph => {
                out.text(NL);
                self.children(node, no, out);
                self.annotations(node, out);
                out.text(NL);
            }
            NodeType::Inline => {
                out.text(indent);
                self.children(node, no, out);
            }
            NodeType::Image => {
                out.text("!");
                out.text("[");
                if let Some(alt) = node.get("alt") {
                    self.value(alt, no, out);
                }
                out.text("]");
                out.text("(");
                self.destination(node.get("src"), no, out);
                self.title(node, out);
                out.text(")");
            }
            NodeType::Link => {
                let children = self.collect_children(node, no).joined();
                let has_title = node.get("title").is_some_and(Value::is_truthy);

                // <https://spec.commonmark.org/0.31.2/#autolinks>: a link whose
                // text is its own destination reprints in the short form.
                if !has_title {
                    if let Some(Value::String(href)) = node.get("href") {
                        if children == *href {
                            out.text(format!("<{href}>"));
                            return;
                        }
                    }
                }

                out.text("[");
                out.text(children);
                out.text("]");
                out.text("(");
                self.destination(node.get("href"), no, out);
                self.title(node, out);
                out.text(")");
            }
            NodeType::Text => match node.get("content") {
                Some(value @ (Value::Variable(_) | Value::Function(_))) => {
                    out.text(format!("{OPEN}{SPACE}"));
                    self.value(value, no, out);
                    out.text(format!("{SPACE}{CLOSE}"));
                }
                Some(other) => {
                    let content = self.text_of(other);
                    let class = if ctx.parent_wraps {
                        Escape::Wrapping
                    } else {
                        Escape::Block
                    };
                    out.text(escape_markdown(&content, class));
                }
                None => {}
            },
            NodeType::Blockquote => {
                let prefix = format!(">{SPACE}");
                let parts: Vec<String> = node
                    .children
                    .iter()
                    .map(|child| {
                        let formatted = self.subtree(child, no);
                        format!("{NL}{indent}{prefix}{formatted}")
                    })
                    .collect();
                // The separator carries no newline, so it lands directly after
                // the previous child's trailing one. That is what produces the
                // bare `> ` line between two quoted paragraphs.
                out.text(parts.join(&format!("{indent}{prefix}")));
            }
            NodeType::Hr => {
                out.text(NL);
                out.text(indent);
                out.text("---");
                out.text(NL);
            }
            NodeType::Fence => {
                let content = match node.get("content") {
                    Some(value) => self.text_of(value),
                    None => String::new(),
                };

                out.text(NL);
                out.text(indent.clone());

                // The boundary has to be longer than any run of backticks the
                // content holds, or the fence closes inside itself.
                let inner = longest_backtick_run(&content);
                let boundary = "`".repeat(if inner > 0 { inner + 1 } else { 3 });
                let needs_newline_before_close = !content.ends_with(NL);

                out.text(boundary.clone());
                if let Some(language) = node.get("language").filter(|value| value.is_truthy()) {
                    let language = self.text_of(language);
                    out.text(language);
                }
                if !node.annotations.is_empty() {
                    out.text(SPACE);
                }
                self.annotations(node, out);
                out.text(NL);
                out.text(indent.clone());
                out.text(
                    content
                        .split('\n')
                        .collect::<Vec<&str>>()
                        .join(&format!("{NL}{indent}")),
                );
                if needs_newline_before_close {
                    out.text(NL);
                }
                out.text(boundary);
                out.text(NL);
            }
            NodeType::Tag => {
                if !node.inline {
                    out.text(NL);
                    out.text(indent.clone());
                }

                let open = format!("{OPEN}{SPACE}");
                let name = node.tag.clone().unwrap_or_default();
                let mut parts = vec![format!("{open}{name}")];
                parts.extend(self.attributes(node));

                let inline_tag = parts.join(SPACE);
                // Upstream counts the opening delimiter twice: once for the
                // `{% ` already in `inline_tag`, and once as the allowance for
                // the ` %}` that will follow it.
                let width = utf16_len(&inline_tag).saturating_add(utf16_len(&open) * 2);
                let is_long = width > self.options.max_tag_opening_width;

                let opening = if !node.inline && is_long {
                    parts.join(&format!("{NL}{}{indent}", SPACE.repeat(utf16_len(&open))))
                } else {
                    inline_tag
                };
                let closer = if node.children.is_empty() { "/" } else { "" };
                out.text(format!("{opening}{SPACE}{closer}{CLOSE}"));

                if !node.children.is_empty() {
                    // Upstream indents nested children when `allowIndentation`
                    // is on. The option does not exist here, so this is the
                    // `false` branch and only that branch.
                    self.children(node, no, out);
                    if !node.inline {
                        out.text(indent.clone());
                    }
                    out.text(format!("{OPEN}{SPACE}/{name}{SPACE}{CLOSE}"));
                }
                if !node.inline {
                    out.text(NL);
                }
            }
            NodeType::List => self.list(node, no, &indent, out),
            NodeType::Item => {
                for (index, child) in node.children.iter().enumerate() {
                    self.node(child, no, out);
                    // The item's own annotation follows its first child, which
                    // is where the author wrote it: at the end of the first
                    // line, not after a nested list.
                    if index == 0 {
                        self.annotations(node, out);
                    }
                }
            }
            NodeType::Strong => self.wrapped(node, no, "**", out),
            NodeType::Em => self.wrapped(node, no, "*", out),
            NodeType::S => {
                // `s` has no `marker` attribute upstream: there is one spelling.
                out.text("~~");
                out.text(self.inline(node, no));
                out.text("~~");
            }
            NodeType::Code => {
                out.text("`");
                let mut inner = Out::default();
                if let Some(content) = node.get("content") {
                    self.value(content, no, &mut inner);
                }
                out.text(inner.joined().trim().to_owned());
                out.text("`");
            }
            NodeType::Hardbreak => {
                out.text(format!("\\{NL}"));
                out.text(indent);
            }
            NodeType::Softbreak => {
                out.text(NL);
                out.text(indent);
            }
            NodeType::Table => self.table(node, ctx, no, &indent, out),
            NodeType::Thead => {
                // `yield head || []`: the first chunk the children produced, or
                // an empty row. An empty *string* is falsy in JavaScript and
                // becomes an empty row too, which is why this is not simply
                // "the first chunk".
                match self.collect_children(node, no).chunks.into_iter().next() {
                    Some(Chunk::Text(text)) if text.is_empty() => out.row(Vec::new()),
                    Some(chunk) => out.chunks.push(chunk),
                    None => out.row(Vec::new()),
                }
            }
            NodeType::Tr => {
                let cells = self
                    .collect_children(node, no)
                    .chunks
                    .into_iter()
                    .map(|chunk| match chunk {
                        Chunk::Text(text) => text,
                        Chunk::Row(cells) => cells.join(","),
                    })
                    .collect();
                out.row(cells);
            }
            NodeType::Td | NodeType::Th => {
                let mut inner = self.collect_children(node, no);
                self.annotations(node, &mut inner);
                out.text(inner.joined().trim().to_owned());
            }
            NodeType::Tbody => self.children(node, no, out),
            NodeType::Comment => {
                let content = match node.get("content") {
                    Some(value) => self.text_of(value),
                    None => String::new(),
                };
                out.text(format!("<!-- {content} -->\n"));
            }
            // An `error` node has no source to reprint -- its tag did not parse
            // -- and a bare `node` is the default a host constructs. Upstream
            // prints nothing for either.
            NodeType::Error | NodeType::Node => {}
        }
    }

    /// A link or image destination: escaped when it is a string, formatted as a
    /// value when it is not.
    fn destination(&mut self, value: Option<&Value>, ctx: Ctx, out: &mut Out) {
        match value {
            Some(Value::String(text)) => out.text(escape_markdown(text, Escape::Parens)),
            Some(other) => self.value(other, ctx, out),
            None => {}
        }
    }

    /// The optional `"title"` after a destination.
    fn title(&mut self, node: &Node<'_>, out: &mut Out) {
        let Some(title) = node.get("title").filter(|value| value.is_truthy()) else {
            return;
        };
        let title = self.text_of(title);
        out.text(format!("{SPACE}\"{title}\""));
    }

    /// `strong` and `em`, which reprint the marker the author used.
    ///
    /// `**bold**` and `__bold__` are one node type with two spellings, and the
    /// parser records which. Normalising them here would rewrite documents for
    /// no reason a reader can see.
    fn wrapped(&mut self, node: &Node<'_>, ctx: Ctx, default: &str, out: &mut Out) {
        let marker = match node.get("marker") {
            Some(Value::String(marker)) => marker.clone(),
            _ => default.to_owned(),
        };
        out.text(marker.clone());
        out.text(self.inline(node, ctx));
        out.text(marker);
    }

    /// Upstream's `list` arm.
    ///
    /// Split out for length. `no` is the context the items see; their
    /// indentation is measured from the prefix this loop builds.
    fn list(&mut self, node: &Node<'_>, no: Ctx, indent: &str, out: &mut Out) {
        // A list is loose when any item holds a paragraph, which is markdown-it's
        // rule and CommonMark's. Only the last item of a tight list keeps its
        // trailing blank line, so that the list ends where the author ended it.
        let is_loose = node.children.iter().any(|item| {
            item.children
                .iter()
                .any(|child| child.node_type == NodeType::Paragraph)
        });
        let ordered = node.get("ordered").is_some_and(Value::is_truthy);
        let marker = match node.get("marker") {
            Some(Value::String(marker)) => Some(marker.clone()),
            _ => None,
        };
        let start = match node.get("start") {
            Some(Value::Number(start)) => *start,
            _ => 1.0,
        };
        let last = node.children.len().saturating_sub(1);

        for (index, item) in node.children.iter().enumerate() {
            let prefix = if ordered {
                let offset = f64::from(u32::try_from(index).unwrap_or(u32::MAX));
                let number = match self.options.ordered_list_mode {
                    // `parseInt(start)` truncates before adding, so a
                    // fractional start counts from its integer part.
                    OrderedListMode::Increment => js::number(start.trunc() + offset),
                    OrderedListMode::Repeat if index == 0 => js::number(start),
                    OrderedListMode::Repeat => "1".to_owned(),
                };
                format!(
                    "{number}{}",
                    marker.clone().unwrap_or_else(|| OL.to_owned())
                )
            } else {
                marker.clone().unwrap_or_else(|| UL.to_owned())
            };

            let mut item = self.subtree(item, no.increment(utf16_len(&prefix).saturating_add(1)));
            if !is_loose || index == last {
                item = item.trim().to_owned();
            }
            out.text(format!("{NL}{indent}{prefix} {item}"));
        }
        out.text(NL);
    }

    /// Upstream's `table` arm: two printings of one node.
    ///
    /// Inside a `{% table %}` tag the rows print back as the list-and-rule
    /// syntax the author wrote. Anywhere else -- a GFM pipe table -- they print
    /// as an aligned pipe table, padded to the widest cell in each column.
    fn table(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, indent: &str, out: &mut Out) {
        let table = self.collect_children(node, no.increment(2));

        if ctx.parent_is_table_tag {
            for (index, chunk) in table.chunks.iter().enumerate() {
                match chunk {
                    // A tag written between rows -- `{% if %}` and its close --
                    // arrives as loose strings. Each non-blank one becomes a
                    // line of its own, which is why chunk boundaries are kept.
                    Chunk::Text(text) => {
                        if !text.trim().is_empty() {
                            out.text(NL);
                            out.text(text.clone());
                        }
                    }
                    Chunk::Row(cells) => {
                        if index != 0 {
                            out.text(NL);
                            out.text(format!("{indent}---"));
                        }
                        for cell in cells {
                            out.text(format!("{NL}{indent}{UL} {cell}"));
                        }
                    }
                }
            }
            out.text(NL);
            return;
        }

        // A pipe table's `thead` and `tbody` yield rows and nothing else, so
        // filtering to rows loses nothing. Upstream indexes strings by column
        // here and would print characters; there is no tree that reaches it.
        let rows: Vec<&Vec<String>> = table
            .chunks
            .iter()
            .filter_map(|chunk| match chunk {
                Chunk::Row(cells) => Some(cells),
                Chunk::Text(_) => None,
            })
            .collect();
        let Some((head, body)) = rows.split_first() else {
            return;
        };

        let mut widths: Vec<usize> = Vec::new();
        for row in &rows {
            for (column, cell) in row.iter().enumerate() {
                let width = utf16_len(cell);
                if let Some(existing) = widths.get_mut(column) {
                    *existing = (*existing).max(width);
                } else {
                    widths.push(width);
                }
            }
        }

        out.text(NL);
        out.text(table_row(&pad(head, &widths)));
        out.text(NL);
        out.text(table_row(&rule(head, &widths)));
        out.text(NL);
        for row in body {
            out.text(table_row(&pad(row, &widths)));
            out.text(NL);
        }
    }
}

/// What a node's children need to know about it.
fn child_ctx(node: &Node<'_>, ctx: Ctx) -> Ctx {
    Ctx {
        indent: ctx.indent,
        parent_wraps: WRAPPING_TYPES.contains(&node.node_type),
        parent_is_table_tag: node.node_type == NodeType::Tag
            && node.tag.as_deref() == Some("table"),
    }
}

/// `'#'.repeat(n.attributes.level || 1)`, bounded.
///
/// `level` is an ordinary attribute, so a host can set it to any [`f64`], and an
/// unbounded `repeat` is an allocation failure rather than a formatting bug.
/// [`MAX_HEADING_LEVEL`] is far above CommonMark's six, so a parsed document
/// never meets it.
fn heading_level(node: &Node<'_>) -> usize {
    match node.get("level") {
        Some(Value::Number(level)) if *level >= 1.0 => {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the guard proves the value is at least 1, and a float \
                          too large for `usize` saturates rather than wrapping"
            )]
            let level = level.trunc() as usize;
            level.min(MAX_HEADING_LEVEL)
        }
        // Upstream's `|| 1` covers an absent level, a level of zero, and a
        // level that is not a number. A negative one throws there; here it is
        // one heading mark, because a `RangeError` is not output.
        _ => 1,
    }
}

/// The longest run of three or more backticks in `content`, or zero.
fn longest_backtick_run(content: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for character in content.chars() {
        if character == '`' {
            run += 1;
            if run >= 3 {
                longest = longest.max(run);
            }
        } else {
            run = 0;
        }
    }
    longest
}

/// `String.prototype.length`: UTF-16 code units.
///
/// Every width in this file is a JavaScript string length -- the tag-opening
/// threshold and the table column widths -- and counting bytes or `char`s
/// instead would move a line break for a document with an astral character in
/// it.
fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Upstream's `formatTableRow`.
fn table_row(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

/// Each cell, padded to its column width.
fn pad(row: &[String], widths: &[usize]) -> Vec<String> {
    row.iter()
        .enumerate()
        .map(|(column, cell)| {
            let width = widths.get(column).copied().unwrap_or_default();
            let padding = width.saturating_sub(utf16_len(cell));
            format!("{cell}{}", SPACE.repeat(padding))
        })
        .collect()
}

/// The `| --- |` line under the header.
fn rule(row: &[String], widths: &[usize]) -> Vec<String> {
    row.iter()
        .enumerate()
        .map(|(column, _)| "-".repeat(widths.get(column).copied().unwrap_or_default()))
        .collect()
}
