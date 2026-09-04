#!/usr/bin/env bash
#
# Publish accent-proust to crates.io.
#
# The sibling accent-sass repository releases three crates and spends most of
# its script on the ordering that a `path` + `version` dependency forces. This
# repository is one crate with no workspace members, so all of that is gone.
# What is left -- and what the script is actually for -- is the part that has
# nothing to do with ordering: refusing to publish a tree that has not passed
# the same gates CI runs, and refusing to publish twice.
#
# Usage:
#   .github/scripts/release.sh --dry-run      # verify only, publishes nothing
#   .github/scripts/release.sh                # publish, prompting once
#
#   MSRV=1.96     toolchain the gates run on (default: the rust-version field)
#   NO_TAG=1      skip creating the git tag

set -uo pipefail

DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT"

PKG=accent-proust
BRANCH=main

die() { echo "error: $*" >&2; exit 1; }
step() { printf '\n=== %s\n' "$*"; }

# --- Preconditions -----------------------------------------------------------

step "Preconditions"

# Release-time state, not packaging state. A real publish refuses to proceed;
# --dry-run only warns, so the dry run is usable from the branch that is
# changing the release itself -- which is when you most want it.
soft() { if [ "$DRY_RUN" -eq 1 ]; then echo "  warning: $*"; else die "$*"; fi; }

[ -z "$(git status --porcelain)" ] || soft "working tree is dirty; commit or stash first"

branch=$(git rev-parse --abbrev-ref HEAD)
[ "$branch" = "$BRANCH" ] || soft "on branch '$branch'; release from $BRANCH"

git fetch -q origin "$BRANCH"
[ "$(git rev-parse HEAD)" = "$(git rev-parse "origin/$BRANCH")" ] \
  || soft "HEAD is not origin/$BRANCH; pull or push first"

# Scoped to [package]: a bare `version = ` grep picks up every dependency.
version=$(awk '/^\[package\]/{p=1} p&&/^version[[:space:]]*=/{print;exit}' Cargo.toml | cut -d'"' -f2)
[ -n "$version" ] || die "no version in Cargo.toml"
echo "  version:  $version"

# 0.0.0 is the placeholder this crate was created with, not a release. Publishing
# it would burn the only version number that cannot later be corrected.
[ "$version" != "0.0.0" ] || soft "version is still the 0.0.0 placeholder; bump it before publishing"

# The manifest does not set `publish = false` today. The check stays anyway: it
# is one line, and if someone reinstates the hold this names the cause instead
# of letting it arrive 200 lines later as a cargo error at the moment of upload.
if awk '/^\[package\]/{p=1} /^\[/&&!/^\[package\]/{p=0} p&&/^publish[[:space:]]*=[[:space:]]*false/{found=1} END{exit !found}' Cargo.toml; then
  soft "Cargo.toml sets publish = false; clear the trademark items and remove it first"
  PUBLISH_BLOCKED=1
else
  PUBLISH_BLOCKED=0
fi

if git rev-parse "v$version" >/dev/null 2>&1; then
  die "tag v$version already exists; bump the version first"
fi

# There is no CHANGELOG.md yet. Require the section once there is one, rather
# than either failing on every run or silently dropping the check when the file
# lands.
if [ -f CHANGELOG.md ]; then
  grep -q "^## \[$version\]" CHANGELOG.md \
    || die "CHANGELOG.md has no '## [$version]' heading; move Unreleased into a dated release"
  echo "  changelog: has a [$version] section"
else
  echo "  changelog: none in the repository (skipped)"
fi

MSRV=${MSRV:-$(awk -F'"' '/^rust-version/{print $2; exit}' Cargo.toml)}
echo "  msrv:     $MSRV"

for tc in "$MSRV" stable; do
  rustup toolchain list 2>/dev/null | grep -q "^$tc" \
    || die "toolchain $tc is not installed (rustup toolchain install $tc)"
  rustup component list --toolchain "$tc" 2>/dev/null | grep -qE "^clippy.*\(installed\)" \
    || die "clippy is not installed for $tc (rustup component add --toolchain $tc clippy)"
done
# Checked up front because a missing rustfmt otherwise surfaces below as
# "cargo fmt reported differences", which names the wrong cause.
rustup component list --toolchain stable 2>/dev/null | grep -qE "^rustfmt.*\(installed\)" \
  || die "rustfmt is not installed for stable (rustup component add --toolchain stable rustfmt)"
echo "  components: clippy on $MSRV and stable, rustfmt on stable"

# --- Gates -------------------------------------------------------------------
#
# These mirror .github/workflows/ci.yml. CI runs them on stable only; the script
# adds the MSRV pass, because `rust-version = "$MSRV"` is a published promise and
# nothing else in the repository tests it.

step "Gates on stable (the CI matrix)"

cargo +stable fmt --all -- --check || die "cargo fmt reported differences"
echo "  fmt: clean"

# Both feature configurations, as CI does. Code inside `#[cfg(feature = ...)]`
# is only linted when that feature is on, so a single default-feature pass
# leaves the no-tokenizer half of the crate unlinted.
for flags in "" "--no-default-features"; do
  # Unquoted on purpose: empty must expand to no argument at all, and bash 3.2
  # (what macOS ships) treats "${arr[@]}" on an empty array as unbound under -u.
  # shellcheck disable=SC2086
  cargo +stable clippy --all-targets $flags -- -D warnings \
    || die "clippy failed on stable ${flags:-(default features)}"
  echo "  clippy (${flags:-default features}): clean"
