//! Where a schema comes from: [`SchemaSource`], the [`SchemaKey`] it is asked
//! with, and [`MapSchemaSource`], the two maps a host fills by hand.
//!
//! Private module; all three are re-exported from [`crate::validate`].

use indexmap::IndexMap;

use crate::ast::{Node, NodeType};
use crate::validate::schema::Schema;

/// What a schema lookup is keyed by: a tag by its name, a node by its type.
///
/// Upstream's `findSchema` branches on whether `node.tag` is set, and so does
/// [`for_node`](SchemaKey::for_node). A tag node is never looked up by its
/// type, even though `tag` is one: the tag's name is the key, and a source
/// that defines no schema for that name has defined nothing for the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchemaKey<'a> {
    /// A tag, by the name written in `{% name %}`.
    Tag(&'a str),
    /// A built-in node, by type.
    Node(NodeType),
}

impl<'a> SchemaKey<'a> {
    /// The key a node is looked up by.
    #[must_use]
    pub fn for_node(node: &'a Node<'_>) -> SchemaKey<'a> {
        match &node.tag {
            Some(tag) => SchemaKey::Tag(tag.as_str()),
            None => SchemaKey::Node(node.node_type),
        }
    }
}

/// Where a schema comes from.
///
/// The validator and the transformer ask one question of a configuration:
/// what is the schema for this node? This trait is that question, and
/// [`Config::schemas`](crate::validate::Config::schemas) holds whatever
/// answers it. [`MapSchemaSource`] is the answer a host assembles by hand;
/// a host with schemas somewhere else -- computed from a component registry,
/// looked up in a sandboxed guest -- implements the trait and hands the
/// config that instead. The crate never learns which.
///
/// # Three properties, each deliberate
///
/// **It is object-safe.** `Config` holds an `Arc<dyn SchemaSource>`, so no
/// generic parameter reaches `Config` or anything holding one.
///
/// **It is synchronous**, for the reason `DIVERGENCES.md` entry 3 gives for
/// schema hooks: the crate performs no I/O, so an async signature would have
/// no reachable implementation and would colour every caller above it.
///
/// **It returns a borrow.** A source that loads a schema from disk on first
/// request cannot implement this, because there is nothing to borrow from
/// after the call returns. The alternative -- `Arc<Schema>` by value -- costs
/// a refcount bump on every lookup in the hot path of both the validator and
/// the transformer, to serve a case no host in this repository has. A host
/// that wants laziness populates its source before handing it over.
///
/// # Implementing it
///
/// [`find`](SchemaSource::find) is the whole contract. [`SchemaKey`] is
/// `#[non_exhaustive]`, as every public enum here is, so an implementation
/// matches it with a wildcard arm that answers `None`; a key it does not
/// know is a schema it does not have, which is the right answer.
///
/// The two provided methods serve diagnostics only. Override them if the
/// source can enumerate what it holds; leave them if it cannot, and
/// `Config`'s `Debug` output says so rather than claiming an empty registry.
///
/// # Examples
///
/// A source that answers from a match arm, with no map at all:
///
/// ```
/// use std::sync::Arc;
///
/// use accent_proust::builtins;
/// use accent_proust::validate::{Schema, SchemaKey, SchemaSource};
///
/// struct OneTag(Schema);
///
/// impl SchemaSource for OneTag {
///     fn find(&self, key: SchemaKey<'_>) -> Option<&Schema> {
///         match key {
///             SchemaKey::Tag("callout") => Some(&self.0),
///             _ => None,
///         }
///     }
/// }
///
/// let config = builtins::config().with_schemas(Arc::new(OneTag(Schema::new().render("aside"))));
/// assert!(config.schemas.find(SchemaKey::Tag("callout")).is_some());
/// assert!(config.schemas.find(SchemaKey::Tag("if")).is_none());
/// ```
pub trait SchemaSource {
    /// The schema for `key`, or `None` when this source does not define one.
    ///
    /// `None` is not an error. The validator reports `tag-undefined` or
    /// `node-undefined` for it, which is the answer a host wants -- upstream's
    /// own `validate` does the same for a name its config lacks.
    fn find(&self, key: SchemaKey<'_>) -> Option<&Schema>;

    /// The tag names this source defines, when it can say.
    ///
    /// Diagnostics only. `None` means "cannot enumerate", which is the honest
    /// answer for a source that computes schemas on demand, and is why this
    /// is an option rather than an empty list.
    fn tag_names(&self) -> Option<Vec<&str>> {
        None
    }

    /// The node types this source defines, when it can say.
    ///
    /// Diagnostics only, as [`tag_names`](SchemaSource::tag_names) is.
    fn node_types(&self) -> Option<Vec<NodeType>> {
        None
    }
}

/// The schemas a host registered, as two maps.
///
/// What `Config` used to hold directly, behind the trait instead: schemas for
/// built-in node types keyed by type, and schemas for tags keyed by name.
/// [`builtin`](MapSchemaSource::builtin) starts both at Markdoc's own, which
/// is where a host that adds a tag or replaces a node schema starts from;
/// [`new`](MapSchemaSource::new) starts both empty, for a host that supplies
/// every schema itself.
///
/// The maps are reachable directly, because they are the whole content and
/// hiding them would only add a method per operation. Fill one, then share it:
///
/// ```
/// use std::sync::Arc;
///
/// use accent_proust::builtins;
/// use accent_proust::validate::{MapSchemaSource, Schema, SchemaKey};
///
/// let mut schemas = MapSchemaSource::builtin();
/// schemas.insert_tag("callout", Schema::new().render("aside"));
///
/// let config = builtins::config().with_schemas(Arc::new(schemas));
/// assert!(config.schemas.find(SchemaKey::Tag("callout")).is_some());
/// assert!(config.schemas.find(SchemaKey::Tag("if")).is_some());
/// ```
#[derive(Clone, Default)]
pub struct MapSchemaSource {
    nodes: IndexMap<NodeType, Schema>,
    tags: IndexMap<String, Schema>,
}

impl MapSchemaSource {
    /// Nothing registered. Every lookup misses, which is what
    /// [`Config::new`](crate::validate::Config::new) has always meant.
    #[must_use]
    pub fn new() -> MapSchemaSource {
        MapSchemaSource::default()
    }

