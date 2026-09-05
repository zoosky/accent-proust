#!/usr/bin/env bash
# Build the npm package for the WebAssembly bindings.
#
# The artifact is generated, never committed: `crates/accent-proust-wasm/pkg`
# is ignored, and this script is the only thing that writes it.
#
# Usage:
#   ./scripts/build-npm.sh            # build into crates/accent-proust-wasm/pkg
#   ./scripts/build-npm.sh --pack     # also run `npm pack --dry-run`
#   ./scripts/build-npm.sh --publish  # build, check, and publish to npm
#
#   OUT=dir                           # write somewhere else, inside the repository
#
# Requires `wasm-bindgen`, whose version must match the `wasm-bindgen` the
# crate compiles against; the script checks that rather than letting a mismatch
# surface as broken glue at run time. The build is `--locked` so the lockfile
# the check read is the lockfile the build used.
#
# There is no wasm-opt step, which is a measured decision rather than an
# omission. Over this artifact `wasm-opt -O3` takes 554,850 bytes to 526,162,
# and 213,250 gzipped to 213,077 -- five percent off the raw file and nothing
# off the download. `lto` and `codegen-units = 1` in the `wasm-release` profile
# have already done the work, so requiring binaryen would buy a rounding error.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

CRATE=crates/accent-proust-wasm
OUT=${OUT:-$CRATE/pkg}
NPM_NAME=accent-proust
PROFILE=wasm-release
TARGET=wasm32-unknown-unknown
STEM=accent_proust_wasm

PACK=0
PUBLISH=0
for arg in "$@"; do
  case "$arg" in
    --pack) PACK=1 ;;
    --publish) PUBLISH=1; PACK=1 ;;
    -h|--help) awk 'NR>1 && !/^#/{exit} NR>1' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }
step() { printf '\n==> %s\n' "$*"; }

# The version a package manifest declares.
manifest_version() {
  awk '/^\[package\]/{p=1} p&&/^version[[:space:]]*=/{print;exit}' "$1" | cut -d'"' -f2
}

# --- Where the output goes ---------------------------------------------------

# `OUT` is documented, so it is attacker-adjacent in the only sense that
# matters here: a typo. The directory is deleted before it is written, and
# `rm -rf $HOME` is not a build step. Resolve it and refuse anything that is
# not a path inside the repository.
out_parent=$(cd "$(dirname "$OUT")" 2>/dev/null && pwd) \
  || die "OUT=$OUT: the parent directory does not exist"
OUT_ABS="$out_parent/$(basename "$OUT")"
case "$OUT_ABS" in
  "$ROOT"/?*) ;;
  *) die "OUT=$OUT resolves to $OUT_ABS, which is outside $ROOT" ;;
esac

# --- Versions ----------------------------------------------------------------

# The library's version is the one the git tag and crates.io track, so it is
# the one the npm package carries. The member declares its own and nothing in
# cargo makes the two agree, so this does.
version=$(manifest_version Cargo.toml)
member_version=$(manifest_version "$CRATE/Cargo.toml")
[ -n "$version" ] || die "no version in Cargo.toml"
[ "$version" = "$member_version" ] || die \
  "Cargo.toml is $version but $CRATE/Cargo.toml is $member_version; they ship together"

command -v wasm-bindgen >/dev/null || die \
  "wasm-bindgen is not on PATH; cargo install wasm-bindgen-cli --locked"

# The generated glue and the compiled wasm agree on an ABI that is not stable
# between wasm-bindgen releases, so a mismatch here is a run-time failure with
# no useful message. The lockfile is the authority for what the crate compiles
# against, which is why the build below is `--locked`: without it cargo may
# re-resolve to a newer patch and compile against something this check never
# saw.
locked=$(awk '/^name = "wasm-bindgen"$/{found=1; next} found&&/^version = /{print; exit}' \
  Cargo.lock | cut -d'"' -f2)
installed=$(wasm-bindgen --version | awk '{print $2}')
[ -n "$locked" ] || die "no wasm-bindgen version in Cargo.lock"
[ "$locked" = "$installed" ] || die \
  "wasm-bindgen $installed is installed but the crate compiles against $locked;
  cargo install wasm-bindgen-cli --version $locked --locked"

step "accent-proust-wasm $version, wasm-bindgen $locked"

# --- Publishing preconditions, before anything is built ----------------------

if [ "$PUBLISH" = 1 ]; then
  step "publish preconditions"
  command -v npm >/dev/null || die "npm is not on PATH"

  # The rule release.sh applies to crates.io: publish a version the changelog
  # carries.
  grep -q "^## \[$version\]" CHANGELOG.md || die \
    "CHANGELOG.md has no section for $version; move Unreleased into a dated one first"

  # And the rule this artifact needs on top of it. A section for the version is
  # not enough when `Unreleased` still has entries: the crate reached $version
  # before the bindings existed, so publishing npm at $version would ship the
  # bindings under a heading that predates them and describe them nowhere.
  unreleased=$(awk '/^## \[Unreleased\]/{f=1; next} /^## \[/{f=0} f' CHANGELOG.md \
    | tr -d '[:space:]')
  [ -z "$unreleased" ] || die \
    "CHANGELOG.md still has entries under Unreleased; bump the version in both
  manifests and move them into a dated section before publishing $version"

  # release.sh refuses to publish twice by checking the git tag. npm is the
  # registry equivalent, and it is the only network call this script makes.
  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    "https://registry.npmjs.org/$NPM_NAME/$version") || die "cannot reach the npm registry"
  [ "$code" = "404" ] || die \
    "$NPM_NAME@$version is already published (registry answered $code); bump the version first"
  echo "  $NPM_NAME@$version is not published yet"
fi

# --- Build -------------------------------------------------------------------

step "cargo build --profile $PROFILE"
cargo build --locked -p accent-proust-wasm --profile "$PROFILE" --target "$TARGET"

step "wasm-bindgen --target web"
rm -rf "$OUT_ABS"
# `web` rather than `bundler`: it is ESM that Vite, native `<script
# type="module">` and a CDN all take unchanged, which is the audience. A
# `bundler` build is additive later if a webpack consumer asks for one.
wasm-bindgen --target web --out-dir "$OUT_ABS" \
  "target/$TARGET/$PROFILE/$STEM.wasm"

# --- Package metadata --------------------------------------------------------

step "package.json"
cat > "$OUT_ABS/package.json" <<JSON
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

cp "$CRATE/README.md" "$OUT_ABS/README.md"
cp LICENSE "$OUT_ABS/LICENSE"

# --- Report ------------------------------------------------------------------

step "built $OUT"
raw=$(wc -c < "$OUT_ABS/${STEM}_bg.wasm" | tr -d ' ')
gz=$(gzip -9 -c "$OUT_ABS/${STEM}_bg.wasm" | wc -c | tr -d ' ')
printf '  %s_bg.wasm  %s bytes  (%s gzipped)\n' "$STEM" "$raw" "$gz"

if [ "$PACK" = 1 ]; then
  step "npm pack --dry-run"
  command -v npm >/dev/null || die "npm is not on PATH"
  (cd "$OUT_ABS" && npm pack --dry-run)
fi

if [ "$PUBLISH" = 1 ]; then
  step "npm publish"
  printf 'publish %s@%s to npm? [y/N] ' "$NPM_NAME" "$version"
  read -r reply
  [ "$reply" = "y" ] || die "cancelled"
  (cd "$OUT_ABS" && npm publish)
  echo
  echo "published $NPM_NAME@$version"
else
  echo
  echo "npm package built. To publish:  ./scripts/build-npm.sh --publish"
fi
