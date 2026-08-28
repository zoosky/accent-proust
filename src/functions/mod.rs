//! The built-in functions available in tag attributes and annotations.
//!
//! Mirrors upstream `src/functions/`: `equals`, `and`, `or`, `not`,
//! `default`, `debug`.
//!
//! Functions are pure and total. They take resolved values and return a value;
//! they cannot fail, cannot observe anything outside their arguments, and have
//! no access to the host.
