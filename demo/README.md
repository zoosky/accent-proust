# The browser demo

`accent-proust` compiled to WebAssembly, with a source pane, a rendered pane,
and a stylesheet you can swap underneath the output.

```sh
./scripts/serve-demo.sh          # builds the package, serves on 8000
```

Then open <http://127.0.0.1:8000/demo/>. A static server is required: the demo
is an ES module and `init()` fetches the `.wasm`, and a browser refuses both
over `file:`.

## What it is for

Two things, in this order.

**Verification.** It exercises all four entry points -- `renderHtml`,
`validate`, `transform` and `format` -- in a real browser, on whatever you
type. The Node tests in `crates/accent-proust-wasm/tests/` prove the bindings
work; this proves they work where they are meant to run.

**A starting point.** The engine has no opinion about markup or styling: it
emits plain HTML, so a design system is a stylesheet swap and nothing more.
`THEMES` in `demo.js` is a list of `{ id, href, wrap, pad }`, and adding one is
adding an entry. It ships with the U.S. Web Design System and with nothing at
all, which are the two ends of that range.

## How the output is isolated

The rendered document goes into an iframe rather than into the page. A full
design system is half a megabyte of CSS that styles `h1` and `button`, so
loading it into the demo would restyle the demo's own chrome -- and the
`usa-prose` wrapper USWDS expects around content it did not author only makes
sense on a document of its own.

The iframe is `sandbox="allow-same-origin"`, which blocks scripts. That is
belt and braces: Markdoc runs with HTML disabled, so a `<script>` in the source
is text by the time it reaches here. A bare `sandbox` would be stricter still,
and does not work -- it gives the frame an opaque origin and Chrome renders
`srcdoc` as a blank page.

## What it does not do

There is no schema configuration, so the tags are Markdoc's built-ins only.
A document using a host's own components reports `tag-undefined` for each of
them, correctly. Host schemas are the next thing the bindings need.
