//! The seam between the corpus and `proust`.
//!
//! Everything else in this harness is finished work: it reads the corpus,
//! compares values, counts, and holds the ratchet. This file is the one that
//! grows, and it is deliberately the only one -- a phase of the port wires its
//! stage in here and changes nothing else.
//!
//! # What each stage must produce
//!
//! Taken from `spec/marktest/index.ts`, which is the definition of how the
//! corpus is graded and does three things worth restating, because each is easy
//! to get subtly wrong when reading the corpus alone:
//!
//! 1. **The tokenizer is not in its default configuration.** It is built with
//!    `allowIndentation: true, allowComments: true`. Comments are an ordinary
//!    feature to port. Indentation is divergence 8 and is why six cases are
//!    annotated rather than failed -- see [`crate::divergence`].
//! 2. **`expectedError` decides the grade on its own.** A case carrying one is
//!    compared on its joined validation messages and its tree is never looked
//!    at, even for the four cases that also carry `expected`.
//! 3. **Validation errors do not fail an ordinary case.** Upstream prints them
//!    and moves on, and `validation: false` suppresses even that. They are a
//!    note attached to a result, never the result.
//!
//! # Why it returns a value rather than being a pipeline
//!
//! The harness never sees `proust`'s types. It asks for an [`Outcome`] and
//! compares it. That keeps the corpus runner from ossifying around whatever the
//! renderable tree looks like this month, and it means a change to the tree's
//! Rust representation touches the conversion in this file rather than the
//! grading in every other one.

// A defect this file finds is a defect in the *harness* -- a config it cannot
// map, a renderable variant it has not been taught -- and the honest response is
// to stop the run and say so. Reporting it as a conformance failure instead
// would put a harness bug in the failing column, where it reads as work
// outstanding on the crate.
#![allow(
    clippy::panic,
    reason = "a harness that cannot grade a case must say so, not grade it wrongly"
)]

use proust::ast::Node;
use proust::parse::{parse_with, ParseOptions, PulldownTokenizer};
use proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar};
use proust::validate::validate_tree;

use crate::config;
use crate::corpus::{Case, Renderer};
use crate::value::Value;

/// What running a case produced.
#[derive(Debug)]
pub enum Outcome {
    /// The renderable tree's children, for a case graded on its tree.
    ///
    /// Carries any validation messages, which are reported alongside the result
    /// and never decide it.
    Tree {
        /// The children of the transformed tree.
        children: Vec<Value>,
        /// Validation messages, one per line, in document order.
        validation: Vec<String>,
    },
    /// Rendered HTML, for a case with `renderer: html`. Compared trimmed.
    Html(String),
    /// Joined validation messages, for a case with `expectedError`.
    ValidationErrors(String),
}

/// A stage the pipeline does not have yet.
///
/// Not an error type. It is the honest answer to "what did this case do?" while
/// the crate is being built, and it is what keeps the harness from reporting a
/// vacuous pass: an unimplemented stage is a failing case, listed by name, with
/// the phase that will fix it.
#[derive(Debug)]
pub struct Unimplemented {
    /// The pipeline stage that is missing.
    pub stage: &'static str,
    /// The epic phase that lands it.
    pub phase: &'static str,
}

impl std::fmt::Display for Unimplemented {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is not implemented (phase {})",
            self.stage, self.phase
        )
    }
}

