//! HTML escaping, reproduced from markdown-it rather than approximated.
//!
//! Upstream's HTML renderer escapes with markdown-it's `escapeHtml`
//! (`reference/src/renderers/html.ts:4`). This crate has no markdown-it, and no
//! two escapers agree on the edges: some add `'` as `&#39;`, some add `/`, some
//! escape non-ASCII. Reaching for the nearest available escaper would produce
//! output that looks right and differs on a character the corpus does not
//! cover -- and this is the function standing between authored content and a
//! browser, so an approximation is not acceptable.
//!
//! # What upstream actually does
//!
//! markdown-it 12.3.2, `lib/common/utils.js`, verified against the published
//! source at that tag. Upstream pins 12.3.2 in `package-lock.json` and patches
//! it (`reference/patches/markdown-it+12.3.2.patch`), but the patch touches
//! nine block rules and never `lib/common/utils.js`, so the escaper in force is
//! stock:
//!
//! ```js
//! var HTML_REPLACEMENTS = {
//!   '&': '&amp;',
//!   '<': '&lt;',
//!   '>': '&gt;',
//!   '"': '&quot;'
//! };
//!
//! function escapeHtml(str) {
//!   if (HTML_ESCAPE_TEST_RE.test(str)) {
//!     return str.replace(HTML_ESCAPE_REPLACE_RE, replaceUnsafeChar);
//!   }
//!   return str;
//! }
//! ```
//!
//! Four characters, each replaced independently in one pass, and nothing else
//! touched. The fast path for a string with none of them is an optimisation,
//! not a behaviour, which is why [`escape_html`] returns a [`Cow`]: same
//! answer, same allocation profile.
//!
//! # Why the single quote is not in the set, and why that is safe here
//!
//! An escaper that omits `'` is only safe if every attribute it feeds is
//! double-quoted. That is not a convention here, it is the renderer: `html.rs`
//! writes `="` and `"` around every value with no branch that could choose
//! otherwise, so an apostrophe in a value can never end the attribute. Adding
//! `&#39;` would be strictly safer in the abstract and a divergence in fact --
//! it changes the bytes upstream produces for ordinary prose.

use std::borrow::Cow;

/// Escape a string exactly as markdown-it's `escapeHtml` does.
///
/// Replaces `&`, `<`, `>` and `"` with their named entities and leaves
/// everything else, including `'`, unchanged. Returns the input borrowed when
/// it contains none of the four.
///
/// # Examples
///
/// ```
/// use proust::render::escape_html;
///
/// assert_eq!(escape_html("a & b"), "a &amp; b");
/// assert_eq!(escape_html(r#""quoted""#), "&quot;quoted&quot;");
/// // Not in upstream's set, so not escaped.
/// assert_eq!(escape_html("it's"), "it's");
/// ```
#[must_use]
pub fn escape_html(input: &str) -> Cow<'_, str> {
    if input.contains(['&', '<', '>', '"']) {
        let mut out = String::with_capacity(input.len());
        escape_html_into(&mut out, input);
        Cow::Owned(out)
    } else {
        Cow::Borrowed(input)
    }
}

/// Append the escaped form of `input` to `out`.
///
/// The renderer builds one string for a whole document, so it appends rather
/// than allocating a `Cow` per text node.
pub(crate) fn escape_html_into(out: &mut String, input: &str) {
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_exactly_the_four_characters_markdown_it_escapes() {
        assert_eq!(escape_html("&"), "&amp;");
        assert_eq!(escape_html("<"), "&lt;");
        assert_eq!(escape_html(">"), "&gt;");
        assert_eq!(escape_html("\""), "&quot;");
    }

    #[test]
    fn leaves_every_other_ascii_character_alone() {
        for byte in 0x20u8..0x7f {
            let ch = char::from(byte);
            if matches!(ch, '&' | '<' | '>' | '"') {
                continue;
            }
            let input = ch.to_string();
            assert_eq!(
                escape_html(&input),
                input,
                "U+{byte:04X} must pass through unchanged"
            );
        }
    }

    #[test]
    fn does_not_escape_the_single_quote() {
        // The most common wrong answer. `&#39;` here would be a divergence.
        assert_eq!(escape_html("it's an 'x'"), "it's an 'x'");
    }

    #[test]
    fn does_not_escape_the_solidus() {
        // The other common addition, from the OWASP-style escapers.
        assert_eq!(escape_html("a/b"), "a/b");
    }

    #[test]
    fn does_not_touch_non_ascii() {
        assert_eq!(
            escape_html("naïve \u{2014} 日本語 \u{1f600}"),
            "naïve — 日本語 😀"
        );
    }

    #[test]
    fn does_not_double_escape() {
        // Each character is replaced once, in one pass, so the ampersand of an
        // entity already in the text is escaped and its tail is not re-read.
        assert_eq!(escape_html("&amp;"), "&amp;amp;");
        assert_eq!(escape_html("&lt;"), "&amp;lt;");
    }

    #[test]
    fn replaces_every_occurrence_not_just_the_first() {
        assert_eq!(escape_html("<<>>"), "&lt;&lt;&gt;&gt;");
    }

    #[test]
    fn mixed_input_keeps_its_surroundings() {
        assert_eq!(
            escape_html(r#"<a href="x">tom & jerry</a>"#),
            "&lt;a href=&quot;x&quot;&gt;tom &amp; jerry&lt;/a&gt;"
        );
    }

    #[test]
    fn borrows_when_there_is_nothing_to_escape() {
        assert!(matches!(escape_html("plain text"), Cow::Borrowed(_)));
        assert!(matches!(escape_html("a & b"), Cow::Owned(_)));
    }

    #[test]
    fn the_empty_string_is_unchanged() {
        assert_eq!(escape_html(""), "");
    }

    #[test]
    fn control_characters_pass_through() {
        // markdown-it's regex is four characters wide. A NUL or a newline is
        // not in it, and inventing an escape for either would diverge.
        assert_eq!(escape_html("a\nb\tc\0d"), "a\nb\tc\0d");
    }
}
