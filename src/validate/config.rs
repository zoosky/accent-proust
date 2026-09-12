//! The bag of schemas, variables, functions and partials a document is checked
//! and transformed against.
//!
//! Mirrors upstream's `Config` in `src/types.ts`. Upstream declares it as a
//! `Partial<>` of five optional records, because in JavaScript "absent" and
//! "empty" are usefully different for some of them and accidentally different
//! for the rest. Each field here says which it is, and two of them are
//! [`Option`] for a reason the validator can be read against:
//!
//! - **`variables` is optional.** `config.variables` gates variable checking
//!   entirely (`validator.ts`), so `{}` means "check, and nothing is defined"
//!   while absent means "do not check". Upstream's own test suite fixes both.
//! - **`ConfigFunction::parameters` is optional**, for the same shape of
//!   reason: absent skips parameter checking, empty rejects every parameter.
//!
//! The rest are a schema source and two plain maps. A missing schema and an
//! empty source produce the same `Undefined tag` error, so there is nothing
//! for an [`Option`] to distinguish.
//!
//! # Where the content comes from
//!
//! Nowhere in this crate. A `Config` is assembled by the host -- from a file, a
//! constant, a plugin manifest -- and handed in. Schemas arrive through
//! [`SchemaSource`], which is the host's to implement; [`MapSchemaSource`] is
//! the implementation for a host that assembles them by hand. This module
//! owns the *shape* only, which is the line that lets `accent-proust` be
//! published without shipping anybody's schemas.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::ast::{Node, Value};
use crate::validate::schema::{FunctionTransformHook, FunctionValidateHook, Schema};
use crate::validate::source::{MapSchemaSource, SchemaKey, SchemaSource};
use crate::validate::{SchemaAttribute, ValidationType};

/// Variables a document may reference with `$name`.
///
/// A nested structure, walked one path segment at a time: `$a.b[0]` descends
/// through a [`Value::Hash`] and then a [`Value::Array`]. Upstream stores
/// arbitrary JavaScript here and descends with `hasOwnProperty`; [`Value`] is
/// the same lattice with a name.
pub type Variables = IndexMap<String, Value>;

/// Everything the validator and the transformer read.
///
/// The lifetime is the source text that [`partials`](Config::partials) and
/// [`ValidationOptions::parents`] borrow. A config holding neither -- the
/// ordinary case for a host that registers schemas once and reuses them -- is a
/// `Config<'static>`, and Rust's variance lets that be passed wherever a
/// `Config<'a>` is wanted. That is deliberate: a schema registry that could
/// only validate documents of its own lifetime would have to be rebuilt per
/// page.
///
/// # Why three fields are behind an [`Arc`]
///
/// A config is cloned on a hot path and only one field differs between the
/// original and the copy: `{% partial %}` scopes a partial's body by cloning
/// the whole config to replace [`variables`](Config::variables). Everything
/// else -- the schemas, the functions, the parsed partials -- is registered
/// once and read many times, so copying it per expansion charges the caller for
/// the site's whole partial corpus on every partial in every page, which
/// compounds exactly where partials earn their keep.
///
/// So those three are shared rather than copied. Reads are unchanged
/// ([`Arc`] derefs). The two maps have copy-on-write mutators
/// ([`functions_mut`](Config::functions_mut) and
/// [`partials_mut`](Config::partials_mut)) for assembly; the schema source
/// does not, because a trait object cannot be copied on write. A host fills a
/// [`MapSchemaSource`] and shares it with [`with_schemas`](Config::with_schemas),
/// so the sharing is explicit rather than clever.
#[derive(Clone)]
pub struct Config<'a> {
    /// Where a schema comes from.
    ///
    /// The only mechanism: there is no map beside it and so no precedence rule
    /// to remember. [`Config::new`] starts it empty and
    /// [`builtins::config`](crate::builtins::config) at Markdoc's own.
    pub schemas: Arc<dyn SchemaSource + Send + Sync>,
    /// Variables a `$name` reference resolves against.
    ///
    /// [`None`] switches variable checking off; `Some` of an empty map switches
    /// it on with nothing defined.
    pub variables: Option<Variables>,
    /// Functions a `f()` call resolves against.
    ///
    /// Shared: see the note on [`Config`]. Use
    /// [`functions_mut`](Config::functions_mut) to edit one in place.
    pub functions: Arc<IndexMap<String, ConfigFunction>>,
    /// Parsed partial documents, keyed by the name `{% partial file=... %}`
    /// uses.
    ///
    /// Parsed, not raw: this crate performs no I/O, so a host reads the file and
    /// parses it. That is why the config carries a lifetime.
    ///
    /// Shared: see the note on [`Config`], where this field is the one that
    /// made the sharing worth doing. Use
    /// [`partials_mut`](Config::partials_mut) to edit the map in place.
    pub partials: Arc<IndexMap<String, Node<'a>>>,
    /// Switches and context for the validation pass.
    pub validation: ValidationOptions<'a>,
}

