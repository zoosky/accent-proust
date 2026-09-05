# The conformance corpus, vendored

Upstream: <https://github.com/markdoc/markdoc>
Revision: `afee1a4f19678d97bf35606aed38b27c5ed5b1df` (tag `v0.5.9`)
Path upstream: `spec/marktest/`

`tests.yaml` is 105 cases of source in, expected output out. It is the progress
measure for this port and the merge gate for every change: the harness in
`tests/conformance/` reads this file directly and reports
`N green, M annotated, P failing (of 105)`.

## Rules

**These files are never edited.** Not to fix a case, not to reword a name, not
to reformat. A case this crate does not satisfy is either a bug to fix or a
divergence to declare in `../DIVERGENCES.md`; changing the corpus to match the
implementation destroys the only measure the port has. Refreshing the corpus to
a newer upstream revision is a deliberate act: replace the files wholesale,
update the revision above, and read the resulting `git diff` as the changelog it
is.

That rule binds bots too, and one cannot read it: `package.json` here declares
`diff` and `yaml-js` for a runner this repository never invokes, which a
dependency bot sees as an ordinary manifest to keep current. `renovate.json`
names `spec/**` in `ignorePaths` for that reason. A bump there would be
unreachable from every test -- nothing here runs JavaScript -- while spending
the clean diff a refresh depends on.

## What was taken, and what was not

Taken verbatim: `index.ts`, `package.json`, `package-lock.json`,
`react-shim.ts`, `tests.yaml`.

Not taken: upstream's `spec/support/` (jasmine configuration for upstream's own
unit-test runner, which has nothing to do with the corpus).

`index.ts` is here even though nothing in this repository runs TypeScript,
because it is the definition of *how the corpus is graded* and no other file
records it. Three of its decisions are load-bearing, and the Rust harness
mirrors them deliberately:

- The tokenizer is constructed with `allowIndentation: true, allowComments:
  true` (lines 21-24). Neither is a library default. `allowIndentation` is
  unreachable here and is divergence 8; six cases are annotated against it.
- A case carrying `expectedError` is graded on validation messages alone; its
  `expected` tree, if it has one, is never compared.
- Validation errors on an ordinary case are reported but do not fail it, and
  `validation: false` suppresses even the report.

## Licence

Upstream is MIT. Its notice ships beside the files as `LICENSE`, because this
directory is redistribution rather than derived work.
