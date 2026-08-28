//! Counting and printing.
//!
//! The report is the epic's shared progress signal, so its shape is fixed:
//!
//! ```text
//! conformance: N green, M annotated, P failing (of 105)
//! ```
//!
//! Green and annotated are counted separately and never merged. A case sliding
//! from green to annotated is a thing that was working and is now given up; if
//! the two shared a column that slide would be invisible, which is precisely the
//! event most worth seeing.

use std::fmt;

use crate::divergence::Annotation;

/// What became of one case.
pub enum Status {
    /// Matched upstream's expectation.
    Green,
    /// Exercises a declared divergence.
    Annotated {
        /// What was given up, and why.
        annotation: &'static Annotation,
        /// Whether the case nevertheless matches upstream.
        ///
        /// An annotated case is still run. If it passes, the divergence may
        /// have stopped applying -- an implementation reached the behaviour by
        /// another route, or upstream's expectation changed under a corpus
        /// refresh -- and the annotation is then a claim the code no longer
        /// supports. Not an error: a prompt to re-read the entry.
        now_passing: bool,
    },
    /// Did not match, and no divergence explains it.
    Failing(Failure),
}

/// A case that did not match.
pub struct Failure {
    /// One line, shared by every case that failed the same way, so the report
    /// can group by it. "parse is not implemented" is one reason; a mismatched
    /// tree is another.
    pub reason: String,
    /// What differed, for this case specifically.
    pub detail: Vec<String>,
}

/// A case and its outcome.
pub struct CaseResult {
    /// The case name.
    pub name: String,
    /// The 1-based line in `tests.yaml`.
    pub line: usize,
    /// Validation messages worth reporting, from a case that still passed.
    ///
    /// Upstream prints these and does not fail the case; suppressed entirely by
    /// `validation: false`.
    pub notes: Vec<String>,
    /// The outcome.
    pub status: Status,
}

/// The three numbers, and the total they are of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Counts {
    /// Cases matching upstream.
    pub green: usize,
    /// Cases annotated against a divergence.
    pub annotated: usize,
    /// Cases neither green nor annotated.
    pub failing: usize,
    /// Cases in the corpus.
    pub total: usize,
}

impl Counts {
    /// Count a set of results.
    pub fn of(results: &[CaseResult]) -> Counts {
        let mut counts = Counts {
            green: 0,
            annotated: 0,
            failing: 0,
            total: results.len(),
        };
        for result in results {
            match result.status {
                Status::Green => counts.green += 1,
                Status::Annotated { .. } => counts.annotated += 1,
                Status::Failing(_) => counts.failing += 1,
            }
        }
        counts
    }
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} green, {} annotated, {} failing (of {})",
            self.green, self.annotated, self.failing, self.total
        )
    }
}

/// The full report: the counts, then every case that is not green, named.
pub fn render(results: &[CaseResult]) -> String {
    use fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "conformance: {}", Counts::of(results));

    let notes: Vec<&CaseResult> = results.iter().filter(|r| !r.notes.is_empty()).collect();
    if !notes.is_empty() {
        let _ = writeln!(
            out,
            "\nvalidation errors reported by upstream's runner, which do not decide a case:"
        );
        for result in notes {
            let _ = writeln!(out, "  {}", location(result));
            for note in &result.notes {
                let _ = writeln!(out, "      {note}");
            }
        }
    }

    let annotated: Vec<&CaseResult> = results
        .iter()
        .filter(|r| matches!(r.status, Status::Annotated { .. }))
        .collect();
    if !annotated.is_empty() {
        let _ = writeln!(out, "\nannotated ({}):", annotated.len());
        for result in annotated {
            let Status::Annotated {
                annotation,
                now_passing,
            } = result.status
            else {
                continue;
            };
            let _ = writeln!(out, "  {}", location(result));
            let _ = writeln!(out, "      {}: {}", annotation.entry, annotation.reason);
            if now_passing {
                let _ = writeln!(
                    out,
                    "      NOTE: this case now matches upstream. Re-read the divergence:                      if it no longer applies, remove the entry and the annotation together."
                );
            }
        }
    }

    // Failures are grouped by reason rather than listed flat. Ninety-nine cases
    // failing for one reason is one fact; ninety-nine lines saying so is a
    // wall that hides the second reason when it appears.
    let mut groups: Vec<(&str, Vec<&CaseResult>)> = Vec::new();
    for result in results {
        let Status::Failing(failure) = &result.status else {
            continue;
        };
        if let Some((_, members)) = groups.iter_mut().find(|(r, _)| *r == failure.reason) {
            members.push(result);
        } else {
            groups.push((&failure.reason, vec![result]));
        }
    }
    if !groups.is_empty() {
        let failing: usize = groups.iter().map(|(_, members)| members.len()).sum();
        let _ = writeln!(out, "\nfailing ({failing}):");
        for (reason, members) in groups {
            let _ = writeln!(out, "\n  {reason} -- {} cases", members.len());
            for result in members {
                let _ = writeln!(out, "    {}", location(result));
                if let Status::Failing(failure) = &result.status {
                    for line in &failure.detail {
                        let _ = writeln!(out, "        {line}");
                    }
                }
            }
        }
    }
    out
}

fn location(result: &CaseResult) -> String {
    format!("spec/marktest/tests.yaml:{}  {}", result.line, result.name)
}
