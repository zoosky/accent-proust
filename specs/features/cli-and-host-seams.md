# A command-line host, and the two seams it needs

Status: proposed
Target: 0.11.0, which is a breaking release -- `Config` loses its `nodes` and
`tags` fields. See "Wiring it into Config".

This specification covers one change with two halves. The first half adds
`SchemaSource` and `TagRenderer` to the library, because both are promised in
the documentation and neither exists in the code. The second half adds
`accent-proust-cli`, a command-line host built on them.

The halves ship together on purpose. A trait written without a consumer is a
guess about what a host needs, and the command-line host is the consumer that
turns both guesses into decisions.

## Where this file lives

`specs/` is this repository's own design documents. It is not `spec/`, which is
upstream's conformance corpus, vendored byte-for-byte and never edited
(`AGENT.md` ground rule 15, enforced by `scripts/check-vendored.sh`). The two
directory names differ by one letter, so check which one you are in before you
edit. `check-vendored.sh` reads its path out of `spec/UPSTREAM.md` and compares
`spec/marktest/` and `spec/LICENSE` only, so nothing here is in its way.

## Why now

`src/lib.rs:29-33` tells a reader that the crate has three seams:

> `Tokenizer` segments Markdown. `SchemaSource` answers "what is the schema for
> this tag name?". `TagRenderer` turns a validated tag plus its rendered
> children into markup.

Only `Tokenizer` exists. Grep `src/` for `trait` and you get three results:
`Tokenizer` (`src/parse/tokenizer.rs:250`), `MatchPattern`
(`src/validate/schema.rs:182`) and `AttributeType`
(`src/validate/attribute_type.rs:136`). `SchemaSource` and `TagRenderer` appear
only in prose, in six places: `src/lib.rs:29,32`, `src/validate/mod.rs:16`,
`src/render/mod.rs:17`, `README.md:116-117`, `CHANGELOG.md:79`, and
`Cargo.toml:11` -- where the workspace layout is justified by "the same
reasoning that keeps `Tokenizer` and `TagRenderer` traits rather than
implementations".

That is a difference between what the crate says and what it does, and this
repository has a rule about those: a difference nobody wrote down is a
difference nobody can plan around. The rule has an escape hatch --
`DIVERGENCES.md` -- but it does not apply here, because this is not a
divergence from upstream. Upstream has no such traits either. It is a claim
about this crate that is not true yet.

So there are two honest endings: write the traits, or delete the claim. This
specification writes them, because a second host is wanted anyway, and a second
host is what makes a seam a seam rather than an interface with one caller.

## Scope

In scope:

- `SchemaSource`, a trait for resolving a node or tag to a schema.
- `TagRenderer`, a trait for turning a renderable tag into markup.
- `crates/accent-proust-cli`, a binary crate with `fmt`, `validate`, `render`
  and `parse` subcommands, built with clap.
- `crates/accent-proust-schema-config`, the declarative configuration
  vocabulary, shared by both hosts rather than written twice.
- CI jobs and gate commands for both new crates.

Out of scope:

- Custom functions from configuration. `ConfigFunction.transform` and
  `.validate` are function bodies (`src/validate/config.rs:203-206`) and no
  configuration format has one. The built-in functions stay available; a host
  that needs its own writes Rust.
- Watch mode, a language server, and an editor integration. Each is a host in
  its own right and none is blocked by this work.
- Changing the conformance count. This specification touches no grading
  behaviour; `conformance-baseline.txt` stays at 95 green, 10 annotated.

## Part one: SchemaSource

### What it replaces

Schema resolution has exactly one choke point today, which is what makes this
change small. `Config::find_schema` (`src/validate/config.rs:149`) is six lines:

```rust
pub fn find_schema(&self, node: &Node<'_>) -> Option<&Schema> {
    match &node.tag {
        Some(tag) => self.tags.get(tag.as_str()),
        None => self.nodes.get(&node.node_type),
    }
}
```

Three callers reach it: `src/validate/validator.rs:278`, and
`src/transform/node.rs:134` and `:188`. `src/transform/node.rs:262` re-exports
it as a free function to mirror upstream's `transformer.findSchema`.

### The trait

