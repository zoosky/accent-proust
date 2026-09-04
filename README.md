# accent-proust

A Rust implementation of the [Markdoc](https://markdoc.dev) language: parse,
validate, transform, render, and format.

Markdoc is CommonMark plus a tag syntax that turns documents into structured,
validatable content instead of pre-rendered HTML:

```markdown
{% callout type="note" %}
Tags nest, take typed attributes, and are checked against a schema.
{% /callout %}
```

## Install

```sh
cargo add accent-proust
```

## Render a document

```rust
use accent_proust::{builtins, parse, render, transform};

let document = parse::parse("# Title\n\nSome *text*.\n");
let tree = transform::transform(&document, &builtins::config());

assert_eq!(
    render::render_all(&tree.into_vec()),
    "<article><h1>Title</h1><p>Some <em>text</em>.</p></article>"
);
```

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

Validation errors are data, not failures: you get a `Vec`, so an editor shows
every problem at once instead of the first one.

```rust
let document = parse::parse("{% callout %}\nBody\n{% /callout %}\n");

for error in validate::validate_tree(&document, &config) {
    println!("{}: {}", error.error.id, error.error.message);
    // attribute-missing-required: Missing required attribute: 'type'
}
```

Error ids match upstream Markdoc exactly, so tooling written against its codes
works unchanged.

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

`format(parse(s))` is idempotent, so a tool can rewrite a file in place, and
`parse(format(ast))` gives back the same tree, so formatting loses nothing.

## Bring your own CommonMark parser

The bundled tokenizer uses `pulldown-cmark`, behind the default
`pulldown-cmark-tokenizer` feature. Turn it off and implement `Tokenizer` if you
already parse CommonMark, or if you pin `pulldown-cmark` to a git revision --
Cargo treats that as a different package, so you would compile two CommonMark
parsers into one binary and render some documents through each.

```toml
accent-proust = { version = "*", default-features = false }
```

A CI job builds and tests the crate in exactly that shape, so it is supported
rather than tolerated. `Tokenizer` is one of three seams; the crate does no I/O,
reads no configuration, and decides no HTML policy. `SchemaSource` answers where
a schema comes from, and `TagRenderer` owns escaping and HTML policy. All three
are yours.

## Compatibility

Ported from upstream Markdoc `v0.5.9` (revision `afee1a4`). The tag language and
the error ids are the contract. CommonMark edge behaviour is not: upstream builds
on markdown-it, this crate on pulldown-cmark. Every deliberate difference is
declared in [`DIVERGENCES.md`](DIVERGENCES.md), never emulated silently.

Upstream's 105-case corpus is vendored and run as the test suite. Nothing fails;
"annotated" is a case exercising a declared divergence, counted apart so that
giving something up stays visible.

```sh
cargo test --test conformance -- --nocapture
# conformance: 95 green, 10 annotated, 0 failing (of 105)
```

## Contributing

The library's minimum supported Rust version is 1.82. Develop on stable, which
the test suite needs. See [AGENT.md](AGENT.md) for the gates and the workflow.

## Licence

MIT. See [LICENSE](LICENSE).

A compatible reimplementation derived from the MIT-licensed Markdoc source.

## The name

Marcel Proust composed *A la recherche du temps perdu* on strips of paper glued
into the manuscript to extend it. Those *paperoles* are exactly what a formatter
does: parse, mutate, and print canonical source.

`accent-proust` belongs to the family of [Accent CMS](https://accentcms.dev)
crates.
