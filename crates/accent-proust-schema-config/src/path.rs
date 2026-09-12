//! Where in a configuration something went wrong.
//!
//! A configuration is a nested object assembled from manifests, so "unknown
//! attribute type" is not an actionable message and
//! `config.tags.callout.attributes.type.type` is. Every error this crate
//! reports carries the path to the value that caused it, and the path is the
//! actionable half of the message, never omitted.

use std::fmt;

/// A dotted path into a configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path(String);

impl Path {
    /// The root of the configuration, spelled `config`.
    #[must_use]
    pub fn root() -> Path {
        Path(String::from("config"))
    }

    /// The path to a named property of this one: `config.tags`.
    #[must_use]
    pub fn child(&self, key: &str) -> Path {
        Path(format!("{}.{key}", self.0))
    }

    /// The path to an element of this one: `config.tags.x.children[0]`.
    #[must_use]
    pub fn index(&self, index: usize) -> Path {
        Path(format!("{}[{index}]", self.0))
    }
}

impl fmt::Display for Path {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
