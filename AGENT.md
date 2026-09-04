# accent-proust - Code guidelines

## Ground rules

**These must always be followed.**

1. **Never push directly to `main`.** Every change goes through a pull request.
2. **Create a branch first.** Use `feature/...`, `fix/...` or `chore/...`.
3. **Run the quality gates before committing.** See "Quality gates" below for
   this repository's commands. They differ from Accent's.
4. **Open a pull request for review.**
5. **Wait for CI.** Pull requests must pass before merging.
6. **No emojis in the codebase.**
7. **Test code before shipping it.** A claim that something works needs a run
   behind it, not an inference.
8. **Never commit debugging leftovers** -- `dbg!`, stray `println!`, commented-out
   experiments.
9. **Never add `Claude`, `Generated with Claude Code`, `Co-Authored-By: Claude`
   or any other AI attribution** to the codebase, commit messages, pull
   requests or issues. This file is the one place such mentions belong,
   because it is addressed to the assistant.
10. **Write self-documenting code.** Every module, struct, enum, trait and
    public function gets a doc comment (`///`, `//!`) explaining its purpose
    and responsibility -- the "why" -- plus error conditions and edge cases.
    Applies to new code; existing code is not rewritten for this alone.
11. **Admit and stop when a URL is unreachable.** When a URL comes up -- an
    upstream issue, a release page, a spec -- **actually fetch it** before
    citing it. If the fetch fails for any reason, say so plainly and ask how
    to proceed. Never fabricate content, version numbers, changelog entries,
    API shapes or repository metadata from training data or inference. An
    unverified claim about an external source is worse than a visible
    blocker.
12. **Explanations, commit messages, commit descriptions, CHANGELOG.md and
    README.md stay short.** Be sharp on the point. Spare your tokens.
13. **Model selection when Fable is the session model.** When the session runs
    on Fable (Mythos-class), pick the best-suited model per delegated task
    rather than letting every subagent inherit the expensive session model:
    `haiku` for mechanical lookups, `sonnet` for routine search and coding,
    `opus`/Fable only for the hardest reasoning, review or judgment. Keep
    Fable for orchestration and final synthesis.
