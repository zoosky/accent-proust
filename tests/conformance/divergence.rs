//! Cases that exercise a declared divergence, and are therefore annotated
//! rather than failed.
//!
//! A case here is not evidence of work outstanding. It is a record of something
//! given up on purpose, with the reason written down in `DIVERGENCES.md`, and it
//! is counted apart from both green and failing so that giving something up
//! stays visible instead of being absorbed into either.
//!
//! Deleting an annotated case from the corpus would be the easy alternative and
//! is exactly what this list exists to avoid: the case stays, upstream's
//! expectation stays readable next to it, and the count says how much of the
//! corpus is out of reach.

/// A corpus case annotated against a divergence.
pub struct Annotation {
    /// The case name, matched exactly. Names are not unique: every case sharing
    /// an annotated name is annotated, which is the intended reading -- the
    /// three cases called "Indented paragraph in a tag" exercise one option
    /// between them.
    pub case: &'static str,
    /// The `DIVERGENCES.md` section this case is charged to.
    pub entry: &'static str,
    /// Why upstream's expectation is unreachable, in one line.
    pub reason: &'static str,
}

/// How many corpus cases are expected to be annotated.
///
/// Asserted, not counted: if a corpus refresh renames a case, its annotation
/// silently stops matching and the case reappears as a failure with no
/// explanation. Pinning the number turns that into an error that names itself.
pub const EXPECTED_COUNT: usize = 6;

const ALLOW_INDENTATION: &str = "DIVERGENCES.md #8 (allowIndentation)";
const ALLOW_INDENTATION_REASON: &str =
    "graded under `allowIndentation: true`, which upstream reaches by patching \
     markdown-it; unreachable above an unpatched CommonMark parser";

/// The annotated cases.
///
/// All six are the same divergence. Upstream's corpus runner constructs its
/// tokenizer with `allowIndentation: true` (`spec/marktest/index.ts:21-24`),
/// which switches off CommonMark's four-space rule across nine block rules, so
/// these cases indent content inside a tag and still expect paragraphs, fences
/// and tables rather than indented code blocks.
pub const ANNOTATED: &[Annotation] = &[
    Annotation {
        case: "Indented paragraph in a tag",
        entry: ALLOW_INDENTATION,
        reason: ALLOW_INDENTATION_REASON,
    },
    Annotation {
        case: "Oddly indented paragraph in a tag",
        entry: ALLOW_INDENTATION,
        reason: ALLOW_INDENTATION_REASON,
    },
    Annotation {
        case: "Indented fence in a tag",
        entry: ALLOW_INDENTATION,
        reason: ALLOW_INDENTATION_REASON,
    },
    Annotation {
        case: "Advanced table with inner content",
        entry: ALLOW_INDENTATION,
        reason: ALLOW_INDENTATION_REASON,
    },
];

/// The annotation for a case name, if it has one.
pub fn lookup(case: &str) -> Option<&'static Annotation> {
    ANNOTATED.iter().find(|a| a.case == case)
}
