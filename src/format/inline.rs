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

use super::escape::{Escape, escape_markdown};
use super::{CLOSE, Ctx, Formatter, OPEN, Out, SPACE};
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
        self.destination(node, node.get("src"), no, out);
        self.title(node, out);
        out.text(")");
    }

    pub(super) fn link(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        let children = self.collect_children(node, no).joined();
        let has_title = node.get("title").is_some_and(Value::is_truthy);

        // <https://spec.commonmark.org/0.31.2/#autolinks>: a link whose text is
        // its own destination reprints in the short form, but only when that
        // destination is an absolute URI. `<README.md>` is not a link at all,
        // and `<a@b.c>` is one to `mailto:a@b.c` (#84, markdoc/markdoc#651).
        if !has_title
            && let Some(Value::String(href)) = node.get("href")
            && children == *href
            && is_absolute_uri(href)
        {
            out.text(format!("<{href}>"));
            return;
        }
        // An email autolink is written in the long form, as upstream writes
        // it, unless its text holds a tag kept as text: only an autolink keeps
        // one literal, and in `[...]` it would read back as a tag.
        if !has_title
            && let Some(Value::String(href)) = node.get("href")
            && href.strip_prefix("mailto:") == Some(children.as_str())
            && is_email(&children)
            && holds_literal_tag(node)
        {
            out.text(format!("<{children}>"));
            return;
        }

        out.text("[");
        out.text(children);
        out.text("]");
        out.text("(");
        self.destination(node, node.get("href"), no, out);
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

    /// A link or image destination: written so that it reads back as the same
    /// string when it is one, formatted as a value when it is not.
    ///
    /// An empty destination before a title is `<>`: written as nothing, the
    /// title would read back as the destination.
    fn destination(&mut self, node: &Node<'_>, value: Option<&Value>, ctx: Ctx, out: &mut Out) {
        match value {
            Some(Value::String(text)) if !text.is_empty() => out.text(written_destination(text)),
            Some(Value::String(_)) | None => {
                if title_of(node).is_some() {
                    out.text("<>");
                }
            }
            Some(other) => self.value(other, ctx, out),
        }
    }

    /// The optional `"title"` after a destination, escaped so that it reads
    /// back as written.
    fn title(&mut self, node: &Node<'_>, out: &mut Out) {
        let Some(title) = title_of(node) else {
            return;
        };
        let title = escape_url_part(&self.text_of(title), &['"']);
        out.text(format!("{SPACE}\"{title}\""));
    }
}

/// Whether `href` is a CommonMark [absolute URI], the only destination an
/// autolink can hold: a scheme of 2 to 32 characters, a `:`, and then no
/// control character, space, `<` or `>`.
///
/// [absolute URI]: https://spec.commonmark.org/0.31.2/#absolute-uri
fn is_absolute_uri(href: &str) -> bool {
    let Some((scheme, rest)) = href.split_once(':') else {
        return false;
    };
    let mut letters = scheme.chars();
    letters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && letters.all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '+' | '.' | '-'))
        && (2..=32).contains(&scheme.len())
        && !rest
            .chars()
            .any(|character| character.is_ascii_control() || matches!(character, ' ' | '<' | '>'))
}

/// The title a link or image has, if any.
fn title_of<'n>(node: &'n Node<'_>) -> Option<&'n Value> {
    node.get("title").filter(|value| value.is_truthy())
}

/// Whether `text` is the address of a CommonMark [email autolink].
///
/// [email autolink]: https://spec.commonmark.org/0.31.2/#email-address
fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
        && domain.split('.').all(label)
}

/// Whether a text node under `node` holds `{%` as text, which only an
/// autolink's text can.
fn holds_literal_tag(node: &Node<'_>) -> bool {
    node.walk().any(
        |child| matches!(child.get("content"), Some(Value::String(text)) if text.contains("{%")),
    )
}

/// A destination string, written so that it reads back as itself (#91).
///
/// The bare form holds no space or control character and does not start with
/// `<`; its parentheses are escaped, as upstream escapes them. Anything else
/// goes between `<` and `>`, inside which `<` and `>` are escaped. Upstream
/// needs only the bare form because its tree holds the encoded destination,
/// which has no space in it; the tree here keeps it as written
/// (`DIVERGENCES.md` entry 17). A line ending is written as a reference, so
/// it needs no brackets.
fn written_destination(text: &str) -> String {
    let bracketed = text.starts_with('<')
        || text.chars().any(|character| {
            (character.is_ascii_control() && !matches!(character, '\n' | '\r')) || character == ' '
        });
    if bracketed {
        format!("<{}>", escape_url_part(text, &['<', '>']))
    } else {
        escape_url_part(text, &['(', ')'])
    }
}

/// A destination or title, with `special` -- what would close it -- escaped,
/// and so that every other character reads back as itself:
///
/// - a backslash is escaped where it would otherwise escape what follows it:
///   before anything written with leading ASCII punctuation, and at the end,
///   where the next character written is the one that closes the part;
/// - an `&` that would start a character reference is escaped;
/// - a line ending, which a destination cannot hold, and U+00A0, which upstream
///   writes as `&nbsp;` everywhere, are written as references.
fn escape_url_part(text: &str, special: &[char]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        match character {
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '\\' if characters
                .peek()
                .is_none_or(|&(_, next)| writes_punctuation(next)) =>
            {
                out.push_str("\\\\");
            }
            '&' if starts_reference(text.get(at + 1..).unwrap_or_default()) => {
                out.push_str("\\&");
            }
            _ if special.contains(&character) => {
                out.push('\\');
                out.push(character);
            }
            _ => out.push(character),
        }
    }
    out
}

/// Whether `next` is written starting with ASCII punctuation: as itself, or as
/// the `&` of the reference that stands for it.
fn writes_punctuation(next: char) -> bool {
    next.is_ascii_punctuation() || matches!(next, '\n' | '\r' | '\u{a0}')
}

/// Whether the text after an `&` makes it a character reference: a name, or
/// `#` and decimal digits, or `#x` and hexadecimal digits, then `;`.
///
/// A name is taken to be one whether or not HTML defines it. Escaping an `&`
/// that needed none costs a backslash and reads back the same.
fn starts_reference(rest: &str) -> bool {
    let Some((body, _)) = rest.split_once(';') else {
        return false;
    };
    let (digits, valid): (&str, fn(char) -> bool) =
        if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
            (hex, |c: char| c.is_ascii_hexdigit())
        } else if let Some(decimal) = body.strip_prefix('#') {
            (decimal, |c: char| c.is_ascii_digit())
        } else {
            (body, |c: char| c.is_ascii_alphanumeric())
        };
    !digits.is_empty() && digits.chars().all(valid)
}
