---
title: Documentation
template: docs
lead: >-
  What the crate is, what it promises, and where each promise is written down.
menu:
  visible: true
  order: 3
description: >-
  Documentation for accent-proust: the Rust API, the JavaScript and WebAssembly
  bindings, the Markdoc language, the architecture, and every declared
  divergence from upstream Markdoc.
---

`accent-proust` implements the [Markdoc](https://markdoc.dev) language in Rust.
Markdoc is CommonMark plus a tag syntax, which is the part that turns a document
into structured, validatable content instead of pre-rendered HTML:

```markdown
{% callout type="note" %}
Tags nest, take typed attributes, and are checked against a schema.
{% /callout %}
```

The crate covers the whole language: source text parses to an AST, an AST
validates against a schema, a validated AST transforms into a renderable tree
and renders to HTML, and a tree prints back to canonical Markdoc source.

## Start here

Pick by where your code runs.

| You are writing | Read |
|---|---|
| A Rust program, a CLI, a static site generator | [Rust](/docs/rust) |
| A browser editor, a preview pane, a Node tool | [JavaScript](/docs/javascript) |
| Markdoc documents, or a schema for them | [The Markdoc language](/docs/language) |
| A host that replaces part of the engine | [Architecture](/docs/architecture) |
| Something that must match upstream exactly | [Divergences](/docs/divergences) |

The generated API reference lives on
[docs.rs](https://docs.rs/accent-proust), which these pages link into rather
than duplicate.

## The two contracts

Two things about this port are promises, and everything else is an
implementation detail that may change.

**The tag language.** Block tags, inline tags, annotations, typed attributes,
variables, functions, partials and the built-in `if`/`else`/`table`/`partial`
tags behave as upstream defines them.

**The error ids.** `tag-undefined`, `attribute-missing-required`,
`attribute-type-invalid` and the rest are upstream's strings, unchanged. Editor
extensions and CI checks bind to those codes, which makes them the part of the
surface this crate is least free to invent.

CommonMark edge behaviour is deliberately *not* on that list. Upstream builds on
markdown-it and this crate builds on pulldown-cmark, and the two disagree in
places. Every disagreement that matters is enumerated in
[Divergences](/docs/divergences) rather than papered over.

## Versioning

The crate is at `0.9.0` rather than `1.0.0` because the API has had no external
users yet. The conventions above are already promises; the shape of the Rust
types is not, and a `0.x` number is the honest way to say so.

> [!NOTE]
> The npm package and the crate share one version number. A release publishes
> both, so `accent-proust 0.10.0` on crates.io and `accent-proust@0.10.0` on npm
> are built from the same commit.

## Licence and provenance

MIT, and a compatible reimplementation derived from the MIT-licensed Markdoc
source. Ported from upstream Markdoc `v0.5.9` (revision `afee1a4`). Upstream's
TypeScript is vendored into the repository unmodified so that a porting change
shows its source in the diff -- it is excluded from the published crate, where
it would be 392 KB of another language.
