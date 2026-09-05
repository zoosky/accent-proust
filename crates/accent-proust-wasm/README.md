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

## Not supported yet

Variables, partials and host-defined tag schemas. Every call uses Markdoc's
built-in configuration: its default nodes, tags and functions. Substitute
variables before calling if you need them.

`parse` is not exposed either — the abstract syntax tree has no JavaScript
shape here yet. `validate` and `transform` cover what a preview and a custom
renderer need.

## Differences from `@markdoc/markdoc`

The tag language and the validation error ids are the contract and are
reproduced exactly. CommonMark edge behaviour is not: Markdoc is built on
markdown-it and this is built on pulldown-cmark. Every deliberate difference is
recorded in [`DIVERGENCES.md`](https://github.com/zoosky/accent-proust/blob/main/DIVERGENCES.md).

## License

MIT
