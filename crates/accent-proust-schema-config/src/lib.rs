//! The declarative schema vocabulary shared by `accent-proust`'s hosts.
//!
//! A Markdoc schema is partly data and partly code. The data half -- a render
//! name, the allowed children, typed attributes with defaults and a closed set
//! of values, slots, a description -- can be written in a file or a JavaScript
//! object. The code half -- `transform` and `validate` hooks, a custom
//! attribute type, a pattern in `matches` -- cannot. This crate is the data
//! half, written once: which keys a declaration may carry at each level, how
//! each maps onto the library's [`Schema`](accent_proust::validate::Schema),
//! and the rule that an unknown key is refused with the path to it rather than
//! dropped.
//!
//! # Why a crate, and why not a `Value`
//!
//! Two hosts read the same vocabulary, the WebAssembly bindings from a
//! `JsValue` and the command-line host from a YAML document, and the first
//! copy lived in the browser host alone. Two copies would drift -- the lists
//! are the format -- so the lists live here, and each host keeps only its own
//! reading, through the [`Declaration`] trait.
//!
//! The trait exists because the reading has to be lazy. Converting a whole
//! `JsValue` into a [`Value`](accent_proust::ast::Value) first would convert a
//! JavaScript function written where a hook is not allowed, and fail there
//! with a message about functions rather than at the key with a message about
//! hooks. So the walk asks for keys and refuses the unknown ones before it
//! reads any value, and converts a value only where the vocabulary carries
//! one through: an attribute's `default`, and `variables`.
//!
//! # What a host does with the result
//!
//! ```
//! use std::sync::Arc;
//!
//! use accent_proust::ast::Value;
//! use accent_proust::builtins;
//! use accent_proust::validate::{MapSchemaSource, SchemaKey};
//! use accent_proust_schema_config::declare;
//! use indexmap::IndexMap;
//!
//! // A configuration, here already in the library's own lattice.
//! let mut callout = IndexMap::new();
//! callout.insert("render".to_owned(), Value::String("aside".to_owned()));
//! let mut tags = IndexMap::new();
//! tags.insert("callout".to_owned(), Value::Hash(callout));
//! let mut root = IndexMap::new();
//! root.insert("tags".to_owned(), Value::Hash(tags));
//!
//! let declared = declare(&Value::Hash(root))?;
//! let mut schemas = MapSchemaSource::builtin();
//! let variables = declared.apply(&mut schemas);
//! let mut config = builtins::config_with(Arc::new(schemas));
//! config.variables = variables;
//!
//! assert!(config.schemas.find(SchemaKey::Tag("callout")).is_some());
//! assert!(config.schemas.find(SchemaKey::Tag("if")).is_some());
//! # Ok::<(), accent_proust_schema_config::Error>(())
//! ```
//!
//! # Errors carry a path and a kind
//!
//! [`Error`] is `config.tags.callout.attributes.type.type` and
//! [`ErrorKind::UnknownAttributeType`], not "invalid schema". The kind is
//! there so that a host can add the reason that is its own: why a hook cannot
//! cross into WebAssembly is the browser's sentence, and why a YAML file
//! cannot hold a function is the command line's. This crate's `Display` says
//! what is true everywhere.

mod declaration;
mod error;
mod path;
mod vocabulary;

pub use declaration::{Declaration, Shape};
pub use error::{Error, ErrorKind};
pub use path::Path;
pub use vocabulary::{ATTRIBUTE_KEYS, Declared, SCHEMA_KEYS, SLOT_KEYS, TOP_LEVEL, declare};
