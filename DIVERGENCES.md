# Divergences from upstream Markdoc

This file is **normative, not a changelog**. It is the complete list of places
where `accent-proust` deliberately behaves differently from
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
invites the belief that there are none. Each entry since was declared in the
pull request that ported the stage which found it: 9 and 10 by the tag-internals
parser, 11 by the segmenter, 12 by the validator, 13 and 14 by the transformer.

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
children.

A fourth, "Using a backtick in a fenced code block string attribute", is charged
here for a subtler reason worth spelling out. It replaces the `fence` schema
with one of its own, which has no `transform` hook -- and a replacement is total
upstream, hook included. Its fence therefore takes the generic path and renders
its *children*, which upstream has because it processed the fence and this crate
does not: here the text is the unrendered `content` attribute that the built-in
hook exists to put back. So the case fails on the `process` default even though
nothing in it mentions `process`.

Every other fence case in the corpus states `process` explicitly and is reached
either way.

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

## 3. Schema hooks are synchronous

**Upstream:** every schema hook returns a `MaybePromise`. `Schema.transform`
and `Schema.validate` may both be async, and `validate` and `transform` at the
top level then return a promise in turn.

**Here:** all of them are ordinary synchronous functions, and so are the
function hooks on `ConfigFunction`.

**Why:** the async variants exist upstream to let a schema fetch while it runs.
This crate performs no I/O by construction, so an async hook would be a
signature with no reachable implementation, and it would colour every caller
above it -- `validate_tree` would have to be async because a schema *might*
be.

**What it costs, exactly.** One test in upstream's `validator.test.ts`, "should
allow async validators", which asserts that an `async validate()` on a node
schema is awaited. It is not ported, because the thing it tests is the thing
this entry declines. Nothing in the conformance corpus is async.

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
`accent-proust` never sees it and has no frontmatter concept.

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

**Here:** the option does not exist. `accent-proust` always behaves as upstream does
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
cases were annotated against this entry when it was written, before there was a
transform stage to grade them through. Four still are: "Indented paragraph in a
tag", which the corpus uses three times, and "Indented fence in a tag".

The other two -- "Oddly indented paragraph in a tag" and "Advanced table with
inner content" -- turned out to be reachable, and the annotations were removed
when the transformer made that visible. Neither actually needs the option:
their indentation is inside a list item or a tag body, where stock CommonMark
already reads it as content rather than as a code block. They were annotated on
the reasonable prediction that anything indented inside a tag would need
`allowIndentation`, and the prediction was too wide. The harness is what caught
it -- an annotated case is still run, and a passing one is reported as a
divergence that has stopped being true.

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

## 10. Maps keep authored order, not JavaScript object order

**Upstream:** every ordered map in Markdoc is a JavaScript object, so iterating
one follows JavaScript's property order, which hoists integer-like keys ahead of
named ones and sorts them ascending. Function parameters are keyed by
`name || index`, so `f(x=1, 2)` iterates as `2, 1`.

**Here:** every such map is an `IndexMap` in authored order. `f(x=1, 2)`
iterates as `1, 2`.

**Why:** hash order is exactly what this crate refuses to have -- rendered
output must be byte-reproducible, and "the map decides" is how that stops being
true. Emulating the hoist would mean reimplementing a JavaScript engine detail
in order to reorder something the author wrote in an order they chose.

**What it costs, exactly.** Two consumers read an order.

The **formatter** prints `Object.values(f.parameters)` and drops the names
(`formatter.ts:117-121`). The two orders therefore agree for every call that is
entirely positional (keys `0, 1, 2`, already ascending) and every call that is
entirely named (no integer keys, insertion order). They differ only when a
named parameter precedes a positional one in the same call, which reprints as
`f(1, 2)` here and `f(2, 1)` upstream. No corpus case does this.