```rust
/// What a lookup is keyed by: a tag by name, a node by type.
#[non_exhaustive]
pub enum SchemaKey<'a> {
    Tag(&'a str),
    Node(NodeType),
}

/// Where a schema comes from.
pub trait SchemaSource {
    /// The schema for `key`, or `None` when the source does not define one.
    ///
    /// `None` is not an error: the validator reports `tag-undefined` or
    /// `node-undefined`, which is the answer a host wants rather than a
    /// degenerate one.
    fn find(&self, key: SchemaKey<'_>) -> Option<&Schema>;
}
```

One provided method joins `find` below, under "Debug must not regress". It is
listed there rather than here because it serves diagnostics and not
resolution, and a reader meeting the trait for the first time should see the
one method that matters.

Three properties are deliberate.

**It is object-safe.** A host stores one as `Arc<dyn SchemaSource>`. No generic
parameter reaches `Config`, so `Config` does not grow one and neither does
anything holding a `Config`.

**It is synchronous**, for the reason `DIVERGENCES.md` entry 3 already gives
for schema hooks: the crate performs no I/O, so an async signature would have
no reachable implementation and would colour every caller above it.

**It returns a borrow.** This is the constraint worth stating up front, because
it rules out a shape people reach for first: a source that loads a schema from
disk on first request cannot implement this trait, since there is nothing to
borrow from after the call returns. Lazy loading needs `Arc<Schema>` by value,
which costs a refcount bump on every lookup in the hot path of both the
validator and the transformer, to serve a case no host in this repository has.
A host that wants laziness populates its map before it hands the source over,
which is what the command-line host does.

### Wiring it into Config

`nodes` and `tags` go private behind a `MapSchemaSource` that implements the
trait, and `Config` holds the trait object alone:

```rust
pub struct Config<'a> {
    /// Where a schema comes from. The only mechanism: there is no map beside
    /// it and so no precedence rule to remember.
    pub schemas: Arc<dyn SchemaSource + Send + Sync>,
    pub variables: Option<Variables>,
    pub functions: Arc<IndexMap<String, ConfigFunction>>,
    pub partials: Arc<IndexMap<String, Node<'a>>>,
    pub validation: ValidationOptions<'a>,
}
```

`find_schema` becomes one line -- `self.schemas.find(key)` -- and keeps its
signature, so `src/validate/validator.rs:278` and `src/transform/node.rs:134`
and `:188` do not change at all.

`functions` and `partials` stay maps. Neither is a schema, `SchemaSource` does
not describe them, and moving them would be a second redesign riding along with
this one.

This is a breaking release. 0.11.0 says so.

#### MapSchemaSource

The default source owns what `Config` used to:

```rust
/// The schemas a host registered, as two maps.
#[derive(Clone, Debug, Default)]
pub struct MapSchemaSource {
    nodes: IndexMap<NodeType, Schema>,
    tags: IndexMap<String, Schema>,
}

impl MapSchemaSource {
    /// Markdoc's own nodes and tags.
    pub fn builtin() -> MapSchemaSource;
    /// Nothing registered. Every lookup misses, which is what
    /// `Config::new` has always meant.
    pub fn new() -> MapSchemaSource;

    pub fn insert_tag(&mut self, name: impl Into<String>, schema: Schema) -> &mut Self;
    pub fn insert_node(&mut self, node_type: NodeType, schema: Schema) -> &mut Self;

    /// The names registered, for diagnostics.
    pub fn tag_names(&self) -> impl Iterator<Item = &str>;
    pub fn node_types(&self) -> impl Iterator<Item = NodeType>;
}

impl SchemaSource for MapSchemaSource {
    fn find(&self, key: SchemaKey<'_>) -> Option<&Schema> { /* today's two lines */ }
}
```

Copy-on-write moves with it. `Config` shares its maps behind an `Arc` and
`tags_mut()` reaches through `Arc::make_mut` (`src/validate/config.rs:121,127`),
which is documented on the site as a property callers rely on
(`site/content/03.docs/01.rust/default.md:79`). Under this design the host
mutates a `MapSchemaSource` it owns and then shares it, so the sharing is
explicit rather than clever. That documentation needs rewriting, not deleting:
the "four maps behind an `Arc`" sentence is now wrong in its count and its
mechanism.

