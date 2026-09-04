//! The three character classes the formatter escapes, and the one it replaces.
//!
//! Upstream writes them as regular expressions passed to
//! `escapeMarkdownCharacters`. Two are global replacements over a character
//! set; the third is a non-global alternation, so it rewrites **one** match in
//! the whole string, and which one depends on the alternation order. Spelling
//! that out is the point of this module: a global replacement of the same
//! alternation would escape a `#` in the middle of a sentence, and a `>` on
//! every line of a run of text.
//!
//! Every class ends with the same substitution: a non-breaking space becomes
//! `&nbsp;`. Upstream carries a `TODO` asking whether the entity should have
//! stayed in the AST. It did not, so the character has to be written back as an
//! entity or the reprinted document collapses it to an ordinary space.

/// Which characters to escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Escape {
    /// `/[()]/g` -- the parentheses that would close a link or image
    /// destination early.
    Parens,
    /// `/[*_~]/g` -- the markers that would close a `strong`, `em` or `s`
    /// wrapper early. Applied to text inside one.
    Wrapping,
    /// `/^\*|#+\s|^>/` -- a leading `*`, a run of `#` before whitespace, or a
    /// leading `>`. Applied to text that is not inside a wrapper, so that a
    /// paragraph beginning with one does not re-parse as a list item, a heading
    /// or a blockquote.
    ///
    /// **Not global, and not anchored per line.** Upstream's regular expression
    /// carries neither `g` nor `m`, so exactly one match is rewritten and `^`
    /// means the start of the whole string.
    Block,
}

/// Escape `text` for the given class, then write non-breaking spaces as
/// entities.
pub(super) fn escape_markdown(text: &str, class: Escape) -> String {
    let escaped = match class {
        Escape::Parens => escape_set(text, &['(', ')']),
        Escape::Wrapping => escape_set(text, &['*', '_', '~']),
        Escape::Block => escape_block(text),
    };
    non_breaking_spaces(&escaped)
}

/// A global replacement of a character set, each match prefixed with `\`.
fn escape_set(text: &str, set: &[char]) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if set.contains(&character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// `/^\*|#+\s|^>/` -- one match, chosen the way a regular expression engine
/// chooses one.
///
/// The engine scans positions left to right and, at each, tries the
/// alternatives in the order written. Only the first two can match at position
/// zero and only the second can match anywhere else, so the whole search is:
/// a leading `*`, else a leading `>`, else the first run of `#` that is
/// followed by whitespace.
///
/// The `>` case is easy to get wrong in the other direction. `#+\s` is tried
/// *before* `^>` at position zero, but the two cannot both match there -- a
/// string starts with one character -- so the order is only visible in the code,
/// never in the output.
fn escape_block(text: &str) -> String {
    let Some(start) = first_block_match(text) else {
        return text.to_owned();
    };
    // The replacement is `\$&` -- a backslash before the whole match -- so only
    // where the match *starts* matters. Everything from there on is unchanged.
    let mut out = String::with_capacity(text.len() + 1);
    if let Some(head) = text.get(..start) {
        out.push_str(head);
    }
    out.push('\\');
    if let Some(rest) = text.get(start..) {
        out.push_str(rest);
    }
    out
}

/// Where the single match starts, if there is one.
fn first_block_match(text: &str) -> Option<usize> {
    if text.starts_with('*') || text.starts_with('>') {
        return Some(0);
    }
    // `#+\s`: a run of `#` followed by one whitespace character. Backtracking
    // inside the run cannot help -- every character before the end of a run of
    // `#` is another `#`, which is not whitespace -- so the only candidate at
    // each position is the whole run.
    let mut indices = text.char_indices().peekable();
    while let Some((index, character)) = indices.next() {
        if character != '#' {
            continue;
        }
        while let Some((_, '#')) = indices.peek().copied() {
            indices.next();
        }
        if let Some((_, whitespace)) = indices.peek().copied()
            && is_js_whitespace(whitespace)
        {
            return Some(index);
        }
    }
    None
}

/// JavaScript's `\s`, near enough.
///
/// Rust's `char::is_whitespace` is Unicode `White_Space`, which differs from
/// ECMAScript's `\s` in two characters: it includes U+0085, which `\s` does
/// not, and excludes U+FEFF, which `\s` does. Both are unreachable in the run
/// of text a Markdown parser hands back after a `#`, and reimplementing the
/// class to recover them would be more code than the difference is worth.
fn is_js_whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

/// Write U+00A0 as `&nbsp;`.
fn non_breaking_spaces(text: &str) -> String {
    if !text.contains('\u{a0}') {
        return text.to_owned();
    }
    text.replace('\u{a0}', "&nbsp;")
}

#[cfg(test)]
mod tests {
    use super::{Escape, escape_markdown};

    #[test]
    fn parentheses_are_escaped_everywhere_they_appear() {
        assert_eq!(
            escape_markdown("https://example.com?q=()", Escape::Parens),
            "https://example.com?q=\\(\\)"
        );
    }

    #[test]
    fn wrapper_markers_are_escaped_everywhere_they_appear() {
        assert_eq!(
            escape_markdown("a _sentence_ with *stars* and ~tildes~", Escape::Wrapping),
            "a \\_sentence\\_ with \\*stars\\* and \\~tildes\\~"
        );
    }

    #[test]
    fn a_leading_star_is_escaped_and_a_later_one_is_not() {
        assert_eq!(
            escape_markdown("* List item", Escape::Block),
            "\\* List item"
        );
        assert_eq!(escape_markdown("a * b * c", Escape::Block), "a * b * c");
    }

    #[test]
    fn a_leading_angle_bracket_is_escaped_and_a_later_one_is_not() {
        assert_eq!(
            escape_markdown("> Blockquote", Escape::Block),
            "\\> Blockquote"
        );
        assert_eq!(
            escape_markdown("Text > not a blockquote", Escape::Block),
            "Text > not a blockquote"
        );
    }

    #[test]
    fn a_run_of_hashes_before_whitespace_is_escaped_once() {
        assert_eq!(escape_markdown("# Heading", Escape::Block), "\\# Heading");
        assert_eq!(
            escape_markdown("### Heading", Escape::Block),
            "\\### Heading"
        );
        // No whitespace after the run, so nothing is a heading and nothing is
        // escaped.
        assert_eq!(
            escape_markdown("#Not a heading", Escape::Block),
            "#Not a heading"
        );
        // Exactly one match, even when two would qualify.
        assert_eq!(escape_markdown("a # b # c", Escape::Block), "a \\# b # c");
    }

    #[test]
    fn a_run_of_hashes_backtracks_to_nothing_rather_than_to_a_shorter_run() {
        // `#+` cannot match `##` here and then find whitespace, and it cannot
        // match `#` either, because the next character is another `#`.
        assert_eq!(escape_markdown("##a## b", Escape::Block), "##a\\## b");
    }

    #[test]
    fn non_breaking_spaces_become_entities_in_every_class() {
        assert_eq!(escape_markdown("a\u{a0}b", Escape::Block), "a&nbsp;b");
        assert_eq!(escape_markdown("a\u{a0}b", Escape::Parens), "a&nbsp;b");
        assert_eq!(escape_markdown("a\u{a0}b", Escape::Wrapping), "a&nbsp;b");
    }
}