    /// Markdoc's own nodes and tags: what
    /// [`builtins::config`](crate::builtins::config) starts from.
    #[must_use]
    pub fn builtin() -> MapSchemaSource {
        MapSchemaSource {
            nodes: crate::validate::nodes::builtin(),
            tags: crate::tags::builtin(),
        }
    }

    /// Register a tag schema, replacing any under the same name.
    pub fn insert_tag(&mut self, name: impl Into<String>, schema: Schema) -> &mut MapSchemaSource {
        self.tags.insert(name.into(), schema);
        self
    }

    /// Register a node schema, replacing any for the same type.
    pub fn insert_node(&mut self, node_type: NodeType, schema: Schema) -> &mut MapSchemaSource {
        self.nodes.insert(node_type, schema);
        self
    }

    /// The tag schemas, by name.
    #[must_use]
    pub fn tags(&self) -> &IndexMap<String, Schema> {
        &self.tags
    }

    /// The tag schemas, for in-place edit.
    pub fn tags_mut(&mut self) -> &mut IndexMap<String, Schema> {
        &mut self.tags
    }

    /// The node schemas, by type.
    #[must_use]
    pub fn nodes(&self) -> &IndexMap<NodeType, Schema> {
        &self.nodes
    }

    /// The node schemas, for in-place edit.
    pub fn nodes_mut(&mut self) -> &mut IndexMap<NodeType, Schema> {
        &mut self.nodes
    }
}

impl SchemaSource for MapSchemaSource {
    fn find(&self, key: SchemaKey<'_>) -> Option<&Schema> {
        match key {
            SchemaKey::Tag(name) => self.tags.get(name),
            SchemaKey::Node(node_type) => self.nodes.get(&node_type),
        }
    }

    fn tag_names(&self) -> Option<Vec<&str>> {
        Some(self.tags.keys().map(String::as_str).collect())
    }

    fn node_types(&self) -> Option<Vec<NodeType>> {
        Some(self.nodes.keys().copied().collect())
    }
}

impl std::fmt::Debug for MapSchemaSource {
    /// The names registered. A `Schema` carries hooks, which have no useful
    /// rendering, so the derived form is unavailable; the keys are what a
    /// reader chasing a `tag-undefined` wants anyway.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapSchemaSource")
            .field("nodes", &self.nodes.keys().collect::<Vec<_>>())
            .field("tags", &self.tags.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_node_is_keyed_by_name_and_a_plain_node_by_type() {
        let mut tag = Node::new(NodeType::Tag);
        tag.tag = Some("callout".to_string());
        assert_eq!(SchemaKey::for_node(&tag), SchemaKey::Tag("callout"));
        assert_eq!(
            SchemaKey::for_node(&Node::new(NodeType::Heading)),
            SchemaKey::Node(NodeType::Heading)
        );
    }

    #[test]
    fn the_map_source_answers_from_its_maps() {
        let mut schemas = MapSchemaSource::new();
        schemas
            .insert_tag("callout", Schema::new())
            .insert_node(NodeType::Heading, Schema::new());

        assert!(schemas.find(SchemaKey::Tag("callout")).is_some());
        assert!(schemas.find(SchemaKey::Node(NodeType::Heading)).is_some());
        assert!(schemas.find(SchemaKey::Tag("nope")).is_none());
        assert!(schemas.find(SchemaKey::Node(NodeType::Paragraph)).is_none());
        // A tag node is never looked up by its type.
        assert!(schemas.find(SchemaKey::Node(NodeType::Tag)).is_none());
    }

    #[test]
    fn the_map_source_can_enumerate_and_the_default_cannot() {
        struct Opaque;
        impl SchemaSource for Opaque {
            fn find(&self, _key: SchemaKey<'_>) -> Option<&Schema> {
                None
            }
        }

        let mut schemas = MapSchemaSource::new();
        schemas
            .insert_tag("b", Schema::new())
            .insert_tag("a", Schema::new());
        // Authored order, not sorted: the same promise every map here makes.
        assert_eq!(schemas.tag_names(), Some(vec!["b", "a"]));
        assert_eq!(schemas.node_types(), Some(vec![]));

        assert_eq!(Opaque.tag_names(), None);
        assert_eq!(Opaque.node_types(), None);
    }

    #[test]
    fn builtin_is_markdocs_vocabulary() {
        let schemas = MapSchemaSource::builtin();
        assert!(schemas.find(SchemaKey::Tag("if")).is_some());
        assert!(schemas.find(SchemaKey::Tag("partial")).is_some());
        assert!(schemas.find(SchemaKey::Node(NodeType::Heading)).is_some());
        assert!(schemas.find(SchemaKey::Tag("callout")).is_none());
    }
}
