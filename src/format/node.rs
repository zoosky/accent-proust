//! The node switch: upstream's `formatNode`, one arm per node type.
//!
//! Read it beside `reference/src/formatter.ts`. The arms yield the same chunks
//! in the same sequence, because the `{% table %}` arm reads chunk boundaries
//! and merging two yields merges two lines.
//!
//! # Why every arm is its own function
//!
//! Upstream is one `switch`. Here each arm is a method, and not for length:
//! this walk recurses once per level of document nesting, and a single function
//! holding every arm's locals carries all of them in every frame. Measured in a
//! debug build, the one-function form cost about 6 KB per level and overflowed
//! a 2 MiB thread stack around 350 levels deep -- below the bound that is
//! supposed to be what stops it. Splitting the arms puts only the live one on
//! the stack.
//!
//! # The shapes that are not just "print the children"
//!
//! - **`blockquote` and `list` re-enter at the top.** They call the whole
//!   formatter on each child and paste the result behind a prefix, rather than
//!   streaming the child's chunks into their own. That is what lets them
//!   left-trim a child independently of its siblings.
//! - **`table` reads chunks, not text** (`super::table`).
//! - **`text` escapes differently depending on its parent** (`super::inline`).

use super::{
    CLOSE, Ctx, Formatter, MAX_FORMAT_DEPTH, MAX_HEADING_LEVEL, NL, OL, OPEN, OrderedListMode, Out,
    SPACE, UL, WRAPPING_TYPES,
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

    /// Upstream's `switch (n.type)`, and nothing else: every arm's own locals
    /// live in the arm.
    fn node_inner(&mut self, node: &Node<'_>, ctx: Ctx, out: &mut Out) {
        // Upstream's `no`: the same options, with this node recorded as the
        // parent its children will see.
        let no = child_ctx(node, ctx);

        match node.node_type {
            NodeType::Document => self.document(node, no, out),
            NodeType::Heading => self.heading(node, ctx, no, out),
            NodeType::Paragraph => self.paragraph(node, no, out),
            NodeType::Inline => {
                out.text(indent(ctx));
                self.children(node, no, out);
            }
            NodeType::Image => self.image(node, no, out),
            NodeType::Link => self.link(node, no, out),
            NodeType::Text => self.text(node, ctx, no, out),
            NodeType::Blockquote => self.blockquote(node, ctx, no, out),
            NodeType::Hr => {
                out.text(NL);
                out.text(indent(ctx));
                out.text("---");
                out.text(NL);
            }
            NodeType::Fence => self.fence(node, ctx, out),
            NodeType::Tag => self.tag(node, ctx, no, out),
            NodeType::List => self.list(node, ctx, no, out),
            NodeType::Item => self.item(node, no, out),
            NodeType::Strong => self.wrapped(node, no, "**", out),
            NodeType::Em => self.wrapped(node, no, "*", out),
            NodeType::S => {
                // `s` has no `marker` attribute upstream: there is one spelling.
                out.text("~~");
                out.text(self.inline_text(node, no));
                out.text("~~");
            }
            NodeType::Code => self.code(node, no, out),
            NodeType::Hardbreak => {
                out.text(format!("\\{NL}"));
                out.text(indent(ctx));
            }
            NodeType::Softbreak => {
                out.text(NL);
                out.text(indent(ctx));
            }
            NodeType::Table => self.table(node, ctx, no, out),
            NodeType::Thead => self.thead(node, no, out),
            NodeType::Tr => self.tr(node, no, out),
            NodeType::Td | NodeType::Th => self.cell(node, no, out),
            NodeType::Tbody => self.children(node, no, out),
            NodeType::Comment => self.comment(node, out),
            // An `error` node has no source to reprint -- its tag did not parse
            // -- and a bare `node` is the default a host constructs. Upstream
            // prints nothing for either.
            NodeType::Error | NodeType::Node => {}
        }
    }

    /// Every child, in order.
    pub(super) fn children(&mut self, node: &Node<'_>, ctx: Ctx, out: &mut Out) {
        for child in &node.children {
            self.node(child, ctx, out);
        }
    }

    /// Every child, into a stream of its own.
    pub(super) fn collect_children(&mut self, node: &Node<'_>, ctx: Ctx) -> Out {
        let mut out = Out::default();
        self.children(node, ctx, &mut out);
        out
    }

    /// Upstream's `formatInline`: the children, joined and trimmed, as one
    /// chunk.
    pub(super) fn inline_text(&mut self, node: &Node<'_>, ctx: Ctx) -> String {
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
    pub(super) fn text_of(&mut self, value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => self.scalar(other),
        }
    }

    fn document(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        // The parser never sets `frontmatter`: metadata blocks belong to the
        // host (`DIVERGENCES.md` entry 7). The branch is ported anyway, so that
        // a host carrying its own frontmatter in that attribute gets it back.
        if let Some(Value::String(frontmatter)) = node.get("frontmatter")
            && !frontmatter.is_empty()
        {
            out.text(format!("---{NL}{frontmatter}{NL}---{NL}{NL}"));
        }
        let children = self.collect_children(node, no).trim_start();
        out.append(children);
    }

    fn heading(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, out: &mut Out) {
        out.text(NL);
        out.text(indent(ctx));
        out.text("#".repeat(heading_level(node)));
        out.text(SPACE);
        let children = self.collect_children(node, no).trim_start();
        out.append(children);
        self.annotations(node, out);
        out.text(NL);
    }

    fn paragraph(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        out.text(NL);
        self.children(node, no, out);
        self.annotations(node, out);
        out.text(NL);
    }

    /// Upstream's `blockquote` arm, with one fix: the prefix goes on every line.
    ///
    /// Upstream writes `NL + indent + prefix + d`, one `> ` per child. That is
    /// right only while every child prints on one line, which is all its own
    /// test covers. Give it `> a\n> b` -- one paragraph, one soft break -- and
    /// the second line comes back without its `> `, outside the quote. See
    /// `DIVERGENCES.md` entry 16.
    fn blockquote(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, out: &mut Out) {
        let prefix = format!("{}>{SPACE}", indent(ctx));
        let parts: Vec<String> = node
            .children
            .iter()
            .map(|child| {
                let formatted = self.subtree(child, no);
                format!("{NL}{}", quote(&formatted, &prefix))
            })
            .collect();
        // The separator carries no newline, so it lands directly after the
        // previous child's trailing one. That is what produces the bare `> `
        // line between two quoted paragraphs.
        out.text(parts.join(&prefix));
    }

    fn fence(&mut self, node: &Node<'_>, ctx: Ctx, out: &mut Out) {
        let indent = indent(ctx);
        let content = match node.get("content") {
            Some(value) => self.text_of(value),
            None => String::new(),
        };

        out.text(NL);
        out.text(indent.clone());

        // The boundary has to be longer than any run of backticks the content
        // holds, or the fence closes inside itself.
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
            // Upstream yields the closing boundary with no indent. It looks
            // right in its own tests because content that *ends* with a newline
            // leaves a trailing empty segment, and the join above indents that.
            // Content without one -- an empty fence, or a last line with no
            // terminator -- closes at column zero instead, which re-parses as a
            // fence that never closed. See `DIVERGENCES.md` entry 16.
            out.text(indent.clone());
        }
        out.text(boundary);
        out.text(NL);
    }

    fn tag(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, out: &mut Out) {
        let indent = indent(ctx);
        if !node.inline {
            out.text(NL);
            out.text(indent.clone());
        }

        let open = format!("{OPEN}{SPACE}");
        let name = node.tag.clone().unwrap_or_default();
        let mut parts = vec![format!("{open}{name}")];
        parts.extend(self.attributes(node));

        let inline_tag = parts.join(SPACE);
        // Upstream counts the opening delimiter twice: once for the `{% `
        // already in `inline_tag`, and once as the allowance for the ` %}` that
        // will follow it.
        let width = super::utf16_len(&inline_tag).saturating_add(super::utf16_len(&open) * 2);
        let is_long = width > self.options.max_tag_opening_width;

        let opening = if !node.inline && is_long {
            parts.join(&format!(
                "{NL}{}{indent}",
                SPACE.repeat(super::utf16_len(&open))
            ))
        } else {
            inline_tag
        };
        // Upstream indents nested children when `allowIndentation` is on. The
        // option does not exist here (`DIVERGENCES.md` entry 8), so this is the
        // `false` branch and only that branch.
        //
        // The children are printed *before* the opening is decided, because
        // whether the tag self-closes depends on what they came to. Upstream
        // asks only whether the child list is empty, so a tag holding children
        // that print nothing -- an empty `table`, an `error` node -- is written
        // as an open and a close with whitespace between, which re-parses as a
        // tag with no children at all. See `DIVERGENCES.md` entry 16.
        let body = if node.children.is_empty() {
            None
        } else {
            let body = self.collect_children(node, no);
            if body.joined().trim().is_empty() {
                None
            } else {
                Some(body)
            }
        };

        let closer = if body.is_none() { "/" } else { "" };
        out.text(format!("{opening}{SPACE}{closer}{CLOSE}"));

        if let Some(body) = body {
            out.append(body);
            if !node.inline {
                out.text(indent);
            }
            out.text(format!("{OPEN}{SPACE}/{name}{SPACE}{CLOSE}"));
        }
        if !node.inline {
            out.text(NL);
        }
    }

    fn item(&mut self, node: &Node<'_>, no: Ctx, out: &mut Out) {
        for (index, child) in node.children.iter().enumerate() {
            self.node(child, no, out);
            // The item's own annotation follows its first child, which is where
            // the author wrote it: at the end of the first line, not after a
            // nested list.
            if index == 0 {
                self.annotations(node, out);
            }
        }
    }

    /// Upstream's `comment` arm, with one fix: an inline comment ends no line.
    ///
    /// Upstream appends a newline to every comment. That is right for a comment
    /// on a line of its own and wrong for one inside a sentence, where it
    /// splits the paragraph -- upstream's own tokenizer has an inline comment
    /// rule, so upstream can produce the node it then misprints. See
    /// `DIVERGENCES.md` entry 17.
    fn comment(&mut self, node: &Node<'_>, out: &mut Out) {
        let content = match node.get("content") {
            Some(value) => self.text_of(value),
            None => String::new(),
        };
        let trailing = if node.inline { "" } else { NL };
        out.text(format!("<!-- {content} -->{trailing}"));
    }

    /// Upstream's `list` arm.
    ///
    /// `ctx` is the list's own context and `no` the one its items see; both are
    /// needed, because an item's indentation is measured from the prefix this
    /// loop builds.
    fn list(&mut self, node: &Node<'_>, ctx: Ctx, no: Ctx, out: &mut Out) {
        let indent = indent(ctx);
        // A list is loose when any item holds a paragraph, which is
        // markdown-it's rule and CommonMark's. Only the last item of a tight
        // list keeps its trailing blank line, so that the list ends where the
        // author ended it.
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

            let width = super::utf16_len(&prefix).saturating_add(1);
            let mut item = self.subtree(item, no.increment(width));
            if !is_loose || index == last {
                item = item.trim().to_owned();
            }
            out.text(format!("{NL}{indent}{prefix} {item}"));
        }
        out.text(NL);
    }
}

/// Every line of `text`, behind `prefix`.
///
/// An empty subtree still gets a marker, so a blockquote holding a node that
/// prints nothing is still a blockquote.
fn quote(text: &str, prefix: &str) -> String {
    if text.is_empty() {
        return prefix.to_owned();
    }
    let mut out = String::with_capacity(text.len() + prefix.len());
    for line in text.split_inclusive('\n') {
        out.push_str(prefix);
        out.push_str(line);
    }
    out
}

/// The leading whitespace for a node at this context.
fn indent(ctx: Ctx) -> String {
    SPACE.repeat(ctx.indent)
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
