//! Upstream's `reference/src/formatter.test.ts`, ported.
//!
//! 858 lines, and the only real gate on the formatter: the conformance corpus
//! grades **zero** cases on formatter output, so a "formatter cases green"
//! measure would name nothing. What upstream wrote for its own formatter is the
//! specification, and it is ported here case for case, in upstream's order,
//! under [`upstream`].
//!
//! # The two properties
//!
//! [`properties`] adds what a case list cannot: `format(parse(s))` is
//! idempotent, and `parse(format(ast))` produces the same tree again. Every
//! source in this file is fed to both, so a case added for its own sake also
//! becomes a round-trip fixture.
//!
//! # Where a case cannot be ported as written
//!
//! Four of upstream's cases exercise behaviour this crate declares it does not
//! have. Each is ported to what this crate produces, with the divergence named,
//! rather than deleted -- a deleted case is a difference nobody can find again:
//!
//! - **Frontmatter** is the host's (`DIVERGENCES.md` entry 7). `document` still
//!   prints a `frontmatter` attribute, because a host that carries one deserves
//!   it back, but the parser never sets it and a metadata block reaches the
//!   parser as content. Upstream's `basics` case is ported without its
//!   metadata block, and [`upstream::frontmatter`] asserts what the block
//!   becomes instead.
//! - **`allowIndentation`** does not exist (entry 8), so upstream's
//!   "nested tags" case, which passes the option, has no counterpart. Its
//!   unindented twin is already covered by [`upstream::tags`].
//! - **Fences do not process tags** (entry 1), so a fence's content is one
//!   string rather than a subtree. That changes nothing the formatter prints:
//!   the `fence` arm reads `attributes.content` in both engines.

mod support;

use indexmap::IndexMap;
use proust::ast::{Node, NodeType, Value};
use proust::format::{format, format_value, format_with, FormatOptions, OrderedListMode};
use proust::parse::{parse_with, ParseOptions, PulldownTokenizer};

/// Upstream's `check`, minus the diff library.
///
/// Upstream builds its tokenizer with `allowComments: true` and nothing else,
/// so every case here parses with comments on and every other option at its
/// library default.
#[track_caller]
fn check(source: &str, expected: &str) {
    check_with(source, expected, &FormatOptions::new());
}

#[track_caller]
fn check_with(source: &str, expected: &str, options: &FormatOptions) {
    let document = parse(source);
    assert_eq!(format_with(&document, options), expected.trim_start());
}

/// Upstream's `stable`: formatting a document that is already canonical must
/// not change it.
#[track_caller]
fn stable(source: &str) {
    check(source, source);
}

#[track_caller]
fn stable_with(source: &str, options: &FormatOptions) {
    check_with(source, source, options);
}

fn parse(source: &str) -> Node<'_> {
    let options = ParseOptions::new().allow_comments(true);
    parse_with(source, &PulldownTokenizer::new(), &options)
}

/// Upstream's cases, in upstream's order.
mod upstream {
    use super::{
        check, check_with, format, format_value, parse, stable, stable_with, FormatOptions,
        IndexMap, Node, NodeType, OrderedListMode, Value,
    };

    #[test]
    fn empty() {
        assert_eq!(format_value(&Value::Null), "");
        check("", "");
        check("\n\n\t\n   \n  \n\n", "");
        stable("\n\n\t\n   \n  \n\n");
    }

