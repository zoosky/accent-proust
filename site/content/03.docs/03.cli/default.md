---
title: Command line
template: docs
lead: >-
  The same engine as a command: format Markdoc, validate it against a schema
  file, and render it, with exit codes a CI pipeline can act on.
menu:
  visible: true
  order: 3
description: >-
  The accent-proust command: fmt, validate, render, transform and parse, a
  YAML or JSON schema file shared with the JavaScript bindings, partials from
  a directory, typed variables, and the exit codes.
---

The library reads no files and decides no policy; a host does. `accent-proust`
is the host for a shell -- the command that makes a documentation repository
a CI gate, and the first place `{% partial %}` works without writing a Rust
program.

## Install

The crate is a workspace member, built from the repository:

```sh
cargo install --path crates/accent-proust-cli
accent-proust --help
```

## Commands

| Command | Does | Needs a schema file |
|---|---|---|
| `fmt` | Reprints Markdoc source in canonical form | No |
| `validate` | Reports what a schema says is wrong | For tags of your own |
| `render` | Prints HTML | For tags of your own |
| `transform` | Prints the renderable tree as JSON | For tags of your own |
| `parse` | Prints the syntax tree as JSON | No |

Every command reads the files named on its command line, or stdin when none
is named. A file that cannot be read is reported and the run goes on, so one
bad path does not hide the rest; the exit code says a path failed.

### `fmt`

```sh
accent-proust fmt docs/*.md            # formatted source to stdout
accent-proust fmt --check docs/*.md    # a unified diff per file that would change
accent-proust fmt --write docs/*.md    # rewrite in place
cat page.md | accent-proust fmt        # stdin to stdout
```

Spacing inside a tag is normalised; your own spellings are left alone, so
`__bold__` stays `__bold__`. `parse(format(ast))` returns the same tree, so
formatting loses nothing, and `fmt` reformats its own output until it stops
changing, so write-then-check is clean by construction. `--check` prints a
diff rather than a list, because a CI log reader wants to know what is wrong,
not only where. Line endings are written as LF; a CRLF file is named by
`--check` and rewritten by `--write`.

### `validate`

```sh
accent-proust validate --config schema.yaml docs/*.md
accent-proust validate --config schema.yaml --format json docs/page.md
```

One line per error, `path:line:column: level[id]: message`, lines and columns
counted from one and the column in characters, as an editor shows it. The
error ids are upstream Markdoc's, so tooling written against its codes reads
them unchanged.

`--format json` prints one object per input, one per line, carrying `file` and
its `errors` in the shape the [JavaScript bindings](/docs/javascript) return,
positions included -- `character` and `offset` in UTF-16 code units,
`byteOffset` in bytes -- so a consumer written against either host reads the
other.

Exit 1 means an error at level `error` or `critical`. A `warning`, `info` or
`debug` is printed and does not fail the run: that is how a schema ships a
rule it wants surfaced but not enforced yet.

### `render`, `transform` and `parse`

`render` prints HTML, inputs concatenated in order. `transform` prints the
renderable tree as JSON, one array per input, one per line, in the shape
upstream's renderers expect -- a tag is `{"$$mdtype": "Tag", "name",
"attributes", "children"}`. `parse` prints the syntax tree the same way,
every node in the field order of upstream's `Node` class, so the output is
what `JSON.stringify(Markdoc.parse(source))` gives. One value per line rather
than one array for the run, because that composes with `jq`.

## The schema file

```sh
accent-proust render \
  --config schema.yaml \
  --partials docs/partials \
  --var version=3 --var channel=stable \
  docs/page.md
```

`--config` is a YAML or JSON file declaring `tags`, `nodes` and `variables`,
in the same vocabulary the JavaScript bindings' `Config` reads. A schema file
written for one host is read by the other unchanged:

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

An unknown key is refused with the path to it -- `config.tags.callout.validate`,
not "invalid schema". A hook cannot be written in a file: `transform` and
`validate` are code, and the refusal says to keep the hook in a Rust host.

> [!WARNING]
> **The command line is never stricter than a Rust host, only more
> convenient.** It sees what a tag declares and never a hook-level check. A
> schema whose real enforcement lives in a hook passes here and fails there.

`--partials DIR` reads every UTF-8 text file under the directory, at any depth,
and `{% partial file="sections/intro.md" /%}` finds it by that path. This is
the thing the browser cannot do, and the reason a command-line host exists.

`--var NAME=VALUE` declares a variable and overrides one the file declared.
`VALUE` is read as YAML by the same reader as the file, so the two can never
disagree about what `3` is:

| Written | Becomes |
|---|---|
| `--var count=3` | the number 3 |
| `--var debug=true` | the boolean true |
| `--var name=production` | the string `production` |
| `--var 'version="3"'` | the string `3` |
| `--var 'tags=[a, b]'` | a list of two strings |

## Exit codes

| Code | Means |
|---|---|
| 0 | Success; for `fmt --check`, nothing would change |
| 1 | A document has a problem: a file that would change, an error at level `error` or `critical` |
| 2 | A usage error, a file that could not be read or written, or a schema file that does not declare |

1 and 2 are kept apart so that CI can tell "the docs are wrong" from "the tool
is misconfigured". They are different alerts.

## Reference

The crate's [README](https://github.com/zoosky/accent-proust/blob/main/crates/accent-proust-cli/README.md)
is the full reference, and `accent-proust <command> --help` prints every flag.
