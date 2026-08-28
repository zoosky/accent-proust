#!/usr/bin/env bash
# Invariant 1: proust compiles and tests pass with no host crate present.
#
# This is trivially true today, which is exactly why it is pinned now rather
# than at publication. The invariant is cheap to hold and expensive to restore,
# and the failure mode it guards against -- a "temporary" dependency on the
# consuming CMS -- is discovered far too late if the only check is a human
# reading a diff.
#
# Three things are asserted, because building alone proves less than it looks:
#
#   1. No crate named after the host appears anywhere in the dependency tree,
#      at any depth, with all features enabled.
#   2. No dependency is declared by local path or git URL. Either would make the
#      crate unbuildable outside this machine and unpublishable, and is the
#      mechanism by which a host dependency would actually arrive.
#   3. The crate builds and tests with default features off, which is the
#      configuration a host supplying its own tokenizer uses.

set -euo pipefail

cd "$(dirname "$0")/.."

fail() { printf 'standalone check FAILED: %s\n' "$1" >&2; exit 1; }

echo "==> no path or git dependencies"
# Section-scoped: a bare grep for 'path =' also hits `[package] readme` style
# keys and any comment that happens to mention one.
offenders=$(awk '
  /^\[/ { in_deps = ($0 ~ /dependencies\]$/); next }
  in_deps && /^[[:space:]]*[A-Za-z0-9_-]+[[:space:]]*=/ && /(^|[{,[:space:]])(path|git)[[:space:]]*=/ { print FILENAME ":" FNR ": " $0 }
' Cargo.toml)
if [ -n "$offenders" ]; then
  printf '%s\n' "$offenders" >&2
  fail "Cargo.toml declares a path or git dependency"
fi

echo "==> dependency tree carries no host crate"
# `{p}` renders as "name version (source)". Only the first field is the package
# name; the source is dropped because a checkout path may legitimately contain
# the host's name, and matching against it produces a false positive that is
# invisible until someone renames a directory.
names=$(cargo tree --all-features --prefix none --format '{p}' | awk '{print $1}' | sort -u)
if printf '%s\n' "$names" | grep -Eiq '^accent'; then
  printf '%s\n' "$names" | grep -Ei '^accent' >&2
  fail "a crate whose name begins with 'accent' is in the dependency tree"
fi

echo "==> builds and tests with default features off"
cargo test --no-default-features --quiet

echo "standalone check passed"
