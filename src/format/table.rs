//! The table arms: two printings of one node type.
//!
//! A `table` node reaches the formatter from two directions, and it prints
//! differently depending on which. Written as a GFM pipe table, it reprints as
//! an aligned pipe table. Written as Markdoc's advanced syntax -- lists
//! separated by thematic breaks inside a `{% table %}` tag -- it reprints as
//! that, because the two are different documents and rewriting one into the
//! other loses the cell content that only the advanced form can hold.
//!
//! # Why this layer needs chunks at all
//!
//! Everywhere else, output is text. Here it is not: a `tr` yields a *row*, and
//! the `{% table %}` printing walks the yielded items to decide what each one
//! is. A row becomes one `- cell` line per cell, preceded by a `---` rule
//! unless it is the header. A **string** between rows -- which is what a
//! `{% if %}` wrapping some rows yields -- becomes a line of its own.
//!
//! That is the whole reason [`Chunk`](super::Chunk) exists, and the reason
//! nothing coalesces adjacent output: each string a tag yields is a separate
//! line here, so joining two of them would join two lines.

use super::{Chunk, Ctx, Formatter, Out, NL, SPACE, UL};
use crate::ast::Node;

impl Formatter<'_> {
    /// Upstream's `table` arm.
    pub(super) fn table(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, out: &mut Out) {
        let table = self.collect_children(node, no.increment(2));
        if ctx.parent_is_table_tag {
            advanced(&table, ctx.indent, out);
        } else {
            pipe(&table, out);
        }
    }

    /// Upstream's `thead` arm: the first chunk the children produced, or an
    /// empty row.
    ///
    /// `head || []` -- an empty *string* is falsy in JavaScript and becomes an
    /// empty row too, which is why this is not simply "the first chunk".
    pub(super) fn thead(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        match self.collect_children(node, no).chunks.into_iter().next() {
            Some(Chunk::Text(text)) if text.is_empty() => out.row(Vec::new()),
            Some(chunk) => out.chunks.push(chunk),
            None => out.row(Vec::new()),
        }
    }

    /// Upstream's `tr` arm: the cells, as a row.
    pub(super) fn tr(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        let cells = self
            .collect_children(node, no)
            .chunks
            .into_iter()
            .map(|chunk| match chunk {
                Chunk::Text(text) => text,
                // A row inside a row is a table inside a cell, which the cell
                // arm below has already flattened. Upstream would coerce the
                // array; this says so rather than dropping it.
                Chunk::Row(cells) => cells.join(","),
            })
            .collect();
        out.row(cells);
    }

    /// Upstream's `td` and `th` arms, which are one arm.
    ///
    /// A cell is joined and trimmed into a single string, annotations included,
    /// because the row above prints it as one item of a `| a | b |` line.
    pub(super) fn cell(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        let mut inner = self.collect_children(node, no);
        self.annotations(node, &mut inner);
        out.text(inner.joined().trim().to_owned());
    }
}

/// Markdoc's advanced syntax: one `- cell` line per cell, rows separated by
/// `---`.
fn advanced(table: &Out, indent: usize, out: &mut Out) {
    let indent = SPACE.repeat(indent);
    for (index, chunk) in table.chunks.iter().enumerate() {
        match chunk {
            // A tag written between rows -- `{% if %}` and its close -- arrives
            // as loose strings. Each non-blank one becomes a line of its own,
            // which is why chunk boundaries are kept.
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
}

/// A GFM pipe table, padded so the columns line up in the source.
fn pipe(table: &Out, out: &mut Out) {
    // A pipe table's `thead` and `tbody` yield rows and nothing else, so
    // filtering to rows loses nothing. Upstream indexes a string by column here
    // and would print characters; no tree reaches it.
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
            let width = super::utf16_len(cell);
            if let Some(existing) = widths.get_mut(column) {
                *existing = (*existing).max(width);
            } else {
                widths.push(width);
            }
        }
    }

    out.text(NL);
    out.text(row(&pad(head, &widths)));
    out.text(NL);
    out.text(row(&rule(head, &widths)));
    out.text(NL);
    for cells in body {
        out.text(row(&pad(cells, &widths)));
        out.text(NL);
    }
}

/// Upstream's `formatTableRow`.
fn row(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

/// Each cell, padded to its column width.
fn pad(cells: &[String], widths: &[usize]) -> Vec<String> {
    cells
        .iter()
        .enumerate()
        .map(|(column, cell)| {
            let width = widths.get(column).copied().unwrap_or_default();
            let padding = width.saturating_sub(super::utf16_len(cell));
            format!("{cell}{}", SPACE.repeat(padding))
        })
        .collect()
}

/// The `| --- |` line under the header.
fn rule(cells: &[String], widths: &[usize]) -> Vec<String> {
    cells
        .iter()
        .enumerate()
        .map(|(column, _)| "-".repeat(widths.get(column).copied().unwrap_or_default()))
        .collect()
}
