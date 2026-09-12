# Changelog

Notable changes to `accent-proust`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A command-line host, `crates/accent-proust-cli`.** One command so far:
  `accent-proust fmt` reprints Markdoc source in canonical form, from files or
  stdin. `--check` prints a unified diff of what would change and exits 1 if
  anything would; `--write` rewrites in place. `fmt` formats to a fixed
  point: `format(parse(s))` settles in one pass on everything but one shape
  the library documents, so `fmt` reformats its own output until it stops
  changing, and write-then-check is clean by construction. CRLF files are
  named by `--check` and rewritten with LF. Exit codes are 0, 1 for a
  document that would change, and 2 for a usage or read error or a document
  that does not settle, kept apart so that CI can tell "the docs are wrong"
  from "the tool is misconfigured". `validate`, `render`, `parse` and
  `transform` follow, in the order `specs/features/cli-and-host-seams.md`
  sequences them.

- **One lint block for the workspace.** The library, the WebAssembly host and
  the command-line host opt into `[workspace.lints]` instead of carrying a
  copy each. Inheritance is opt-in per member, so sharing binds no future host
  that leaves the line out; what it does is keep three crates from drifting
  apart by accident.

- **`TagRenderer`, the seam between the renderable tree and its markup.** The
  HTML renderer is now `render::Html` behind a trait, and `render_with` and
  `render_all_with` take any other implementation: a different escaping
  policy, a different void-element list, or a format that is not HTML.
  `render` and `render_all` are unchanged and produce the same bytes.

  The trait is `open`, `close` and `text`, plus a provided `number` whose
  default spells a value as ECMAScript does; `open` answers with `Children`,
  a `#[non_exhaustive]` enum of `Render` or `Skip`. It is deliberately not
  "here is a tag, here is a callback for its children". The renderer walks an
  explicit stack because nesting depth is the document's to choose, and a
  callback would put that depth back on the host's stack one frame per level.
  The walk stays in the crate and the host only writes bytes.

  `escape_html_into` and `attribute_value` are public so an implementation
  can keep the parts of upstream's behaviour it wants. The escaper does not
  replace `'`, which was safe while only `Html` used it and is now a
  documented condition: text and double-quoted attributes only.

### Changed

- **Breaking: `Config` holds a `SchemaSource` instead of two maps.** `nodes`,
  `tags`, `nodes_mut()` and `tags_mut()` are gone. `config.schemas` is an
  `Arc<dyn SchemaSource + Send + Sync>`, `MapSchemaSource` is the
  implementation that holds what the two maps held, and `builtins::config_with`
  builds a config around one -- the built-in functions added, the built-in
  schemas built once. `builtins::config()` is unchanged, and
  `Config::with_schemas` swaps the source on a config already held.
  Registering a tag on top of the built-ins was

  ```rust
  let mut config = builtins::config();
  config.tags_mut().insert("callout".to_string(), schema);
  ```

  and is now

  ```rust
  let mut schemas = MapSchemaSource::builtin();
  schemas.insert_tag("callout", schema);
  let config = builtins::config_with(Arc::new(schemas));
  ```

  One mechanism rather than a source consulted ahead of the maps, so there is
  no precedence rule to document. `SchemaSource::find` returns a borrow, which
  rules out loading a schema on first request by design: a host populates its
  source before handing it over. `Config`'s `Debug` still prints the
  registered names, through the trait's provided `tag_names` and
  `node_types`, and prints `None` for a source that cannot enumerate.
  `SchemaKey` is exhaustive, the one public enum here that is: an
  implementation matches both ways a node can be looked up, and a third --
  none is foreseen -- would fail to compile rather than silently miss.

## [0.10.0] - 2026-09-06

### Added

- **A project website, with the documentation and a live playground.** `site/`
  is a landing page, five documentation pages -- Rust, JavaScript, the Markdoc
  language, the architecture, and the divergences -- and a playground that runs
  the WebAssembly engine in the reader's browser. `scripts/build-site.sh`
  builds it and `.github/workflows/pages.yml` publishes it to GitHub Pages.

  It is built with Accent CMS, the sibling generator this crate ships
  alongside, and it is not a workspace member: nothing in `Cargo.toml` knows it
  exists and `scripts/check-standalone.sh` is unaffected. The playground's
  engine is compiled from `crates/accent-proust-wasm` on every build rather
  than committed, so it can never be a stale copy of a release.

- **A WebAssembly build, published to npm as `accent-proust`.**
  `crates/accent-proust-wasm` exposes `validate`, `renderHtml`, `transform` and
  `format` to a browser or any other JavaScript host. The renderable tree it
  returns carries Markdoc's `$$mdtype` marker, so a renderer written against
  `@markdoc/markdoc` maps tag names onto components unchanged. Variables,
  partials and host-defined schemas are not across the boundary yet.

  Nothing in the library changed: the binding is a workspace member, and the
  mapping to JavaScript objects lives there rather than behind a `serde`
  feature here.

- **Host schema configuration for the WebAssembly bindings.** `new Config({
  tags, nodes, variables })` builds a validator configuration from declared
  data and merges it over Markdoc's built-ins; `validate`, `renderHtml` and
  `transform` are methods on it. Without this the bindings knew only Markdoc's
  own tags, so a document written for a host reported `tag-undefined` for every
  component it used.

  Hooks, custom attribute types, `RegExp` in `matches` and host functions are
  code and do not cross, so the browser is never stricter than the server. They
  are refused by name with the path to them rather than dropped, because a
  schema that half arrives hides the half that is missing.

- **Diagnostic positions in UTF-16 code units.** A location edge from the
  WebAssembly bindings carries `line`, `character`, `offset` and `byteOffset`,
  and every field but the last is counted the way JavaScript counts a string
  index. The engine measures UTF-8 bytes, an editor position is UTF-16, and the
  two agree until an author writes a character outside ASCII -- at which point
  an unconverted offset underlines the wrong character. Converting at the
  boundary means no host rediscovers this.

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

[Unreleased]: https://github.com/zoosky/accent-proust/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/zoosky/accent-proust/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/zoosky/accent-proust/releases/tag/v0.9.0