#### Debug must not regress

`Config`'s hand-written `Debug` (`src/validate/config.rs:209-224`) prints the
registered `nodes` and `tags` keys, and its doc comment says why: it is "what
you want when a `tag-undefined` error disagrees with what you thought you
registered". A `dyn SchemaSource` cannot enumerate anything, so that
affordance disappears silently unless the trait keeps it:

```rust
pub trait SchemaSource {
    fn find(&self, key: SchemaKey<'_>) -> Option<&Schema>;

    /// The tag names this source defines, when it can say.
    ///
    /// Diagnostics only. `None` means "cannot enumerate", which is the honest
    /// answer for a source that computes schemas on demand, and is why this
    /// returns an option rather than an empty iterator.
    fn tag_names(&self) -> Option<Vec<&str>> { None }
}
```

`MapSchemaSource` overrides it; `Config::fmt` prints what it gets and
`"<opaque>"` otherwise. A provided method, so no implementor is forced to care.

#### Migration

33 call sites across 13 files outside `src/validate/config.rs` touch
`config.tags`, `config.nodes`, `tags_mut()` or `nodes_mut()`. They are not
evenly distributed, and the shape of the fix is the same everywhere:

| Where | Sites | Fix |
|---|---|---|
| `tests/validator.rs` | ~8 | `config.tags = tags(vec![..])` becomes a `MapSchemaSource` built then assigned to `config.schemas` |
| `crates/accent-proust-wasm/src/config.rs:91,102,127` | 3 | Build one `MapSchemaSource` from `builtin()`, merge declarations into it, share it once |
| `tests/conformance/config.rs:60-61` | 2 | Same, in the corpus harness |
| `README.md:47`, `examples/readme.rs:31`, `site/.../01.rust/default.md:57,79` | 4 | The headline example changes shape; see below |
| `src/builtins.rs:10,36,42-43` | 4 | `builtins::config()` wraps `MapSchemaSource::builtin()` |
| `src/validate/nodes.rs:7` | 1 | Doc comment |

`builtins::config()` keeps its signature and its meaning, so the common case is
untouched. What changes is registering a tag on top of it. Today:

```rust
let mut config = builtins::config();
config.tags_mut().insert("callout".to_string(), schema);
```

After:

```rust
let mut schemas = MapSchemaSource::builtin();
schemas.insert_tag("callout", schema);

let mut config = builtins::config();
config.schemas = Arc::new(schemas);
```

Three lines instead of two, and one concept more. That is the price of the
single mechanism, and it is worth paying once rather than documenting a
precedence rule forever -- but it lands in the first example every new reader
meets, in the README, the crate docs and the site. Provide a shorthand so the
common case stays two lines:

```rust
impl Config<'_> {
    /// Replace the schema source. Chainable, for the registering case.
    pub fn with_schemas(self, schemas: Arc<dyn SchemaSource + Send + Sync>) -> Self;
}
```

`Arc::get_mut` is not the shorthand to reach for. It fails whenever the source
is already shared, which is exactly when a caller would want it, so it would
work in the doctest and fail in the host.

#### What this costs the WebAssembly host

`crates/accent-proust-wasm/src/config.rs` merges host declarations over the
built-ins by calling `tags_mut()` and `nodes_mut()` on a `builtins::config()`
(lines 91 and 102). It rebuilds the same way: start from
`MapSchemaSource::builtin()`, merge into it, share it once at the end. That is
one fewer `Arc::make_mut` per declaration than today, so it is a small
improvement rather than a tax -- but it is a change to a shipped host, and it
belongs in the same pull request as the trait, not a follow-up.

### What the command-line host does with it

It implements the trait over an `IndexMap` it filled by reading a configuration
file. That is the whole point: the map is built from a file the crate never
opened, which is the sentence `src/lib.rs:29-31` has been making since 0.1.

## Part two: TagRenderer

### The constraint that shapes it

`render_into` (`src/render/html.rs:145`) walks an explicit stack where upstream
recurses, and its doc comment says why:

> Nesting depth in a renderable tree comes from the document that produced it,
> which is attacker-controlled, and a stack overflow in Rust aborts the process
> rather than raising something a caller could catch.

