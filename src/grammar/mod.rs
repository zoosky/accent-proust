//! The tag-internals grammar: what appears between `{%` and `%}`.
//!
//! Mirrors upstream `src/grammar/tag.pegjs` -- a 176-line PEG covering tag
//! names, attributes, values, variables, function calls, and annotations. Here
//! it becomes a hand-written recursive-descent parser over the same grammar.
//!
//! This is the crate's outermost attack surface: it is fed arbitrary text from
//! arbitrary documents. It must never panic, and a property test asserts that
//! against generated input rather than trusting review.
