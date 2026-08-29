//! Finding `{%` and `%}` in raw text.
//!
//! Ported from the part of upstream `src/utils.ts` the segmenter needs:
//! `OPEN`, `CLOSE`, `findTagEnd` and `containsMarkdocTagInUrl`. The rest of
//! that file -- the schema and transform helpers -- lands with the transformer.
//!
//! # Why a state machine rather than a search for `%}`
//!
//! A tag attribute can hold a string, and a string can hold `%}`:
//! `{% foo bar="100%}" %}`. Searching for the first `%}` closes the tag inside
//! the string and leaves the parser looking at nonsense. Upstream tracks three
//! states -- normal, inside a string, and just after a backslash -- and so does
//! this. Nothing else about the tag's grammar is known here; that is the
//! grammar's job. This only has to find where the tag stops.

/// The opening delimiter.
pub const OPEN: &str = "{%";

/// The closing delimiter.
pub const CLOSE: &str = "%}";

#[derive(Clone, Copy)]
enum State {
    Normal,
    String,
    Escape,
}

/// The byte offset of the `%}` that closes a tag opened at or after `start`.
///
/// Returns the offset of the `%` itself, as upstream does, so the caller adds
/// [`CLOSE`]`.len()` to step past it. [`None`] means the tag is never closed,
/// which is not an error: unclosed `{%` is ordinary text.
///
/// `start` is a byte offset into `content`. An offset that is out of range or
/// not on a character boundary yields [`None`] rather than panicking -- this is
/// fed arbitrary text and panic-freedom is a published promise.
#[must_use]
pub fn find_tag_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if start > bytes.len() {
        return None;
    }
    let mut state = State::Normal;
    let mut pos = start;
    while pos < bytes.len() {
        let byte = *bytes.get(pos)?;
        match state {
            State::String => match byte {
                b'"' => state = State::Normal,
                b'\\' => state = State::Escape,
                _ => {}
            },
            State::Escape => state = State::String,
            State::Normal => {
                if byte == b'"' {
                    state = State::String;
                } else if bytes.get(pos..pos + CLOSE.len()) == Some(CLOSE.as_bytes()) {
                    return Some(pos);
                }
            }
        }
        pos += 1;
    }
    None
}

/// Whether a run of inline text puts a tag or a variable inside a URL.
///
/// Ported from upstream's link plugin, which raises `href-format-invalid` for
/// this. It works on the raw text of an inline run rather than on a parsed
/// link, because the interesting cases are exactly the ones that fail to parse
/// as a link: `https://example.com/{% tag %}` is a bare autolink-shaped string
/// with a tag glued to it, and `[Link](https://{% $x %})` does not parse as a
/// link at all once a space appears in the destination.
///
/// The rule, spelled as upstream spells it: find the first `{%`, walk back to
/// the preceding whitespace, and report whether the text between there and the
/// tag contains one of `protocols` followed by `://`. Only the first tag is
/// considered -- a second one is unreachable, which is upstream's behaviour and
/// is kept rather than improved, because widening it would flag documents
/// upstream accepts.
#[must_use]
pub fn contains_markdoc_tag_in_url(content: &str, protocols: &[&str]) -> bool {
    let bytes = content.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes.get(pos..pos + OPEN.len()) != Some(OPEN.as_bytes()) {
            pos += 1;
            continue;
        }
        if find_tag_end(content, pos).is_none() {
            // An unclosed `{%` is not a tag. Step over the delimiter and keep
            // looking, as upstream does.
            pos += OPEN.len();
            continue;
        }
        let mut start = pos;
        while start > 0
            && !bytes
                .get(start - 1)
                .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            start -= 1;
        }
        let prefix = content.get(start..pos).unwrap_or("");
        return protocols
            .iter()
            .any(|protocol| prefix.contains(&format!("{protocol}://")));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_closing_delimiter() {
        assert_eq!(find_tag_end("{% foo %}", 0), Some(7));
        assert_eq!(find_tag_end("a {% foo %} b", 2), Some(9));
    }

    #[test]
    fn a_closing_delimiter_inside_a_string_does_not_close_the_tag() {
        let content = r#"{% foo bar="100%}" %}"#;
        assert_eq!(find_tag_end(content, 0), Some(19));
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        let content = r#"{% foo bar="a\"%}b" %}"#;
        assert_eq!(find_tag_end(content, 0), Some(20));
    }

    #[test]
    fn an_unclosed_tag_is_not_an_error() {
        assert_eq!(find_tag_end("{% foo", 0), None);
        assert_eq!(find_tag_end("{% foo \"unterminated %}", 0), None);
    }

    #[test]
    fn a_start_past_the_end_yields_none_rather_than_panicking() {
        assert_eq!(find_tag_end("{% %}", 99), None);
    }

    /// Ported from `tokenizer/plugins/link.test.ts`.
    #[test]
    fn urls_carrying_tags_are_detected() {
        let http = ["http", "https"];
        assert!(!contains_markdoc_tag_in_url(
            "The link is https://example.com. {% tag /%})",
            &http
        ));
        assert!(!contains_markdoc_tag_in_url("[Link]({% tag %})", &http));
        assert!(contains_markdoc_tag_in_url(
            "https://example.com/{% tag %}content{% /tag %})",
            &http
        ));
        assert!(contains_markdoc_tag_in_url(
            "https://en.wikipedia.org/wiki/Exam_(disambiguation){% tag /%}",
            &http
        ));
        assert!(contains_markdoc_tag_in_url(
            "[Link](https://{% $variable.custom_value %})",
            &http
        ));
        assert!(contains_markdoc_tag_in_url(
            "[Link](https://example.com/{% tag /%})",
            &http
        ));
        assert!(contains_markdoc_tag_in_url(
            "[Link](https://example.com/{% tag %}content{% /tag %})",
            &http
        ));
    }

    /// Ported from `link.test.ts`, "rejects custom protocols defined in the
    /// config with markdoc variable".
    #[test]
    fn the_protocol_list_is_the_callers() {
        assert!(contains_markdoc_tag_in_url(
            "[Link](vscode://{% $variable.custom_value %})",
            &["vscode"]
        ));
        assert!(!contains_markdoc_tag_in_url(
            "[Link](vscode://{% $variable.custom_value %})",
            &["http", "https"]
        ));
    }

    #[test]
    fn an_unclosed_tag_in_a_url_is_stepped_over() {
        assert!(!contains_markdoc_tag_in_url(
            "https://example.com/{% unclosed",
            &["https"]
        ));
    }
}
