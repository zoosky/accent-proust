---
title: Divergences
template: docs
lead: >-
  The sixteen places this implementation deliberately behaves differently from
  upstream Markdoc, why each one was chosen, and the rule that keeps the list
  complete.
menu:
  visible: true
  order: 5
description: >-
  Every deliberate difference between accent-proust and upstream Markdoc
  v0.5.9, grouped by cause: the CommonMark engine, deliberate limits, API
  shape, unimplemented surface, and upstream defects that are fixed rather
  than reproduced.
---

Ported from upstream Markdoc `v0.5.9` (revision `afee1a4`). Two rules govern
what follows, and they are the reason the list can be trusted to be complete.

**Divergences are declared, never discovered.** A behaviour difference found
while chasing a conformance failure is either a bug to fix or a new entry in
`DIVERGENCES.md`, added in the same pull request that finds it, with a sentence
saying why emulating upstream was rejected. It is never left implicit.

**The error vocabulary never diverges.** Upstream validation error ids are kept
identical, because that is the part external tooling binds to. Renaming an id
would itself be a divergence and would need an entry.

> [!NOTE]
> The authoritative document is
> [`DIVERGENCES.md`](https://github.com/zoosky/accent-proust/blob/main/DIVERGENCES.md)
> in the repository, which is normative and carries the full reasoning for each
> entry. This page is a map of it.

## How they are counted

Conformance is to the **tag language**, not to markdown-it's CommonMark
minutiae. Upstream's own 105-case corpus is vendored and run as the test suite;
a case that fails because of a CommonMark difference is annotated with the
divergence it exercises and counted separately from a failure.

```text
conformance: 95 green, 10 annotated, 0 failing (of 105)
```

That split is what makes "we chose this" distinguishable from "we have not done
this yet". A case that should stop being green becomes an entry here and moves
from `green` to `annotated` -- it never becomes a smaller number in the
baseline.

The file started at eight entries on purpose: an empty divergence file invites
the belief that there are none.

## Because the CommonMark engine is different

Upstream builds on markdown-it. This crate builds on pulldown-cmark. Most of the
list follows from that one decision, which is entry 2.

| # | Divergence |
|---|---|
| 1 | **Fences do not process tags by default.** Upstream parses tags inside code fences and lets a fence opt out; here it is the other way round |
| 2 | **The CommonMark engine is pulldown-cmark, not markdown-it.** The root cause of most of this table |
| 5 | **Heading attributes are CommonMark's `{#id}`,** and Markdoc annotations are not ported for headings |
| 6 | **GFM alerts and a `callout` tag coexist,** and neither is rewritten into the other |
| 7 | **Metadata blocks are stripped** before the tag layer sees the document |
| 11 | **Upstream's two disabled markdown-it rules are only half reachable** from this engine |
| 13 | **A block tag indented inside a list item is not part of the item.** Upstream's block-tag rule is a markdown-it block rule, which changes where the boundary falls |

## Because a document is untrusted input

Three limits exist that upstream does not have. Upstream recurses without a
bound in each of these places; an unbounded recursion over attacker-supplied
text is a denial-of-service shape, and a stack overflow aborts the process
rather than raising anything a caller can catch.

| # | Divergence |
|---|---|
| 9 | **Nested values are depth-limited** |
| 14 | **Transform recursion is depth-limited** |
| 15 | **Formatting is depth-limited** |

This is the same commitment as the panic-freedom lints described under
[Architecture](/docs/architecture#panic-freedom): the crate is an open parser,
so its attack surface is part of its API.

## Because Rust is not JavaScript

| # | Divergence |
|---|---|
| 3 | **Schema hooks are synchronous.** Upstream's hooks return a `MaybePromise` |
| 10 | **Maps keep authored order, not JavaScript object order.** Which is also what makes output byte-reproducible across runs |
| 12 | **`matches` takes a host-supplied pattern, not a regular expression.** Upstream accepts a `RegExp`, and a regular-expression engine is not something this crate is willing to require |

Entry 12 is the one most likely to affect a schema you are porting. It is also
why a `RegExp` in `matches` is refused by name at the
[JavaScript boundary](/docs/javascript#what-does-not) rather than silently
dropped.

## Not implemented

| # | Divergence |
|---|---|
| 4 | **The React renderers are unimplemented.** Upstream ships `renderers/react`, dynamic and static. Here, `transform` returns the same tree and the host renders it |
| 8 | **The `allowIndentation` tokenizer option is not implemented** |

## Fixed rather than reproduced

| # | Divergence |
|---|---|
| 16 | **Four round-trip defects in upstream's formatter are fixed.** Upstream emits output that does not parse back to the tree it came from; this crate does not reproduce that |

Entry 16 is the only one that makes this implementation *more* correct than the
original rather than differently correct, and it is declared here for the same
reason as the rest: a difference nobody wrote down is a difference nobody can
plan around. `format(parse(s))` being idempotent and `parse(format(ast))`
returning the same tree are properties this crate tests, and they are the
properties those four defects break.
