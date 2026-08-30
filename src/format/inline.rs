//! The inline arms: links, images, text, code, and the wrappers.
//!
//! Split out of the node switch so that only the live arm's locals sit on the
//! stack at each level of nesting (see `super::node`).
//!
//! # The one rule worth reading twice
//!
//! `text` escapes against its **parent**, not against itself. Inside `strong`,
//! `em` or `s` it escapes `*`, `_` and `~`, because an unescaped one would close
//! the wrapper early. Anywhere else it escapes the single leading character
//! that would turn the line into a different block -- a `*` into a list item, a
//! `>` into a blockquote, a run of `#` into a heading. The two sets are
//! disjoint on purpose: escaping the block characters inside emphasis would
//! litter a sentence with backslashes for no reader's benefit.

use super::escape::{escape_markdown, Escape};
use super::{Ctx, Formatter, Out, CLOSE, OPEN, SPACE};
use crate::ast::{Node, Value};

impl Formatter<'_> {
    pub(super) fn image(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
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

    pub(super) fn link(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        let children = self.collect_children(node, no).joined();
        let has_title = node.get("title").is_some_and(Value::is_truthy);

        // <https://spec.commonmark.org/0.31.2/#autolinks>: a link whose text is
        // its own destination reprints in the short form.
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

    pub(super) fn text(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, out: &mut Out) {
        match node.get("content") {
            // A variable or a function in text position is a tag again, which
            // is how `{% $user.name %}` inside a sentence survives a round trip.
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
        }
    }

    pub(super) fn code(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        out.text("`");
        let mut inner = Out::default();
        if let Some(content) = node.get("content") {
            self.value(content, no, &mut inner);
        }
        out.text(inner.joined().trim().to_owned());
        out.text("`");
    }

    /// `strong` and `em`, which reprint the marker the author used.
    ///
    /// `**bold**` and `__bold__` are one node type with two spellings, and the
    /// parser records which. Normalising them here would rewrite documents for
    /// no reason a reader can see.
    pub(super) fn wrapped(&mut self, node: &Node<'_>, ctx: Ctx, default: &str, out: &mut Out) {
        let marker = match node.get("marker") {
            Some(Value::String(marker)) => marker.clone(),
            _ => default.to_owned(),
        };
        out.text(marker.clone());
        out.text(self.inline_text(node, ctx));
        out.text(marker);
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
}
