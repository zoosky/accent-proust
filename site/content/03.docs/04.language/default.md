---
title: The Markdoc language
template: docs
lead: >-
  CommonMark plus a tag syntax. What the syntax is, what this implementation
  ships built in, and where the language stops and your schema starts.
menu:
  visible: true
  order: 4
description: >-
  A reference for the Markdoc language as accent-proust implements it: tags,
  annotations, attributes, variables, functions, partials, and the built-in
  if, else, table, partial and slot tags.
---

Markdoc is CommonMark with one addition: a tag syntax delimited by `{% %}`.
Everything CommonMark does, Markdoc does. Everything else on this page is the
addition.

The canonical language reference is [markdoc.dev](https://markdoc.dev/docs).
This page says what the syntax is and what *this* implementation ships, which
is the part a reference for the TypeScript original cannot tell you.

## Tags

A **block tag** stands on its own lines and wraps content:

```markdown
{% callout type="note" %}
Tags nest, and their content is Markdoc all the way down.
{% /callout %}
```

An **inline tag** sits inside a paragraph:

```markdown
Press {% kbd key="Ctrl-S" /%} to save.
```

A tag that wraps nothing closes itself with `/%}`. A tag that wraps content
needs a matching `{% /name %}`.

Tag names are not built in. `callout` above means nothing until a schema says
what it renders to and what attributes it takes -- until then it reports
`tag-undefined`, which is the language working, not failing. See
[Rust](/docs/rust#define-a-tag) or [JavaScript](/docs/javascript#your-own-tags)
for how to declare one.

## Annotations

An annotation attaches attributes to the CommonMark node it follows, without
introducing an element of its own:

```markdown
# Getting started {% #install .lead %}

A paragraph with an id. {% #intro %}
```

`{% #install %}` sets `id`, and `{% .lead %}` appends to `class`. Both are
shorthands over the same attribute syntax: `{% id="install" %}` is the long
form.

> [!WARNING]
> Heading attributes in this implementation are CommonMark's `{#id}` syntax, and
> Markdoc annotations are not ported for headings. This is a declared divergence
> -- see [Divergences](/docs/divergences).

## Attributes

Attributes are typed, and the type is checked against the schema at validate
time.

```markdown
{% callout
   type="warning"
   level=3
   dismissible=true
   tags=["a", "b"]
   meta={ author: "kim", draft: false } %}
```

| Type | Written as |
|---|---|
| String | `"double quoted"` |
| Number | `42`, `3.14`, `-1` |
| Boolean | `true`, `false` |
| Null | `null` |
| Array | `[1, "two", true]` |
| Object | `{ key: "value", nested: { a: 1 } }` |

A map keeps the order it was authored in, not the order a hash table would
produce, so rendered output is byte-reproducible across runs.

Nested values are depth-limited rather than unbounded; a document is untrusted
input and an arbitrarily deep literal is a denial-of-service shape, not a
document. Transform and format are limited the same way.

## Variables

`$name` refers to a value the host supplies:

```markdown
{% if $flags.beta %}
Welcome to the beta, {% $user.name %}.
{% /if %}

Path segments work with dots and brackets: {% $items[0].title %}
```

Variables resolve during **transform**, not during parse, and what they resolve
against is host data. If the host declares no variables at all, variable
checking is switched off rather than reporting every reference as undefined --
a tool that does not know the host's variables should say nothing rather than
say everything is wrong.

## Functions

A function call is `name(args)` and appears anywhere a value does:

```markdown
{% if and($flags.beta, not($user.optedOut)) %}
{% $title | default("Untitled") %}
```

Six functions are built in:

| Function | Does |
|---|---|
| `and(...)` | Logical and. No arguments is `true` |
| `or(...)` | Logical or. No arguments is `false` |
| `not(x)` | Logical negation |
| `equals(...)` | All arguments equal. No arguments is `true` |
| `default(value, fallback)` | `fallback` when `value` is undefined or null |
| `debug(x)` | The value, rendered for inspection |

A host can register more. In Rust a `ConfigFunction` declares its parameters so
that the validator can check a call before anything runs. Host-defined functions
do not cross into the JavaScript bindings, because a function is code.

## Built-in tags

Five tags ship with the language and are registered by
`builtins::config()`.

### `{% if %}` and `{% else %}`

```markdown
{% if $user.admin %}
Administrator tools.
{% else /%}
Ask an administrator.
{% /if %}
```

### `{% table %}`

Turns a list structure into a table, for tables whose cells contain block
content and therefore cannot be written with pipes.

### `{% partial %}`

```markdown
{% partial file="header.md" variables={ title: "Home" } /%}
```

The host reads the file and parses it; the crate does no I/O. The parsed
document goes into the configuration under the name the tag uses. Partials are
**not** available through the JavaScript bindings yet.

### `{% slot %}`

Names a region a parent tag fills.

## Comments

```markdown
{% comment %}
Not rendered, and not in the output at all.
{% /comment %}
```

## Fences

A fenced code block is a `<pre>` with a `data-language` attribute and, by
upstream's own design, **no `<code>` element inside**. That is worth knowing
before you write CSS for it -- it is upstream's shape rather than a liberty
this port takes, and `data-language` is the hook a highlighter or a design
system's code component attaches to.

Tags inside a fence are not processed by default. That is a declared divergence
from upstream, whose default is the other way; see
[Divergences](/docs/divergences).

## Node types

The AST node types this implementation knows, for anyone writing a schema
against `nodes` rather than `tags`:

`document`, `heading`, `paragraph`, `blockquote`, `list`, `item`, `fence`,
`code`, `text`, `strong`, `em`, `link`, `image`, `hr`, `table`, `thead`,
`tbody`, `tr`, `th`, `td`, `inline`, `softbreak`, `hardbreak`, `comment`,
`node`, `tag`, `error`.

Every public enum in the Rust API is `#[non_exhaustive]`, because Markdoc gained
node types across its own 0.5.x line and spelling them exhaustively would turn
each new one into a breaking release.

## Error ids

Validation produces ids, not prose, and the ids are upstream's:

| Id | Means |
|---|---|
| `tag-undefined` | No schema declares this tag |
| `node-undefined` | No schema declares this node type |
| `tag-placement-invalid` | The tag is not allowed where it appears |
| `tag-selfclosing-has-children` | A self-closing tag was given content |
| `attribute-undefined` | The tag's schema does not declare this attribute |
| `attribute-missing-required` | A required attribute was not given |
| `attribute-type-invalid` | The value is not of the declared type |
| `attribute-value-invalid` | The value is not in the declared `matches` set |
| `child-invalid` | A child node is not allowed inside this tag |
| `slot-undefined` | The tag declares no slot of that name |
| `slot-missing-required` | A required slot was not filled |
| `variable-undefined` | No variable of that name, when variable checking is on |
| `function-undefined` | No function of that name |
| `parameter-undefined` | The function declares no parameter of that name |
| `parameter-missing-required` | A required function parameter was not given |
| `parameter-type-invalid` | A function argument is not of the declared type |

That is the complete set the validator emits, and each string is upstream's
unchanged. Each error also carries a `level` -- `debug`, `info`, `warning`,
`error` or `critical` -- which a schema can override per attribute through
`errorLevel`.

Try a document in the [playground](/playground)
to see what a given mistake actually reports.