14. **Write in the Google developer documentation style**
    (<https://developers.google.com/style>): second person, active voice,
    present tense, sentence case headings, plain language, and the fewest
    words that stay accurate. This covers commit messages, pull request
    bodies, code comments and this file.

    **Concise is not terse.** A pull request body records why a change was
    made, what was measured, and what was deliberately not done; that
    reasoning is the artifact. Cut the padding around an argument, never the
    argument. A finding stated in one sentence instead of three is better; a
    finding omitted is not.

    Applies to text written from now on. Existing documents are not rewritten
    for style alone.

15. **Never edit `spec/` or `reference/`.** Both are vendored upstream trees at
    the ported revision. Editing either destroys the thing they exist for: a
    corpus you can trust and a diff you can read.

## Workflow for every change

```bash
# 1. Branch (never work on main)
git checkout -b fix/my-change

# 2. Change, then run the gates
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features

# 3. Commit and push
git add . && git commit -m "Describe the change"
git push -u origin fix/my-change

# 4. Open a pull request
```

## Quality gates

CI runs every job on **stable**. Run the same commands locally:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
./scripts/check-standalone.sh
```

Clippy runs **twice**, over both feature configurations. Code inside
`#[cfg(feature = "...")]` is only linted when that feature is on, and a
`#[cfg(not(feature = "..."))]` block is only compiled when it is off, so a
single default-feature pass leaves half the crate unlinted.

`cargo test` needs no submodule and no network. The conformance corpus is
vendored under `spec/`.

### The MSRV

`rust-version = "1.82"` is a promise about the **library**, and CI does not
test it. Check it by hand when you touch anything a consumer compiles:

```bash
cargo +1.82 clippy --lib -- -D warnings
cargo +1.82 clippy --lib --no-default-features -- -D warnings
```

`--lib` only. Running the test suite needs a newer toolchain than 1.82, because
the conformance harness reads YAML through `saphyr`, which pulls
`ordered-float`, which requires rustc 1.90. That is a floor for `cargo test` in
this repository, not for consumers: nobody builds a dev-dependency of a crate
they depend on. Develop on stable.

### CI jobs

All in `.github/workflows/ci.yml`. Every one gates.

| Job | What it does |
|---|---|
| `Format` | `cargo fmt --all --check` |
| `Clippy (default)`, `Clippy (no-default-features)` | clippy over both feature configurations |
| `Test` | `cargo test --all-features` |
| `Docs` | `cargo doc --no-deps --all-features` with `-D warnings` |
| `Standalone (Invariant 1)` | `scripts/check-standalone.sh` |
| `Conformance` | runs the corpus and publishes the count to the run summary |

## The two ratchets

Both fail on drift in either direction, and both are the point of the project
rather than bookkeeping.

**Conformance.** `conformance-baseline.txt` records what the corpus counter
stood at. `cargo test --test conformance` compares the run against it and fails
on any difference: a drop is a regression and is not mergeable, a rise is a
baseline that was not updated in the same commit. It is deliberately not an
absolute `105/105` gate, which would leave every pull request failing a
required check until the port finished.

**Divergences.** A case that should stop being green is a *divergence*, which
means an entry in `DIVERGENCES.md` and a move from `green` to `annotated` -- not
a smaller number in the baseline. `DIVERGENCES.md` is normative, not a
changelog.

Upstream error ids are the hard contract. External tooling binds to them, so
that is the one place where divergence is disallowed outright rather than
declared.

## Layout

The module tree mirrors upstream file-for-file wherever the code is a pure
function of its inputs, so a future upstream commit diffs cleanly against its
Rust counterpart:

| Upstream | Here |
|---|---|
| `src/ast/` | `src/ast/` |
| `src/grammar/tag.pegjs` | `src/grammar/` |
| `src/parser.ts`, `src/tokenizer/` | `src/parse/` |
| `src/validator.ts`, `src/schema.ts`, `src/schema-types/` | `src/validate/` |
| `src/transformer.ts`, `src/transforms/` | `src/transform/` |
| `src/renderers/html.ts` | `src/render/` |
| `src/formatter.ts` | `src/format/` |
| `src/functions/` | `src/functions/` |
| `src/tags/` | `src/tags/` |

Two upstream trees are vendored at the ported revision, and neither is ever
edited (rule 15):

| Path | What | Why |
|---|---|---|
| `spec/` | the conformance corpus and its runner | it is the test suite, so a fresh clone runs it with `cargo test` and nothing else |
| `reference/` | upstream's TypeScript, its unit tests, and its markdown-it patch | a porting pull request shows its source in the same diff, and the yearly upstream refresh is `git diff` rather than a second checkout |

`reference/` is `exclude`d from the packaged crate. `spec/` is not: a package
that cannot run its own tests is the worse trade.

## Testing conventions

Integration tests live in `tests/`, one file per pipeline stage
(`parser.rs`, `validator.rs`, `formatter.rs`, `conformance/`, ...). Suites that
need the bundled tokenizer declare `required-features =
["pulldown-cmark-tokenizer"]` in `Cargo.toml`, so
`cargo test --no-default-features` skips them instead of failing to compile.
`tests/tokenizer_seam.rs` is deliberately not gated: it implements `Tokenizer`
by hand and runs in both lanes, which is the configuration a host supplying its
own CommonMark parser uses.

The README's Rust blocks live twice: in `README.md`, and in
`examples/readme.rs`, which CI runs so the outputs the README quotes are known
to be real. `tests/readme.rs` fails if the two drift. Edit both, keeping
document order and copying blocks verbatim.

Panic-freedom is a published promise, backed by proptests. Keep it: `panic`,
`unwrap_used`, `expect_used` and `indexing_slicing` are `warn` at the crate
level, and CI turns warnings into errors. Where a bound is genuinely proven,
`#[allow]` it with a comment saying why.

## Releasing

`.github/scripts/release.sh --dry-run` runs every gate above plus the package
checks. Drop `--dry-run` to publish.

The version is still `0.0.0`. The script refuses to publish that, because it is
the placeholder the crate was created with and the one version number that
cannot be corrected afterwards. Bump it first.

## Session completion

**Work is not complete until `git push` succeeds.**

1. Run the quality gates if code changed.
2. Push to the remote. `git status` must show the branch up to date with
   origin.
3. Verify everything is committed and pushed.
4. Hand off: say what was done, what was measured, and what is left.

Never stop before pushing -- that strands the work locally. Never say "ready
to push when you are"; push. If the push fails, resolve it and retry.
