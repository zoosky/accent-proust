# Changelog

Notable changes to `accent-proust`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A WebAssembly build, published to npm as `accent-proust`.**
  `crates/accent-proust-wasm` exposes `validate`, `renderHtml`, `transform` and
  `format` to a browser or any other JavaScript host. The renderable tree it
  returns carries Markdoc's `$$mdtype` marker, so a renderer written against
  `@markdoc/markdoc` maps tag names onto components unchanged. Variables,
  partials and host-defined schemas are not across the boundary yet.

  Nothing in the library changed: the binding is a workspace member, and the
  mapping to JavaScript objects lives there rather than behind a `serde`
  feature here.

## [0.9.0] - 2026-09-04

First release. The engine is complete: source text parses to an AST, an AST
validates against a schema, a validated AST transforms into a renderable tree
and renders to HTML, and a tree prints back to canonical Markdoc source.

`0.9.0` rather than `1.0.0` because the API has had no external users yet. The
conventions below are already promises; the shape of the types is not.

### Added

- **Parse.** A segmenter over raw text -- block-level `{% %}` lines, inline
  spans, fence interception -- feeding Markdown segments to a `Tokenizer`.
  Ported from upstream Markdoc `v0.5.9` (revision `afee1a4`).
- **Validate.** Schemas, attribute types, and the validator. Upstream's error
  ids are reproduced exactly, because external tooling binds to them.
- **Transform and render.** `transform` builds a renderable tree; `render`
  emits HTML.
- **Format.** Canonical Markdoc source from a tree. `format(parse(s))` is
  idempotent and `parse(format(ast))` returns the same tree, so a formatter can
  rewrite a file in place without losing anything.
- **Three seams for the host**, because this crate does no I/O, reads no
  configuration, and decides no HTML policy: `Tokenizer` for Markdown
  segmentation, `SchemaSource` for where a schema comes from, and `TagRenderer`
  for escaping and markup.
- **The `pulldown-cmark-tokenizer` feature**, on by default, supplying a
  `Tokenizer` over pulldown-cmark. Turning it off leaves the trait and every
  layer above it, so a host that already parses CommonMark does not compile a
  second parser. That configuration is built and tested by CI.

### Conformance

95 of upstream's 105 corpus cases match, 10 exercise a declared divergence, and
none fail. The corpus is vendored under `spec/` and is the test suite;
`conformance-baseline.txt` is a ratchet that fails on drift in either direction.

The 16 deliberate differences are recorded in `DIVERGENCES.md`, which is
normative rather than a changelog. The largest is that CommonMark edge
behaviour is not part of the contract: upstream builds on markdown-it and this
crate builds on pulldown-cmark.

### Guarantees

- **Panic-freedom**, asserted by property tests over arbitrary input and over
  values a caller assembles through the public API. Every public recursive type
  writes out `Drop`, `Clone`, `PartialEq` and `Debug` by hand, so that no
  traversal recurses per level and overflows the stack. No `unsafe` anywhere.
- **Deterministic output.** Attribute order is authored order, never hash
  order, so two runs over the same input produce identical bytes.
- **Validation errors are data.** Validating returns a `Vec`, so an editor can
  show every problem at once. `Result::Err` is reserved for internal invariants.
- **Public enums are `#[non_exhaustive]`**, so a new upstream node type is not a
  breaking release.
- **MSRV 1.96** for the library, normalised across the Accent crates, on Rust
  edition 2024.

[Unreleased]: https://github.com/zoosky/accent-proust/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/zoosky/accent-proust/releases/tag/v0.9.0
