#!/usr/bin/env bash
# Build the npm package for the WebAssembly bindings.
#
# The artifact is generated, never committed: `crates/accent-proust-wasm/pkg`
# is ignored, and this script is the only thing that writes it. Publishing is
# `npm publish` from that directory, which `--pack` stops short of.
#
# Usage:
#   ./scripts/build-npm.sh            # build into crates/accent-proust-wasm/pkg
#   ./scripts/build-npm.sh --pack     # also run `npm pack --dry-run`
#
#   OUT=dir                           # write somewhere else
#
# Requires `wasm-bindgen`, whose version must match the `wasm-bindgen` the
# crate compiles against; the script checks that rather than letting a mismatch
# surface as broken glue at run time.
#
# There is no wasm-opt step, which is a measured decision rather than an
# omission. Over this artifact `wasm-opt -O3` takes 554,850 bytes to 526,162,
# and 213,250 gzipped to 213,077 -- five percent off the raw file and nothing
# off the download. `lto` and `codegen-units = 1` in the `wasm-release` profile
# have already done the work, so requiring binaryen would buy a rounding error.

set -euo pipefail

cd "$(dirname "$0")/.."

CRATE=crates/accent-proust-wasm
OUT=${OUT:-$CRATE/pkg}
NPM_NAME=accent-proust
PROFILE=wasm-release
TARGET=wasm32-unknown-unknown
STEM=accent_proust_wasm

PACK=0
for arg in "$@"; do
  case "$arg" in
    --pack) PACK=1 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }
step() { printf '\n==> %s\n' "$*"; }

# --- Versions ----------------------------------------------------------------

version=$(awk '/^\[package\]/{p=1} p&&/^version[[:space:]]*=/{print;exit}' \
  "$CRATE/Cargo.toml" | cut -d'"' -f2)
[ -n "$version" ] || die "no version in $CRATE/Cargo.toml"

command -v wasm-bindgen >/dev/null || die \
  "wasm-bindgen is not on PATH; cargo install wasm-bindgen-cli --locked"

# The generated glue and the compiled wasm agree on an ABI that is not stable
# between wasm-bindgen releases, so a mismatch here is a run-time failure with
# no useful message. The lockfile is the authority for what the crate compiled
# against.
locked=$(awk '/^name = "wasm-bindgen"$/{found=1; next} found&&/^version = /{print; exit}' \
  Cargo.lock | cut -d'"' -f2)
installed=$(wasm-bindgen --version | awk '{print $2}')
[ -n "$locked" ] || die "no wasm-bindgen version in Cargo.lock"
[ "$locked" = "$installed" ] || die \
  "wasm-bindgen $installed is installed but the crate compiles against $locked;
  cargo install wasm-bindgen-cli --version $locked --locked"

step "accent-proust-wasm $version, wasm-bindgen $locked"

# --- Build -------------------------------------------------------------------

step "cargo build --profile $PROFILE"
cargo build -p accent-proust-wasm --profile "$PROFILE" --target "$TARGET"

step "wasm-bindgen --target web"
rm -rf "$OUT"
# `web` rather than `bundler`: it is ESM that Vite, native `<script
# type="module">` and a CDN all take unchanged, which is the audience. A
# `bundler` build is additive later if a webpack consumer asks for one.
wasm-bindgen --target web --out-dir "$OUT" \
  "target/$TARGET/$PROFILE/$STEM.wasm"

# --- Package metadata --------------------------------------------------------

step "package.json"
cat > "$OUT/package.json" <<JSON
{
  "name": "$NPM_NAME",
  "version": "$version",
  "description": "Markdoc for the browser: parse, validate, transform, render and format, compiled to WebAssembly from Rust.",
  "license": "MIT",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/zoosky/accent-proust.git"
  },
  "homepage": "https://github.com/zoosky/accent-proust",
  "keywords": [
    "markdoc",
    "markdown",
    "commonmark",
    "wasm",
    "webassembly"
  ],
  "type": "module",
  "module": "${STEM}.js",
  "types": "${STEM}.d.ts",
  "sideEffects": false,
  "exports": {
    ".": {
      "types": "./${STEM}.d.ts",
      "default": "./${STEM}.js"
    },
    "./${STEM}_bg.wasm": "./${STEM}_bg.wasm"
  },
  "files": [
    "${STEM}.js",
    "${STEM}.d.ts",
    "${STEM}_bg.wasm",
    "${STEM}_bg.wasm.d.ts",
    "README.md",
    "LICENSE"
  ]
}
JSON

cp "$CRATE/README.md" "$OUT/README.md"
cp LICENSE "$OUT/LICENSE"

# --- Report ------------------------------------------------------------------

step "built $OUT"
raw=$(wc -c < "$OUT/${STEM}_bg.wasm" | tr -d ' ')
gz=$(gzip -9 -c "$OUT/${STEM}_bg.wasm" | wc -c | tr -d ' ')
printf '  %s_bg.wasm  %s bytes  (%s gzipped)\n' "$STEM" "$raw" "$gz"

if [ "$PACK" = 1 ]; then
  step "npm pack --dry-run"
  command -v npm >/dev/null || die "npm is not on PATH"
  (cd "$OUT" && npm pack --dry-run)
fi

echo
echo "npm package built. To publish:  cd $OUT && npm publish"