The **HTML renderer** iterates a tag's attributes in map order
(`renderers/html.ts:38`), so the same hoist would reorder attributes in rendered
markup. The grammar admits `[a-zA-Z0-9_-]+` as an attribute name, which includes
`1`, so `{% x bar="b" 1="a" %}` renders as `<x bar="b" 1="a">` here and
`<x 1="a" bar="b">` upstream. Neither is valid HTML and no corpus case writes
one; it is recorded because the alternative is discovering it.

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
the cases that indent content inside a tag are already annotated against
entry 8, which subsumes them, and "Disabled setext heading" is reachable
because the `lheading` half *is* reproduced. A document that relies on four-space
indentation to write a paragraph -- rather than to nest one inside a tag --
renders as code here and as prose upstream.

---

The last three come from the stages above the parser. One is a field of the
schema shape whose JavaScript type this crate has no equivalent of and declines
to acquire; the other two the transformer found, which is the first stage
positioned to see either.

## 12. `matches` takes a host-supplied pattern, not a regular expression

**Upstream:** `SchemaAttribute.matches` is `RegExp | string[] | null`. A schema
writes `matches: /^[a-z-]+$/` and the validator calls `matches.test(value)`,
reporting `Attribute 'x' must match /^[a-z-]+$/. Got 'Y' instead.` -- the
pattern's own source, interpolated into the message.

**Here:** the string-list and null forms are ported unchanged. The regular
expression form becomes `SchemaMatches::Pattern`, which holds a host
implementation of the `MatchPattern` trait: a predicate over the coerced value,
plus the spelling the message quotes. A host that wants Markdoc's exact
behaviour supplies a matcher over its own regular expression engine and spells
`display()` as `/source/flags`.

**Why:** the alternative is a regular expression dependency in a leaf parsing
library, for one optional field of one schema type. It would also not buy
fidelity: JavaScript's regular expression dialect is not Rust's, so
`matches: /(?<=a)b/` would either fail to compile or mean something else, and a
schema ported by copying the literal across would be quietly wrong rather than
loudly unsupported. Making the host bring its own engine keeps the choice of
dialect where the schema is written.

**What it costs, exactly.** Nothing the corpus or upstream's unit tests
measure: no case in `spec/marktest/tests.yaml` sets `matches` at all, no
built-in Markdoc schema uses the regular expression form, and `validator.test.ts`
exercises only the string-list form. The error id and message shape are
unchanged, so tooling reading `attribute-value-invalid` sees what it saw
before. What is given up is the ability to paste a Markdoc schema's regular
expression literal into a Rust schema and have it work without a matcher around
it.
## 13. A block tag indented inside a list item is not part of the item

**Upstream:** the Markdoc block-tag rule is a markdown-it *block rule*,
registered after `list`, `heading` and `blockquote`. By the time it runs, the
container parser has already stripped a list item's indentation, so a tag
written two spaces in under `* Some content` opens inside that item:

```markdown
{% table %}
* Cell 1
* Some content

  {% if $foo %}
  Conditional block
  {% /if %}
{% /table %}
```

Upstream reads the `{% if %}` as content of the second cell. The conditional's
paragraphs render inside the `<td>`.

**Here:** the segmenter resolves tag syntax *before* the CommonMark parse (the
one redesign, `src/parse/segment.rs`), so it has no containers to consult. A
line whose only content is a block tag splits the document wherever it appears,
indented or not, and the list ends there. The tag becomes a sibling of the list
rather than a child of its last item.

**Why:** knowing that a line sits inside a list item means having parsed the
list, and the segmenter runs first by construction -- that ordering is what
reproduces markdown-it's "the tag rule consumed the tag before the emphasis rule
saw it" without a ruler to hook (entry 2). Recovering the container structure
means implementing CommonMark's container phase in the segmenter, ahead of the
CommonMark parser it feeds, and then keeping the two in agreement forever. That
is a second Markdown implementation to maintain, and the failure mode when they
drift is silent: a document that segments one way and parses another. Guessing
from indentation alone is worse than either -- four spaces inside a list item is
item content, four spaces outside one is a code block, and the segmenter cannot
tell which it is looking at.

**What it costs, exactly.** One corpus case, "Advanced table with conditional
inside cell", which is annotated against this entry. Its inline conditional
(`* {% if $foo %}...{% /if %}`) is unaffected -- inline tags are masked, not
split -- and only the indented block form diverges. Upstream's
`transforms/table.test.ts` has the same shape in "does not produce errors for
valid conditionals within a cell"; the ported test asserts what this crate
produces and names this entry, so fixing the segmenter turns that test red
rather than leaving it silently wrong.

