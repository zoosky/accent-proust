---
title: Markdoc, in Rust
template: home
menu:
  visible: false
  order: 0
description: >-
  accent-proust is a Rust implementation of the Markdoc language: parse,
  validate, transform, render, and format. It compiles to WebAssembly, so the
  same engine runs in a browser.

# Everything below is read by home.html.jinja through `page.custom.*`. A
# landing page is a layout with slots rather than a document, so its copy lives
# here as data instead of as body prose -- which also means the wording can
# change without touching a template.

eyebrow: "v0.9.0 - MIT - forbid(unsafe_code)"
headline: "Markdoc,"
headline_accent: "in Rust."
lead: >-
  Parse, validate, transform, render and format the Markdoc language. A library
  with no I/O, no configuration and no HTML policy of its own -- and the same
  engine, compiled to WebAssembly, running in the playground on this site.

pipeline_title: Five stages, and you can stop at any of them
pipeline_lead: >-
  Markdoc is CommonMark plus a tag syntax, which turns a document into
  structured content instead of pre-rendered HTML. Each stage below is a
  separate entry point: a linter stops after validate, an editor stops after
  transform, and a static site runs the lot.

stages:
  - name: parse
    description: >-
      Source text to an AST. Block tags, inline spans and annotations, with a
      byte range on every node.
  - name: validate
    description: >-
      The AST against a schema. Returns a list of errors, never a failure, so
      an editor shows all of them at once.
  - name: transform
    description: >-
      A validated AST to a renderable tree. Variables and functions resolve
      here; tags become nodes a host can map to components.
  - name: render
    description: >-
      The tree to HTML. Escaping and element policy live behind a trait, so a
      host that wants different markup supplies it.
  - name: format
    description: >-
      A tree back to canonical Markdoc source. Idempotent, and round-trips
      without losing anything.

figures_title: The conformance corpus is the test suite
figures_lead: >-
  Upstream's own 105 cases are vendored into the repository and run on every
  commit, against a baseline file that fails the build if the numbers move in
  either direction. Nothing fails; the ten cases that do not match upstream
  exactly are counted apart, because giving something up should stay visible.

figures:
  - value: "95"
    tone: green
    label: cases matching upstream exactly
  - value: "10"
    tone: gold
    label: cases exercising a declared divergence
  - value: "16"
    tone: cyan
    label: divergences, each written down
  - value: "0"
    tone: coral
    label: lines of unsafe code, enforced by the compiler

features_title: What the library actually promises
features_lead: >-
  A short list, because each item on it is a thing the code is arranged around
  rather than a thing it happens to do.

features:
  - title: Upstream's error ids, unchanged
    body: >-
      <code>attribute-missing-required</code>, <code>tag-undefined</code> and
      the rest are reproduced exactly. External tooling binds to those codes,
      so they are the field this crate is least free to invent.
  - title: Diagnostics are data
    body: >-
      Validation returns a <code>Vec</code>, not a <code>Result</code>. A
      document with mistakes still transforms and still renders, because a
      preview pane that blanks on the first error is worse than one that shows
      the error.
  - title: Bring your own CommonMark parser
    body: >-
      The bundled tokenizer sits behind a default feature. Turn it off,
      implement <code>Tokenizer</code>, and nothing above it changes. A CI job
      builds and tests the crate in exactly that shape.
  - title: Three seams, all yours
    body: >-
      <code>Tokenizer</code> segments Markdown, <code>SchemaSource</code>
      answers where a schema comes from, and <code>TagRenderer</code> owns
      escaping and HTML policy. The crate does no I/O and reads no
      configuration.
  - title: The same engine in a browser
    body: >-
      A WebAssembly binding publishes <code>validate</code>,
      <code>renderHtml</code>, <code>transform</code> and <code>format</code>
      to npm. Diagnostic positions arrive in UTF-16 code units, so an editor
      underlines the right character without converting anything.
  - title: Panic-freedom as a lint, not a habit
    body: >-
      <code>unsafe_code</code> is forbidden and
      <code>panic</code>/<code>unwrap</code>/<code>expect</code>/<code>indexing_slicing</code>
      are denied in CI. This is an open parser fed arbitrary text: its attack
      surface is part of its API.

band_title: Type into it and watch all five stages run
band_lead: >-
  The playground is this engine compiled to WebAssembly. Edit the source on the
  left and the HTML, the diagnostics and the renderable tree update on the
  right -- locally, with nothing sent anywhere.
---

## Why a port, and why this one

Markdoc's reference implementation is TypeScript. A Rust program that wants the
language has three options: shell out to Node, reimplement the parts it needs,
or use a port. The first two are how content pipelines acquire a Node dependency
and a subtly different dialect.

This is the third. Upstream's TypeScript is vendored into the repository
unmodified, so a change that ports a behaviour shows its source in the diff and
the yearly refresh is a diff rather than an archaeology exercise. Where the two
cannot agree -- upstream builds on markdown-it and this crate on pulldown-cmark,
and those disagree about CommonMark edge cases -- the difference is written down
in [Divergences](/docs/divergences) and never emulated silently.

The tag language and the error ids are the contract. CommonMark edge behaviour
is not, and pretending otherwise would be the more expensive lie.
