# Attribute spans: where each attribute was written

Status: implemented, in this pull request.
Target: 0.12.0. Additive -- nothing that compiled against 0.11.0 stops
compiling. The minor bump is the pre-1.0 convention for a new public type, not
a migration.

A tag's `location` says where the tag is. Nothing said where any one of its
attributes is, so a consumer that wants to change `type="note"` to
`type="warning"` has to find those bytes itself. This adds the positions the
parser already knows.

## Why the library and not the consumer

The consumer is Accent's admin inspector, which edits one attribute of the tag
under the caret and must leave the rest of the document untouched -- the
author's spacing, their quoting, and the tag body included.

Three ways to get there, and only one of them is honest:

1. **Re-scan the tag text in the consumer.** It has `Node::location`, so it has
   the tag's bytes. Finding the attribute inside them means re-implementing
   string literals, escapes, nested hashes and arrays -- the grammar, a second
   time, in a second language of edge cases. Two parsers for one syntax disagree
   eventually, and the disagreement shows up as a corrupted document.
2. **Reprint the whole tag through the formatter.** Correct, and it rewrites
   parts the author did not touch: `{% callout  type="note" %}` comes back
   canonically spaced. A tool that tidies what it was not asked to tidy is a
   tool people stop pointing at their content.
3. **Report the spans from the parser that already computed them.** The cursor
   knows the offset of every item it consumed; it was discarding the number.

This is 3. The owner chose it on 2026-09-16 over 1 and 2.

## What it adds

- `grammar::AttributeSpan { all, value }` -- byte ranges relative to the tag
  body, which is the only frame the grammar has.
- `grammar::parse_tag_spanned`, returning the item and one span per attribute.
  `parse_tag` is now this function with the spans dropped, so the two cannot
  disagree about what parses.
- `ast::AttributeLocation { all, value }` -- the same information in document
  coordinates, with line and column and the borrowed text, like every other
  location in the crate.
- `Node::annotation_locations`, parallel to `Node::annotations`.

`all` covers the item as written (`type="note"`, `#intro`, `.lead`). `value`
covers the value alone, and is the range a rewrite replaces.

## Decisions worth keeping

**The spans travel beside `Attribute`, not inside it.** `Attribute` mirrors
upstream's `{type, name, value}`; the command-line host serialises it in exactly
that shape, and `grammar/tests.rs` is a case-for-case port of upstream's
assertions about it. Fields added there would put byte ranges through a hundred
ported assertions and make the mirror something other than a mirror. Beside it,
the change is additive and the oracle is untouched.

**`value` is `None` for `#id` and `.class`.** Their value is implied by the
syntax rather than written: `#intro` carries the string `intro`, `.lead` carries
`true`. There is no range whose text is the value, and offering the identifier
would invite a rewrite that puts something in it the shortcut cannot spell.

**A `primary` value is spanned where it was written**, with `all` and `value`
the same range. Its name is synthetic -- there is no `primary=` in the source.

**`Node::annotation_locations` is a parallel list, not a map keyed by name.**
`.foo .bar` collapses into one `class` attribute, so a map cannot address either
one, and `a=1 a=2` would lose the loser. The list is either empty or exactly one
entry per annotation, which a debug assertion holds the parser to.

**A fence annotated through its info string gets no locations.** The tokenizer
reports that string's text but not where it sits, so there is no offset to
translate against. Reporting a guess would be worse than reporting nothing: a
consumer cannot tell a wrong offset from a right one. The two cases never mix on
one node -- a fence has no inline run, so it cannot also collect a positioned
annotation.

## Tests

`src/grammar/tests.rs`, module `attribute_spans`, covers the body-relative
spans: the item and value ranges, the shortcuts, a primary value, escapes inside
a string, containers, a multibyte value, a tag written across lines, repeated
names, and a tag with none. Its strongest case re-parses the text each span
covers and asserts it gives the same attribute back.

`tests/attribute_spans.rs` covers the document coordinates: line and column,
multibyte text earlier in the file, a tag indented inside a list, an inline
annotation landing on its block, the fence exception, the option switched off,
the parallel invariant across a whole document, and -- the case the feature
exists for -- replacing one value and finding every other byte unchanged.

Four mutations were run against them: dropping the document translation,
widening the value span to the whole item, ignoring the location option, and
mis-spanning a primary value. Each is caught.

## Not in this change

- **No spans inside a value.** An element of `items=[1, 2]` has no range of its
  own. Nothing needs one yet, and the shape to report them in is a decision
  better made by the first consumer that does.
- **No change to what parses**, to any error, or to any rendered byte. The
  conformance count is unmoved.