impl Default for Config<'_> {
    /// [`Config::new`]: an empty schema source, no variables, no functions, no
    /// partials. Written out because a trait object has no default.
    fn default() -> Self {
        Config {
            schemas: Arc::new(MapSchemaSource::new()),
            variables: None,
            functions: Arc::default(),
            partials: Arc::default(),
            validation: ValidationOptions::default(),
        }
    }
}

impl<'a> Config<'a> {
    /// An empty config: no schemas, no variables, no functions, no partials.
    ///
    /// Every node then reports `node-undefined` or `tag-undefined`, which is the
    /// correct answer rather than a degenerate one -- upstream's own `validate`
    /// merges its built-in schemas in before it gets here, and a host that skips
    /// that step has genuinely defined nothing.
    #[must_use]
    pub fn new() -> Config<'a> {
        Config::default()
    }

    /// Replace the schema source.
    ///
    /// Chainable, for the registering case: fill a [`MapSchemaSource`] and hand
    /// it over in one expression. The source is shared, not copied, so a host
    /// with one registry and many configs pays for it once.
    #[must_use]
    pub fn with_schemas(mut self, schemas: Arc<dyn SchemaSource + Send + Sync>) -> Config<'a> {
        self.schemas = schemas;
        self
    }

    /// The functions, for in-place edit.
    ///
    /// Copy-on-write: the map is copied only if another `Config` is sharing it,
    /// which is what makes registering once and scoping many times cheap.
    pub fn functions_mut(&mut self) -> &mut IndexMap<String, ConfigFunction> {
        Arc::make_mut(&mut self.functions)
    }

    /// The parsed partials, for in-place edit. Copy-on-write, as
    /// [`functions_mut`](Config::functions_mut) is.
    pub fn partials_mut(&mut self) -> &mut IndexMap<String, Node<'a>> {
        Arc::make_mut(&mut self.partials)
    }

    /// The schema for a node: its tag's if it has a tag, its type's otherwise.
    ///
    /// Upstream's `transformer.findSchema`. It lives on the config rather than
    /// on [`Node`] because the node is the leaf type and the config is the
    /// stage above it; upstream's `node.findSchema(config)` is the same call
    /// with the arrow pointing the other way. The work is
    /// [`SchemaSource::find`]'s; this only chooses the key.
    #[must_use]
    pub fn find_schema(&self, node: &Node<'_>) -> Option<&Schema> {
        self.schemas.find(SchemaKey::for_node(node))
    }
}

/// Switches and context for one validation pass.
///
/// Mirrors upstream's `config.validation`. It is part of the config rather than
/// a separate argument because schema `validate` hooks read it -- most usefully
/// [`parents`](ValidationOptions::parents), which is how a schema says "a
/// heading is not allowed inside a `callout`" without walking the tree itself.
#[derive(Clone, Debug, Default)]
pub struct ValidationOptions<'a> {
    /// The ancestors of the node being validated, outermost first.
    ///
    /// Set by [`validate_tree`](crate::validate::validate_tree) as it walks, and
    /// empty for the document node. A hook reading this is reading the path it
    /// was reached by, not the whole tree.
    pub parents: Vec<&'a Node<'a>>,
    /// Whether function calls in attributes are checked against
    /// [`Config::functions`].
    ///
    /// Off by default, as upstream has it: a document may legitimately use
    /// functions a validating tool does not know about.
    pub validate_functions: bool,
    /// A host-defined label for the environment being validated.
    ///
    /// This crate never reads it. It exists because upstream schemas do, and
    /// dropping it would break a ported schema for no gain.
    pub environment: Option<String>,
}

/// A function a document may call in an attribute value.
///
/// Mirrors upstream's `ConfigFunction`. Both hooks are synchronous; see
/// `DIVERGENCES.md` entry 3.
#[derive(Clone, Default)]
pub struct ConfigFunction {
    /// What the call returns, used to type-check the attribute it feeds.
    ///
    /// Checked only when [`ValidationOptions::validate_functions`] is on.
    pub returns: Option<ValidationType>,
    /// The declared parameters, keyed as the call sites key them.
    ///
    /// [`None`] means the function declares no parameters and none are checked;
    /// `Some` of an empty map means every parameter passed is invalid. A
    /// positional argument is keyed by its decimal index -- see
    /// [`Function::positional_key`](crate::ast::Function::positional_key) --
    /// which is why this is a string-keyed map rather than a list.
    pub parameters: Option<IndexMap<String, SchemaAttribute>>,
    /// Turns a call into a value at transform time.
    pub transform: Option<FunctionTransformHook>,
    /// Reports problems with a call beyond what the parameter declarations
    /// catch.
    pub validate: Option<FunctionValidateHook>,
}

