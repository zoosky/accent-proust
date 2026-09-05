#!/usr/bin/env bash
# The vendored corpus is byte-for-byte what upstream published.
#
# `spec/UPSTREAM.md` opens its rules with "These files are never edited", and
# until now that rule lived only in prose. Prose does not fail a build, and this
# one was broken within a month of being written: a dependency bot bumped
# `yaml-js` in `spec/marktest/package.json` and every check passed, because
# nothing here runs JavaScript and no test reads that manifest. The bump also
# cost the lockfile upstream's own `resolved` URL, which npm rewrote on its way
# past. `renovate.json` now skips `spec/**`, but that is configuration aimed at
# one bot rather than an invariant; a hand edit still sails through. This is the
# invariant.
#
# The pin is read out of UPSTREAM.md rather than restated here, so a refresh
# stays the deliberate one-line act that file describes: move the revision,
# replace the files, and this check grades the result against the new revision.
# Both halves of a half-done refresh fail -- a revision bumped without the files
# following, and files edited without the revision moving.
#
# The whole directory is compared, not a list of filenames, so a file added
# beside the corpus or quietly deleted from it fails the same way an edit does.
# `spec/LICENSE` is checked too: it is upstream's notice, redistributed, and the
# licence a redistribution ships under is not a detail to let drift.
#
# Cost is one blobless, depth-1 fetch of two upstream paths -- well under a
# second, and no toolchain.

set -euo pipefail

cd "$(dirname "$0")/.."

manifest=spec/UPSTREAM.md

fail() { printf 'vendored check FAILED: %s\n' "$1" >&2; exit 1; }

# Anchored, and each result is required below. A silently empty match would
# turn this check into an expensive no-op, which is the one failure a guard
# must not have.
upstream=$(sed -n 's|^Upstream: <\(https://[^>]*\)>$|\1|p' "$manifest")
revision=$(sed -n 's|^Revision: `\([0-9a-f]\{40\}\)`.*|\1|p' "$manifest")
upstream_dir=$(sed -n 's|^Path upstream: `\(.*[^/]\)/*`$|\1|p' "$manifest")

[ -n "$upstream" ] || fail "$manifest has no 'Upstream: <url>' line"
[ -n "$revision" ] || fail "$manifest has no 'Revision: \`<40 hex>\`' line"
[ -n "$upstream_dir" ] || fail "$manifest has no 'Path upstream: \`<dir>\`' line"

local_dir="spec/$(basename "$upstream_dir")"
[ -d "$local_dir" ] || fail "$local_dir is not a directory"

echo "==> $upstream at ${revision:0:7}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git init -q "$work"
git -C "$work" remote add origin "$upstream"

# GitHub serves an arbitrary commit to a want, so the tag this revision carries
# is never fetched and cannot be moved out from under the check. Retried
# because a network blip should read as a network blip, not as a corpus that
# drifted -- but a revision upstream does not have is a wrong pin, not a blip,
# and waiting out three attempts to say so helps nobody.
fetched=false
for attempt in 1 2 3; do
  if err=$(git -C "$work" fetch -q --depth 1 --filter=blob:none origin "$revision" 2>&1); then
    fetched=true
    break
  fi
  case $err in
    *'not our ref'*|*'not found'*|*"couldn't find remote ref"*)
      printf '%s\n' "$err" >&2
      fail "$manifest pins $revision, which $upstream does not have"
      ;;
  esac
  printf '%s\n' "$err" >&2
  echo "    fetch attempt $attempt failed, retrying" >&2
  sleep $((attempt * 5))
done
[ "$fetched" = true ] || fail "could not fetch $revision from $upstream"

# --no-cone because cone mode always materialises every file at the repository
# root, which here is upstream's own package.json and a dozen other blobs this
# check has no business downloading.
git -C "$work" sparse-checkout set --no-cone "$upstream_dir" LICENSE
git -C "$work" checkout -q FETCH_HEAD

echo "==> $local_dir matches upstream"
# -r so an added or deleted file is a difference too, not just an edited one.
if ! diff -ru "$work/$upstream_dir" "$local_dir"; then
  fail "$local_dir has drifted from $upstream at $revision -- see the diff above.
  These files are vendored redistribution and are never edited. If this is a
  deliberate corpus refresh, replace the files wholesale and move the Revision
  line in $manifest in the same commit. If it is a dependency bot, it should
  not have been here: renovate.json ignores spec/**."
fi

echo "==> spec/LICENSE matches upstream's notice"
if ! diff -u "$work/LICENSE" spec/LICENSE; then
  fail "spec/LICENSE has drifted from $upstream at $revision -- see the diff above"
fi

echo "vendored check passed"
