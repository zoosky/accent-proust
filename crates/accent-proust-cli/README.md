# accent-proust, the command

The command-line host for [accent-proust](https://github.com/zoosky/accent-proust),
a Rust implementation of the [Markdoc](https://markdoc.dev) language.

The library reads no files and decides no policy; a host does. This is the
second host beside the WebAssembly bindings, with one command per stage the
library exposes.

## Build

```sh
cargo build -p accent-proust-cli --release
target/release/accent-proust --help
```

## Commands

| Command | Does | Needs configuration |
|---|---|---|
| `fmt` | Reprints Markdoc source in canonical form | No |
| `validate` | Reports what a schema says is wrong | For tags of your own |
| `render` | Prints HTML | For tags of your own |
| `transform` | Prints the renderable tree as JSON | For tags of your own |
| `parse` | Prints the syntax tree as JSON | No |

Every command reads the files named on its command line, or stdin when none
is named. `validate`, `render`, `transform` and `parse` take `--file LABEL`
for what stdin is called in diagnostics; `fmt` has no diagnostics to label
it in. A file that cannot be read is reported and the run goes on, so one
bad path does not hide the rest.

### `fmt`

Spacing inside a tag is normalised; your own spellings are left alone, so
`__bold__` stays `__bold__`.

```sh
accent-proust fmt docs/*.md            # formatted source to stdout
accent-proust fmt --check docs/*.md    # a unified diff per file that would change
accent-proust fmt --write docs/*.md    # rewrite in place
cat page.md | accent-proust fmt        # stdin to stdout
```

`parse(format(ast))` returns the same tree, tested in the library, so
formatting loses nothing. `format(parse(s))` settles in one pass on every
document but one shape the library documents -- a paragraph beginning with a
fence marker re-parses as a fence -- so `fmt` reformats its own output until
it stops changing, and refuses with exit 2 if four passes are not enough. That
is the contract `--check` and `--write` need: write, then check, is clean.
`--check` writes nothing and prints a diff rather than a list, because a CI log
reader wants to know what is wrong, not only where.

`fmt` writes LF line endings. A CRLF file is reported by `--check` as changed,
by name rather than as a diff of every line, and rewritten with LF by
`--write`.

Two options, both defaulting to the library's:

| Flag | Meaning |
|---|---|
| `--max-tag-opening-width COLUMNS` | The width past which a block tag's opening breaks across lines. Default 80 |
| `--ordered-list-mode repeat\|increment` | Whether a numbered list reprints its numbers. Default `repeat` |

### `validate`

```sh
accent-proust validate --config schema.yaml docs/**/*.md
accent-proust validate --config schema.yaml --format json docs/page.md
```

One line per error, `path:line:column: level[id]: message`, lines and columns
counted from one and the column in characters, as an editor shows it.
`--format json` prints one object per input, one per line, carrying `file`
and its `errors` in the shape the WebAssembly bindings return, positions
included: `line`, `character` and `offset` in UTF-16 code units and
`byteOffset` in bytes, exactly as the bindings write them, so a consumer
written against either host reads the other. Error ids are upstream
Markdoc's, so tooling written against its codes reads either format
unchanged.

Exit 1 means an error at level `error` or `critical`. A `warning`, `info` or
`debug` is printed and does not fail the run: that is how a schema ships a
rule it wants surfaced but not enforced yet.

### `render`, `transform`, `parse`

`render` prints HTML, inputs concatenated in order. `transform` prints the
renderable tree as JSON, one array per input, one per line -- a tag is an
object carrying `$$mdtype: "Tag"`, `name`, `attributes` and `children`, in
that order, as upstream's renderers expect and as `JSON.stringify` writes
upstream's own. `parse` prints the syntax tree the same way, every node in
the field order of upstream's `Node` class, with numbers in ECMAScript's
spelling and `tag` and `location` omitted rather than `null` when a node has
none -- so the output is what `JSON.stringify(Markdoc.parse(source))` gives,
byte for byte, with positions in the bindings' shape. It reads no
configuration because parsing needs none.

## Configuration

```sh
accent-proust render \
  --config schema.yaml \
  --partials docs/partials \
  --var version=3 --var channel=stable \
  docs/page.md
```

`--config` is a YAML or JSON file -- one document; a second after `---` is
refused rather than dropped -- declaring `tags`, `nodes` and `variables`, in
the vocabulary [`accent-proust-schema-config`](../accent-proust-schema-config)
defines and the WebAssembly bindings read too:

```yaml
tags:
  callout:
    render: aside
    attributes:
      type:
        type: String
        required: true
        matches: [note, warning]
variables:
  channel: stable
```

An unknown key is refused with the path to it, `config.tags.callout.validate`
and not "invalid schema". A hook cannot be written in a file: `transform` and
`validate` are code, and the reason a configuration file is refused says to
keep the hook in a Rust host. **The command line sees what a tag declares and
never a hook-level check, so it is never stricter than a Rust host, only more
convenient.** A schema whose real enforcement lives in a hook passes here and
fails there.

`--partials DIR` reads every UTF-8 text file under the directory, at any
depth, and `{% partial file="sections/intro.md" /%}` finds it by that path.
A file that is not text -- an image beside the partials -- is passed over,
and a document that names it is told so where it does. A symbolic link to a
file is read; one to a directory is not followed. This is the thing the
browser cannot do, and the reason a command-line host exists.

`--var NAME=VALUE` declares a variable, and overrides one the file declared.
`VALUE` is read as YAML by the same reader as the file, so the two can never
disagree about what `3` is:

| Written | Becomes |
|---|---|
| `--var count=3` | the number 3 |
| `--var debug=true` | the boolean true |
| `--var name=production` | the string `production` |
| `--var missing=null`, `--var missing=` | null |
| `--var 'version="3"'` | the string `3` |
| `--var 'tags=[a, b]'` | a list of two strings |

## Exit codes

| Code | Means |
|---|---|
| 0 | Success; for `--check`, nothing would change |
| 1 | A document has a problem: `fmt --check` found a file that would change, `validate` found an error |
| 2 | A usage error, a file that could not be read or written, a configuration that does not declare, or a document the formatter does not settle on |

1 and 2 are kept apart so that CI can tell "the docs are wrong" from "the tool
is misconfigured". They are different alerts.
