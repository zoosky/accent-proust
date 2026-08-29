# proust

A Rust implementation of the [Markdoc](https://markdoc.dev) language: parse,
validate, transform, render, and format.

> **Status: scaffold, measured.** The layout, the licence, the divergence
> budget, CI and the conformance harness are in place. No language surface is
> implemented yet, and nothing is published. See
> [Conformance](#conformance) for the live number.

## What this is

Markdoc is a Markdown-based authoring language: CommonMark, plus a tag syntax
(`{% callout type="note" %} ... {% /callout %}`) that turns documents into
structured, validatable content rather than pre-rendered HTML. `proust`
implements that language in Rust, as a library:

```text
parse  ->  AST  ->  validate  ->  transform  ->  renderable tree  ->  format
```

It does no I/O, reads no configuration, and decides no HTML policy. It does not
know what a theme, a file, or a plugin is. Everything host-specific enters
through data the caller passes in or through a trait the caller implements:

| Seam | Shape | Implemented by |
|---|---|---|
| Markdown segmentation | `trait Tokenizer` | this crate (pulldown-cmark, default feature) or the host |
| Schema source | `trait SchemaSource` | the host |
| Tag rendering | `trait TagRenderer` | the host |

That boundary is the crate's main design constraint, and it is enforced in CI:
one job builds and tests `proust` with nothing else present. A library that
reaches into its host cannot be published, and a library that can be published
cannot be where the host's decisions live.

### A note for hosts that already parse CommonMark

The bundled tokenizer depends on `pulldown-cmark` from crates.io. A host that
pins `pulldown-cmark` to a git revision is pinning a *different package* as far
as Cargo is concerned, so enabling the bundled tokenizer would compile two
CommonMark parsers into one binary and render some documents through each.

Turn the `pulldown-cmark-tokenizer` feature off and implement `Tokenizer` over
the parser you already pin. That is what the seam is for, and it is the
supported configuration -- the standalone CI job builds and tests this crate in
exactly that shape.

## Compatibility, precisely

**Ported from upstream Markdoc at revision `afee1a4`, tag `v0.5.9`** -- 2,879
lines of TypeScript across `src/`, excluding tests.

"Markdoc-compatible" here means three specific things, and not more:

- **The tag language is the contract.** The same source produces the same AST
  and the same validation results for the constructs the upstream conformance
  corpus covers.
- **Validation error ids are identical.** Tooling written against Markdoc's
  error codes works unchanged. This is the part that must never drift.
- **CommonMark edge behaviour is not the contract.** Upstream is built on
  markdown-it and this crate is built on pulldown-cmark. Where the two engines
  disagree, the difference is declared in
  [`DIVERGENCES.md`](DIVERGENCES.md), never emulated silently.

Every deliberate difference lives in that file, which starts at eight entries
rather than empty.

One of the eight is worth knowing before reading the conformance number:
upstream's corpus is graded under a **non-default tokenizer configuration**
(`allowIndentation: true, allowComments: true`), and `allowIndentation` works
only because upstream patches markdown-it itself. That option is unreachable
here and six of the 105 cases exercise it.

## Conformance

Upstream's `spec/marktest/tests.yaml` -- 105 cases of source in, expected
AST / validation errors / HTML out -- is the progress measure and the merge
gate. It is vendored under [`spec/`](spec/UPSTREAM.md) at the ported revision
and never edited.

```sh
cargo test --test conformance -- --nocapture
```

```text
conformance: 0 green, 6 annotated, 99 failing (of 105)
```

"Annotated" is a case that fails because it exercises a declared divergence;
it is counted apart from a failure so that giving something up stays visible
instead of being absorbed. All six are
[divergence 8](DIVERGENCES.md), the `allowIndentation` option upstream's corpus
runner switches on and this crate cannot reach.

`conformance-baseline.txt` is the ratchet. The harness fails on any drift from
it: a drop is a regression and is not mergeable, and a rise is a baseline that
was not updated in the same commit. It is not an absolute `105/105` gate, which
would leave every pull request failing a required check until the port
finished, and a check that is always red is a check nobody reads.

The harness exists before the parser on purpose. A conformance counter written
after the implementation grades what was built; one written before it grades
what was meant.

## Layout

The module tree mirrors upstream file-for-file wherever the code is a pure
function of its inputs, so a future upstream commit diffs cleanly against its
Rust counterpart:

| Upstream | Here |
|---|---|
| `src/ast/` | `src/ast/` |
| `src/grammar/tag.pegjs` | `src/grammar/` |
| `src/parser.ts`, `src/tokenizer/` | `src/parse/` |
| `src/validator.ts`, `src/schema.ts`, `src/schema-types/` | `src/validate/` |
| `src/transformer.ts`, `src/transforms/` | `src/transform/` |
| `src/renderers/html.ts` | `src/render/` |
| `src/formatter.ts` | `src/format/` |
| `src/functions/` | `src/functions/` |
| `src/tags/` | `src/tags/` |

Two upstream trees are vendored at the ported revision, and neither is ever
edited:

| Path | What | Why |
|---|---|---|
| [`spec/`](spec/UPSTREAM.md) | the conformance corpus and its runner | it is the test suite, so a fresh clone runs it with `cargo test` and nothing else |
| [`reference/`](reference/UPSTREAM.md) | upstream's TypeScript, its unit tests, and its markdown-it patch | a porting pull request shows its source in the same diff, and the yearly upstream refresh is `git diff` rather than a second checkout |

`reference/` is `exclude`d from the packaged crate. No Rust tooling reads
either.

## Building

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Minimum supported Rust version: 1.82. That is a promise about the library, and
a consumer never builds a dev-dependency -- but running the tests in this
repository needs a newer toolchain than that, because the conformance harness
reads YAML with a crate that does. Contributors should be on stable.

## Licence and attribution

MIT. See [LICENSE](LICENSE).

This project is a compatible reimplementation, derived from the MIT-licensed
Markdoc source. It is not affiliated with, endorsed by, or a product of Stripe.

**MARKDOC is a trademark of Stripe, Inc.**, which also owns the upstream
Markdoc project. The mark is used here only to describe what this software is
compatible with.

## The name

Marcel Proust, who composed *A la recherche du temps perdu* by writing on
strips of paper and gluing them into the manuscript to extend it. Those
*paperoles* are, exactly, what a formatter does: parse, mutate, and print
canonical source. The name refers to nothing of Stripe's.
