# Divergences from upstream Markdoc

This file is **normative, not a changelog**. It is the complete list of places
where `proust` deliberately behaves differently from
[Markdoc](https://github.com/markdoc/markdoc) at `afee1a4` (v0.5.9), and it is
the only place a difference is allowed to be recorded.

Two rules govern it:

1. **Divergences are declared, never discovered.** A behaviour difference found
   while chasing a conformance failure is either a bug to fix or a new entry
   here, added in the same pull request that discovers it, with a sentence
   saying why emulating upstream was rejected. It is never left implicit.
2. **The error vocabulary never diverges.** Upstream validation error ids are
   kept identical, because that is the part external tooling binds to. Renaming
   an id is itself a divergence and needs an entry.

Conformance is to the **tag language**, not to markdown-it's CommonMark
minutiae. Corpus cases that fail because of a CommonMark difference are
annotated with the divergence they exercise and counted separately from
failures, so `N green, M annotated, P failing` distinguishes "we chose this"
from "we have not done this yet".

The file started at **eight entries**, on purpose. An empty divergence file
invites the belief that there are none. Entries 9 and 10 were added by the
tag-internals parser, declared in the pull request that ported it.

---

## 1. Fences do not process tags by default

**Upstream:** tags inside code fences are parsed; a fence opts out with
`{% process=false %}`.

**Here:** the default is inverted. Fence content is literal, and a fence opts
*in* with `{% process=true %}`.

**Why:** the primary consumer is a documentation site whose fences quote
Markdoc and Jinja examples constantly. Under the upstream default, every such
example is parsed as markup rather than shown as text, which makes the common
case the one requiring an annotation. Flipping the default costs an annotation
on a rare case instead of on almost every one.

**What it costs, exactly.** Most of the corpus's fence cases are unaffected,
because upstream's fence schema renders `attributes.content` when the node has
no children (`schema.ts:59-64`), which is what a literal fence produces. Three
cases do rely on the default and are annotated against this entry: "Conditional
and variable in code example with indentation", "Tag after a comment in a code
example", and "Multiple sequential tags in a code example". Each has a fence
carrying no `process` annotation and expects its content split into text and tag
children. Every other fence case in the corpus states `process` explicitly and
is reached either way.

## 2. The CommonMark engine is pulldown-cmark, not markdown-it

**Upstream:** markdown-it, whose block ruler, inline ruler and a core pass
Markdoc hooks directly.

**Here:** [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark), a
pull parser, reached through the `Tokenizer` seam rather than by hooking it.

**Why:** a Rust port of markdown-it exists but has had no commit in over two
years, and adopting it would convert "we maintain a Markdoc engine" into "we
also maintain a CommonMark implementation". The consequence is that CommonMark
edge behaviour differs wherever the two engines differ; entries 5-7 are the
specific cases where that difference is decided rather than incidental.

## 3. Transform hooks are synchronous

**Upstream:** `transform` accepts a `MaybePromise`, so a schema's `transform`
may be async.

**Here:** transform hooks are ordinary synchronous functions.

**Why:** the async variant exists upstream to let a schema fetch during
transform. This crate performs no I/O by construction, so an async hook would
be a signature with no reachable implementation, and it would colour every
caller above it.

## 4. The React renderers are unimplemented

**Upstream:** ships `renderers/react` (dynamic and static).

**Here:** the HTML renderer only. The renderable tree is public, so a React
renderer remains possible outside the crate.

**Why:** there is no Rust consumer of a React renderer. Porting one would be
untested surface area shipped for symmetry.

---

The next three are precedence rules. They exist because the host enables
CommonMark extensions that markdown-it's default set does not have, and three
of them overlap Markdoc features in ways that need a decision rather than a
merge. **The conformance corpus cannot arbitrate them**, because upstream's
cases never exercise syntax markdown-it lacks -- which is exactly why they are
declared here on day one instead of being discovered as failures later.

## 5. Heading attributes are CommonMark's, and Markdoc annotations are not ported for headings

**Overlap:** pulldown-cmark's `ENABLE_HEADING_ATTRIBUTES` gives
`# Title {#id .cls}`. Markdoc's annotation syntax expresses the same thing.

**Here:** the CommonMark spelling wins. Markdoc annotations are not ported for
headings.

**Why:** `# Title {#id}` already works and is already used, and the host's
heading-anchor stage depends on it. Porting the annotation syntax for headings
would ship two spellings of one thing, which is worse than either alone.

## 6. GFM alerts and a `callout` tag coexist; neither is rewritten into the other

**Overlap:** `> [!NOTE]` blockquote alerts cover roughly the ground a `callout`
component would.

**Here:** an alert stays Markdown and renders as Markdown. A component stays a
component. Neither is silently converted into the other.

**Why:** rewriting alerts into tags would make a Markdown construct's output
depend on which schemas happen to be registered. The two are different
authoring affordances with different escape hatches, and collapsing them
removes the plain-Markdown one.

## 7. Metadata blocks are stripped before the tag layer sees the document

**Overlap:** pulldown-cmark can emit `---` and `+++` frontmatter as events;
Markdoc has a frontmatter tokenizer plugin.

**Here:** frontmatter is the host's, parsed and removed upstream of this crate.
`proust` never sees it and has no frontmatter concept.

**Why:** frontmatter is document metadata, not document content, and the host
already parses it before rendering. Handing it to the tag layer as well would
give one construct two owners.

**What it costs, exactly.** One corpus case, named "Frontmatter", feeds a
document that still has its metadata block and expects the block to contribute
nothing to the output. Upstream's tokenizer plugin removes it; here the host
would have, and the corpus runner is not the host. The three `---`-delimited
lines therefore reach the parser as content and become a thematic break and a
paragraph. The case is annotated against this entry. The alternative -- having
the harness strip frontmatter so the case passes -- was rejected: it would make
the runner do something the crate does not, which is measurement shaped to fit.

---

## 8. The `allowIndentation` tokenizer option is not implemented

**Upstream:** `Tokenizer` accepts `allowIndentation: true`, which disables
CommonMark's four-space rule across nine block rules -- blockquote, code,
fence, heading, hr, html_block, lheading, list -- so that content nested inside
a tag may be indented without becoming a code block. The formatter has a
matching branch: with the option on, it indents nested tag children
(`formatter.ts:300`).

It works because upstream **patches markdown-it itself**
(`patches/markdown-it+12.3.2.patch`), adding the option to nine rule files.

**Here:** the option does not exist. `proust` always behaves as upstream does
with the option off, which is stock CommonMark, and the formatter never indents
nested tag children.

**Why:** the option is not reachable without patching the CommonMark parser,
and the host forbids that outright -- pulldown-cmark is tracked at zero
divergence from upstream, with fixes contributed upstream and the revision
bumped, never carried locally. Emulating the option would mean reimplementing
nine block rules above a parser that will not cooperate, in order to reproduce
a mode that stock Markdoc also does not enable by default.

**What it costs, exactly.** Upstream's *library* default is `allowIndentation:
false`, so this matches default Markdoc. But upstream's **conformance corpus
runs with the option on** (`spec/marktest/index.ts:21-24`, which constructs its
tokenizer with `allowIndentation: true, allowComments: true`). Six of the 105
cases exercise it, and they name themselves: "Indented paragraph in a tag"
(three of them), "Oddly indented paragraph in a tag", "Indented fence in a
tag", and "Advanced table with inner content". Those six are annotated against
this entry rather than counted as failures.

The formatter is unaffected in a way worth stating, because the opposite is
easy to assume: its indenting branch is gated on the same option, so with the
option absent it never emits indented nested children, and
`parse(format(ast))` round-trips under ordinary CommonMark rules. The option
being missing is what keeps the formatter self-consistent, not what threatens
it.

---

The next two come from the tag-internals parser. Both are consequences of the
target language rather than choices about Markdoc: one is a stack that cannot
be caught when it overflows, the other is a map that does not reorder its keys.

## 9. Nested values are depth-limited

**Upstream:** the PEG parser recurses without a bound. A value nested past the
JavaScript stack limit throws a `RangeError`, which is not a `SyntaxError`, so
the tokenizer's `catch` rethrows it and it escapes as an unhandled error.

**Here:** nesting is bounded at `grammar::MAX_VALUE_DEPTH` (64). Past it the
value fails to parse and the error says so.

**Why:** the same input in Rust would overflow the stack, and a stack overflow
aborts the process. It cannot be caught, so the crate's panic-freedom
promise -- "an open parser fed arbitrary text returns" -- is only true with a
bound. Emulation was rejected because there is nothing to emulate: upstream's
behaviour here is an uncaught host error, not a specification, and reproducing
"crash the process" is not a compatibility goal.

The bound is far above authored content. A value nested 64 deep is not a
document, and no corpus case exceeds three.

## 10. Function parameters keep authored order, not JavaScript object order

**Upstream:** parameters are one JavaScript object keyed by `name || index`, so
iteration follows JavaScript's property order, which hoists integer-like keys
ahead of named ones and sorts them ascending. `f(x=1, 2)` iterates as `2, 1`.

**Here:** parameters are an `IndexMap` in authored order. `f(x=1, 2)` iterates
as `1, 2`.

**Why:** hash order is exactly what this crate refuses to have -- rendered
output must be byte-reproducible, and "the map decides" is how that stops being
true. Emulating the hoist would mean reimplementing a JavaScript engine detail
in order to reorder something the author wrote in an order they chose.

**What it costs, exactly.** The only consumer of the order is the formatter,
which prints `Object.values(f.parameters)` and drops the names
(`formatter.ts:117-121`). The two orders therefore agree for every call that is
entirely positional (keys `0, 1, 2`, already ascending) and every call that is
entirely named (no integer keys, insertion order). They differ only when a
named parameter precedes a positional one in the same call, which reprints as
`f(1, 2)` here and `f(2, 1)` upstream. No corpus case does this.

## 11. Upstream's two disabled markdown-it rules are only half reachable

**Upstream:** its tokenizer disables two markdown-it block rules outright
(`src/tokenizer/index.ts`): `lheading`, so `Testing\n-------` is a paragraph
followed by a thematic break rather than a level-2 heading; and `code`, so four
spaces of indentation never produce a code block. This is separate from
`allowIndentation` (entry 8): these two are off in stock Markdoc, with no
option, while `allowIndentation` is an option upstream reaches by patching
markdown-it.

**Here:** the `lheading` disable **is** reproduced. The `code` disable is not:
an indented block becomes a CommonMark indented code block, where upstream
produces a paragraph.

**Why:** the two are not equally reachable. A setext heading is recoverable after
the fact from the node pulldown-cmark produces -- it is the only heading whose
span does not begin with `#`, and its underline is the last line of that span --
so undoing it is a local rewrite of one node, with the source right there to
read. An indented code block is not recoverable, because the difference is not
in the node: with `code` disabled, markdown-it's other block rules still refuse
indented input, so every one of them falls through to `paragraph`. Reproducing
that means knowing which rules declined and why, which means reimplementing
markdown-it's indent guards above a parser that does not expose them -- the
exact trade entry 2 exists to refuse. Emulating half of it and calling it done
would be worse than declaring it.

**What it costs, exactly.** Nothing the corpus currently charges to this entry:
the six cases that indent content inside a tag are already annotated against
entry 8, which subsumes them, and "Disabled setext heading" is reachable
because the `lheading` half *is* reproduced. A document that relies on four-space
indentation to write a paragraph -- rather than to nest one inside a tag --
renders as code here and as prose upstream.