done

# --all-features, matching CI: this is the run that enforces the conformance
# ratchet, since tests/conformance compares itself against
# conformance-baseline.txt and fails on drift in either direction.
cargo +stable test --all-features || die "tests failed"
echo "  tests: pass"

RUSTDOCFLAGS="-D warnings" cargo +stable doc --no-deps --all-features >/dev/null \
  || die "cargo doc reported warnings"
echo "  docs: clean"

# Invariant 1: no host crate anywhere in the tree, no path or git dependencies.
# Exactly the class of mistake that is unrecoverable once a version is on the
# index, so it gates the publish and not only the pull request.
./scripts/check-standalone.sh >/dev/null || die "the standalone check failed (run it directly for the report)"
echo "  standalone: pass"

# --- MSRV --------------------------------------------------------------------
#
# `rust-version` is a promise made to consumers, and CI does not test it: every
# job above runs on stable. It is checked here because a release is the moment
# the promise becomes binding on strangers.
#
# `--lib` because that is what a consumer compiles, and `check` rather than
# `clippy` because the question `rust-version` answers is whether the library
# builds. Linting is stable's job, and the gates above already do it over both
# feature configurations.

step "MSRV $MSRV"

for flags in "" "--no-default-features"; do
  # shellcheck disable=SC2086
  cargo "+$MSRV" check --lib $flags \
    || die "the crate does not build on $MSRV ${flags:-(default features)}; either fix it or raise rust-version"
  echo "  lib on $MSRV (${flags:-default features}): builds"
done

# --- Package check -----------------------------------------------------------

step "Package contents"

# A dry run happens on a working branch, so it tolerates a dirty tree; a real
# publish requires a clean one, enforced in the preconditions. Written as two
# explicit calls rather than an array of flags, for the bash 3.2 reason above.
#
# Keep stderr: swallowing it turns "tree is dirty" into a misleading
# "produced nothing".
if [ "$DRY_RUN" -eq 1 ]; then
  listing=$(cargo package --list --allow-dirty 2>&1)
else
  listing=$(cargo package --list 2>&1)
fi
[ $? -eq 0 ] || { echo "$listing" >&2; die "cargo package --list failed"; }

files=$(echo "$listing" | grep -c .)
echo "$listing" | grep -qx 'README.md' \
  || die "$PKG would publish without a README (check its readme field and include list)"

# `exclude = ["reference/"]` keeps 392 KB of vendored TypeScript out of the
# .crate. An exclude that silently stops matching is invisible in a diff and
# obvious in a download, so assert it here.
! echo "$listing" | grep -q '^reference/' \
  || die "the package includes reference/; the exclude in Cargo.toml is not matching"

# The corpus is deliberately *not* excluded: a package that cannot run its own
# tests is the worse trade. Assert that too, so the next person trimming the
# package has to argue with a failing check rather than a comment.
echo "$listing" | grep -q '^spec/' \
  || die "the package omits spec/; the conformance corpus must ship with the crate"

printf '  %-16s %3s files, README present, reference/ excluded, spec/ included\n' "$PKG" "$files"

# `--list` only reads a file list; it never builds the packaged crate. A real
# `cargo publish` does, so a dry run that skips this can still be followed by a
# publish that fails during verification.
if [ "$DRY_RUN" -eq 1 ]; then
  step "Verifying the crate builds from its package"
  if [ "$PUBLISH_BLOCKED" -eq 1 ]; then
    echo "  skipped: cargo refuses --dry-run while publish = false is set"
    echo "  (cargo package below is the closest available substitute)"
    if cargo package --allow-dirty >/dev/null 2>&1; then
      echo "  $PKG: packages and builds"
    else
      cargo package --allow-dirty 2>&1 | tail -20 >&2
      die "the crate does not build from its packaged form"
    fi
  elif cargo publish --dry-run --allow-dirty >/dev/null 2>&1; then
    echo "  $PKG: packages and builds"
  else
    cargo publish --dry-run --allow-dirty 2>&1 | tail -20 >&2
    die "the crate does not build from its packaged form"
  fi
fi

if [ "$DRY_RUN" -eq 1 ]; then
  step "Dry run complete"
  echo "  Everything that can be checked before publishing passed."
  if [ "$PUBLISH_BLOCKED" -eq 1 ]; then
    echo "  A real publish is still blocked by publish = false in Cargo.toml."
  elif [ "$version" = "0.0.0" ]; then
    echo "  A real publish still refuses the 0.0.0 placeholder. Bump the version."
  else
    echo "  Re-run without --dry-run to publish $version."
  fi
  exit 0
fi

# --- Publish -----------------------------------------------------------------

step "Publish $version to crates.io"
echo "  This is irreversible: crates.io versions cannot be deleted, only yanked."
printf '  Type the version to confirm: '
read -r reply
[ "$reply" = "$version" ] || die "confirmation did not match; nothing published"

cargo publish || die "publish failed"

# --- Tag ---------------------------------------------------------------------

if [ "${NO_TAG:-0}" = "1" ]; then
  step "Skipping tag (NO_TAG=1)"
else
  step "Tagging v$version"
  git tag -a "v$version" -m "v$version"
  git push origin "v$version"
fi

step "Done"
cat <<EOF
  Published $PKG $version.

  Next: bump the pin in the consuming CMS. It depends on this project by git
  revision, not by version, so a crates.io release does not reach it on its own.
EOF
