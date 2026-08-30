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

use proust::ast::Node;
use proust::parse::{parse_with, ParseOptions, PulldownTokenizer};
use proust::validate::validate_tree;

use crate::config;
use crate::corpus::{Case, Renderer};
use crate::value::Value;

/// What running a case produced.
///
/// No variant is constructed by [`run`] yet -- the stages that produce them are
/// the port. They are constructed by the harness's own tests, which grade every
/// corpus case against its own expectation, so the grading path is exercised on
/// real data before any of it is reachable from the engine.
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
/// Parse and validate are implemented; transform and render are not. So this
/// dispatches on how the corpus grades the case and answers with the first
/// stage that is missing, by name. A blanket "parse is not implemented" told
/// every case the same untrue thing; naming the stage makes the failing column
/// readable as a work list rather than a wall.
///
/// # Why a schema error can still be reported as unimplemented
///
/// Upstream's `Markdoc.validate` merges its built-in node, tag and function
/// schemas into the caller's config before validating (`index.ts`,
/// `mergeConfig`). Those are schema *content* and belong to the transform
/// stage, so the config assembled here from the corpus is the case's own
/// declarations and nothing else -- and a document whose `document` node or
/// `table` tag has no schema reports `node-undefined` or `tag-undefined` for
/// every node upstream had a built-in for.
///
/// Reporting that as a mismatch would bury the six validation cases under a
/// diff about missing built-ins. So an undefined built-in is reported as the
/// missing stage it is, which keeps the failing column a work list: each of
/// those cases turns green when the built-in schemas land, with no further
/// change here.
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
/// Everything graded on a tree or on HTML needs the transform stage, because
/// `expected` in the corpus is the **renderable** tree, not the AST.
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

    match case.renderer {
        Renderer::Tree => Err(Unimplemented {
            stage: "transform",
            phase: "D",
        }),
        Renderer::Html => Err(Unimplemented {
            stage: "transform and the html renderer",
            phase: "D/E",
        }),
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
