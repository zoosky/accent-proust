//! Segmentation and document parsing: source text in, AST out.
//!
//! Mirrors upstream `src/parser.ts` and `src/tokenizer/`, and is the one part
//! of the port with no line-by-line source. Upstream hooks markdown-it's block
//! ruler, inline ruler, and a core pass; none of those exist here.
//!
//! The Rust equivalent is a segmenter over the raw text -- block-level `{% %}`
//! line detection, inline spans inside text runs, fence interception -- that
//! feeds each Markdown segment to a `Tokenizer` and lifts the resulting
//! events, with their source spans, into the AST under the current tag scope.
//! The behaviour to reproduce is the behaviour the conformance corpus fixes,
//! not markdown-it's mechanics.
//!
//! `Tokenizer` is the seam that keeps the CommonMark engine a detail. The
//! bundled implementation over pulldown-cmark sits behind the
//! `pulldown-cmark-tokenizer` feature; a host that already parses CommonMark
//! can implement the trait instead and avoid compiling a second parser.

mod scan;
mod segment;
mod tokenizer;

#[cfg(feature = "pulldown-cmark-tokenizer")]
mod pulldown;

pub use scan::{contains_markdoc_tag_in_url, find_tag_end, CLOSE, OPEN};
pub use tokenizer::{Alignment, Container, ContainerKind, Event, Spanned, Tokenizer};

#[cfg(feature = "pulldown-cmark-tokenizer")]
pub use pulldown::PulldownTokenizer;