## 14. Transform recursion is depth-limited

**Upstream:** `transformer.node` recurses into `transformer.children`, which
recurses back into `transformer.node`, with no bound. A document nested deeply
enough exhausts V8's stack and throws a `RangeError` a caller can catch.

**Here:** the same recursion stops at `MAX_TRANSFORM_DEPTH` -- 512 levels. A node
below that depth transforms to nothing, and its ancestors render normally.

**Why:** the same reason as entry 9, one stage further up. Nesting depth is
attacker-controlled -- `{% a %}` repeated is one level per line -- and the Rust
equivalent of V8's `RangeError` is a stack overflow, which aborts the process
and cannot be caught. A crate that promises panic-freedom over arbitrary input
needs a bound rather than a hope.

The recursion cannot simply be made iterative, which would have been the better
answer. A schema `transform` hook receives a node and calls back into
`transform::children` for its content, so unrolling the walk onto an explicit
stack would mean giving every hook a continuation -- changing the signature a
host writes against, and changing it for a case no real document reaches. The
bound is counted in a thread-local rather than passed as an argument for the
same reason: a hook that forgot to thread it would silently disable the guard.

**What it costs, exactly.** Nothing the corpus contains, and nothing a person
writes: 512 levels of nesting is far past where HTML stops meaning anything.
It is reachable by a generated or hostile document, and such a document renders
truncated rather than taking the process down.

**And what still holds because of it.** `Scalar` has no iterative `Drop`, on the
grounds that scalar nesting comes from the value grammar, which entry 9 bounds
at 64. The transform stage keeps that true: it builds a `Scalar` only through
`Scalar::from_value` over a value that resolution has already bounded, and never
synthesises one from document structure. Slot content, which *does* track
document depth, goes into the attribute map as `RenderableTreeNodes` -- whose
`Tag` carries the iterative `Drop`. A later stage that builds scalars from
document structure breaks that assumption and needs to say so.

## 15. Formatting is depth-limited

**Upstream:** `formatNode` recurses into `formatChildren`, which recurses back
into `formatNode`, with no bound; `formatScalar` recurses into nested arrays and
hashes the same way. A tree nested deeply enough exhausts V8's stack and throws
a `RangeError` a caller can catch.

**Here:** both walks stop at `format::MAX_FORMAT_DEPTH` -- 128 levels. A node
below that depth prints as nothing, and its ancestors print normally; a value
below it prints as nothing inside brackets that still close.

**Why:** the same reason as entries 9 and 14, one stage further along. Nesting
depth is attacker-controlled, the Rust equivalent of V8's `RangeError` is a
stack overflow, and a stack overflow aborts the process rather than raising
anything a caller can catch. `src/lib.rs` states panic-freedom over arbitrary
input as an API-level promise, and that promise is only true with a bound at
every layer that recurses over document structure.

An iterative rewrite was considered and rejected, for a different reason from
entry 14's. The obstacle here is not a hook signature: it is that two arms --
`blockquote` and `list` -- do not stream their children's output into their own.
They call the formatter *at the top* on each child, take the finished string,
and paste it behind a prefix, because each child has to be trimmed
independently of its siblings. Unrolling the walk onto an explicit stack means
unrolling those two re-entries as well, which is a continuation per list item
in order to remove a bound that no document reaches.

**Why 128 and not the transform stage's 512.** Because the number was measured
rather than chosen, and the first choice was wrong. This walk carries a fatter
frame than the transform's: printing a tag builds several strings before it
recurses. Written as upstream writes it -- one function holding every arm --
each frame carried every arm's locals, cost about 6 KB per level, and overflowed
a 2 MiB thread stack at around 350 levels, which is *under* the 512 that was
supposed to prevent exactly that. Splitting each arm into its own function put
only the live arm on the stack and moved the ceiling a little past 700. 512
would then sit inside the ceiling by less than half, which is not a margin for a
published promise. 128 sits inside it by a factor of five, in the least
favourable configuration this crate is built in: a debug build on the 2 MiB
stack that `cargo test` gives every test.

