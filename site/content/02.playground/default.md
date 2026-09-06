---
title: Playground
template: playground
lead: >-
  The engine on this page is accent-proust compiled to WebAssembly. Edit the
  source and the preview, the diagnostics and the renderable tree update as you
  type.
menu:
  visible: true
  order: 2
description: >-
  An interactive Markdoc playground. Parse, validate, transform and render
  Markdoc in your browser with accent-proust, compiled to WebAssembly.
---

## What to try

The sample starts with one deliberate mistake. `{% callout %}` is not a
Markdoc built-in and this page passes no schema, so the **Diagnostics** tab has
an entry for it: `tag-undefined`, which is upstream's own error id. Delete the
tag and the tab empties.

A few other things worth doing here:

- **Switch the stylesheet.** The engine emits plain HTML with no classes of its
  own, so a design system is a stylesheet swap and nothing more. The preview
  frame loads the U.S. Web Design System, or nothing at all.
- **Press Format.** `format` prints the tree back as canonical Markdoc. Add
  stray spaces inside a tag and they normalise; leave `__bold__` alone and it
  stays `__bold__`, because your own spellings are yours.
- **Open the Tree tab.** That is what `transform` returns -- upstream's
  `RenderableTreeNode` shape, `$$mdtype` marker and all, which is what a React
  or Vue renderer maps onto components.
- **Paste something large.** The timing line under the panes measures the whole
  pipeline, not one stage of it.

## What the page does not do

It passes no schema configuration, so the tags are Markdoc's built-ins only.
That is why the undefined tag in the sample stays undefined -- it is the
clearest way to show what the diagnostics are for.

The bindings themselves do take one. `new Config({ tags, nodes, variables })`
builds a validator configuration from declared data and merges it over the
built-ins, and the stages become methods on it. See the
[JavaScript API](/docs/javascript) for the shape of that object and for the
four things which deliberately do not cross the boundary.

> [!NOTE]
> Nothing you type here leaves your browser. The page is static, the engine is a
> WebAssembly module fetched once, and there is no endpoint behind any of it.
