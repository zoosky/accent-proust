// The two pieces of page chrome that need behaviour: the theme toggle and the
// search overlay. Deferred, because neither is needed before first paint --
// the theme itself is applied by a small blocking script in the head, and this
// only handles the switching.
//
// No framework and no build step. Both features are a few dozen lines, and a
// dependency would be larger than the thing it delivered.

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

const root = document.documentElement;
const toggle = document.getElementById("theme-toggle");

// What the reader is looking at right now: their explicit choice if they made
// one, otherwise whatever the operating system says.
function currentTheme() {
  const chosen = root.getAttribute("data-theme");
  if (chosen === "light" || chosen === "dark") return chosen;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function labelToggle() {
  if (!toggle) return;
  const next = currentTheme() === "dark" ? "light" : "dark";
  toggle.setAttribute("aria-label", `Switch to ${next} theme`);
  toggle.title = `Switch to ${next} theme`;
}

if (toggle) {
  labelToggle();

  toggle.addEventListener("click", () => {
    const next = currentTheme() === "dark" ? "light" : "dark";
    root.setAttribute("data-theme", next);
    try {
      localStorage.setItem("theme", next);
    } catch (e) {
      // Site data blocked. The theme still applies for this page view; it just
      // will not survive a reload, which is a better outcome than throwing.
    }
    labelToggle();
  });
}

// Follow the system while the reader has expressed no preference of their own.
// Without this, a machine switching to night mode at sunset leaves a first-time
// visitor's page in the theme it was loaded with.
window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => {
  if (!root.hasAttribute("data-theme")) labelToggle();
});

// ---------------------------------------------------------------------------
// Search overlay
// ---------------------------------------------------------------------------

const overlay = document.getElementById("search-overlay");
const trigger = document.getElementById("search-trigger");

if (overlay && trigger) {
  const input = overlay.querySelector(".acms-search-input");
  const results = overlay.querySelector(".acms-search-results");
  const backdrop = document.getElementById("search-backdrop");
  let lastFocused = null;

  function open() {
    if (!overlay.hidden) return;
    lastFocused = document.activeElement;
    overlay.hidden = false;
    trigger.setAttribute("aria-expanded", "true");
    input.focus();
    input.select();
  }

  function close() {
    if (overlay.hidden) return;
    overlay.hidden = true;
    trigger.setAttribute("aria-expanded", "false");
    // Blur before restoring focus, and do it explicitly. Hiding an element
    // does not reliably move focus off a descendant within the same task, and
    // `body.focus()` is a no-op -- so without this the search input stays the
    // active element after the dialog closes, the `/` guard below sees an
    // INPUT and decides the reader is typing, and the shortcut opens the
    // dialog exactly once per page load.
    input.blur();
    if (lastFocused && lastFocused.focus && lastFocused !== document.body) {
      lastFocused.focus();
    }
  }

  trigger.addEventListener("click", open);
  backdrop.addEventListener("click", close);

  document.addEventListener("keydown", (event) => {
    // `/` and Cmd/Ctrl-K, the two shortcuts a reader is likely to try. `/` is
    // ignored while typing, or it would swallow a slash in a search box or a
    // textarea -- the playground's editor is one.
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName)
      || document.activeElement.isContentEditable;

    if (event.key === "/" && !typing && !event.metaKey && !event.ctrlKey) {
      event.preventDefault();
      open();
      return;
    }
    if (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      open();
      return;
    }
    if (overlay.hidden) return;

    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }

    // Arrow keys and Enter walk the result list. The search script owns the
    // list's contents; this owns only which row is current, tracked with a
    // class so the script is free to re-render underneath it.
    const items = [...results.querySelectorAll(".acms-search-result")];
    if (items.length === 0) return;
    const index = items.findIndex((item) => item.classList.contains("is-active"));

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = (index + step + items.length) % items.length;
      items.forEach((item) => item.classList.remove("is-active"));
      items[next].classList.add("is-active");
      items[next].scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter" && index >= 0) {
      event.preventDefault();
      items[index].click();
    }
  });

  // A fresh query invalidates the highlighted row.
  input.addEventListener("input", () => {
    results.querySelectorAll(".is-active").forEach((item) => item.classList.remove("is-active"));
  });

  // Clicking a result navigates; close first so returning via the back button
  // does not land on an open dialog.
  results.addEventListener("click", (event) => {
    if (event.target.closest(".acms-search-result")) close();
  });
}