The obvious `TagRenderer` shape -- hand the host a tag and a callback that
renders its children -- puts recursion straight back through the trait
boundary, one host frame per level. It would undo the reason the stack exists
and quietly break the panic-freedom promise the crate publishes, and it would
do so in code the crate cannot see.

So the trait must emit markup for one tag without ever rendering its children.
The crate keeps the stack; the host keeps escaping, void-element policy and
element naming.

### The trait

```rust
/// Whether a tag's children are rendered after its opening markup.
#[non_exhaustive]
pub enum Children {
    /// Render them, then call `close`.
    Render,
    /// Skip them and emit no closing markup -- a void element.
    Skip,
}

/// Turns renderable tags into markup.
///
/// Implementations write into `out` rather than returning a `String`, so a
/// document renders into one allocation.
pub trait TagRenderer {
    /// Write the markup that opens `tag`, and say whether children follow.
    fn open(&self, out: &mut String, tag: &Tag) -> Children;

    /// Write the markup that closes `tag`.
    ///
    /// Called only after `open` returned [`Children::Render`] for it.
    fn close(&self, out: &mut String, tag: &Tag);

    /// Write `text` in text position, escaped to the host's policy.
    fn text(&self, out: &mut String, text: &str);
}
```

`open` takes the whole `Tag` (`src/renderable.rs:466`) because attribute
rendering is part of HTML policy: today `open_tag` (`src/render/html.rs:171`)
lowercases attribute names, escapes values, and stops at
`is_void_element` (`src/render/html.rs:64`). All three of those are decisions a
different host makes differently, and all three are inside the one method.

`text` is separate because escaping applies to scalars in text position, which
the stack reaches without going through `open` at all.

`close` takes the whole `Tag` as well, for the same reason in the other
direction: whatever `open` derived from the attributes -- an element name
chosen by a `level`, say -- `close` has to derive again, and it can only do
that from the same input. A name alone would make the plain HTML case work and
the policy case silently wrong, which is the case the seam exists for.

### Compatibility

The existing behaviour becomes `render::Html`, a unit struct implementing the
trait with exactly today's code moved into it. `render`, `render_all` and
`escape_html` keep their signatures and delegate to it, so no caller changes
and `escape_html` stays public -- the WebAssembly host and the site both use
it.

Add `render_with(node, &renderer)` and `render_all_with(nodes, &renderer)`
beside them. That mirrors how `format`/`format_with`
(`src/format/mod.rs:203,220`) and `parse`/`parse_with` (`src/parse/mod.rs:159,
171`) are already paired, so the naming needs no explanation.

### Depth

`render_into`'s stack is heap-allocated and already bounded by the tree, which
the transformer bounded at `MAX_TRANSFORM_DEPTH`
(`src/transform/node.rs:63`, 512). A `TagRenderer` does not change that: the
host writes into a string and returns, and the crate keeps popping. Say so in
the trait docs, because the first question a reader asks is whether this needs
a depth limit of its own.

## Part three: the command-line host

### Where it lives

`crates/accent-proust-cli`, beside `crates/accent-proust-wasm`. `Cargo.toml:9`
already states the rule: "Members are hosts. A WebAssembly binding is a host
like any other, so it gets a crate beside the library."

`publish = false` at first. The binary is useful in the repository before it is
useful on crates.io, and a published binary is a name and a release cadence to
own.

### Commands

| Command | Does | Needs configuration |
|---|---|---|
| `fmt` | Reprints canonical Markdoc source | No |
| `validate` | Reports schema violations | Yes, for custom tags |
| `render` | Prints HTML | Yes, for custom tags |
| `parse` | Prints the AST as JSON | No |
| `transform` | Prints the renderable tree as JSON | Yes, for custom tags |

`fmt` is the one to build first. `format(parse(s))` is idempotent and
`parse(format(ast))` returns the same tree, both tested, which is exactly the
contract `--check` and `--write` need. It needs no configuration at all, so it
is useful on day one to anyone with a Markdoc file.

Flags:

- `fmt [--check] [--write] [--max-tag-opening-width N]
  [--ordered-list-mode repeat|increment]`. The last two are `FormatOptions`
  (`src/format/mod.rs:144`), which has exactly two fields, so the mapping is
  total rather than a selection.
