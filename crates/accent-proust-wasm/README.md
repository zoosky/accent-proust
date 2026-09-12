# accent-proust

[Markdoc](https://markdoc.dev) for the browser: parse, validate, transform,
render and format, compiled to WebAssembly from
[a Rust implementation](https://github.com/zoosky/accent-proust) of the
language.

Markdoc is CommonMark plus a tag syntax that turns documents into structured,
validatable content instead of pre-rendered HTML:

```markdown
{% callout type="note" %}
Tags nest, take typed attributes, and are validated against a schema.
{% /callout %}
```

## Install

```sh
npm install accent-proust
```

## Use

The module is ESM and initialises once, before any other call.

### In a browser

`init()` with no argument resolves the `.wasm` beside the JavaScript, so Vite,
native `<script type="module">` and a CDN all work unchanged.

```js
import init, { validate, renderHtml, transform, format } from "accent-proust";

await init();

const source = "# Title {% #intro %}\n";

renderHtml(source); // '<article><h1 id="intro">Title </h1></article>'
validate(source);   // []
format(source);     // '# Title {% #intro %}\n'
transform(source);  // the renderable tree, below
```

Pass an explicit location when your bundler does not rewrite the default one:

```js
import init from "accent-proust";
import wasm from "accent-proust/accent_proust_wasm_bg.wasm?url";

await init({ module_or_path: wasm });
```

### In Node

**`init()` with no argument does not work in Node.** The default location is a
`file:` URL, and Node's `fetch` rejects those — you get `TypeError: fetch
failed`. Read the file and hand it over:

```js
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import init, { renderHtml } from "accent-proust";

const wasm = fileURLToPath(
  import.meta.resolve("accent-proust/accent_proust_wasm_bg.wasm")
);
await init({ module_or_path: readFileSync(wasm) });

renderHtml("# Title\n"); // '<article><h1>Title</h1></article>'
```

This package ships the browser build only. A `nodejs` build, which would make
`init()` work unaided, is additive and not here yet.

## The four entry points

Each takes the document source and parses from scratch. Nothing is cached
between calls and no value holds a reference into WebAssembly memory, so there
is nothing to free.

### `renderHtml(source): string`

The whole pipeline. A document with validation errors still renders, because a
preview pane that blanks on the first mistake is worse than one that shows it.

### `validate(source): ValidateError[]`

Diagnostics, empty when the document is clean. The `error.id` is Markdoc's own
id unchanged — it is what external tooling binds to.

```js
validate("{% callout %}\n{% /callout %}\n");
// [{
//   type: "tag",
//   lines: [0, 1, 1, 2],
//   location: { start: {...}, end: {...} },
//   error: {
//     id: "tag-undefined",
//     level: "critical",
//     message: "Undefined tag: 'callout'"
//   }
// }]
```

Positions are counted in **UTF-16 code units** -- what JavaScript means by a
string index, and what a CodeMirror position, a Monaco position and an LSP
`character` all are. The engine measures UTF-8 bytes internally and the
conversion happens before the value crosses the boundary, so a position can go
straight into an editor with no arithmetic:

```js
const { start, end } = validate(source)[0].location;
view.dispatch({ selection: { anchor: start.offset, head: end.offset } });
```

Each edge carries four fields. `line` is zero-based. `character` is code units
from the start of the line. `offset` is code units from the start of the
document, so it is usable as a position directly. `byteOffset` is the engine's
own unit, for a host that wants to index back into the source as bytes; it is
the only field that is not code units, and it is named so that it cannot be
mistaken for one.

### `transform(source): RenderableTreeNode[]`

The renderable tree, for a host that owns its own markup. A tag is an object
carrying `$$mdtype: "Tag"`, exactly as `@markdoc/markdoc` produces, so a React
or Vue renderer written against Markdoc maps tag names onto components without
changes:

```js
transform("# Title {% #intro %}\n");
// [{ $$mdtype: "Tag", name: "article", attributes: {}, children:
//   [{ $$mdtype: "Tag", name: "h1", attributes: { id: "intro" },
//      children: ["Title "] }] }]
```

Anything that is not a tag is the scalar it renders to; a text node is a plain
string.

### `format(source): string`

Canonical Markdoc source. `format(format(s)) === format(s)`, and a formatted
document parses to the document the original parsed to, so an editor can
rewrite a buffer in place without losing anything.

## Your own tags

The four functions above use Markdoc's built-in configuration, so a tag your
host defines reports `tag-undefined`. Build a `Config` once and call the same
stages on it:

```js
import init, { Config } from "accent-proust";
await init();

const config = new Config({
  tags: {
    callout: {
      render: "Callout",
      attributes: {
        type: { type: "String", default: "note", matches: ["note", "warning"] },
        title: { type: "String", required: true },
      },
    },
  },
  variables: { flags: { beta: true } },
});

config.validate(source);   // knows `callout`, checks its attributes
config.renderHtml(source);
config.transform(source);
```

Declarations merge over the built-ins, so `{% if %}` and `{% partial %}` keep
working. Construction does the parsing and checking once; the methods then cost
what the free functions cost, which is what an editor revalidating on every
keystroke needs. Call `config.free()` when you are done with it.

`nodes` takes the same shape as `tags`, keyed by Markdoc node type, so you can
change what a `heading` renders as.

The vocabulary -- which keys a declaration may carry, and what each means -- is
[`accent-proust-schema-config`](https://github.com/zoosky/accent-proust/tree/main/crates/accent-proust-schema-config),
shared with the [command line](https://github.com/zoosky/accent-proust/tree/main/crates/accent-proust-cli).
This host reads it from an object and the command line from a YAML or JSON
file, so a schema declared for one is accepted by the other: a JSON file the
command line reads is what `JSON.parse` hands `new Config` here.

### What crosses, and what does not

A schema is data and crosses whole: `render`, `children`, `attributes`,
`slots`, `selfClosing`, `inline`, `description`, and on an attribute `type`,
`default`, `required`, `matches`, `render`, `errorLevel`.

Attribute types are written as the strings `"String"`, `"Number"`, `"Boolean"`,
`"Object"`, `"Array"`. Markdoc uses the JavaScript constructors, and a
constructor is a function. An array of them is a union, one level deep: a
list inside the list is refused.

A schema object is read key by key, so a class instance or a proxy works as
well as an object literal. Only a value carried through whole -- an attribute's
`default`, a variable -- has to be a plain object, because a `Date` or a `Map`
would flatten to `{}` on the way. An explicitly `undefined` variable is
`null`, which is what `user: session?.user` means by it.

A hook is code, and code does not cross: `transform`, `validate`, a custom
attribute type, a `RegExp` in `matches`, and host-defined `functions`. **So the
browser is never stricter than your server, only faster.** Keep the server as
the authority; this is an early warning.

Nothing is dropped in silence. A configuration carrying something that cannot
cross is refused, naming the path to it:

```
config.tags.callout.attributes.type.type: unknown attribute type "Str";
expected String, Number, Boolean, Object, Array, or an array of those
```

A schema that half arrives is worse than one that does not, because the missing
half is invisible until an author trips over it. For the same reason a property
whose getter throws is refused as unreadable rather than read as absent: a
block that was written and then lost is that failure in another form.

## Not supported yet

Partials, and host-defined functions. A parsed partial borrows its source, so
holding both across the boundary needs a design this does not have. Both are
refused by name rather than ignored.

`parse` is not exposed either: the abstract syntax tree has no JavaScript shape
here yet. `validate` and `transform` cover what a preview and a custom renderer
need.

## Differences from `@markdoc/markdoc`

The tag language and the validation error ids are the contract and are
reproduced exactly. CommonMark edge behaviour is not: Markdoc is built on
markdown-it and this is built on pulldown-cmark. Every deliberate difference is
recorded in [`DIVERGENCES.md`](https://github.com/zoosky/accent-proust/blob/main/DIVERGENCES.md).

## License

MIT