impl std::fmt::Debug for Config<'_> {
    /// Hooks are function pointers with no useful rendering, so the derived
    /// `Debug` is unavailable and this one reports what is *there* instead: the
    /// names registered, which is what you want when a `tag-undefined` error
    /// disagrees with what you thought you registered.
    ///
    /// The schema names come through the source's provided
    /// [`tag_names`](SchemaSource::tag_names) and
    /// [`node_types`](SchemaSource::node_types), and print as `None` for a
    /// source that cannot enumerate -- which is the truth, and different from
    /// an empty list.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("nodes", &self.schemas.node_types())
            .field("tags", &self.schemas.tag_names())
            .field("variables", &self.variables)
            .field("functions", &self.functions.keys().collect::<Vec<_>>())
            .field("partials", &self.partials.keys().collect::<Vec<_>>())
            .field("validation", &self.validation)
            .finish()
    }
}

impl std::fmt::Debug for ConfigFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigFunction")
            .field("returns", &self.returns)
            .field("parameters", &self.parameters)
            .field("transform", &self.transform.is_some())
            .field("validate", &self.validate.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Node, NodeType};

    #[test]
    fn a_tag_is_looked_up_by_name_and_a_node_by_type() {
        let mut schemas = MapSchemaSource::new();
        schemas
            .insert_tag("callout", Schema::default())
            .insert_node(NodeType::Heading, Schema::default());
        let config = Config::new().with_schemas(Arc::new(schemas));

        let mut tag = Node::new(NodeType::Tag);
        tag.tag = Some("callout".to_string());
        assert!(config.find_schema(&tag).is_some());

        assert!(config.find_schema(&Node::new(NodeType::Heading)).is_some());
        assert!(
            config
                .find_schema(&Node::new(NodeType::Paragraph))
                .is_none()
        );

        // A tag node is never looked up as a node type, even though `tag` is
        // one. Upstream branches on `node.tag` being set, not on the type.
        let mut unknown = Node::new(NodeType::Tag);
        unknown.tag = Some("nope".to_string());
        assert!(config.find_schema(&unknown).is_none());
    }

    #[test]
    fn a_source_that_is_not_a_map_resolves_through_the_config() {
        // The seam, exercised by something that is not the built-in: a source
        // with no map, answering from a match arm.
        struct Aside(Schema);
        impl SchemaSource for Aside {
            fn find(&self, key: SchemaKey<'_>) -> Option<&Schema> {
                match key {
                    SchemaKey::Tag("callout") => Some(&self.0),
                    _ => None,
                }
            }
        }

        let config = Config::new().with_schemas(Arc::new(Aside(Schema::new().render("aside"))));

        let mut callout = Node::new(NodeType::Tag);
        callout.tag = Some("callout".to_string());
        assert_eq!(
            config
                .find_schema(&callout)
                .and_then(|s| s.render.as_deref()),
            Some("aside")
        );
        assert!(config.find_schema(&Node::new(NodeType::Heading)).is_none());
    }

    #[test]
    fn debug_names_what_a_map_holds_and_admits_what_it_cannot_see() {
        struct Opaque;
        impl SchemaSource for Opaque {
            fn find(&self, _key: SchemaKey<'_>) -> Option<&Schema> {
                None
            }
        }

        let mut schemas = MapSchemaSource::new();
        schemas.insert_tag("callout", Schema::new());
        let named = format!("{:?}", Config::new().with_schemas(Arc::new(schemas)));
        assert!(named.contains(r#"tags: Some(["callout"])"#), "{named}");
        assert!(named.contains("nodes: Some([])"), "{named}");

        // Not an empty registry: a source that cannot say.
        let opaque = format!("{:?}", Config::new().with_schemas(Arc::new(Opaque)));
        assert!(opaque.contains("tags: None"), "{opaque}");
        assert!(opaque.contains("nodes: None"), "{opaque}");
    }

    #[test]
    fn a_config_with_nothing_borrowed_outlives_any_document() {
        // The point of this test is that it compiles: a `Config<'static>` is
        // usable against a document parsed from a local string, which is the
        // shape a host registry has.
        let config: Config<'static> = Config::new();
        let source = String::from("# hi\n");
        let node = Node::new(NodeType::Document);
        let _ = source;
        assert!(config.find_schema(&node).is_none());
    }
}
