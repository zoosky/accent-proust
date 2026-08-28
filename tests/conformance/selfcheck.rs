//! Tests for the harness itself.
//!
//! The grading code is the part of this repository that is most easily wrong
//! without anyone noticing: while the engine returns "not implemented" for every
//! case, nothing exercises the comparison, and a harness that grades leniently
//! would be discovered only by the phase that trusted it. So the comparison is
//! tested against the corpus directly -- feed a case its own expectation and it
//! must pass; perturb the expectation and it must not.

use crate::corpus::{self, Case, Renderer};
use crate::engine::Outcome;
use crate::value::Value;
use crate::{grade, CORPUS};

// A helper, so outside a `#[test]` function and outside the relaxation in
// `clippy.toml`. The panic is the point: a corpus that will not load is not a
// test failure to report, it is the ground the tests stand on.
#[expect(
    clippy::panic,
    reason = "a test helper fails the way the tests it serves do"
)]
fn cases() -> Vec<Case> {
    match corpus::load(CORPUS) {
        Ok(cases) => cases,
        Err(e) => panic!("the vendored corpus could not be read: {e}"),
    }
}

/// The whole corpus is well-formed, and is the size everything else assumes.
#[test]
fn the_corpus_loads() {
    let cases = cases();
    assert_eq!(cases.len(), 105, "the corpus is 105 cases");
    assert!(
        cases.iter().all(|case| !case.code.is_empty()),
        "every case has source"
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.renderer == Renderer::Html)
            .count(),
        4,
        "four cases are graded on HTML"
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.expected_error.is_some())
            .count(),
        9,
        "nine cases are graded on validation errors"
    );
    assert_eq!(
        cases.iter().filter(|case| case.slots).count(),
        9,
        "nine cases parse with slots enabled"
    );
    assert_eq!(
        cases.iter().filter(|case| !case.report_validation).count(),
        3,
        "three cases suppress validation reporting"
    );
}

/// Handing every case its own expectation back must grade as a pass.
///
/// This is the property the whole harness rests on. If it fails, the comparison
/// is wrong -- not the engine -- and every green case counted afterwards is
/// counted on a broken scale.
#[test]
fn a_case_graded_against_its_own_expectation_passes() {
    for case in cases() {
        let outcome = match (&case.expected_error, case.renderer, &case.expected) {
            (Some(expected), _, _) => Outcome::ValidationErrors(expected.clone()),
            (None, Renderer::Html, Some(Value::Str(expected))) => Outcome::Html(expected.clone()),
            (None, Renderer::Tree, Some(Value::Seq(children))) => Outcome::Tree {
                children: children.clone(),
                validation: Vec::new(),
            },
            (None, renderer, expected) => panic!(
                "{}: a {renderer:?} case expects {:?}",
                case.name,
                expected.as_ref().map(Value::kind)
            ),
        };
        let mut notes = Vec::new();
        let failure = grade(&case, outcome, &mut notes);
        assert!(
            failure.is_none(),
            "{}: grading a case against its own expectation failed: {:?}",
            case.name,
            failure.map(|f| f.detail)
        );
    }
}

/// An empty tree does not pass for a case that expects one.
#[test]
fn a_missing_tree_does_not_pass() {
    let cases = cases();
    let case = cases
        .iter()
        .find(|case| case.name == "Ordered list")
        .unwrap_or_else(|| panic!("the corpus no longer has the case this test uses"));

    let mut notes = Vec::new();
    let failure = grade(
        case,
        Outcome::Tree {
            children: Vec::new(),
            validation: Vec::new(),
        },
        &mut notes,
    );
    let failure = failure.unwrap_or_else(|| panic!("an empty tree graded as a pass"));
    assert_eq!(failure.reason, "tree differs from expected");
    assert!(
        failure.detail.iter().any(|line| line.contains("items")),
        "the diff says the tree is the wrong length: {:?}",
        failure.detail
    );
}

/// Producing the wrong kind of result is a failure, not a mis-grade.
#[test]
fn the_wrong_result_shape_is_a_failure() {
    let cases = cases();
    let case = cases
        .iter()
        .find(|case| case.renderer == Renderer::Tree && case.expected_error.is_none())
        .unwrap_or_else(|| panic!("the corpus has no tree case"));

    let mut notes = Vec::new();
    let failure = grade(
        case,
        Outcome::Html("<p>whatever</p>".to_string()),
        &mut notes,
    )
    .unwrap_or_else(|| panic!("html graded as a pass for a tree case"));
    assert!(
        failure.reason.contains("wrong kind of result"),
        "{}",
        failure.reason
    );
}

/// `validation: false` suppresses the note; anything else keeps it.
#[test]
fn validation_notes_follow_the_corpus_flag() {
    let cases = cases();
    let quiet = cases
        .iter()
        .find(|case| !case.report_validation && case.expected_error.is_none())
        .unwrap_or_else(|| panic!("the corpus has no case with validation: false"));

    let outcome = || Outcome::Tree {
        children: match &quiet.expected {
            Some(Value::Seq(children)) => children.clone(),
            _ => Vec::new(),
        },
        validation: vec!["something was wrong".to_string()],
    };

    let mut notes = Vec::new();
    let _ = grade(quiet, outcome(), &mut notes);
    assert!(
        notes.is_empty(),
        "validation: false still reported {notes:?}"
    );

    let loud = cases
        .iter()
        .find(|case| case.report_validation && case.expected_error.is_none())
        .unwrap_or_else(|| panic!("the corpus has no ordinary case"));
    let mut notes = Vec::new();
    let _ = grade(
        loud,
        Outcome::Tree {
            children: match &loud.expected {
                Some(Value::Seq(children)) => children.clone(),
                _ => Vec::new(),
            },
            validation: vec!["something was wrong".to_string()],
        },
        &mut notes,
    );
    assert_eq!(notes.len(), 1, "an ordinary case reports its validation");
}

/// Attribute order is not a conformance failure; a missing attribute is.
#[test]
fn map_comparison_ignores_key_order_and_nothing_else() {
    let a = Value::Map(vec![
        ("tag".to_string(), Value::Str("h1".to_string())),
        ("id".to_string(), Value::Str("x".to_string())),
    ]);
    let reordered = Value::Map(vec![
        ("id".to_string(), Value::Str("x".to_string())),
        ("tag".to_string(), Value::Str("h1".to_string())),
    ]);
    let short = Value::Map(vec![("tag".to_string(), Value::Str("h1".to_string()))]);

    assert_eq!(a, reordered);
    assert_ne!(a, short);
    assert_eq!(Value::Int(1), Value::Float(1.0));
    assert_ne!(Value::Int(1), Value::Str("1".to_string()));
    assert_ne!(Value::Bool(true), Value::Int(1));
}

/// A difference is reported by path, because a path is what an implementer
/// needs.
#[test]
fn the_diff_names_the_path() {
    let expected = Value::Seq(vec![Value::Map(vec![(
        "attributes".to_string(),
        Value::Map(vec![("id".to_string(), Value::Str("asdf".to_string()))]),
    )])]);
    let actual = Value::Seq(vec![Value::Map(vec![(
        "attributes".to_string(),
        Value::Map(vec![("id".to_string(), Value::Str("other".to_string()))]),
    )])]);

    let differences = crate::diff::describe(&expected, &actual);
    assert_eq!(differences.len(), 1);
    assert!(
        differences[0].starts_with("[0].attributes.id: expected \"asdf\", got \"other\""),
        "{}",
        differences[0]
    );
}
