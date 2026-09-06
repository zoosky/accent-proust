# The project website

<https://zoosky.github.io/accent-proust>

The landing page, the documentation, and a playground that runs the engine in
your browser. Built with [Accent CMS](https://accentcms.dev) -- the sibling
project this crate ships alongside -- so the pages are rendered by one half of
the family and the playground by the other.

```sh
./scripts/build-site.sh --serve     # build everything, serve on 4450
```

Then open <http://127.0.0.1:4450/accent-proust/>. The path prefix is not a
mistake: `site.url` in `config.yaml` is the GitHub Pages project URL, and
`accent serve-static` mounts the preview under the same prefix the deployment
uses, so a local check exercises the deployed URLs rather than an approximation
of them.

A static server is required. The playground is an ES module and `init()`
fetches the `.wasm`; a browser refuses both over `file:`.

## What you need

| Tool | Why | Get it |
|---|---|---|
| `accent` | Renders the pages | [AccentCMS/accent releases](https://github.com/AccentCMS/accent/releases) |
| `wasm-bindgen` | Generates the playground's JavaScript glue | `cargo install wasm-bindgen-cli --version <the one in Cargo.lock> --locked` |
| The `wasm32-unknown-unknown` target | Compiles the engine | `rustup target add wasm32-unknown-unknown` |

Nothing in `Cargo.toml` knows this directory exists. The crate has no build
dependency on the CMS, `scripts/check-standalone.sh` is unaffected, and a
contributor who never touches the site never installs any of the above.

## Layout

| Path | What |
|---|---|
| `config.yaml` | Site configuration. The `site.url` path component is what prefixes every internal link |
| `content/` | The pages, as Markdown with frontmatter. Directory order (`01.`, `02.`) is menu order |
| `content/default.md` | The landing page, at the site root rather than behind a redirect |
| `themes/proust/` | The theme: templates, stylesheet, and the playground script |
| `themes/proust/assets/wasm/` | **Generated.** The engine, staged here by `scripts/build-site.sh` |
| `output/` | **Generated.** What gets deployed |

The last two are gitignored. The engine is rebuilt from
`crates/accent-proust-wasm` on every site build rather than committed, for the
same reason the npm package is not committed: a checked-in binary is a copy
that can disagree with the source beside it.

## Adding a page

Create a directory under `content/` with a `default.md`:

```markdown
---
title: Something new
template: docs
lead: One sentence under the title.
menu:
  visible: true
  order: 6
---

Body.
```

A page under `content/03.docs/` appears in the documentation sidebar with no
template change -- the sidebar is filtered out of the global page list, in
`menu.order` sequence. Set `menu.visible: false` to keep a page off both the
sidebar and the header.

## Design

Colour comes from the Accent design system: Electric Coral `#FF4F3E`, Neon Cyan
`#00D4FF`, Warm Gold `#FFB347`, Mint `#34D399`, Void `#08090D`, Snow `#F5F5F7`.
Dark is the authored direction and light is derived from it, both driven by one
set of custom properties in `themes/proust/assets/css/main.scss`.

The favicon set in `themes/proust/assets/` is the Accent mark, copied from
`accentcms/site-brand/themes/accent-ds/assets/`. Copied rather than shared
because the two repositories have no build-time link; if the mark changes, copy
it again.

## Deployment

`.github/workflows/pages.yml` builds and publishes on a push to `main` that
touches the site, the engine, or the scripts that assemble them. It downloads a
pinned `accent` release and verifies its checksum rather than building the CMS
from source.

Pages must be enabled once, by hand: **Settings -> Pages -> Source: GitHub
Actions**.
