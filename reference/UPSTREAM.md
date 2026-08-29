# The upstream TypeScript, vendored for reading

Upstream: <https://github.com/markdoc/markdoc>
Revision: `afee1a4f19678d97bf35606aed38b27c5ed5b1df` (tag `v0.5.9`)
Paths upstream: `src/`, `index.ts`, `patches/`

This is the source `proust` is ported from, committed unmodified so that the
port is reviewable against it. It is not built, not linted, not formatted, and
not published: `exclude` in `Cargo.toml` keeps it out of the packaged `.crate`,
and no Rust tooling looks at `.ts` files.

## Why it is in the repository at all

Two properties depend on it, and both stop being real the moment the source
lives only in a sibling checkout that other contributors do not have:

- **A porting pull request shows its source.** The Rust and the TypeScript it
  transliterates appear in one diff, so review is comparison rather than
  recollection.
- **The yearly upstream refresh is a diff.** `git diff <old> <new>` over this
  directory is the complete list of what changed upstream since the ported
  revision. Without the ported revision in the tree there is nothing to diff
  against.

`src/` includes upstream's own `*.test.ts` files on purpose: each phase of the
port is gated on the ported unit tests as well as on the corpus, and those tests
are where the intended behaviour of an individual function is written down.

`patches/` comes with it because upstream patches markdown-it itself
(`markdown-it+12.3.2.patch`, which adds `allowIndentation` to nine block rules).
A reference tree without the patch misrepresents the behaviour being ported --
reading `tokenizer/index.ts` alone suggests the option is ordinary markdown-it
configuration, and the whole reasoning behind divergence 8 rests on it not
being.

## Rules

**Never edited.** A note about the port goes in the Rust, in `../DIVERGENCES.md`,
or in a commit message -- never in this tree, which must stay byte-identical to
the revision named above so the diff property holds.

## Licence

Upstream is MIT. Its notice ships beside the files as `LICENSE`.