- `validate [--format human|json]`.
- Global: `--config <path>`, `--partials <dir>`, `--var NAME=VALUE`,
  `--file <label>`.

`--check` prints a unified diff of what would change, per file, and exits 1 if
anything would. rustfmt's behaviour rather than prettier's: a file list answers
"which files" and a diff also answers "and what is wrong with them", which is
the question a reader of a failed CI log actually has. The list is recoverable
from the diff headers; the diff is not recoverable from the list without
running the tool again locally.

The cost is a diff implementation. Take a dependency rather than write one --
`similar` is the usual choice and is already the kind of dependency a host
carries. Keep it out of the library, which needs no diff.

Read from stdin when no path is given and write to stdout, so the tool composes
with the rest of a documentation pipeline.

### Exit codes

| Code | Means |
|---|---|
| 0 | Success, and for `--check` no file would change |
| 1 | The document has validation errors, or `--check` found a file to reformat |
| 2 | Usage error, unreadable file, or unparseable configuration |

Separating 1 from 2 is what lets CI distinguish "the docs are wrong" from "the
tool is misconfigured", which are different alerts.

### Diagnostics

`ValidateError` (`src/validate/validator.rs:53`) carries `lines`, an optional
`Location`, and a `ValidationError` whose `id` is a `&'static str`
(`src/ast/error.rs:71-79`). `Location` (`src/ast/location.rs:67`) carries
`file`, `start`, `end` and the borrowed source text of the span. That is
everything a diagnostic needs, on two conditions the host has to meet:

- Parse with `ParseOptions::location(true)`, or every location is `None`.
- Parse with `ParseOptions::file(label)`, since `Location.file` is a label the
  caller supplies and not a path the crate discovered.

Human format: `path:line:column: error[id]: message`. JSON format: the error
list, ids verbatim. Upstream's ids are the one place this project refuses to
diverge, so a `--format json` consumer written against Markdoc's codes works
unchanged, and that is worth saying in the CLI's README.

### Configuration

The configuration file carries the declarative half of a schema. The other half
is code: `Schema.transform` and `.validate`, `SchemaAttribute.validate`, and
`SchemaMatches::Pattern` are all `Arc<dyn Fn>` or a trait object
(`src/validate/schema.rs:43-74`), and none of them has a spelling in a text
file.

**The key vocabulary already exists and must not be invented again.**
`crates/accent-proust-wasm/src/config.rs` is 438 lines doing this exact job for
the browser, with fixed allowlists at lines 41-66:

```rust
const TOP_LEVEL: &[&str]      = &["tags", "nodes", "variables"];
const SCHEMA_KEYS: &[&str]    = &["render", "children", "attributes", "slots",
                                  "selfClosing", "inline", "description"];
const ATTRIBUTE_KEYS: &[&str] = &["type", "default", "required", "matches",
                                  "render", "errorLevel", "description"];
const SLOT_KEYS: &[&str]      = &["render", "required"];
```

Its refusal policy matters as much as its keys: an unknown key is an error
naming the path to it, because "a schema that half arrives is worse than one
that does not, because the half that is missing is invisible until an author
trips over it". The command-line host takes both.

#### The vocabulary is a crate

`crates/accent-proust-schema-config` owns the key lists, the refusal policy,
the path-building for error messages, and the mapping from declarations to
`Schema`, `SchemaAttribute` and `SchemaSlot`. Both hosts depend on it. Each
keeps only its own deserialisation: `JsValue` walking for the browser, serde
for the command-line host.

A crate rather than a feature on the library, for one reason that outranks the
extra manifest: **the library's dependency tree stays as it is.** A
`schema-config` feature would put serde in `accent-proust`'s tree for anyone
who enabled it, and `scripts/check-standalone.sh` exists precisely to keep that
tree honest. A separate crate cannot leak into it, and the library stays
publishable with the dependency list it has today.

The crate is not a host, so it inherits none of the host reasoning in
`Cargo.toml:9-11`. It sits beside the hosts as what they share. `publish =
false` until a host outside this repository needs it.

Two things it must not become. It does not depend on clap, because the
WebAssembly host would then carry an argument parser. And it does not own the
`Config` assembly -- it produces schemas, and the host decides what to do with
them, because partials and variables come from places only one host has.

