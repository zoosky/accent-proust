//! What a tag or a node type is allowed to be.
//!
//! Mirrors upstream's `Schema`, `SchemaAttribute`, `SchemaSlot` and
//! `SchemaMatches` in `src/types.ts`. This is the crate's central extension
//! point: everything a host teaches `proust` about its own content model
//! arrives as one of these.
//!
//! # Hooks are synchronous
//!
//! Upstream types `transform` and `validate` as `MaybePromise`, so a schema may
//! fetch while it transforms. This crate performs no I/O by construction, so an
//! async hook would be a signature with no reachable implementation that
//! coloured every caller above it. Both are ordinary functions here.
//! `DIVERGENCES.md` entry 3 declares it.
//!
//! # Why the hooks are higher-ranked
//!
//! A hook is written once and run against documents of every lifetime, so its
//! type quantifies over the document's:
//!
//! ```text
//! for<'a, 'c> Fn(&'a Node<'a>, &'c Config<'a>) -> Vec<ValidationError<'a>>
//! ```
//!
//! Only the node's lifetime is tied to the result, which is what lets an error
//! quote the span it found. The config reference is free, which is what lets
//! [`validate_tree`](crate::validate::validate_tree) hand a hook a config it
//! owns for the duration of the walk. An ordinary closure satisfies this
//! without annotation.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::ast::{ErrorLevel, Function, Node, NodeType, ValidationError, Value};
use crate::renderable::RenderableTreeNodes;
use crate::validate::Config;
use crate::validate::ValidationType;

/// Turns a node into renderable output.
///
/// Upstream `Schema['transform']`, minus the promise.
pub type TransformHook = Arc<
    dyn for<'a, 'c> Fn(&'a Node<'a>, &'c Config<'a>) -> RenderableTreeNodes + Send + Sync + 'static,
>;

/// Reports problems with a node that the declarative fields cannot express.
///
/// Upstream `Schema['validate']`, minus the promise.
pub type ValidateHook = Arc<
    dyn for<'a, 'c> Fn(&'a Node<'a>, &'c Config<'a>) -> Vec<ValidationError<'a>>
        + Send
        + Sync
        + 'static,
>;

/// Reports problems with one attribute value.
///
/// Upstream `SchemaAttribute['validate']`. The name is passed through because
/// the message usually quotes it -- `The value of 'bar.baz' must be ...` -- and
/// one function is often registered for several attributes.
pub type AttributeValidateHook = Arc<
    dyn for<'a, 'c> Fn(&Value, &'c Config<'a>, &str) -> Vec<ValidationError<'a>>
        + Send
        + Sync
        + 'static,
>;

/// Computes the allowed values for an attribute from the config.
///
/// Upstream spells `matches` as `SchemaMatches | ((config) => SchemaMatches)`.
/// A dynamic list is how a host says "one of the pages that exist", which it
/// cannot know when the schema is written.
pub type MatchesHook =
    Arc<dyn for<'a, 'c> Fn(&'c Config<'a>) -> Option<SchemaMatches> + Send + Sync + 'static>;

/// Turns a resolved call into a value.
///
/// Upstream `ConfigFunction['transform']`. The parameters arrive resolved and
/// keyed exactly as the call site keyed them.
pub type FunctionTransformHook = Arc<
    dyn for<'a, 'c> Fn(&IndexMap<String, Value>, &'c Config<'a>) -> Value + Send + Sync + 'static,
>;

/// Reports problems with a call that its parameter declarations cannot express.
///
/// Upstream `ConfigFunction['validate']`.
pub type FunctionValidateHook = Arc<
    dyn for<'a, 'c> Fn(&Function, &'c Config<'a>) -> Vec<ValidationError<'a>>
        + Send
        + Sync
        + 'static,
>;

/// Whether a value reaches the rendered output, and under what name.
///
/// Upstream spells this `render?: boolean | string` on both attributes and
/// slots. Three states, and the third is the one a union of two Rust types
/// would lose: `true` and absent both mean "render under the authored name",
/// `false` means "this is schema input, not output", and a string renames it --
/// which is how a Markdoc attribute becomes a differently spelled HTML one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RenderPolicy {
    /// Render under the authored name. Upstream's `true`, and its default.
    #[default]
    Named,
    /// Do not render. Upstream's `false`.
    Hidden,
    /// Render under a different name. Upstream's string form.
    Renamed(String),
}

impl RenderPolicy {
    /// The output name for a value authored as `key`, or [`None`] if it is
    /// hidden.
    #[must_use]
    pub fn output_name<'k>(&'k self, key: &'k str) -> Option<&'k str> {
        match self {
            RenderPolicy::Named => Some(key),
            RenderPolicy::Hidden => None,
            RenderPolicy::Renamed(name) => Some(name.as_str()),
        }
    }
}

