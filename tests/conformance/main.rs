//! The conformance harness: upstream's corpus, run against this crate.
//!
//! `spec/marktest/tests.yaml` is 105 cases of Markdoc source with the tree, the
//! HTML, or the validation errors it should produce. It is vendored from
//! upstream at the ported revision and never edited. This harness reads it,
//! runs every case, and reports one line:
//!
//! ```text
//! conformance: N green, M annotated, P failing (of 105)
//! ```
//!
//! That number is the epic's shared progress signal, and it is the reason the
//! harness exists before the parser does. A conformance counter written after
//! the implementation grades what was built; one written before it grades what
//! was meant, and it cannot be quietly shaped to fit.
//!
//! # How to read a run
//!
//! ```sh
//! cargo test --test conformance -- --nocapture
//! ```
//!
//! `--nocapture` because a passing test's output is swallowed otherwise, and on
//! a green run the count is the whole point. A failing run prints it anyway.
//!
//! # The three columns
//!
//! - **green** -- matches upstream.
//! - **annotated** -- fails because it exercises a divergence declared in
//!   `DIVERGENCES.md`. Counted apart from failing, so that what was given up
//!   stays visible instead of being absorbed into either neighbour.
//! - **failing** -- everything else: work outstanding.
//!
//! # Layout
//!
//! - [`corpus`] reads the YAML. The only module that knows YAML exists.
//! - [`value`] is the shape a corpus expectation and a rendered tree both
//!   reduce to.
//! - [`engine`] is the seam into `proust`, and the one file a phase of the port
//!   edits.
//! - [`divergence`] lists the annotated cases.
//! - [`diff`] says what differed; [`report`] counts and prints; [`baseline`]
//!   holds the ratchet.

mod baseline;
mod corpus;
mod diff;
mod divergence;
mod engine;
mod report;
mod selfcheck;
mod value;

use corpus::{Case, Renderer};
use report::{CaseResult, Counts, Failure, Status};
use value::Value;

/// The vendored corpus, compiled in.
///
/// `include_str!` rather than a runtime read: the corpus is not optional, a
/// checkout without it is broken rather than unmeasured, and compiling it in
/// makes an edit to the file rebuild the harness that grades it.
const CORPUS: &str = include_str!("../../spec/marktest/tests.yaml");

/// The normative record of what this crate gives up on purpose.
///
/// Read here only to check that it and [`divergence::ANNOTATED`] still describe
/// the same set of cases. Two lists of the same thing in two files drift; the
/// one an author edits is whichever they happened to open.
const DIVERGENCES: &str = include_str!("../../DIVERGENCES.md");

/// The front door, which quotes the count.
///
/// Checked against the run for the same reason the baseline is: a number
/// written by hand in prose is a number that goes stale, and this one is the
/// first thing anyone reads about the crate.
const README: &str = include_str!("../../README.md");

#[test]
fn conformance() {
    let cases = match corpus::load(CORPUS) {
        Ok(cases) => cases,
        Err(e) => panic!("the vendored corpus could not be read: {e}"),
    };

    let results: Vec<CaseResult> = cases.iter().map(run).collect();
    let counts = Counts::of(&results);

    // Printed before anything can fail, so the number is in the log of every
    // run, green or red.
    println!("{}", report::render(&results));

    check_annotations(&results);
    check_readme(counts);

    let recorded = match baseline::read() {
        Ok(recorded) => recorded,
        Err(e) => panic!("{e}"),
    };
    if let Err(mismatch) = baseline::check(counts, recorded) {
        panic!("\n{mismatch}\n");
    }
}

/// Run one case and grade it.
///
/// An annotated case is run like any other. Skipping it would be cheaper and
/// would hide the one thing worth knowing about an annotation: that it has
/// stopped being true.
fn run(case: &Case) -> CaseResult {
    let mut result = CaseResult {
        name: case.name.clone(),
        line: case.line,
        notes: Vec::new(),
        status: Status::Green,
    };

    let failure = match engine::run(case) {
        Ok(outcome) => grade(case, outcome, &mut result.notes),
        Err(unimplemented) => Some(Failure {
            reason: unimplemented.to_string(),
            detail: Vec::new(),
        }),
    };

    result.status = match (divergence::lookup(&case.name), failure) {
        (Some(annotation), failure) => Status::Annotated {
            annotation,
            now_passing: failure.is_none(),
        },
        (None, Some(failure)) => Status::Failing(failure),
        (None, None) => Status::Green,
    };
    result
}

