---
title: Rust
template: docs
lead: >-
  Install the crate, run the pipeline, define a tag, and read the errors. Every
  snippet on this page is compiled and run by the repository's test suite.
menu:
  visible: true
  order: 1
description: >-
  The accent-proust Rust API: parse, validate, transform, render and format
  Markdoc, with schemas, variables, partials and a swappable CommonMark
  tokenizer.
---

## Install

```sh
cargo add accent-proust
```

The minimum supported Rust version is declared in the manifest and checked
before every release. The bundled CommonMark tokenizer is behind the default
`pulldown-cmark-tokenizer` feature; see [Bring your own
tokenizer](#bring-your-own-tokenizer) below for the other shape.

## Render a document

The whole pipeline, in three calls. `builtins::config()` is Markdoc's own
schemas -- headings, paragraphs, lists, `if`, `table`, `partial` and the rest.

```rust
use accent_proust::{builtins, parse, render, transform};

let document = parse::parse("# Title\n\nSome *text*.\n");
let tree = transform::transform(&document, &builtins::config());

assert_eq!(
    render::render_all(&tree.into_vec()),
    "<article><h1>Title</h1><p>Some <em>text</em>.</p></article>"
);
```

`parse` never fails. A malformed tag is a node in the tree carrying an error,
not a `Result::Err`, because a document that is half-written is the normal state
of a document being written.

## Define a tag

A tag needs a schema before it validates or renders. `render` names the element
to emit; declared attributes reach the output, undeclared ones are an error.

```rust
use accent_proust::validate::{self, Schema, SchemaAttribute, ValidationType};

let mut config = builtins::config();
config.tags_mut().insert(
    "callout".to_string(),
    Schema::new().render("div").attribute(
        "type",
        SchemaAttribute {
            attribute_type: Some(ValidationType::String),
            required: true,
            ..SchemaAttribute::default()
        },
    ),
);

let document = parse::parse("{% callout type=\"note\" %}\nBody\n{% /callout %}\n");
assert!(validate::validate_tree(&document, &config).is_empty());

let tree = transform::transform(&document, &config);
assert_eq!(
    render::render_all(&tree.into_vec()),
    "<article><div type=\"note\"><p>Body</p></div></article>"
);
```

`tags_mut()` is copy-on-write. `Config` shares its four maps behind an `Arc`, so
registering a schema registry once and cloning the config per document is cheap
-- which is the shape a static site generator wants, since it scopes the same
config a few thousand times.

## Read the errors

Validation errors are data, not failures. You get a `Vec`, so an editor shows
every problem at once instead of the first one.

```rust
let document = parse::parse("{% callout %}\nBody\n{% /callout %}\n");

for error in validate::validate_tree(&document, &config) {
    println!("{}: {}", error.error.id, error.error.message);
    // attribute-missing-required: Missing required attribute: 'type'
}
```

`error.id` is upstream Markdoc's id, unchanged. That is the field external
tooling binds to, so it is the field this crate is least free to invent. Each
error also carries the byte range of the node it came from, which is what an
editor needs to underline the offending span.

## Format

`format` prints a tree as canonical Markdoc source. It normalises spacing inside
a tag and leaves your own spellings alone, so `__bold__` stays `__bold__`.

```rust
use accent_proust::format;

let document = parse::parse("{% callout   type=\"note\"  %}\nBody\n{% /callout %}\n");
assert_eq!(
    format::format(&document),
    "{% callout type=\"note\" %}\nBody\n{% /callout %}\n"
);
```

Two properties make this safe to run over a file in place:
`format(parse(s))` is idempotent, and `parse(format(ast))` gives back the same
tree. The first means a formatter can run twice without churn; the second means
formatting loses nothing.

## Variables, functions and partials

Those three live on `Config` alongside the schemas.

| Field | Holds | Note |
|---|---|---|
| `variables` | What `$name` resolves against | `None` switches variable checking off entirely; `Some` of an empty map switches it on with nothing defined |
| `functions` | What `f()` calls resolve against | A `ConfigFunction` declares its parameters so the validator can check a call |
| `partials` | Parsed documents, keyed by the name `{% partial file="..." %}` uses | Parsed, not raw -- the crate does no I/O, so the host reads the file and parses it |

That last row is why `Config` carries a lifetime: it borrows the partial
documents the host parsed.

> [!NOTE]
> The distinction between `variables: None` and `variables: Some(empty)` is load
> bearing. A tool that does not know the host's variables should use `None` and
> report nothing, rather than report every `$reference` in the document as
> undefined.

## Bring your own tokenizer

`Tokenizer` is the one real trait seam in the crate. The bundled implementation
uses `pulldown-cmark`, behind the default feature:

```toml
accent-proust = { version = "*", default-features = false }
```

Turn the feature off and implement `Tokenizer` if you already parse CommonMark,
or if you pin `pulldown-cmark` to a git revision -- Cargo treats that as a
different package, so you would otherwise compile two CommonMark parsers into
one binary and render some documents through each.

```rust
use accent_proust::parse::tokenizer::{Spanned, Tokenizer};

struct MyTokenizer;

impl Tokenizer for MyTokenizer {
    fn tokenize<'s>(&self, source: &'s str) -> Vec<Spanned<'s>> {
        // Emit Start/End events over byte ranges into `source`.
        todo!()
    }
}
```

The contract is short and strict: every `Start` is matched by an `End` of the
same kind and properly nested, ranges are byte ranges into `source` that are
non-decreasing in start order and land on character boundaries, and a
container's range covers its delimiters as well as its content -- the layer
above reads markers back out of the source, so an emphasis node's `marker`
attribute is the `*` or `_` at the start of its span.

A CI job builds and tests the crate with the feature off, so this shape is
supported rather than tolerated.

## What the crate will not do for you

No I/O, no configuration file, no concept of a theme, a template or a plugin.
Two responsibilities in particular are deliberately left outside:

**Where a schema comes from.** You build a `Config`. Whether the schemas in it
came from a constant, a YAML file, a database or a sandboxed guest is not the
crate's business, and no trait pretends to abstract it.

**HTML policy.** `render::render_all` is a convenience that emits upstream's
markup. A host that wants different elements, different escaping or a template
engine walks the renderable tree itself -- it is a plain tree of tags and
scalars, which is exactly what `transform` returns for that purpose.

## Reference

The generated API documentation is on
[docs.rs/accent-proust](https://docs.rs/accent-proust). Start at the crate root:
the module docs there state the conventions the whole surface commits to,
including why every public enum is `#[non_exhaustive]` and what panic-freedom
means for values you build yourself.