/// A closed set of values an attribute may take.
///
/// Upstream's `SchemaMatches = RegExp | string[] | null`, with `null` spelled
/// here as [`Option::None`] on the field.
#[derive(Clone)]
#[non_exhaustive]
pub enum SchemaMatches {
    /// One of these exact strings.
    ///
    /// Compared as upstream compares them, with `Array.includes`: a
    /// non-string attribute value is never a member, so `{% foo bar=1 %}`
    /// against `["1"]` is invalid.
    Values(Vec<String>),
    /// Whatever a host-supplied pattern accepts.
    ///
    /// Upstream takes a JavaScript `RegExp`. This crate has no regular
    /// expression engine and does not want one -- see `DIVERGENCES.md` entry 12
    /// -- so a host supplies the predicate and the spelling the error message
    /// quotes.
    Pattern(Arc<dyn MatchPattern + Send + Sync + 'static>),
    /// Computed from the config each time the attribute is checked.
    ///
    /// Resolved once. If a hook returns another hook, no check runs, which is
    /// what upstream does: after one resolution it tests `Array.isArray` and
    /// `instanceof RegExp`, and a function is neither.
    Dynamic(MatchesHook),
}

/// A host-supplied pattern, standing in for upstream's `RegExp`.
///
/// Two methods because upstream's error message interpolates the pattern
/// itself: `Attribute 'x' must match /^a/. Got 'b' instead.` A predicate alone
/// could decide the check but not write the message.
pub trait MatchPattern {
    /// Whether the value matches.
    ///
    /// The value arrives already coerced to a string the way JavaScript's
    /// `RegExp.test` coerces its argument, so a pattern never has to think
    /// about the value lattice.
    fn is_match(&self, value: &str) -> bool;

    /// How the pattern is written in an error message.
    ///
    /// Upstream renders a `RegExp` as `/source/flags`. A host porting a Markdoc
    /// schema should spell it the same way, so that tooling reading the message
    /// sees what it saw before.
    fn display(&self) -> &str;
}

/// One attribute of a tag or node.
///
/// Mirrors upstream's `SchemaAttribute`. Every field is optional there and
/// carries its own default here; the two that are not plain [`Option`] are
/// [`render`](SchemaAttribute::render), which has three states rather than two,
/// and [`required`](SchemaAttribute::required), where absent and `false` mean
/// the same thing.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct SchemaAttribute {
    /// The type the value must have.
    ///
    /// [`None`] accepts anything, which is upstream's behaviour for an
    /// attribute declared without a `type`.
    pub attribute_type: Option<ValidationType>,
    /// Whether and how the value reaches the output.
    pub render: RenderPolicy,
    /// The value used when the attribute is absent.
    ///
    /// Applied by the transformer, not the validator: a default does not
    /// satisfy [`required`](SchemaAttribute::required), which is upstream's
    /// behaviour and is the reason the two can be set together without
    /// contradiction.
    pub default: Option<Value>,
    /// Whether the attribute must be present.
    pub required: bool,
    /// A closed set of acceptable values.
    pub matches: Option<SchemaMatches>,
    /// A host check that the declarative fields cannot express.
    pub validate: Option<AttributeValidateHook>,
    /// The level reported for a type or value mismatch on this attribute.
    ///
    /// [`None`] means [`ErrorLevel::Error`]. Lowering it to a warning is how a
    /// host ships a rule it wants surfaced but not enforced yet.
    pub error_level: Option<ErrorLevel>,
    /// Prose for a generated schema reference.
    pub description: Option<String>,
}

/// One named region inside a tag.
///
/// Mirrors upstream's `SchemaSlot`. A slot is content, not a value, so it has
/// no type and no matches -- only whether it is required and whether it renders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SchemaSlot {
    /// Whether and how the rendered slot reaches the output.
    ///
    /// A rendered slot becomes an *attribute* of the transformed tag holding
    /// the slot's own transformed nodes, which is why
    /// [`Tag::attributes`](crate::renderable::Tag::attributes) is not a map of
    /// scalars.
    pub render: RenderPolicy,
    /// Whether the slot must be filled.
    pub required: bool,
}

/// What a tag or a node type is allowed to be, and what it becomes.
///
/// Mirrors upstream's `Schema`. The declarative fields are checked by
/// [`validator`](crate::validate::validator) in the order upstream checks them,
/// because the order is observable: a document with several problems reports
/// them in one sequence, and a tool diffing that sequence would see a reordering
/// as a change.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct Schema {
    /// The element name the transformer emits.
    ///
    /// [`None`] means the node renders as its children with no element around
    /// them, which is how `document` and `inline` disappear from the output.
    pub render: Option<String>,
    /// The node types allowed as children.
    ///
    /// [`None`] allows anything; `Some` of an empty list allows nothing. A
    /// child of type [`NodeType::Error`] is never reported, because the parser
    /// has already said what is wrong with it and a second complaint about its
    /// placement is noise.
    pub children: Option<Vec<NodeType>>,
    /// The declared attributes, in authored order.
    pub attributes: IndexMap<String, SchemaAttribute>,
    /// The declared slots, in authored order.
    pub slots: IndexMap<String, SchemaSlot>,
    /// Whether the tag must be written `{% foo /%}` with no children.
    pub self_closing: bool,
    /// Whether the tag must appear inline or as a block.
    ///
    /// [`None`] allows either, which is upstream's default and is why the
    /// three-state spelling is needed: `Some(false)` means "block only", not
    /// "no opinion".
    pub inline: Option<bool>,
    /// Turns the node into renderable output.
    pub transform: Option<TransformHook>,
    /// Reports problems the declarative fields cannot express.
    pub validate: Option<ValidateHook>,
    /// Prose for a generated schema reference.
    pub description: Option<String>,
}