/// Compare what the engine produced against what the corpus expects.
///
/// [`None`] is a pass. Validation messages worth reporting are appended to
/// `notes`, which never decide the result -- upstream prints them and moves on,
/// and `validation: false` suppresses even that.
pub(crate) fn grade(
    case: &Case,
    outcome: engine::Outcome,
    notes: &mut Vec<String>,
) -> Option<Failure> {
    // The corpus says how a case is graded; the engine says what it produced.
    // Comparing the two rather than dispatching on the engine's answer alone is
    // what makes a stage returning the wrong shape a visible failure instead of
    // a case graded by the wrong rule.
    match (&case.expected_error, case.renderer, outcome) {
        // `expectedError` decides the grade on its own: upstream compares the
        // joined validation messages and never looks at the tree, including for
        // the four cases that carry both.
        (Some(expected), _, engine::Outcome::ValidationErrors(actual)) => mismatch(
            "validation errors differ",
            diff::describe_text(expected, &actual),
        ),
        (
            None,
            Renderer::Tree,
            engine::Outcome::Tree {
                children,
                validation,
            },
        ) => {
            if case.report_validation {
                notes.extend(validation);
            }
            // A case with no `expected` is graded against an empty tree, as
            // upstream does. Every case in the corpus has one or an
            // `expectedError`, so this is a guard rather than a path.
            let expected = case.expected.clone().unwrap_or(Value::Seq(Vec::new()));
            mismatch(
                "tree differs from expected",
                diff::describe(&expected, &Value::Seq(children)),
            )
        }
        (None, Renderer::Html, engine::Outcome::Html(actual)) => {
            let Some(Value::Str(expected)) = &case.expected else {
                return Some(Failure {
                    reason: "an html case expects a string".to_string(),
                    detail: vec![format!(
                        "expected is {}",
                        case.expected.as_ref().map_or("absent", Value::kind)
                    )],
                });
            };
            mismatch(
                "html differs from expected",
                diff::describe_text(expected, &actual),
            )
        }
        (expected_error, renderer, produced) => Some(Failure {
            reason: "the engine produced the wrong kind of result for how the case is graded"
                .to_string(),
            detail: vec![format!(
                "graded as {}, engine produced {}",
                match (expected_error, renderer) {
                    (Some(_), _) => "validation errors",
                    (None, Renderer::Tree) => "a tree",
                    (None, Renderer::Html) => "html",
                },
                match produced {
                    engine::Outcome::Tree { .. } => "a tree",
                    engine::Outcome::Html(_) => "html",
                    engine::Outcome::ValidationErrors(_) => "validation errors",
                }
            )],
        }),
    }
}

fn mismatch(reason: &str, detail: Vec<String>) -> Option<Failure> {
    if detail.is_empty() {
        return None;
    }
    Some(Failure {
        reason: reason.to_string(),
        detail,
    })
}

/// The corpus must contain every annotated case, and exactly as many as
/// declared.
///
/// Annotations match by name. A corpus refresh that renames a case would
/// otherwise drop its annotation without a word, and the case would reappear in
/// the failing column with no trace of the decision behind it. Pinning the count
/// turns that silence into an error that explains itself.
fn check_annotations(results: &[CaseResult]) {
    for annotation in divergence::ANNOTATED {
        assert!(
            results.iter().any(|r| r.name == annotation.case),
            "no corpus case is named {:?}, but it is annotated against {}. \
             If the corpus was refreshed and the case renamed, follow the rename; \
             if it was removed upstream, remove the annotation.",
            annotation.case,
            annotation.entry
        );
    }

    // Whitespace-normalised, because the file is prose: a case name that reads
    // as one string here is line-wrapped there, and a check that cannot see
    // through a line break would only teach people to reflow the paragraph.
    let declared = DIVERGENCES.split_whitespace().collect::<Vec<_>>().join(" ");
    for annotation in divergence::ANNOTATED {
        assert!(
            declared.contains(annotation.case),
            "case {:?} is annotated here but DIVERGENCES.md does not mention it. \
             The annotation is the bookkeeping; the file is the declaration, and a \
             case given up without one is a case discovered rather than declared.",
            annotation.case
        );
    }

    let annotated = results
        .iter()
        .filter(|r| matches!(r.status, Status::Annotated { .. }))
        .count();
    assert_eq!(
        annotated,
        divergence::EXPECTED_COUNT,
        "{annotated} cases are annotated, but tests/conformance/divergence.rs declares \
         {}. The corpus and the declared divergences disagree about how much of it is \
         out of reach.",
        divergence::EXPECTED_COUNT
    );
}

/// The README quotes the current count. It has to be the current count.
fn check_readme(counts: Counts) {
    let line = format!("conformance: {counts}");
    assert!(
        README.contains(&line),
        "README.md does not quote this run. Replace the count in its Conformance \
         section with:\n\n    {line}\n"
    );
}
