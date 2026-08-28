//! Printing an AST back to canonical Markdoc source.
//!
//! Mirrors upstream `src/formatter.ts`, the largest single file in the port at
//! 506 lines, and the hardest: it is the only layer whose correctness is
//! judged against the exact bytes it emits.
//!
//! It is also what makes tooling possible -- editing a document as a tree and
//! writing it back, mechanically migrating one syntax to another, showing an
//! author the canonical form of what they wrote.
//!
//! Two properties gate it, beyond the corpus:
//!
//! - `format(parse(s))` is idempotent.
//! - `parse(format(ast))` round-trips the AST.
