//! The table rewrite, which turns a `{% table %}` tag into a real table.
//!
//! Ported from upstream `src/transforms/table.ts`. It is a *parse*-stage pass
//! despite living here, because that is where upstream runs it: `parser()`
//! finishes building the tree and then applies every transform in
//! `src/transforms/index.ts`, of which this is the only one.
//!
//! # What it rewrites
//!
//! Markdoc's advanced table syntax writes rows as lists and separates them with
//! thematic breaks:
//!
//! ```markdown
//! {% table %}
//! * Heading 1
//! * Heading 2
//! ---
//! * Cell 1
//! * Cell 2
//! {% /table %}
//! ```
//!
//! CommonMark sees that as a list, an `hr`, and another list. This pass
//! reinterprets it: the first list becomes the header row, each later list
//! becomes a body row, the list items become cells, and the whole thing is
//! wrapped in a synthetic `table > thead | tbody` that the built-in node
//! schemas already know how to render.
//!
//! # Why a conditional is special-cased
//!
//! A row can be wrapped in `{% if %}`, so the pass looks inside a conditional
//! and converts the lists it finds there too. Which tags may wrap rows is a
//! parameter rather than a hard-coded `if`, because upstream made it one: a
//! host with its own conditional registers it, and every *other* tag between
//! rows is rejected rather than silently kept, since a component wrapping
//! `<tr>` elements produces invalid HTML.
//!
//! # Errors, not failures
//!
//! Content that is neither a row nor a permitted wrapper raises `table-syntax`
//! on the node that contains it, carrying the *offending child's* location
//! rather than the table's. That is deliberate upstream and worth keeping: an
//! editor underlines the paragraph the author has to indent, not the tag thirty
//! lines above it.

use crate::ast::{ErrorLevel, Node, NodeType, ValidationError};

/// The tags allowed to wrap table rows when the caller names none.
///
/// Upstream's `ParserArgs.conditionalTags` documents `['if']` as its default,
/// and the built-in `if` tag is the only wrapper the language itself provides.
pub const DEFAULT_CONDITIONAL_TAGS: &[&str] = &["if"];

/// Rewrite every `{% table %}` tag in `document` into a `table` node.
///
/// `conditional_tags` names the tags that may wrap rows. Pass
/// [`DEFAULT_CONDITIONAL_TAGS`] for upstream's default.
///
/// The walk is iterative for the reason every walk in this crate is: nesting
/// depth is attacker-controlled, and a recursive descent turns a deep document
/// into a stack overflow, which aborts the process rather than raising anything
/// a caller could catch.
///
/// Order matches upstream's `document.walk()` -- slots before children, parents
/// before descendants -- because a table nested inside a cell is reached through
/// the rewritten tree, and the two orders disagree about which table is seen
/// first.
pub fn apply(document: &mut Node<'_>, conditional_tags: &[&str]) {
    let mut stack: Vec<&mut Node<'_>> = vec![document];
    while let Some(node) = stack.pop() {
        if node.node_type == NodeType::Tag && node.tag.as_deref() == Some("table") {
            rewrite(node, conditional_tags);
        }
        // Reversed, so popping yields upstream's order: slots first, then
        // children, each front to back.
        let mut group: Vec<&mut Node<'_>> = Vec::new();
        for slot in node.slots.values_mut() {
            group.push(slot);
        }
        for child in &mut node.children {
            group.push(child);
        }
        group.reverse();
        stack.append(&mut group);
    }
}

/// Rewrite one `{% table %}` tag whose children are lists and thematic breaks.
fn rewrite(node: &mut Node<'_>, conditional_tags: &[&str]) {
    // A tag with no children has nothing to reinterpret, and one that already
    // holds a `table` node came from GFM pipe syntax, which CommonMark parsed
    // into a table for us.
    match node.children.first() {
        None => return,
        Some(first) if first.node_type == NodeType::Table => return,
        Some(_) => {}
    }

    let mut children = std::mem::take(&mut node.children);
    let mut rest = children.split_off(1);
    // `split_off(1)` leaves exactly the first child behind, and the match above
    // proved there is one.
    let Some(first) = children.pop() else { return };

    let mut thead = Node::new(NodeType::Thead);
    let mut tbody = Node::new(NodeType::Tbody);

    // A leading `hr` -- an advanced table written with no header -- is dropped
    // rather than reported. Upstream drops it, and the corpus fixes the
    // behaviour: "Advanced table without header" expects an empty `thead`.
    if first.node_type == NodeType::List {
        thead.push(convert_to_row(first, NodeType::Th));
    }

    for row in rest.drain(..) {
        if let Some(row) = accept_row(node, row, conditional_tags) {
            tbody.push(row);
        }
    }

    // Upstream aliases the tag's attribute object onto the new table node
    // rather than moving it. Cloning is the closest Rust gets, and it keeps the
    // tag node intact for the formatter, which reprints what it was given.
    let table = Node::with(
        NodeType::Table,
        node.attributes.clone(),
        vec![thead, tbody],
        None,
    );
    node.children = vec![table];
}

