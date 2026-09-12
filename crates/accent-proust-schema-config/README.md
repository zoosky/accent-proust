# accent-proust-schema-config

The declarative half of a Markdoc schema, as a vocabulary both of
[accent-proust](https://github.com/zoosky/accent-proust)'s hosts read.

A schema is partly data -- a name, the allowed children, typed attributes, a
render policy -- and partly code: `transform` and `validate` hooks, a custom
attribute type, a pattern in `matches`. The data half can be written in a
file or a JavaScript object; the code half cannot. This crate is the data
half, written once: which keys a declaration may carry, how each maps onto the
library's `Schema`, and the rule that an unknown key is refused with the path
to it rather than silently dropped -- because a schema that half arrives is
worse than one that does not, and the missing half is invisible until an
author trips over it.

## Who reads what

The crate walks a `Declaration`, a trait with seven methods, and each host
implements it for the shape its configuration arrives in: a `JsValue` in the
browser, a YAML document on the command line. Keys are checked before values
are converted, so a hook written where a key is not allowed is refused by name
without anyone trying to read it.

```rust
let declared = accent_proust_schema_config::declare(&declaration)?;
let mut schemas = MapSchemaSource::builtin();
let variables = declared.apply(&mut schemas);
let mut config = builtins::config_with(Arc::new(schemas));
config.variables = variables;
```

## The vocabulary

| Level | Keys |
|---|---|
| top | `tags`, `nodes`, `variables` |
| schema | `render`, `children`, `attributes`, `slots`, `selfClosing`, `inline`, `description` |
| attribute | `type`, `default`, `required`, `matches`, `render`, `errorLevel`, `description` |
| slot | `render`, `required` |

Spellings are upstream Markdoc's, so a manifest reads the same on both sides.
An attribute `type` is one of `String`, `Number`, `Boolean`, `Object`, `Array`,
or an array of those for a union. `matches` is an array of acceptable values;
a regular expression is not supported, because the engine carries no regular
expression engine on purpose. A schema under `nodes.tag` is refused: a tag is
looked up by its name, and a schema registered for the node type would never
apply.

Errors carry the path to the value that caused them, `config.tags.callout.
attributes.type.type` and not "invalid schema", and a structured kind so that
a host can add its own reason -- why a hook cannot cross into WebAssembly is
the browser's sentence to write, not this crate's.
