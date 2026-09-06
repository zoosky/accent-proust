#!/usr/bin/env bash
#
# Build the project website in `site/`.
#
# Two artifacts go into one directory. The pages come from Accent CMS, the
# sibling static site generator this crate ships alongside; the playground's
# engine comes from `scripts/build-npm.sh`, which is the same WebAssembly
# package that goes to npm. Building the engine here rather than committing it
# means the playground can never be a stale copy of a release: it is whatever
# the working tree compiles to.
#
# Usage:
#   ./scripts/build-site.sh                 # build into site/output
#   ./scripts/build-site.sh --serve         # build, then serve it on 4450
#   ./scripts/build-site.sh --skip-wasm     # reuse the engine already staged
#
#   BASE_URL=...   the deployed URL, which also sets the path prefix
#                  (default: the `site.url` in site/config.yaml)
#   ACCENT=...     the Accent binary to use (default: `accent` on PATH)
#   PORT=...       the port `--serve` listens on (default: 4450)

set -uo pipefail

SERVE=0
SKIP_WASM=0
for arg in "$@"; do
  case "$arg" in
    --serve) SERVE=1 ;;
    --skip-wasm) SKIP_WASM=1 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

SITE=$ROOT/site
PKG=$ROOT/crates/accent-proust-wasm/pkg
# The theme's assets directory, not `site/media`. Accent copies both verbatim,
# and this one keeps the engine next to the script that imports it -- which is
# what lets `playground.js` reach it with a relative specifier and stay correct
# under a deployment path prefix it knows nothing about.
STAGE=$SITE/themes/proust/assets/wasm
ACCENT=${ACCENT:-accent}
# Matches `server.port` in site/config.yaml, and deliberately not 4400 or 4440:
# those belong to sibling Accent sites that may be running at the same time.
PORT=${PORT:-4450}

die() { echo "error: $*" >&2; exit 1; }
step() { printf '\n=== %s\n' "$*"; }

command -v "$ACCENT" >/dev/null \
  || die "the Accent CMS binary is not on PATH.
  Install it from https://github.com/AccentCMS/accent/releases, or set ACCENT=/path/to/accent"

# --- The engine --------------------------------------------------------------

if [ "$SKIP_WASM" -eq 1 ]; then
  step "Reusing the staged engine"
  [ -f "$STAGE/accent_proust_wasm_bg.wasm" ] \
    || die "--skip-wasm was passed but $STAGE holds no engine; run without it once"
else
  step "Building the WebAssembly engine"
  ./scripts/build-npm.sh >/dev/null || die "scripts/build-npm.sh failed (run it directly for the report)"
fi

if [ "$SKIP_WASM" -eq 0 ]; then
  step "Staging the engine into the theme"
  rm -rf "$STAGE"
  mkdir -p "$STAGE"
  # Only the three files the browser actually loads. The package's README,
  # LICENSE and package.json are npm's business and would be published as dead
  # URLs under /theme/assets/wasm/.
  for file in accent_proust_wasm.js accent_proust_wasm_bg.wasm accent_proust_wasm.d.ts; do
    [ -f "$PKG/$file" ] || die "$PKG/$file is missing; did scripts/build-npm.sh change its output?"
    cp "$PKG/$file" "$STAGE/$file"
  done
  printf '  %s\n' "$(du -h "$STAGE/accent_proust_wasm_bg.wasm" | cut -f1) engine staged"
fi

# --- The pages ---------------------------------------------------------------

step "Building the site"

cd "$SITE"

# No flags in the common case: `site.url` in config.yaml is the GitHub Pages
# project URL, and its path component is what makes every internal link start
# with `/accent-proust`. The default build is therefore already the artifact
# Pages wants, and `accent serve-static` reads the same config and mounts the
# preview under the same prefix -- so a local preview exercises the deployed
# URLs rather than an approximation of them.
if [ -n "${BASE_URL:-}" ]; then
  "$ACCENT" build --clean --base-url "$BASE_URL" || die "accent build failed"
else
  "$ACCENT" build --clean || die "accent build failed"
fi

# Tell GitHub Pages not to run the output through Jekyll. Belt and braces: a
# Pages site published from an Actions artifact is served as uploaded, so
# nothing should process this. But Jekyll drops every path beginning with an
# underscore, and the search index lives at `_search/` -- so if Jekyll ever
# does run over this directory, the failure is a search box that silently
# returns nothing, which is the kind of bug that survives a release.
#
# It does not fix a legacy Pages build. That one builds the repository root,
# not this directory, and the fix for it is to set the Pages source to GitHub
# Actions -- see site/README.md.
touch output/.nojekyll

# The engine is the one asset whose absence is invisible until someone opens the
# playground and reads a console. Assert it landed.
[ -f output/theme/assets/wasm/accent_proust_wasm_bg.wasm ] \
  || die "the build produced no engine under output/theme/assets/wasm/"

step "Done"
echo "  output: $SITE/output"

if [ "$SERVE" -eq 1 ]; then
  step "Serving"
  echo "  http://127.0.0.1:$PORT/accent-proust/"
  echo
  echo "  The playground needs a real server: it is an ES module and init()"
  echo "  fetches the .wasm, and a browser refuses both over file:."
  # `serve-static` serves the built artifact, which is what is being checked --
  # `accent serve` would re-render from content and could hide a build problem.
  # Plain HTTP: TLS here means a self-signed certificate and an interstitial on
  # every fresh browser profile, which buys nothing on a loopback preview.
  exec "$ACCENT" serve-static --dir output --port "$PORT" --no-tls
fi
