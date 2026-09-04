//! Upstream's `src/transforms/table.test.ts`, ported.
//!
//! 213 lines, and the specification for the one pass that runs inside `parse`.
//! Every assertion there is about `table-syntax`: which content between rows is
//! accepted, which is rejected, and -- the part worth porting carefully -- which
//! *node* the resulting error points at.
//!
//! # What is ported differently, and why
//!
//! Upstream reaches the errors through `Markdoc.validate`, which is the
//! validator's job and a different goal. It does not have to: the table pass
//! attaches its errors to nodes, and the validator only collects them. So this
//! suite collects them itself and asserts on the same fields upstream does.
//!
//! Upstream also asserts `error.lines` and `location.end.line`, both of which
//! are `start.line + 1` there because its location is a line map. This crate's
//! location is a byte range with the text borrowed, so a paragraph occupying one
//! line ends on that line rather than the next. The assertions keep
//! `start.line` -- which is what "the error points at the right content" means --
//! and drop the end line rather than assert a number that describes upstream's
//! representation instead of this one.

use accent_proust::ast::{ErrorLevel, Node, ValidationError};
use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};

/// Every `table-syntax` error in a document, in walk order.
///
/// Upstream's `errors.filter((e) => e.error.id === 'table-syntax')`, without the
/// validator in between.
fn table_syntax_errors<'n, 'a>(document: &'n Node<'a>) -> Vec<&'n ValidationError<'a>> {
    let mut out: Vec<&ValidationError<'a>> = Vec::new();
    let mut collect = |node: &'n Node<'a>| {
        out.extend(
            node.errors
                .iter()
                .filter(|error| error.id == "table-syntax"),
        );
    };
    collect(document);
    for node in document.walk() {
        collect(node);
    }
    out
}

/// Upstream's `validate(string)`: parse, then look at what the parse reported.
fn parse_document(source: &str) -> Node<'_> {
    parse_with(
        source,
        &PulldownTokenizer::new(),
        &ParseOptions::new().allow_comments(true),
    )
}

/// The zero-based start line of an error, or `None` if it carries no location.
fn start_line(error: &ValidationError<'_>) -> Option<usize> {
    error.location.map(|location| location.start.line)
}

#[test]
fn produces_an_error_for_non_row_content_at_the_row_level_of_a_table() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 * Cell 1\n\
                 * Cell 2\n\
                 {% if $foo %}\n\
                 This is invalid conditional content at the row level of the table.\n\
                 {% /if %}\n\
                 This is invalid non-conditional content at the row level.\n\
                 {% /table %}";

    let document = parse_document(input);
    let errors = table_syntax_errors(&document);

    // One error for the paragraph inside the conditional, one for the bare
    // paragraph at the row level.
    assert_eq!(errors.len(), 2, "{errors:#?}");
    for error in &errors {
        assert_eq!(error.level, ErrorLevel::Critical);
        assert!(error.message.contains("paragraph"), "{}", error.message);
        assert!(error.message.contains("indented"), "{}", error.message);
    }

    // The errors point at the invalid content, not at the table.
    let lines: Vec<Option<usize>> = errors.iter().map(|error| start_line(error)).collect();
    assert!(
        lines.contains(&Some(7)),
        "no error on the conditional's paragraph: {lines:?}"
    );
    assert!(
        lines.contains(&Some(9)),
        "no error on the row-level paragraph: {lines:?}"
    );
}

#[test]
fn does_not_produce_errors_for_valid_conditional_rows() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 {% if $foo %}\n\
                 * Row 1 Cell 1\n\
                 * Row 1 Cell 2\n\
                 {% /if %}\n\
                 ---\n\
                 {% if $bar %}\n\
                 * Row 2 Cell 1\n\
                 * Row 2 Cell 2\n\
                 {% else /%}\n\
                 * Alt Row 2 Cell 1\n\
                 * Alt Row 2 Cell 2\n\
                 {% /if %}\n\
                 {% /table %}";

    let document = parse_document(input);
    assert!(table_syntax_errors(&document).is_empty());
}

/// Upstream's "does not produce errors for valid conditionals within a cell",
/// inverted by `DIVERGENCES.md` entry 12.
///
/// The conditional is written two spaces in under `* Cell 2`, which upstream
/// reads as that cell's content because its block-tag rule runs after the
/// container parser. The segmenter here runs before one, so the tag splits the
/// document and arrives at the row level, where a paragraph is not a row.
///
/// The assertion is the divergent outcome rather than upstream's, deliberately:
/// a segmenter that ever learns about containers turns this test red, which is
/// the notification the divergence entry is asking for.
#[test]
fn a_conditional_indented_within_a_cell_leaves_the_cell() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 * Cell 1\n\
                 * Cell 2\n\
                 \x20 {% if $foo %}\n\
                 \x20 This is a conditional paragraph inside cell 2.\n\
                 \x20 {% else /%}\n\
                 \x20 This is an alternate paragraph inside cell 2.\n\
                 \x20 {% /if %}\n\
                 {% /table %}";

    let document = parse_document(input);
    let errors = table_syntax_errors(&document);
    assert_eq!(
        errors.len(),
        2,
        "expected the two paragraphs of the escaped conditional to be rejected \
         at the row level: {errors:#?}"
    );
    for error in &errors {
        assert!(error.message.contains("paragraph"), "{}", error.message);
    }
}

#[test]
fn does_not_produce_errors_for_a_conditional_with_multiple_rows_and_hr_separators() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 {% if $foo %}\n\
                 * Row 1 Cell 1\n\
                 * Row 1 Cell 2\n\
                 ---\n\
                 * Row 2 Cell 1\n\
                 * Row 2 Cell 2\n\
                 {% /if %}\n\
                 {% /table %}";

    let document = parse_document(input);
    assert!(table_syntax_errors(&document).is_empty());
}

#[test]
fn does_not_produce_errors_for_comments_in_a_table() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 {% comment %}\n\
                 comment row\n\
                 {% /comment %}\n\
                 * Cell 1\n\
                 * Cell 2\n\
                 ---\n\
                 {% if $foo %}\n\
                 {% comment %}\n\
                 comment inside conditional\n\
                 {% /comment %}\n\
                 * Row Cell 1\n\
                 * Row Cell 2\n\
                 {% /if %}\n\
                 {% /table %}";

    let document = parse_document(input);
    assert!(table_syntax_errors(&document).is_empty());
}

#[test]
fn produces_an_error_for_invalid_tags_inside_a_table_conditional() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 {% if $foo %}\n\
                 {% callout %}\n\
                 This is not a valid row\n\
                 {% /callout %}\n\
                 {% /if %}\n\
                 {% /table %}";

    let document = parse_document(input);
    let errors = table_syntax_errors(&document);

    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].level, ErrorLevel::Critical);
    assert!(
        errors[0].message.contains("tag callout"),
        "{}",
        errors[0].message
    );
    // The error points at the callout tag, not at the `if` or the table.
    assert_eq!(start_line(errors[0]), Some(5));
}

#[test]
fn produces_an_error_for_non_conditional_tags_at_the_row_level_of_a_table() {
    let input = "{% table %}\n\
                 * Heading 1\n\
                 * Heading 2\n\
                 ---\n\
                 {% callout %}\n\
                 This is not a valid row\n\
                 {% /callout %}\n\
                 {% /table %}";

    let document = parse_document(input);
    let errors = table_syntax_errors(&document);

    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].level, ErrorLevel::Critical);
    assert!(errors[0].message.contains("tag"), "{}", errors[0].message);
    // The error points at the callout tag, not at the table.
    assert_eq!(start_line(errors[0]), Some(4));
}
