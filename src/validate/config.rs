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
//! The other three are plain maps. A missing schema and an empty schema map
//! produce the same `Undefined tag` error, so there is nothing for an
//! [`Option`] to distinguish.
//!
//! # Where the content comes from
//!
//! Nowhere in this crate. A `Config` is assembled by the host -- from a file, a
//! constant, a plugin manifest -- and handed in. This module owns the *shape*
//! only, which is the line that lets `proust` be published without shipping
//! anybody's schemas.

use indexmap::IndexMap;

use crate::ast::{Node, NodeType, Value};
use crate::validate::schema::{FunctionTransformHook, FunctionValidateHook, Schema};
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
#[derive(Clone, Default)]
pub struct Config<'a> {
    /// Schemas for built-in node types, keyed by type.
    pub nodes: IndexMap<NodeType, Schema>,
    /// Schemas for tags, keyed by tag name.
    pub tags: IndexMap<String, Schema>,
    /// Variables a `$name` reference resolves against.
    ///
    /// [`None`] switches variable checking off; `Some` of an empty map switches
    /// it on with nothing defined.
    pub variables: Option<Variables>,
    /// Functions a `f()` call resolves against.
    pub functions: IndexMap<String, ConfigFunction>,
    /// Parsed partial documents, keyed by the name `{% partial file=... %}`
    /// uses.
    ///
    /// Parsed, not raw: this crate performs no I/O, so a host reads the file and
    /// parses it. That is why the config carries a lifetime.
    pub partials: IndexMap<String, Node<'a>>,
    /// Switches and context for the validation pass.
    pub validation: ValidationOptions<'a>,
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

    /// The schema for a node: its tag's if it has a tag, its type's otherwise.
    ///
    /// Upstream's `transformer.findSchema`. It lives on the config rather than
    /// on [`Node`] because the node is the leaf type and the config is the
    /// stage above it; upstream's `node.findSchema(config)` is the same call
    /// with the arrow pointing the other way.
    #[must_use]
    pub fn find_schema(&self, node: &Node<'_>) -> Option<&Schema> {
        match &node.tag {
            Some(tag) => self.tags.get(tag.as_str()),
            None => self.nodes.get(&node.node_type),
        }
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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("nodes", &self.nodes.keys().collect::<Vec<_>>())
            .field("tags", &self.tags.keys().collect::<Vec<_>>())
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
    use crate::ast::Node;

    #[test]
    fn a_tag_is_looked_up_by_name_and_a_node_by_type() {
        let mut config = Config::new();
        config.tags.insert("callout".to_string(), Schema::default());
        config.nodes.insert(NodeType::Heading, Schema::default());

        let mut tag = Node::new(NodeType::Tag);
        tag.tag = Some("callout".to_string());
        assert!(config.find_schema(&tag).is_some());

        assert!(config.find_schema(&Node::new(NodeType::Heading)).is_some());
        assert!(config
            .find_schema(&Node::new(NodeType::Paragraph))
            .is_none());

        // A tag node is never looked up as a node type, even though `tag` is
        // one. Upstream branches on `node.tag` being set, not on the type.
        let mut unknown = Node::new(NodeType::Tag);
        unknown.tag = Some("nope".to_string());
        assert!(config.find_schema(&unknown).is_none());
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