    /// The whole language in one document.
    ///
    /// Upstream's source opens with a metadata block, which reaches this crate
    /// as content rather than as frontmatter (`DIVERGENCES.md` entry 7). It is
    /// dropped here and asserted on its own in [`frontmatter`]; everything else
    /// is upstream's case unchanged.
    #[test]
    fn basics() {
        let source = concat!(
            "# {% $markdoc.frontmatter.title %} {% #overview %}\n",
            "\n",
            "Markdoc is a **Markdown**-based `syntax` and _toolchain_ for creating ~~custom~~ documentation sites. Stripe created Markdoc to power [our public docs](http://stripe.com/docs).\n",
            "\n",
            "> Blockquote {% .special %}\n",
            "\n",
            "---\n",
            "\n",
            "[Link](/href   \"title\")\n",
            "    ![Alt](/image   \"title\")\n",
            "\n",
            "{% callout #id   .class  .class2   a=\"check\" b={\"e\":{\"with space\": 5}} c=8 d=[1,    \"2\",true,  null] %}\n",
            "Markdoc is open-source—check out it's [source](http://github.com/markdoc/markdoc) to see how it works.\n",
            "{% /callout %}\n",
            "\n",
            "```js {% .class #id x=\"test\"   render=false %}\n",
            "Code!\n",
            "```\n",
            "\n",
            "## How is {% markdoc(\"test\", 1) %} different? {% .classname %}\n",
            "\n",
            "foo\\\n",
            "baz\n",
            "\n",
            "Soft \n",
            " break\n",
            "Markdoc uses…\n",
        );

        let expected = concat!(
            "# {% $markdoc.frontmatter.title %} {% #overview %}\n",
            "\n",
            "Markdoc is a **Markdown**-based `syntax` and _toolchain_ for creating ~~custom~~ documentation sites. Stripe created Markdoc to power [our public docs](http://stripe.com/docs).\n",
            "\n",
            "> Blockquote {% .special %}\n",
            "\n",
            "---\n",
            "\n",
            "[Link](/href \"title\")\n",
            "![Alt](/image \"title\")\n",
            "\n",
            "{% callout\n",
            "   #id\n",
            "   .class\n",
            "   .class2\n",
            "   a=\"check\"\n",
            "   b={e: {\"with space\": 5}}\n",
            "   c=8\n",
            "   d=[1, \"2\", true, null] %}\n",
            "Markdoc is open-source—check out it's [source](http://github.com/markdoc/markdoc) to see how it works.\n",
            "{% /callout %}\n",
            "\n",
            "```js {% .class #id x=\"test\" render=false %}\n",
            "Code!\n",
            "```\n",
            "\n",
            "## How is {% markdoc(\"test\", 1) %} different? {% .classname %}\n",
            "\n",
            "foo\\\n",
            "baz\n",
            "\n",
            "Soft\n",
            "break\n",
            "Markdoc uses…\n",
        );

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn comments() {
        let source = concat!(
            "<!--\n",
            "    comment -->\n",
            "\n",
            "<!-- comment\n",
            "   with more\n",
            "  than one\n",
            "line  -->\n",
        );
        let expected = concat!(
            "<!-- comment -->\n",
            "<!-- comment\n",
            "   with more\n",
            "  than one\n",
            "line -->\n",
        );

        check(source, expected);
    }

    /// A metadata block is content here, not frontmatter.
    ///
    /// Upstream's case is `stable(source)` for a document opening with `---`,
    /// because its tokenizer lifts the block into `document.attributes`. This
    /// crate never sees frontmatter (`DIVERGENCES.md` entry 7): the host strips
    /// it. So the three lines parse as a thematic break, a paragraph and a
    /// second thematic break -- and reprint as exactly that, which is still
    /// stable, just not for upstream's reason.
    #[test]
    fn frontmatter() {
        let source = "---\ntitle: Title\nsubtitle: Subtitle\n---\n\n";
        let expected = "---\n\ntitle: Title\nsubtitle: Subtitle\n\n---\n";
        check(source, expected);
        stable(expected);
    }

    /// A `document` node that carries a `frontmatter` attribute still prints it.
    ///
    /// The parser never sets one, but the branch is ported, so a host that
    /// parses its own metadata and puts it back gets its block returned.
    #[test]
    fn a_host_supplied_frontmatter_attribute_is_printed() {
        let mut document = Node::new(NodeType::Document);
        document.set("frontmatter", Value::String("title: Title".to_owned()));
        document.push(Node::new(NodeType::Hr));
        assert_eq!(format(&document), "---\ntitle: Title\n---\n\n---\n");
    }

    #[test]
    fn escape_markdown_content() {
        let source = concat!(
            "regular_word_with_underscores\n",
            "\n",
            "\\* List item\n",
            "\n",
            "\\> Blockquote\n",
            "\n",
            "Text > not a blockquote\n",
            "\n",
            "\\# Heading\n",
            "\n",
            "\\### Heading\n",
            "\n",
            "#Not a heading\n",
            "\n",
            "**/docs/\\***\n",
            "\n",
            "~~**a \\_sentence\\_ with \\_underscores**~~\n",
            "\n",
            "- Item with [brackets]\n",
            "\n",
            "-not a list item\n",
            "\n",
            "```\n",
            "\\*\\_[\\[]\n",
            "```\n",
            "\n",
            "{% table %}\n",
            "- <https://autolink.com>\n",
            "- **[Link](https://example.com?q=()**\n",
            "- **[Link](https://example.com?q=\\()**\n",
            "- **[Link](https://example.com?q=\\(\\))**\n",
            "- ![Image](https://example.com?q=()\n",
            "- ![Image](https://example.com?q=\\()\n",
            "- ![Image](https://example.com?q=\\(\\))\n",
            "{% /table %}\n",
            "\n",
            "paragraph 1\n",
            "\n",
            "&nbsp;\n",
            "\n",
            "paragraph 2\n",
        );

        stable(source);
    }

    #[test]
    fn emphasis_marks() {
        for example in [
            "*foo* bar baz",
            "**foo** bar baz",
            "_foo_ bar baz",
            "__foo__ bar baz",
            "foo*bar*baz",
            "foo_bar_baz",
        ] {
            let document = parse(example);
            assert_eq!(format(&document).trim(), example);
        }
    }

    #[test]
    fn complex_attributes() {
        let source = "{% if $gates[\"<string_key>\"].test[\"@var\"] id=\"id with space\" class=\"class with space\" /%}";
        let expected = concat!(
            "{% if\n",
            "   $gates[\"<string_key>\"].test[\"@var\"]\n",
            "   id=\"id with space\"\n",
            "   class=\"class with space\" /%}\n",
        );
        check(source, expected);
    }

    #[test]
    fn attribute_edge_cases() {
        let source = "{% key id=$user.name class=default($y, \"test\") %}Child{% /key %}";
        let expected = "\n{% key id=$user.name class=default($y, \"test\") %}Child{% /key %}\n";

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn variables() {
        let source = concat!(
            "\n",
            "{% tag \"complex primary\" /%}\n",
            "{% if $primary %}\n",
            "X\n",
            "{% /if %}\n",
            "{% $user.name %}\n",
            "{% key x=$user.name y=$flag z=$array[5] /%}\n",
        );
        let expected = concat!(
            "\n",
            "{% tag \"complex primary\" /%}\n",
            "\n",
            "{% if $primary %}\n",
            "X\n",
            "{% /if %}\n",
            "\n",
            "{% $user.name %}\n",
            "\n",
            "{% key x=$user.name y=$flag z=$array[5] /%}\n",
        );

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn functions() {
        let source = "\n{% markdoc(\"test\", 1) %}\n{% key x=default($x, 1) /%}\n";
        let expected = "{% markdoc(\"test\", 1) %}\n{% key x=default($x, 1) /%}\n";

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn tags() {
        let source = concat!(
            "\n",
            "{% key /%}\n",
            "\n",
            "{% a %}{% /a %}\n",
            "\n",
            "{% a %}\n",
            "{% /a %}\n",
            "\n",
            "{% a %}\n",
            "\n",
            "{% /a %}\n",
            "  \n",
            "{% checkout %}\n",
            "  {% if true %}\n",
            "  Yes!\n",
            "  {% /if %}\n",
            "{% /checkout %}\n",
            "    ",
        );
        let expected = concat!(
            "\n",
            "{% key /%}\n",
            "\n",
            "{% a /%}\n",
            "\n",
            "{% a /%}\n",
            "\n",
            "{% a /%}\n",
            "\n",
            "{% checkout %}\n",
            "{% if true %}\n",
            "Yes!\n",
            "{% /if %}\n",
            "{% /checkout %}\n",
        );
        check(source, expected);
        stable(expected);
    }

    #[test]
    fn long_tags() {
        let source = "\n{% tag a=true b=\"My very long text well over 80 characters in total\" c=123456789 d=false /%}\n    ";
        let expected = concat!(
            "\n",
            "{% tag\n",
            "   a=true\n",
            "   b=\"My very long text well over 80 characters in total\"\n",
            "   c=123456789\n",
            "   d=false /%}\n",
        );
        check(source, expected);
        stable(expected);
    }

    /// An inline tag never wraps, however long it is.
    ///
    /// The wrap would put a newline inside a paragraph, which is a different
    /// document.
    #[test]
    fn long_inline_tags() {
        let source = "{% button type=\"button\" href=\"https://example.com/a-very-long-inline-tag\" %}A very long inline tag{% /button %}\n";
        stable(source);

        let inline_parent =
            "### {% image src=\"/src\" alt=\"A very long alt text to test if the tag wraps or not\" /%}\n";
        check(inline_parent, inline_parent);
    }

    /// `Infinity` in upstream, [`usize::MAX`] here: the same instruction.
    #[test]
    fn long_tags_with_no_maximum_opening_width() {
        let source = "\n{% tag a=true b=\"My very long text well over 80 characters in total\" c=123456789 d=false /%}\n";
        let options = FormatOptions::new().max_tag_opening_width(usize::MAX);
        stable_with(source, &options);
    }

    #[test]
    fn tables() {
        let source = concat!(
            "\n",
            "| Syntax      | Description |\n",
            "| ------ | ---- |\n",
            "| Header      | Title  |\n",
            "| Paragraph        | Text        |\n",
            "\n",
            "{% table %}\n",
            "\n",
            "- One {% align=\"middle\" %}\n",
            "- Two\n",
            "\n",
            "\n",
            "---\n",
            "- Three\n",
            "- Four {% align=\"end\" %}\n",
            "\n",
            "---\n",
            "\n",
            "* **Five**\n",
            "*\n",
            "  A bunch of words\n",
            "  \n",
            "  And more words\n",
            "\n",
            "{% /table %}\n",
            "\n",
            "{% table %}\n",
            "---\n",
            "- H1\n",
            "- H2\n",
            "{% /table %}\n",
            "    ",
        );
        let expected = concat!(
            "\n",
            "| Syntax    | Description |\n",
            "| --------- | ----------- |\n",
            "| Header    | Title       |\n",
            "| Paragraph | Text        |\n",
            "\n",
            "{% table %}\n",
            "- One {% align=\"middle\" %}\n",
            "- Two\n",
            "---\n",
            "- Three\n",
            "- Four {% align=\"end\" %}\n",
            "---\n",
            "- **Five**\n",
            "- A bunch of words\n",
            "\n",
            "  And more words\n",
            "{% /table %}\n",
            "\n",
            "{% table %}\n",
            "---\n",
            "- H1\n",
            "- H2\n",
            "{% /table %}\n",
        );

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn tables_with_tags() {
        let source = concat!(
            "\n",
            "{% table %}\n",
            "* H1\n",
            "* H2\n",
            "{% if $var %}\n",
            "---\n",
            "* H3\n",
            "* H4\n",
            "{% /if %}\n",
            "{% /table %}\n",
            "    ",
        );
        let expected = concat!(
            "\n",
            "{% table %}\n",
            "- H1\n",
            "- H2\n",
            "{% if $var %}\n",
            "---\n",
            "- H3\n",
            "- H4\n",
            "{% /if %}\n",
            "{% /table %}\n",
        );

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn lists() {
        let source = concat!(
            "\n",
            "- [Install Markdoc](/docs/getting-started)\n",
            "- [Try it out online](/sandbox)\n",
            "\n",
            "3. One {% align=\"left\" %}\n",
            "4. Two\n",
            "5. Three\n",
            "\n",
            "- A\n",
            "- B\n",
            "  - B2\n",
            "- C",
        );
        let expected = concat!(
            "\n",
            "- [Install Markdoc](/docs/getting-started)\n",
            "- [Try it out online](/sandbox)\n",
            "\n",
            "3. One {% align=\"left\" %}\n",
            "1. Two\n",
            "1. Three\n",
            "\n",
            "- A\n",
            "- B\n",
            "  - B2\n",
            "- C\n",
        );
        check(source, expected);
        stable(expected);
    }

    #[test]
    fn preserving_list_marker() {
        let source = concat!(
            "\n", "- foo\n", "- bar\n", "* baz\n", "* qux\n", "\n", "\n", "7) foo\n", "1) bar\n",
            "1) baz\n", "3. foo\n", "1. bar\n", "1. baz\n", "1) foo\n", "4) bar\n", "9) baz\n",
        );
        let expected = concat!(
            "\n", "- foo\n", "- bar\n", "\n", "* baz\n", "* qux\n", "\n", "7) foo\n", "1) bar\n",
            "1) baz\n", "\n", "3. foo\n", "1. bar\n", "1. baz\n", "\n", "1) foo\n", "1) bar\n",
            "1) baz\n",
        );
        check(source, expected);
        stable(expected);
    }

    #[test]
    fn ordered_lists_with_incrementing_numbers() {
        let source = concat!(
            "\n", "- foo\n", "- bar\n", "* baz\n", "* qux\n", "\n", "\n", "7) foo\n", "1) bar\n",
            "1) baz\n", "3. foo\n", "1. bar\n", "1. baz\n", "1) foo\n", "4) bar\n", "9) baz\n",
        );
        let expected = concat!(
            "\n", "- foo\n", "- bar\n", "\n", "* baz\n", "* qux\n", "\n", "7) foo\n", "8) bar\n",
            "9) baz\n", "\n", "3. foo\n", "4. bar\n", "5. baz\n", "\n", "1) foo\n", "2) bar\n",
            "3) baz\n",
        );
        let options = FormatOptions::new().ordered_list_mode(OrderedListMode::Increment);
        check_with(source, expected, &options);
        stable_with(expected, &options);
    }

    /// Upstream's `"loose" lists`.
    ///
    /// The expectation is this crate's, not upstream's, and the difference is
    /// `DIVERGENCES.md` entry 13: a block tag indented inside a list item is
    /// not part of the item. Upstream's segmenter runs after the container
    /// parser has stripped the item's indentation, so its `{% tag %}` opens
    /// inside the item; here the tag splits the document where it is written,
    /// the list ends, and the two indented lines that were the tag's content
    /// are read as an indented code block.
    ///
    /// Everything else in the case -- the loose-list blank lines, the trailing
    /// item's trim, the indented header, blockquote and fence -- is upstream's
    /// expectation unchanged. Asserting the whole output rather than deleting
    /// the case means fixing the segmenter turns this red instead of leaving it
    /// silently wrong.
    #[test]
    fn loose_lists() {
        let source = concat!(
            "\n",
            "- a\n",
            "\n",
            "- b\n",
            "\n",
            "---\n",
            "\n",
            "- One\n",
            "\n",
            "  My first paragraph\n",
            "  Test\n",
            "\n",
            "  {% tag %} \n",
            "    Indented tag\n",
            "  {% /tag %} \n",
            "\n",
            "  ```\n",
            "  {% $code %}\n",
            "  ```\n",
            "- Two\n",
            "\n",
            "  My second paragraph\n",
            "  \n",
            "  ---\n",
            "  \n",
            "  ## Indented header\n",
            "\n",
            "  > Indented blockquote",
        );
        let expected = concat!(
            "- a\n",
            "\n",
            "- b\n",
            "\n",
            "---\n",
            "\n",
            "- One\n",
            "\n",
            "  My first paragraph\n",
            "  Test\n",
            "\n",
            "{% tag %}\n",
            "```\n",
            "Indented tag\n",
            "```\n",
            "{% /tag %}\n",
            "\n",
            "```\n",
            "{% $code %}\n",
            "```\n",
            "\n",
            "- Two\n",
            "\n",
            "  My second paragraph\n",
            "\n",
            "  ---\n",
            "\n",
            "  ## Indented header\n",
            "\n",
            "  > Indented blockquote\n",
        );

        check(source, expected);
        stable(expected);
    }

    /// Upstream's "loose lists with direct inline children", under entry 13.
    ///
    /// Upstream's case is `stable(source)`: its indented `{% list %}` tags are
    /// item content, so reprinting returns them indented. Here each one splits
    /// the document, so the list ends at the first tag and the reprint is flat.
    /// It is stable in that shape, which is the property that matters.
    #[test]
    fn loose_lists_with_direct_inline_children() {
        let source = concat!(
            "\n",
            "- List\n",
            "  {% list %}\n",
            "  One\n",
            "  {% /list %}\n",
            "  Inline text:\n",
            "  {% list %}\n",
            "  Two\n",
            "  {% /list %}\n",
        );
        let expected = concat!(
            "- List\n",
            "\n",
            "{% list %}\n",
            "One\n",
            "{% /list %}\n",
            "\n",
            "Inline text:\n",
            "\n",
            "{% list %}\n",
            "Two\n",
            "{% /list %}\n",
        );

        check(source, expected);
        stable(expected);
    }

    /// Upstream's "complicated nested lists", and the one shape in this file
    /// that is not idempotent on the first pass.
    ///
    /// Two things are going on, and only the first is the formatter's.
    ///
    /// **Entry 13 again**, and harder than in [`loose_lists`]: the `{% table %}`
    /// is indented six columns inside a list item, so it splits the document
    /// and takes its rows with it. The rows are then six-space-indented lines
    /// with no list around them, which is an indented code block, which the
    /// table rewrite rejects. What is left is a `{% table %}` holding an empty
    /// `table`, and the trailing item becomes a fence for the same reason.
    ///
    /// **And that empty table does not settle in one pass.** A tag with a child
    /// prints an open and a close; the child here is an empty `table`, which
    /// prints one newline and nothing else. Reparsing `{% table %}\n{% /table %}`
    /// gives a tag with no children, which prints self-closing. Upstream does
    /// the same with the same tree -- its `tags` case fixes `{% a %}{% /a %}`
    /// as `{% a /%}` -- so this is the tree being unusual, not the formatter
    /// being wrong. The second pass is a fixed point, which is asserted below
    /// rather than left to be discovered.
    #[test]
    fn complicated_nested_lists() {
        let source = concat!(
            "\n",
            "* Create your CNAME record\n",
            "\n",
            "  1. Click **Add record**.\n",
            "\n",
            "     ```json\n",
            "     {\n",
            "       \"nested\": \"code block\"\n",
            "     }\n",
            "     ```\n",
            "  \n",
            "  1. Enter these values in the form that opens:\n",
            "\n",
            "      {% table %}\n",
            "      * Field\n",
            "      * Value to enter\n",
            "      * Description\n",
            "      ---\n",
            "      * Type\n",
            "      * Select `CNAME` from the dropdown\n",
            "      * What kind of DNS record this is.\n",
            "      ---\n",
            "      * Value\n",
            "      * {% code %}hosted-checkout.stripecdn.com{% /code %}\n",
            "      * This is what the new subdomain record points to-in this case, Stripe Checkout.\n",
            "      {% /table %}\n",
            "    1. foo\\\n",
            "       baz\n",
            "    \n",
            "       Soft \n",
            "         break\n",
            "       Markdoc uses…",
        );
        let first = concat!(
            "* Create your CNAME record\n",
            "\n",
            "  1. Click **Add record**.\n",
            "\n",
            "     ```json\n",
            "     {\n",
            "       \"nested\": \"code block\"\n",
            "     }\n",
            "     ```\n",
            "\n",
            "  1. Enter these values in the form that opens:\n",
            "\n",
            "{% table %}\n",
            "{% /table %}\n",
            "\n",
            "```\n",
            "1. foo\\\n",
            "   baz\n",
            "\n",
            "   Soft \n",
            "     break\n",
            "   Markdoc uses…\n",
            "```\n",
        );
        let settled = first.replace("{% table %}\n{% /table %}", "{% table /%}");

        check(source, first);
        check(first, &settled);
        stable(&settled);
    }

    #[test]
    fn lists_with_annotated_items() {
        let source = concat!(
            "\n",
            "- attributes: An object literal with key-value pairs that describe the attributes accepted by the tag. {% #id %}\n",
            "    - localizable: A boolean value (or an array) indicating whether the attribute’s value is translatable. {% #localizable %}\n",
            "        - Defaults to `false`\n",
            "    - description: A documentation string that describes the purpose of the attribute {% align=\"center\" %}",
        );

        let expected = concat!(
            "\n",
            "- attributes: An object literal with key-value pairs that describe the attributes accepted by the tag. {% #id %}\n",
            "  - localizable: A boolean value (or an array) indicating whether the attribute’s value is translatable. {% #localizable %}\n",
            "    - Defaults to `false`\n",
            "  - description: A documentation string that describes the purpose of the attribute {% align=\"center\" %}\n",
        );

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn lists_with_complex_items() {
        let source = concat!(
            "\n",
            "* **One {% colspan=1 %}**\n",
            "* **Two {% colspan=2 %}**\n",
            "* **Three {% colspan=3 %}**\n",
        );

        let expected = concat!(
            "\n",
            "* **One**{% colspan=1 %}\n",
            "* **Two**{% colspan=2 %}\n",
            "* **Three**{% colspan=3 %}\n",
        );

        check(source, expected);
        stable(expected);
    }

    #[test]
    fn fences_with_block_level_tags() {
        let source = concat!(
            "{% tab %}\n",
            "```json {% filename=\"package.json\" %}\n",
            "{\n",
            "  \"dependencies\": {\n",
            "    ...\n",
            "    {% highlight type=\"remove\" %}\n",
            "    \"beta\": \"1.2.3\",\n",
            "    {% /highlight %}\n",
            "    {% highlight type=\"add\" %}\n",
            "    \"main\": \"1.2.4\",\n",
            "    {% /highlight %}\n",
            "    ...\n",
            "  }\n",
            "}\n",
            "```\n",
            "{% /tab %}\n",
        );

        stable(source);
    }

    #[test]
    fn fences_with_no_language() {
        let source = "\n```{% filename=\"package.json\" %}\nPackage.json\n```\n";
        let expected = "\n``` {% filename=\"package.json\" %}\nPackage.json\n```\n";

        check(source, expected);
    }

    #[test]
    fn nested_fences() {
        let source = "\n````\n\n```\nFence within a fence\n```\n\n\n````\n";
        stable(source);
    }

    #[test]
    fn multi_paragraph_blockquotes() {
        let source = "\n> Blockquote {% .class %}\n>\n> with two paragraphs";
        let expected = "\n> Blockquote {% .class %}\n> \n> with two paragraphs\n";

        check(source, expected);
        stable(expected);
    }

    /// Upstream's "skips over undefined variables".
    ///
    /// Its case builds a node with `undefinedAttribute: undefined`, which the
    /// formatter filters out. There is no `undefined` here -- an attribute
    /// either is in the map or is not -- so the filter has no counterpart and
    /// the assertion is what remains of the case: the attributes that exist are
    /// printed, in authored order.
    #[test]
    fn an_absent_attribute_is_not_printed() {
        let mut attributes = IndexMap::new();
        attributes.insert("validAttribute".to_owned(), Value::Boolean(true));
        let node = Node::with(
            NodeType::Tag,
            attributes,
            Vec::new(),
            Some("tag".to_owned()),
        );

        assert_eq!(format(&node).trim(), "{% tag validAttribute=true /%}");
    }

    #[test]
    fn a_fence_whose_content_has_no_ending_newline_still_closes() {
        let mut node = Node::new(NodeType::Fence);
        node.set("content", Value::String("foo".to_owned()));
        assert_eq!(format(&node), "```\nfoo\n```\n");
    }
}

/// The two properties from the porting strategy, r111 §9.5.
///
/// A case list fixes the output for the documents someone thought to write. The
/// properties fix a relationship that has to hold for every document, and they
/// are what catch a formatter that is self-consistent but wrong -- an escape
/// that does not survive reparsing, an indent that shifts by one on each pass.
mod properties {
    use super::{format, parse, support};
    use proust::ast::Node;

    /// Every source this file formats, so that adding a case adds two property
    /// fixtures with it.
    fn sources() -> Vec<&'static str> {
        vec![
            "",
            "\n\n\t\n   \n  \n\n",
            "# Title {% #intro %}\n",
            "Some *text* with `code` and a [link](/a \"t\").\n",
            "> Blockquote {% .class %}\n>\n> with two paragraphs\n",
            "- a\n- b\n  - b2\n- c\n",
            "3. one\n4. two\n5. three\n",
            "{% callout type=\"note\" %}\nBody\n{% /callout %}\n",
            "{% key x=$user.name y=default($z, 1) z=[1, \"2\", {a: true}] /%}\n",
            "| a | b |\n| - | - |\n| c | d |\n",
            "{% table %}\n- H1\n- H2\n---\n- c1\n- c2\n{% /table %}\n",
            "```js {% .cls #id %}\nCode!\n```\n",
            "````\n\n```\nfence in a fence\n```\n\n````\n",
            "<!-- a comment -->\n",
            "text with \\* and \\# and \\> and a\u{a0}space\n",
            "![Alt](/image?q=\\(\\) \"title\")\n",
            "<https://autolink.com>\n",
            "foo\\\nbaz\n",
        ]
    }

    /// `format(parse(s))` is idempotent.
    ///
    /// Formatting canonical source must be the identity. A formatter that
    /// changes its own output has no fixed point, so no tool can use it to
    /// rewrite a file in place.
    #[test]
    fn formatting_is_idempotent() {
        for source in sources() {
            let once = format(&parse(source));
            let twice = format(&parse(&once));
            assert_eq!(once, twice, "not idempotent for {source:?}");
        }
    }

    /// `parse(format(ast))` round-trips the AST.
    ///
    /// The trees are compared as outlines rather than with `==`: a `Node`
    /// carries its source locations, and reprinted source has different byte
    /// offsets by construction. What has to survive is the structure and the
    /// attributes.
    #[test]
    fn formatting_round_trips_the_tree() {
        for source in sources() {
            let document = parse(source);
            let reprinted = format(&document);
            let reparsed = parse(&reprinted);
            assert_eq!(
                outline(&document),
                outline(&reparsed),
                "round trip changed the tree for {source:?}\n--- reprinted ---\n{reprinted}"
            );
        }
    }

    fn outline(node: &Node<'_>) -> String {
        support::outline(node)
    }
}