**Also bounded, for the same reason at the allocator rather than the stack:** a
heading prints at most 1,024 `#` characters. `level` is an ordinary attribute,
so a host can set it to any `f64`, and `"#".repeat(n)` for a large `n` is an
allocation failure, which also aborts. The bound is far above CommonMark's six
levels.

**What it costs, exactly.** Nothing the corpus contains, nothing upstream's
formatter test contains, and nothing a person writes. 128 levels is around forty
levels of *authored* nesting, because a paragraph of text is already four --
document, paragraph, inline, text -- and a list adds two per level. A generated
or hostile document prints truncated rather than taking the process down.

## 16. Four round-trip defects in upstream's formatter are fixed, not reproduced

**Upstream:** the formatter emits output that does not parse back to the tree it
came from, in four shapes. Each is reachable from an ordinary document, and all
four are silent -- the output looks plausible.

1. **A blockquote marks only its first line.** `formatter.ts:236-241` writes
   `NL + indent + prefix + d`, one `> ` per *child*. A child that prints on more
   than one line -- a paragraph with a soft break, a list, a fence -- comes back
   with its later lines outside the quote. `> a\n> b` reprints as `> a\nb`.
2. **An inline comment ends a line.** The `comment` arm appends a newline to
   every comment. Upstream's tokenizer has an inline comment rule, so a comment
   inside a sentence is a real node, and printing it splits the paragraph.
3. **An indented fence closes at column zero.** The `fence` arm yields the
   closing boundary with no indent. It looks correct in upstream's own tests
   because content that *ends* with a newline leaves a trailing empty segment
   that the indent-join above it indents. Content without one -- an empty fence,
   or a last line with no terminator -- closes unindented, and the reprint is a
   fence that never closes.
4. **A tag with children that print nothing is written open-and-closed.** The
   `tag` arm asks whether the child list is empty. A tag holding a child that
   prints only whitespace -- an empty `table` node, an `error` node -- is
   therefore written as `{% x %}` ... `{% /x %}` with nothing between, which
   re-parses as a tag with *no* children. Upstream's own `tags` case fixes
   `{% a %}{% /a %}` as `{% a /%}`, so its output does not settle until a second
   pass.

**Here:** the prefix goes on every line, an inline comment ends no line, the
closing boundary carries the indent, and a tag whose children come to nothing
self-closes.

**Why:** because the round trip is the specification, not a nice property. r111
§9.5 states `parse(format(ast))` as one of the two gates on this stage, and the
first consumer is a rewriting tool: the host's `migrate` command uses this
formatter as its writer over a whole content tree. Reproducing defect 1 means
silently moving the second line of every two-line blockquote out of its quote,
in every migrated file, with nothing in the output that looks wrong. That is not
a compatibility win worth having, and "upstream does it too" is not a defence a
corrupted document accepts.

All four were found by the property test rather than by reading, which is the
argument for having written it: the shapes are individually obvious and
collectively invisible.

**What it costs, exactly.** Output differs from upstream only where upstream's
own output does not re-parse. The first three fire in no other case -- upstream's
`multi-paragraph blockquotes`, `"loose" lists` and `fences with no ending
newline` cases all reprint byte-identically here, because their content already
takes the branch that was correct. The fourth changes `{% x %}{% /x %}` into
`{% x /%}` for a tag whose children print nothing, which is the same
normalisation upstream already applies to a tag with no children at all.

**And what is deliberately *not* fixed.** Upstream escapes exactly three things
at the start of a run of text: a `*`, a `>`, and a run of `#`. There are more
than three ways for text to look like a block -- a leading fence marker, a
trailing backslash, a `|`, a `1.` -- and a reprint can therefore still change
what a line means, for a tree that a document could not have produced in the
first place. The escape set is upstream's contract and widening it changes the
bytes of every document that contains a backtick; it stays as it is, and
`tests/formatter.rs` carries a case naming the limitation so it is recorded
rather than rediscovered.