/// Run one case through `proust`.
///
/// Every stage but the formatter is implemented, and the formatter grades no
/// case. So this dispatches on how the corpus grades the case, and the one
/// answer it still has to give by name is the schema a case declares that the
/// built-ins do not cover.
///
/// # Why an undefined schema is still reported as a missing stage
///
/// Upstream's `Markdoc.validate` merges its built-in node, tag and function
/// schemas into the caller's config before validating (`index.ts`,
/// `mergeConfig`), and so does the config this harness assembles -- it starts
/// from `builtins::config()`. A `node-undefined` or `tag-undefined` that
/// survives that is a schema the *corpus case* relies on and neither the
/// built-ins nor its own `config:` block declares.
///
/// Reporting it as a mismatch would bury the case under a diff about a missing
/// schema rather than about the error it is testing, so it is reported as the
/// missing stage it is.
///
/// # What a parsed document can already grade
///
/// A case whose `expectedError` is a **grammar** error. Upstream's runner joins
/// the messages `validate` returns, and `validate` returns each node's own
/// errors before it consults any schema -- so for a document whose only problem
/// is a tag that does not parse, the parser's output *is* the validator's.
/// Short-circuiting there rather than validating is deliberate: those documents
/// have no schemas either, so validating them would replace an exact match with
/// an `Undefined node` report.
///
/// Everything else is graded on the transformed tree -- `expected` in the corpus
/// is the *renderable* tree, not the AST -- or on the HTML rendered from it.
pub fn run(case: &Case) -> Result<Outcome, Unimplemented> {
    // The corpus is graded under a non-default configuration, and this is where
    // that is honoured: `spec/marktest/index.ts:21-24` builds its tokenizer with
    // `allowComments: true`. The other option it sets, `allowIndentation`, is
    // divergence 8 and has nowhere to be set.
    let options = ParseOptions::new().allow_comments(true).slots(case.slots);
    let document = parse_with(&case.code, &PulldownTokenizer::new(), &options);

    if case.expected_error.is_some() {
        let messages = parse_errors(&document);
        if !messages.is_empty() {
            return Ok(Outcome::ValidationErrors(messages.join("\n")));
        }

        // A config that fails to map is a defect in this harness, and
        // `check_configs` fails the run with the reason. Falling back to an
        // empty one here keeps that the single place it is reported.
        let config = config::build(case).unwrap_or_default();
        let found = validate_tree(&document, &config);
        if found
            .iter()
            .any(|found| matches!(found.error.id, "node-undefined" | "tag-undefined"))
        {
            return Err(Unimplemented {
                stage: "the built-in node and tag schemas",
                phase: "D",
            });
        }
        return Ok(Outcome::ValidationErrors(
            found
                .iter()
                .map(|found| found.error.message.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }

    let config = match config::build(case) {
        Ok(config) => config,
        Err(e) => panic!(
            "{}: its config block could not be translated: {e}",
            case.name
        ),
    };
    let transformed = proust::transform::transform(&document, &config);

    match case.renderer {
        // `render_all` takes the list form, and `into_vec` is the flattening
        // upstream's renderer does when it is handed the union's array arm.
        Renderer::Html => Ok(Outcome::Html(proust::render::render_all(
            &transformed.into_vec(),
        ))),
        Renderer::Tree => Ok(Outcome::Tree {
            children: article_children(transformed),
            // Schema validation is Goal C's. What the parser found is reported
            // meanwhile, which is honest rather than partial: these are exactly
            // the errors upstream's `validate` collects before it consults a
            // schema.
            validation: parse_errors(&document),
        }),
    }
}

/// The children of the `<article>` a transformed document renders as.
///
/// Upstream's runner compares `output.children`, where `output` is the whole
/// document put through its React renderer. Everything the corpus grades lives
/// one level inside the root element, so the root is unwrapped -- and its
/// absence is an empty tree rather than an error, which is upstream's
/// `output.children || []`.
fn article_children(nodes: RenderableTreeNodes) -> Vec<Value> {
    match react(nodes).get("children") {
        Some(Value::Seq(children)) => children.clone(),
        _ => Vec::new(),
    }
}

/// The renderable tree as upstream's React shim renders it.
///
/// This is the part of the grading that the corpus does not show, and it is
/// three rules from `renderers/react/react.ts` plus the shim in
/// `spec/marktest/react-shim.ts`:
///
/// 1. **`class` is renamed `className`** and moved to the end of the attribute
///    list, and a falsy `class` is dropped entirely.
/// 2. **An empty attribute map is omitted**, not rendered as `{}`. React is
///    handed `null` for it, and the shim only sets the key for a truthy value.
/// 3. **An empty child list is omitted**, for the same reason.
///
/// Ignoring any of the three turns most of the corpus red for a difference that
/// is in the shim rather than in the tree.
///
/// A fourth rule is the one easiest to miss, because it is a *difference*
/// between two paths that look alike. A list of nodes reached as a **child**
/// goes through `render`, which wraps it in a `Fragment` element; the same list
/// reached as an **attribute value** goes through `deepRender`, which leaves it
/// a plain JSON array. Slots arrive by the second path, so
/// [`react_attribute`] is not [`react`] with a different name.
fn react(nodes: RenderableTreeNodes) -> Value {
    match nodes {
        RenderableTreeNodes::One(node) => react_node(node),
        // A list reaches `React.createElement(Fragment, null, ...)`, which the
        // shim renders as a `Fragment` element.
        RenderableTreeNodes::Many(nodes) => {
            let children: Vec<Value> = nodes.into_iter().map(react_node).collect();
            let mut out = vec![("tag".to_string(), Value::Str("Fragment".to_string()))];
            if !children.is_empty() {
                out.push(("children".to_string(), Value::Seq(children)));
            }
            Value::Map(out)
        }
        // The enums are `#[non_exhaustive]`, which has no effect inside the
        // crate and does here. A variant this harness has not been taught is a
        // change to the renderable tree, and rendering it as nothing would
        // report that as a conformance failure.
        other => panic!("the renderable tree grew a shape this harness cannot grade: {other:?}"),
    }
}

/// One attribute value, as `deepRender` renders it.
///
/// The corpus expects a rendered slot to be a JSON array of elements -- "Basic
/// slot" wants `attributes: {bar: [{tag: p, ...}]}` -- because `deepRender`
/// maps over an array rather than handing it to `React.createElement`.
fn react_attribute(nodes: RenderableTreeNodes) -> Value {
    match nodes {
        RenderableTreeNodes::One(node) => react_node(node),
        RenderableTreeNodes::Many(nodes) => Value::Seq(nodes.into_iter().map(react_node).collect()),
        other => panic!("the renderable tree grew a shape this harness cannot grade: {other:?}"),
    }
}

fn react_node(node: RenderableTreeNode) -> Value {
    // `Tag` carries a manual iterative `Drop`, so its fields are taken rather
    // than moved out -- the same tax `ast::Node` charges, and for the same
    // reason: a renderable tree is as deep as the document that produced it.
    let mut tag = match node {
        RenderableTreeNode::Scalar(value) => return scalar(&value),
        RenderableTreeNode::Tag(tag) => *tag,
        other => {
            panic!("the renderable tree grew a shape this harness cannot grade: {other:?}")
        }
    };

    let mut attributes: Vec<(String, Value)> = Vec::new();
    let mut class_name: Option<Value> = None;
    for (key, value) in std::mem::take(&mut tag.attributes) {
        if key == "class" {
            let rendered = react_attribute(value);
            if truthy(&rendered) {
                class_name = Some(rendered);
            }
            continue;
        }
        attributes.push((key, react_attribute(value)));
    }
    if let Some(class_name) = class_name {
        attributes.push(("className".to_string(), class_name));
    }

    let children: Vec<Value> = std::mem::take(&mut tag.children)
        .into_iter()
        .map(react_node)
        .collect();

    let mut out = vec![("tag".to_string(), Value::Str(std::mem::take(&mut tag.name)))];
    if !attributes.is_empty() {
        out.push(("attributes".to_string(), Value::Map(attributes)));
    }
    if !children.is_empty() {
        out.push(("children".to_string(), Value::Seq(children)));
    }
    Value::Map(out)
}

/// JavaScript's `Boolean()`, which is what `if (className)` applies.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(boolean) => *boolean,
        Value::Int(number) => *number != 0,
        Value::Float(number) => *number != 0.0 && !number.is_nan(),
        Value::Str(text) => !text.is_empty(),
        Value::Seq(_) | Value::Map(_) => true,
    }
}

/// A rendered scalar as the harness's comparison value.
fn scalar(value: &Scalar) -> Value {
    match value {
        Scalar::Null => Value::Null,
        Scalar::Boolean(boolean) => Value::Bool(*boolean),
        Scalar::Number(number) => Value::Float(*number),
        Scalar::String(text) => Value::Str(text.clone()),
        Scalar::Array(items) => Value::Seq(items.iter().map(scalar).collect()),
        Scalar::Object(entries) => Value::Map(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), scalar(value)))
                .collect(),
        ),
        other => panic!("`Scalar` grew a variant this harness cannot grade: {other:?}"),
    }
}

/// Every error the parser itself attached, in document order.
///
/// This is the part of upstream's `validate` that needs no schema: it walks the
/// tree and collects `node.errors` before adding any of its own. Reporting that
/// subset is honest rather than partial -- a case whose expectation also names a
/// schema error will not match it, and will be listed as failing with the
/// difference shown.
fn parse_errors(document: &Node<'_>) -> Vec<String> {
    let mut out: Vec<String> = document
        .errors
        .iter()
        .map(|error| error.message.clone())
        .collect();
    for node in document.walk() {
        out.extend(node.errors.iter().map(|error| error.message.clone()));
    }
    out
}