Accept YAML and JSON. YAML is what a documentation repository already has, and
every JSON file is valid YAML, so one parser serves both.

### What the CLI can do that the browser cannot

The WebAssembly `TOP_LEVEL` is `tags`, `nodes`, `variables`. Two absences are
the command-line host's reason to exist.

**Partials.** `config.partials` holds *parsed* nodes keyed by the name
`{% partial file=... %}` uses, because the crate performs no I/O
(`src/validate/config.rs:89-98`). A browser has no filesystem. A CLI does:
`--partials <dir>` reads each file, parses it, and fills the map. This makes
`{% partial %}` usable outside a bespoke Rust host for the first time.

**Whole-repository validation.** Many files, one exit code. Neither the library
nor the playground can be a CI gate; this is the thing that can.

#### Typed variables

`--var` carries typed values, not strings. `Variables` is
`IndexMap<String, Value>` (`src/validate/config.rs:41`), so every type a
Markdoc document can hold is already expressible, and a string-only flag would
make `{% if $debug %}` true for `--var debug=false`. That is a bug the flag
would be handing to every user who writes it.

Parse the part after `=` as a YAML scalar, which is the parser the
configuration file already brings:

| Written | Becomes |
|---|---|
| `--var count=3` | `Value::Number(3.0)` |
| `--var debug=true` | `Value::Boolean(true)` |
| `--var name=production` | `Value::String("production")` |
| `--var missing=null` | `Value::Null` |
| `--var 'version="3"'` | `Value::String("3")` |
| `--var 'tags=[a, b]'` | `Value::Array([String, String])` |

One rule to document, and it is YAML's own: quote it to force a string. Reusing
the config parser means `--var` and `--config` never disagree about what `3`
is, which is the failure a hand-rolled inference rule would eventually produce.

There is one numeric type and it is `f64` (`src/ast/value.rs:44-50`), so `3`
and `3.0` are the same value and no integer-versus-float question arises.

Split on the first `=` only, so `--var 'title=a=b'` is the string `a=b`. A name
with an `=` in it is not addressable, which is correct: the grammar does not
admit one.

### Two implementation notes that will otherwise cost a rewrite

**Sources must outlive the config.** `Config<'a>` borrows its partials, `Node<'a>`
borrows its source, and `Location<'a>` borrows both the file label and the span
text. The natural `for file in files { read; parse; process }` loop does not
compile once partials are shared across files. Structure `main` as: read every
source into an owned arena, then parse the partials, then build the `Config`,
then process. Decide this before writing the loop.

**Lints are per-crate, on purpose.** There is no `[workspace.lints]`; the
WebAssembly crate's manifest explains that a shared block "would be a decision
taken on behalf of every future host". Copy the same block -- `missing_docs`,
`unsafe_code = "forbid"`, clippy `pedantic`, and the four panic-freedom lints.
Keep `unwrap_used` and `panic` denied through the pipeline and take an
`#[allow]` with a reason comment at the `main` boundary, where exiting with a
code is the correct behaviour rather than a lapse.

### Gates and CI

`default-members = ["."]` (`Cargo.toml:17`) holds a bare `cargo test`,
`cargo clippy --all-targets` and the standalone, MSRV and conformance lanes to
the library, so a new member joins none of them automatically. That is the
intended behaviour, not an oversight, and it means each new crate brings its
own:

- A `CLI` job in `.github/workflows/ci.yml`, modelled on `WebAssembly`:
  `cargo clippy -p accent-proust-cli --all-targets -- -D warnings` and
  `cargo test -p accent-proust-cli`.
- A `Schema config` job doing the same for
  `accent-proust-schema-config`. It is the crate both hosts depend on, so it
  going unlinted is the worst of the three to leave to chance.
- A row each in the CI table in `AGENT.md`, and their commands in the "Quality
  gates" section beside the WebAssembly ones, which are already listed by name
  for the same reason.

`scripts/check-standalone.sh` needs no change. It reads the root `Cargo.toml`
only, and a member depending on the library by path is exactly what a host
does.

MSRV is 1.96 for the library. The CLI is not on that lane -- `cargo check --lib`
does not reach it -- so clap's own floor governs and no promise is made about
building the binary on 1.96.