/// Decide what one child of a `{% table %}` tag is, and return the row it
/// contributes.
///
/// [`None`] means the child contributes no row: a separator, a comment, or
/// content that was rejected -- in which case the error is already attached to
/// `table`.
fn accept_row<'a>(
    table: &mut Node<'a>,
    mut row: Node<'a>,
    conditional_tags: &[&str],
) -> Option<Node<'a>> {
    if row.node_type == NodeType::List {
        return Some(convert_to_row(row, NodeType::Td));
    }
    if is_conditional_tag(&row, conditional_tags) {
        rewrite_conditional_row(&mut row, conditional_tags);
        return Some(row);
    }
    if row.node_type != NodeType::Hr && !is_comment(&row) {
        table.errors.push(unexpected_node_error(&row));
    }
    None
}

/// Convert the lists inside a conditional into rows, and reject anything else.
///
/// The permitted contents are narrow on purpose: rows, `hr` separators between
/// them, comments, `{% else %}`, and further conditionals. A `hr` is dropped
/// here rather than kept, which is what lets one `{% if %}` hold several rows.
fn rewrite_conditional_row(row: &mut Node<'_>, conditional_tags: &[&str]) {
    let children = std::mem::take(&mut row.children);
    let mut kept = Vec::with_capacity(children.len());
    for child in children {
        if child.node_type == NodeType::Hr {
            continue;
        }
        if child.node_type == NodeType::List {
            kept.push(convert_to_row(child, NodeType::Td));
            continue;
        }
        let structural = is_comment(&child)
            || child.tag.as_deref() == Some("else")
            || is_conditional_tag(&child, conditional_tags);
        if structural {
            // A nested conditional keeps its own children unexamined, exactly as
            // upstream leaves them: the outer pass only guards the row level.
            kept.push(child);
        } else {
            // `child` is dropped here, which is upstream's `continue`: rejected
            // content does not reach the table.
            row.errors.push(unexpected_node_error(&child));
        }
    }
    row.children = kept;
}

/// Turn a list into a row and its items into cells.
///
/// The list's own attributes are discarded -- `ordered` and `marker` describe a
/// list, and the node is no longer one -- while each item keeps its own, so an
/// annotation such as `{% colspan=2 %}` written on an item survives onto the
/// cell.
fn convert_to_row(mut node: Node<'_>, cell: NodeType) -> Node<'_> {
    node.node_type = NodeType::Tr;
    node.attributes.clear();
    for child in &mut node.children {
        child.node_type = cell;
    }
    node
}

/// Whether a node is a tag the caller nominated as a row wrapper.
fn is_conditional_tag(node: &Node<'_>, conditional_tags: &[&str]) -> bool {
    node.node_type == NodeType::Tag
        && node
            .tag
            .as_deref()
            .is_some_and(|tag| conditional_tags.contains(&tag))
}

/// Whether a node is a comment, in either spelling.
///
/// `{% comment %}` is a Markdoc tag; `comment` is the node an HTML comment
/// becomes when comments are enabled. Both are invisible at the row level.
fn is_comment(node: &Node<'_>) -> bool {
    node.node_type == NodeType::Comment
        || (node.node_type == NodeType::Tag && node.tag.as_deref() == Some("comment"))
}

/// `table-syntax`: content appeared where a row was expected.
///
/// The message is upstream's, word for word, because the conformance corpus
/// compares it character by character. The location is the offending node's, not
/// the table's, so an editor can point at the content that has to be indented.
fn unexpected_node_error<'a>(node: &Node<'a>) -> ValidationError<'a> {
    let what = match &node.tag {
        Some(tag) => format!("{} {tag}", node.node_type),
        None => node.node_type.to_string(),
    };
    let error = ValidationError::new(
        "table-syntax",
        ErrorLevel::Critical,
        format!(
            "Found {what} where a list was expected. \
             Make sure all content inside table cells is indented."
        ),
    );
    match node.location {
        Some(location) => error.at(location),
        None => error,
    }
}
