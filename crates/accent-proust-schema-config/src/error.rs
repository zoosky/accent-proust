//! What can be wrong with a declaration, and where.
//!
//! Structured rather than a string, because two hosts phrase the same refusal
//! differently: an unknown `validate` key is "a hook is code, and code does
//! not cross into WebAssembly" in the browser and "a YAML file cannot hold a
//! function" on the command line. The kind and the path are this crate's; the
//! reason is the host's to add, and [`Error`]'s own `Display` says what is
//! true everywhere.

use std::fmt;

use accent_proust::ast::NodeType;

use crate::path::Path;

/// A problem with a declaration, at a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Where. Never omitted: a configuration is a document a person wrote.
    pub path: Path,
    /// What.
    pub kind: ErrorKind,
}

impl Error {
    /// A problem of `kind` at `path`.
    #[must_use]
    pub fn new(path: Path, kind: ErrorKind) -> Error {
        Error { path, kind }
    }
}

/// The problems a declaration can have.
///
/// `#[non_exhaustive]`, as every public enum here is: a host matching on it
/// to add a reason keeps a wildcard arm, and a kind added later reaches that
/// host as this crate's own message rather than as a compile error.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A key the vocabulary does not have, at this level.
    ///
    /// The path already ends in the key; it is repeated here so that a host
    /// can match on it -- `transform`, `functions`, `partials` -- and say why
    /// that particular key cannot be honoured.
    UnknownKey {
        /// The key as written.
        key: String,
        /// What was allowed in its place.
        expected: &'static [&'static str],
    },
    /// The wrong shape of value: `expected` says which, as prose.
    Expected(&'static str),
    /// A value with no Markdoc counterpart, described by the host: a function,
    /// a symbol, a date.
    NoCounterpart(String),
    /// A node name the library does not know.
    UnknownNodeType(String),
    /// A schema declared under `nodes.tag`, which would never apply.
    TagAsNodeType,
    /// An attribute type that is not one of the five, or a union of them.
    UnknownAttributeType(String),
    /// An error level that is not one of the five.
    UnknownErrorLevel(String),
    /// A schema's `render` written as `true`, which has no name to fall back
    /// on the way an attribute's does.
    UnrenderableTrue,
    /// A `matches` that is not a list of values -- most often a pattern.
    MatchesNotAList,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = &self.path;
        match &self.kind {
            ErrorKind::UnknownKey { expected, .. } => write!(
                f,
                "{at}: unrecognised key. Expected one of {}",
                expected.join(", ")
            ),
            ErrorKind::Expected(what) => write!(f, "{at}: expected {what}"),
            ErrorKind::NoCounterpart(what) => write!(
                f,
                "{at}: a {what} has no Markdoc counterpart; use a string, number, boolean, \
                 null, array or plain object"
            ),
            ErrorKind::UnknownNodeType(name) => {
                let known: Vec<&str> = NodeType::ALL.iter().map(|node| node.as_str()).collect();
                write!(
                    f,
                    "{at}: unknown node type {name:?}; expected one of {}",
                    known.join(", ")
                )
            }
            ErrorKind::TagAsNodeType => write!(
                f,
                "{at}: a tag is looked up by its name, never as the node type \"tag\"; declare \
                 it under \"tags\""
            ),
            ErrorKind::UnknownAttributeType(name) => write!(
                f,
                "{at}: unknown attribute type {name:?}; expected String, Number, Boolean, \
                 Object, Array, or an array of those"
            ),
            ErrorKind::UnknownErrorLevel(name) => write!(
                f,
                "{at}: unknown error level {name:?}; expected one of debug, info, warning, \
                 error, critical"
            ),
            ErrorKind::UnrenderableTrue => {
                write!(f, "{at}: expected an element name or false, not true")
            }
            ErrorKind::MatchesNotAList => write!(
                f,
                "{at}: expected an array of acceptable values. A regular expression is not \
                 supported: the engine carries no regular expression engine on purpose"
            ),
        }
    }
}

impl std::error::Error for Error {}
