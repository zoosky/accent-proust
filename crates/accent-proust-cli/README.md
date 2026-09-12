# accent-proust, the command

The command-line host for [accent-proust](https://github.com/zoosky/accent-proust),
a Rust implementation of the [Markdoc](https://markdoc.dev) language.

The library reads no files and decides no policy; a host does. This is the
second host beside the WebAssembly bindings, and it ships one command at a
time as `specs/features/cli-and-host-seams.md` sequences them.

## Build

```sh
cargo build -p accent-proust-cli --release
target/release/accent-proust --help
```

## `fmt`

Reprints Markdoc source in canonical form. Spacing inside a tag is normalised;
your own spellings are left alone, so `__bold__` stays `__bold__`.

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

## Exit codes

| Code | Means |
|---|---|
| 0 | Success; for `--check`, nothing would change |
| 1 | `--check` found a file that would change |
| 2 | A usage error, a file that could not be read or written, or a document the formatter does not settle on |

1 and 2 are kept apart so that CI can tell "the docs are wrong" from "the tool
is misconfigured". They are different alerts.
