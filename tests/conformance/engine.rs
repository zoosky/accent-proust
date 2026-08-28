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

use crate::corpus::Case;
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
/// Today every case returns [`Unimplemented`]: the crate is a scaffold, and the
/// harness exists before the parser on purpose -- a conformance counter written
/// after the fact grades what was built, and one written before it grades what
/// was meant.
///
/// The order the stages arrive in is the order of the phases: the tag-internals
/// grammar and the tokenizer (A and B) make `parse` answerable, at which point
/// this function starts returning [`Outcome::Tree`] for cases that need nothing
/// further, and the counter starts moving.
pub fn run(_case: &Case) -> Result<Outcome, Unimplemented> {
    Err(Unimplemented {
        stage: "parse",
        phase: "A/B",
    })
}
