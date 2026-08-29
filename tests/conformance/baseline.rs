//! The ratchet.
//!
//! `conformance-baseline.txt` records what the corpus counter stood at when the
//! last change landed, and the harness refuses any run that does not match it
//! exactly. That is a stronger rule than "must not go down", and it is the one
//! the epic asks for: a pull request that turns cases green raises the baseline
//! in the same commit, so the file is a ledger of the port rather than a
//! high-water mark nobody maintains.
//!
//! # Why a ratchet rather than the obvious gate
//!
//! Wiring `105/105` into CI would be simpler and would make every pull request
//! in this repository fail a required check from the day this harness lands
//! until the day the formatter does. A check that is always red is a check
//! nobody reads, and it would destroy the red/green signal exactly while it is
//! the only signal the port has.
//!
//! # Why the baseline is read at runtime
//!
//! The corpus is compiled in with `include_str!`: it must exist, and a missing
//! one is a broken checkout. The baseline is different -- it is legitimately
//! absent exactly once, before the first run, and that run is the one that
//! proves the harness fails at zero rather than passing vacuously. Reading it at
//! runtime is what lets that first run report `0 green` and print the file to
//! write, instead of failing to compile.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::report::Counts;

/// The recorded counts.
#[derive(Clone, Copy, Debug)]
pub struct Baseline {
    /// Cases matching upstream when the baseline was written.
    pub green: usize,
    /// Cases annotated against a divergence.
    pub annotated: usize,
    /// Cases in the corpus.
    pub total: usize,
}

/// Where the baseline lives.
pub fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance-baseline.txt")
}

/// Read the baseline, or [`None`] if the file is not there.
pub fn read() -> Result<Option<Baseline>, String> {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };

    let (mut green, mut annotated, mut total) = (None, None, None);
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(key), Some(value), None) = (fields.next(), fields.next(), fields.next()) else {
            return Err(format!(
                "conformance-baseline.txt:{}: expected `<key> <count>`, got {line:?}",
                number + 1
            ));
        };
        let value = value.parse::<usize>().map_err(|e| {
            format!(
                "conformance-baseline.txt:{}: {value:?} is not a count: {e}",
                number + 1
            )
        })?;
        match key {
            "green" => green = Some(value),
            "annotated" => annotated = Some(value),
            "total" => total = Some(value),
            other => {
                return Err(format!(
                    "conformance-baseline.txt:{}: unknown key {other:?}",
                    number + 1
                ));
            }
        }
    }

    match (green, annotated, total) {
        (Some(green), Some(annotated), Some(total)) => Ok(Some(Baseline {
            green,
            annotated,
            total,
        })),
        _ => Err("conformance-baseline.txt must set green, annotated and total".to_string()),
    }
}

/// The file contents that would record `counts`.
pub fn file_for(counts: Counts) -> String {
    format!(
        "# The conformance ratchet. Written by `cargo test --test conformance`,\n\
         # which fails when the run does not match it exactly.\n\
         #\n\
         # green     cases matching upstream's expectation\n\
         # annotated cases exercising a declared divergence (see DIVERGENCES.md)\n\
         # total     cases in the vendored corpus\n\
         #\n\
         # Raising green is the point. Lowering it is a regression and is not\n\
         # mergeable: a case that should stop being green is a divergence, which\n\
         # means an entry in DIVERGENCES.md and a move to annotated, not a\n\
         # smaller number here.\n\
         green {}\n\
         annotated {}\n\
         total {}\n",
        counts.green, counts.annotated, counts.total
    )
}

/// Why a run did not satisfy the ratchet.
pub struct Mismatch {
    /// The explanation, already formatted.
    message: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Compare a run against the baseline.
pub fn check(counts: Counts, baseline: Option<Baseline>) -> Result<(), Mismatch> {
    let Some(baseline) = baseline else {
        return Err(Mismatch {
            message: format!(
                "There is no conformance-baseline.txt.\n\n\
                 This run scored {counts}. If that is the number this crate should be \
                 held to from now on, write it to {}:\n\n{}",
                path().display(),
                indent(&file_for(counts))
            ),
        });
    };

    if counts.total != baseline.total {
        return Err(Mismatch {
            message: format!(
                "The corpus changed size: {} cases, baseline says {}.\n\n\
                 The corpus is vendored and is never edited in place, so this means it was \
                 refreshed from a newer upstream revision. That is a deliberate act: update \
                 spec/UPSTREAM.md with the new revision and rewrite the baseline in the same \
                 commit, so the diff shows what upstream changed.",
                counts.total, baseline.total
            ),
        });
    }

    if counts.green < baseline.green {
        return Err(Mismatch {
            message: format!(
                "Conformance regressed: {} green, baseline {}.\n\n\
                 A change that turns a green case red is not mergeable, whatever else it \
                 fixes. If the case should stop passing, that is a divergence: write the \
                 entry in DIVERGENCES.md, annotate the case in tests/conformance/divergence.rs, \
                 and it moves to the annotated column instead of disappearing from the green \
                 one.\n\n\
                 The failing cases are listed above.",
                counts.green, baseline.green
            ),
        });
    }

    if counts.annotated > baseline.annotated {
        return Err(Mismatch {
            message: format!(
                "More cases are annotated than the baseline records: {} against {}.\n\n\
                 Annotating a case gives something up. It is allowed, and it is exactly why \
                 the column exists -- but it is declared, never absorbed: the entry goes in \
                 DIVERGENCES.md and the count goes in the baseline, in the pull request that \
                 does it.",
                counts.annotated, baseline.annotated
            ),
        });
    }

    if counts.green != baseline.green || counts.annotated != baseline.annotated {
        return Err(Mismatch {
            message: format!(
                "Conformance moved: {counts}.\n\n\
                 Baseline says {} green, {} annotated. If that is what this commit meant to \
                 do -- cases turned green, or a divergence resolved and its annotation \
                 removed -- raise the ledger in the same commit; a baseline that lags is one \
                 nobody trusts to catch a regression. If it is not, the number is telling you \
                 something: a case that leaves the annotated column without turning green has \
                 gone to the failing one, and it is listed above.\n\n\
                 Write to {}:\n\n{}",
                baseline.green,
                baseline.annotated,
                path().display(),
                indent(&file_for(counts))
            ),
        });
    }

    Ok(())
}

fn indent(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out
}