impl Schema {
    /// A schema that constrains nothing and renders nothing.
    ///
    /// Useful as a base to fill in, and as the honest answer for a tag a host
    /// wants accepted but has nothing to say about.
    #[must_use]
    pub fn new() -> Schema {
        Schema::default()
    }

    /// The same schema, rendering as `element`.
    #[must_use]
    pub fn render(mut self, element: impl Into<String>) -> Schema {
        self.render = Some(element.into());
        self
    }

    /// The same schema with one more attribute.
    #[must_use]
    pub fn attribute(mut self, name: impl Into<String>, attribute: SchemaAttribute) -> Schema {
        self.attributes.insert(name.into(), attribute);
        self
    }

    /// The same schema with one more slot.
    #[must_use]
    pub fn slot(mut self, name: impl Into<String>, slot: SchemaSlot) -> Schema {
        self.slots.insert(name.into(), slot);
        self
    }
}

impl std::fmt::Debug for Schema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Schema")
            .field("render", &self.render)
            .field("children", &self.children)
            .field("attributes", &self.attributes)
            .field("slots", &self.slots)
            .field("self_closing", &self.self_closing)
            .field("inline", &self.inline)
            .field("transform", &self.transform.is_some())
            .field("validate", &self.validate.is_some())
            .field("description", &self.description)
            .finish()
    }
}

impl std::fmt::Debug for SchemaAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaAttribute")
            .field("attribute_type", &self.attribute_type)
            .field("render", &self.render)
            .field("default", &self.default)
            .field("required", &self.required)
            .field("matches", &self.matches)
            .field("validate", &self.validate.is_some())
            .field("error_level", &self.error_level)
            .field("description", &self.description)
            .finish()
    }
}

impl std::fmt::Debug for SchemaMatches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaMatches::Values(values) => f.debug_tuple("Values").field(values).finish(),
            SchemaMatches::Pattern(pattern) => {
                f.debug_tuple("Pattern").field(&pattern.display()).finish()
            }
            SchemaMatches::Dynamic(_) => f.write_str("Dynamic(..)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_render_policy_has_three_states() {
        assert_eq!(RenderPolicy::default().output_name("id"), Some("id"));
        assert_eq!(RenderPolicy::Hidden.output_name("id"), None);
        assert_eq!(
            RenderPolicy::Renamed("data-id".to_string()).output_name("id"),
            Some("data-id")
        );
    }

    #[test]
    fn a_hook_is_an_ordinary_closure() {
        // The point of this test is that it compiles without a lifetime
        // annotation on the closure: the higher-ranked hook types are only
        // worth having if a host can write one by hand.
        let schema = Schema {
            validate: Some(Arc::new(|node: &Node<'_>, _config: &Config<'_>| {
                vec![ValidationError::new(
                    "example",
                    ErrorLevel::Warning,
                    format!("saw {}", node.name()),
                )]
            })),
            ..Schema::new()
        };
        let node = Node::new(NodeType::Paragraph);
        let config = Config::new();
        let hook = schema.validate.expect("just set");
        assert_eq!(hook(&node, &config).first().map(|e| e.id), Some("example"));
    }

    #[test]
    fn an_error_may_quote_the_span_it_found() {
        // The other half of the signature's job: a hook's error borrows the
        // document, so it can carry a location rather than only a message.
        let source = String::from("# heading\n");
        let lines = crate::ast::Lines::new(&source);
        let mut node = Node::new(NodeType::Heading);
        node.location = Some(lines.locate(0..9, None));

        let hook: ValidateHook = Arc::new(|node: &Node<'_>, _config: &Config<'_>| {
            let mut error = ValidationError::new("example", ErrorLevel::Error, "no");
            error.location = node.location;
            vec![error]
        });
        let config = Config::new();
        let errors = hook(&node, &config);
        assert_eq!(
            errors.first().and_then(|e| e.location).map(|l| l.text),
            Some("# heading")
        );
    }

    #[test]
    fn a_schema_builds_by_parts() {
        let schema = Schema::new()
            .render("aside")
            .attribute(
                "type",
                SchemaAttribute {
                    required: true,
                    ..SchemaAttribute::default()
                },
            )
            .slot("footer", SchemaSlot::default());
        assert_eq!(schema.render.as_deref(), Some("aside"));
        assert!(schema.attributes["type"].required);
        assert!(schema.slots.contains_key("footer"));
    }
}
