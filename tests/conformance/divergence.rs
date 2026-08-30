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
pub const EXPECTED_COUNT: usize = 10;

const ALLOW_INDENTATION: &str = "DIVERGENCES.md #8 (allowIndentation)";
const LITERAL_FENCES: &str = "DIVERGENCES.md #1 (fences do not process tags by default)";
const LITERAL_FENCES_REASON: &str =
    "expects a fence with no `process` annotation to have its content split into \
     text and tag children, which is upstream's default and the inverse of this \
     crate's";
const LITERAL_FENCES_UNHOOKED_REASON: &str =
    "replaces the `fence` schema with one carrying no transform hook, so the \
     fence renders its children -- which upstream has, because its fences \
     process tags by default, and this crate's do not";
const NO_FRONTMATTER: &str = "DIVERGENCES.md #7 (metadata blocks are the host's)";
const NO_FRONTMATTER_REASON: &str =
    "feeds a document that still carries its metadata block; the host removes \
     frontmatter before this crate sees it, and the corpus runner is not the host";
const INDENTED_BLOCK_TAG: &str = "DIVERGENCES.md #12 (an indented block tag leaves its list item)";
const INDENTED_BLOCK_TAG_REASON: &str =
    "writes a block `{% if %}` two spaces in under a list item and expects it to \
     be the item's content; the segmenter runs before the container parser, so \
     the tag splits the document instead";
const ALLOW_INDENTATION_REASON: &str =
    "graded under `allowIndentation: true`, which upstream reaches by patching \
     markdown-it; unreachable above an unpatched CommonMark parser";

/// The annotated cases.
///
/// Four are the `allowIndentation` set. Upstream's corpus runner constructs its
/// tokenizer with `allowIndentation: true` (`spec/marktest/index.ts:21-24`),
/// which switches off CommonMark's four-space rule across nine block rules, so
/// these cases indent content inside a tag and still expect paragraphs, fences
/// and tables rather than indented code blocks.
///
/// It was six until the transformer landed and two of them started passing.
/// Their indentation sits inside a list item or a tag body, where stock
/// CommonMark reads it as content already, so the option was never what they
/// needed. Removing an annotation that has stopped being true is the point of
/// running annotated cases rather than skipping them.
///
/// Four more are fences that rely on upstream's `process` default, which
/// divergence 1 inverts. Three expect a fence's content split into text and tag
/// children; the fourth replaces the `fence` schema with one that has no
/// transform hook, which leaves the generic path rendering children a literal
/// fence does not have. Every other fence case in the corpus states `process`
/// explicitly and is reached either way.
///
/// One is frontmatter, which divergence 7 makes the host's rather than this
/// crate's.
///
/// One writes a block tag indented inside a list item, which divergence 12 puts
/// out of reach: the segmenter resolves tag syntax before the container parser
/// runs, so it has no list item to put the tag inside.
pub const ANNOTATED: &[Annotation] = &[
    Annotation {
        case: "Indented paragraph in a tag",
        entry: ALLOW_INDENTATION,
        reason: ALLOW_INDENTATION_REASON,
    },
    Annotation {
        case: "Indented fence in a tag",
        entry: ALLOW_INDENTATION,
        reason: ALLOW_INDENTATION_REASON,
    },
    Annotation {
        case: "Conditional and variable in code example with indentation",
        entry: LITERAL_FENCES,
        reason: LITERAL_FENCES_REASON,
    },
    Annotation {
        case: "Tag after a comment in a code example",
        entry: LITERAL_FENCES,
        reason: LITERAL_FENCES_REASON,
    },
    Annotation {
        case: "Multiple sequential tags in a code example",
        entry: LITERAL_FENCES,
        reason: LITERAL_FENCES_REASON,
    },
    Annotation {
        case: "Using a backtick in a fenced code block string attribute",
        entry: LITERAL_FENCES,
        reason: LITERAL_FENCES_UNHOOKED_REASON,
    },
    Annotation {
        case: "Frontmatter",
        entry: NO_FRONTMATTER,
        reason: NO_FRONTMATTER_REASON,
    },
    Annotation {
        case: "Advanced table with conditional inside cell",
        entry: INDENTED_BLOCK_TAG,
        reason: INDENTED_BLOCK_TAG_REASON,
    },
];

/// The annotation for a case name, if it has one.
pub fn lookup(case: &str) -> Option<&'static Annotation> {
    ANNOTATED.iter().find(|a| a.case == case)
}