### Testing

- Trait-level unit tests in the library: a `SchemaSource` that is not a
  `MapSchemaSource` -- one that answers from a match arm, say -- resolves
  through `Config`, and a `TagRenderer` that emits something other than HTML
  round-trips through `render_all_with`.
- A `SchemaSource` that returns `None` from `tag_names` renders in `Config`'s
  `Debug` without panicking or claiming an empty registry.
- A `TagRenderer` depth test: a tree near `MAX_TRANSFORM_DEPTH` renders through
  a custom renderer without touching the host's stack. This is the test that
  fails if the trait is ever reshaped to render children itself, which is the
  mistake worth pinning.
- CLI integration tests over fixture files, asserting both stdout and the exit
  code. The exit codes are the contract a CI pipeline binds to, so an assertion
  on stdout alone is not enough.
- `fmt --check` against the repository's own documentation examples, which
  gives the idempotence property a real corpus for free.

## Risks

**A breaking change that arrives twice.** `Config` loses two public fields and
two methods, and 33 call sites move. The failure mode is doing it in stages --
a deprecated `tags_mut()` kept "for one release" alongside the source -- which
restores the precedence rule this design exists to avoid, in the release notes
instead of the code. Land the whole of step 2 or none of it.

**The vocabulary crate drifting back into a host.** It exists to be shared; the
first change that is easier to make in the CLI's copy will be made in the CLI's
copy unless the crate is where the keys live from the day the CLI reads its
first configuration file. That is why step 4 precedes the CLI's `validate`.

**A permissive CLI reading as authoritative.** The CLI sees declarative schemas
only, so a host whose real enforcement lives in a `validate` hook gets a CLI
that passes documents production rejects. The WebAssembly module already states
this exactly -- "the editor is never stricter than the server, only faster" --
and the CLI's README should say the same in its own words.

**The traits landing with one implementation each.** A seam with a single
implementation is an interface, and the two CI feature lanes exist because half
a crate otherwise goes unlinted. Keep the alternate-implementation unit tests
above, so both traits are exercised by something that is not the built-in.

## Sequencing

Each step is a pull request that leaves the repository releasable.

1. `TagRenderer` plus `render::Html`, `render_with`, `render_all_with`. No
   behaviour change; the existing renderer moves behind the trait.
2. `SchemaSource`, `SchemaKey`, `MapSchemaSource`, and `Config.schemas`
   replacing `nodes` and `tags`. Breaking, and the whole of it: the library, the
   33 call sites, the WebAssembly host, the README, the examples and the site
   page that documents copy-on-write.
3. `crates/accent-proust-cli` with `fmt` only -- including `--check` and its
   diff -- plus the CI job and the `AGENT.md` rows. Useful on its own, and needs
   neither trait.
4. `crates/accent-proust-schema-config`, extracted from the WebAssembly host,
   with that host moved on to it in the same pull request. Extracting without
   moving the only existing consumer leaves two copies, which is the state this
   step exists to prevent.
5. `validate`, `render`, `parse` and `transform`, plus `--partials` and
   `--var`.
6. `CHANGELOG.md` entries throughout, with 0.11.0 marked breaking and the
   `Config` migration written out in it. A reader upgrading needs the
   before-and-after, not a list of renamed items.

Steps 1 and 3 are independent of everything else and can land in any order or
in parallel. Step 2 is the one that wants its own release. Step 5 depends on
both 3 and 4.

## Decisions taken

Recorded because each closes a question this specification opened, and the
reasoning is shorter to keep than to reconstruct.

1. **`Config` holds a `SchemaSource` alone**, rather than a source consulted
   ahead of the maps. One mechanism and no precedence rule, at the cost of a
   breaking release and a three-line registration example.
2. **The configuration vocabulary is a crate**, not a feature on the library.
   It keeps serde out of the library's dependency tree, which
   `scripts/check-standalone.sh` exists to protect.
3. **`fmt --check` prints a unified diff.** A CI log reader wants what is wrong,
   not only where.
4. **`--var` takes typed values**, parsed as YAML scalars by the same parser the
   configuration file uses, so the two can never disagree about what `3` is.
