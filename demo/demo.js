// The demo's whole job is to prove the engine works in a browser and to make
// swapping the stylesheet under it a one-click operation. Every call below is
// synchronous WebAssembly; there is no server and no network after load.

import init, { renderHtml, validate, transform, format } from "./pkg/accent_proust_wasm.js";

// Rendered output goes into a sandboxed iframe rather than into this document.
// Two reasons, and the second is the one that matters: a full design system is
// half a megabyte of CSS that styles `h1` and `button`, so loading it into the
// page would restyle the demo's own chrome; and a sandbox with no
// `allow-scripts` means nothing in the rendered document can execute, whatever
// a visitor pastes in.
//
// `id` is what the stylesheet select shows. `href` is loaded inside the iframe
// only. `wrap` is the class the design system expects around prose it did not
// generate itself.
const THEMES = [
  {
    id: "U.S. Web Design System 3.14.0",
    href: "https://cdn.jsdelivr.net/npm/@uswds/uswds@3.14.0/dist/css/uswds.min.css",
    // USWDS styles components by class and leaves bare HTML alone on purpose.
    // `usa-prose` is its own answer for content it did not author, which is
    // exactly what a rendered Markdoc document is.
    wrap: "usa-prose",
    pad: "2rem",
  },
  {
    id: "Unstyled (browser default)",
    href: null,
    wrap: "",
    pad: "1rem",
  },
];

const SAMPLE = `# Markdoc in the browser {% #top %}

This document is parsed, validated, transformed and rendered by
**accent-proust**, compiled to WebAssembly. Switch the stylesheet on the right
and the same HTML is restyled -- the engine does not know or care which design
system is loaded.

## What the engine does

- CommonMark, so *emphasis*, \`code\` and [links](https://markdoc.dev) work.
- Tags, which is the part CommonMark does not have.
- Annotations, like the \`{% #top %}\` on the heading above.

{% if true %}
Tags nest and take typed attributes. This paragraph is inside an \`if\`.
{% /if %}

| Stage | Runs where |
|---|---|
| parse | here |
| validate | here |
| transform | here |
| render | here |

\`\`\`rust
let document = accent_proust::parse::parse(source);
\`\`\`

## Diagnostics are the other half

The tag below is not defined, so the Diagnostics tab has one entry for it. The
error id there is Markdoc's own, unchanged, because that is what external
tooling binds to. Delete the line and the tab empties.

{% callout type="note" %}
An undefined tag renders as nothing and reports \`tag-undefined\`.
{% /callout %}
`;

const $ = (id) => document.getElementById(id);
const source = $("source");
const status = $("status");
const themeSelect = $("theme");
const preview = $("preview");
const timing = $("timing");

let ready = false;

// --- Rendering ---------------------------------------------------------------

function theme() {
  return THEMES[themeSelect.selectedIndex] ?? THEMES[0];
}

// The iframe is written with `srcdoc`, so the document is built as a string.
// Everything interpolated here is either a constant from THEMES or the
// engine's own escaped output.
function frame(html) {
  const t = theme();
  const link = t.href
    ? `<link rel="stylesheet" href="${t.href}">`
    : "";
  const open = t.wrap ? `<div class="${t.wrap}">` : "<div>";
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">${link}
<style>body{margin:0;padding:${t.pad}}</style></head>
<body>${open}${html}</div></body></html>`;
}

function renderDiagnostics(errors) {
  const count = $("diagnostic-count");
  count.textContent = String(errors.length);
  count.hidden = errors.length === 0;

  const panel = $("diagnostics");
  if (errors.length === 0) {
    panel.innerHTML = `<p class="ok">No problems found.</p>`;
    return;
  }
  // Built as DOM rather than as an HTML string: `message` is engine output and
  // carries the offending tag name, which came from the document.
  panel.replaceChildren(
    ...errors.map((e) => {
      const item = document.createElement("div");
      item.className = `diagnostic level-${e.error.level}`;

      const head = document.createElement("p");
      head.className = "diagnostic-head";
      const id = document.createElement("code");
      id.textContent = e.error.id;
      const where = document.createElement("span");
      where.className = "where";
      // Locations are zero-based; humans count lines from one.
      const line = e.location ? e.location.start.line + 1 : null;
      where.textContent = line === null ? e.type : `${e.type}, line ${line}`;
      head.append(id, where);

      const message = document.createElement("p");
      message.textContent = e.error.message;

      item.append(head, message);
      return item;
    })
  );
}

function run() {
  if (!ready) return;
  const text = source.value;
  const started = performance.now();

  const html = renderHtml(text);
  const errors = validate(text);
  const tree = transform(text);

  const elapsed = performance.now() - started;

  preview.srcdoc = frame(html);
  renderDiagnostics(errors);
  $("html").firstElementChild.textContent = html;
  $("tree").firstElementChild.textContent = JSON.stringify(tree, null, 2);

  const kb = (new Blob([text]).size / 1024).toFixed(1);
  // A warm run over a small document lands under the clock's resolution --
  // browsers coarsen `performance.now()` deliberately. Printing "0.00 ms" is
  // true and reads like a broken clock, so say what is actually known.
  const took = elapsed < 0.1 ? "under 0.1 ms" : `${elapsed.toFixed(1)} ms`;
  timing.textContent =
    `${kb} KB parsed, validated, transformed and rendered in ${took}`;
}

// --- Wiring ------------------------------------------------------------------

// One frame of debounce keeps typing smooth without making the preview feel
// detached. The engine is fast enough that a longer delay would only add lag.
let queued = 0;
function schedule() {
  cancelAnimationFrame(queued);
  queued = requestAnimationFrame(run);
}

for (const t of THEMES) {
  const option = document.createElement("option");
  option.textContent = t.id;
  themeSelect.append(option);
}

source.addEventListener("input", schedule);
themeSelect.addEventListener("change", run);

$("format").addEventListener("click", () => {
  const start = source.selectionStart;
  source.value = format(source.value);
  source.setSelectionRange(start, start);
  run();
});

for (const tab of document.querySelectorAll('[role="tab"]')) {
  tab.addEventListener("click", () => {
    for (const other of document.querySelectorAll('[role="tab"]')) {
      const on = other === tab;
      other.setAttribute("aria-selected", String(on));
      $(other.dataset.tab).hidden = !on;
    }
  });
}

// --- Start -------------------------------------------------------------------

try {
  await init();
  ready = true;
  source.value = SAMPLE;
  $("format").disabled = false;
  status.textContent = "Engine ready.";
  status.classList.add("ok");
  run();
} catch (error) {
  status.textContent =
    "The engine did not load. Build it first: ./scripts/build-npm.sh, then serve the repository root.";
  status.classList.add("bad");
  source.value = SAMPLE;
  source.disabled = true;
  // Keep the real error in the console; the line above is the actionable half.
  console.error(error);
}
