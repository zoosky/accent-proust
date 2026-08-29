//! Reading `spec/marktest/tests.yaml` into cases.
//!
//! This is the only module that knows YAML exists. Everything above it works in
//! [`Value`], so replacing the YAML reader is a change to one file.
//!
//! The loader is strict on purpose. An unrecognised key is a hard error rather
//! than something skipped, because the corpus is refreshed wholesale from
//! upstream: a key added in a later Markdoc release would otherwise be dropped
//! silently, and the cases using it would be graded against half their own
//! definition.

use std::fmt;

use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};

use crate::value::Value;

/// Which renderer a case is graded through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// The default: compare the renderable tree's children against `expected`.
    ///
    /// Upstream runs these through its React renderer and diffs the resulting
    /// JSON. The renderer is incidental there -- what is compared is the tree --
    /// so here it is compared directly, and no React renderer is implied.
    Tree,
    /// Compare rendered HTML, trimmed, against `expected`.
    Html,
}

/// One corpus case.
#[derive(Debug)]
pub struct Case {
    /// Upstream's name for the case. Not unique: three cases are called
    /// "Indented paragraph in a tag".
    pub name: String,
    /// 1-based line of the case in `tests.yaml`, so a failure is clickable.
    pub line: usize,
    /// The Markdoc source under test.
    pub code: String,
    /// The Markdoc config: tags, nodes, variables, functions, partials.
    pub config: Option<Value>,
    /// Whether the parser runs with slots enabled.
    pub slots: bool,
    /// Whether validation errors are worth reporting for this case.
    ///
    /// `validation: false` means "this case is knowingly invalid, do not report
    /// it". It never decides pass or fail, in upstream or here.
    pub report_validation: bool,
    /// How the case is graded.
    pub renderer: Renderer,
    /// The expected tree or HTML.
    pub expected: Option<Value>,
    /// The expected validation messages, newline-separated.
    ///
    /// When present this is the whole grade: upstream compares validation output
    /// and never looks at the tree, even for the four cases that carry both.
    pub expected_error: Option<String>,
}

/// A corpus that could not be read.
///
/// This is always a defect in the vendored file or in this loader, never a
/// conformance result, so it is kept apart from a failing case.
#[derive(Debug)]
pub struct CorpusError {
    line: usize,
    message: String,
}

impl fmt::Display for CorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tests.yaml:{}: {}", self.line, self.message)
    }
}

fn error(line: usize, message: impl Into<String>) -> CorpusError {
    CorpusError {
        line,
        message: message.into(),
    }
}

/// Parse the corpus.
pub fn load(source: &str) -> Result<Vec<Case>, CorpusError> {
    let documents = MarkedYaml::load_from_str(source)
        .map_err(|e| error(e.marker().line(), format!("not valid YAML: {e}")))?;

    let Some(root) = documents.first() else {
        return Err(error(0, "the corpus is empty"));
    };
    let YamlData::Sequence(cases) = &root.data else {
        return Err(error(
            root.span.start.line(),
            "expected the corpus to be a sequence of cases",
        ));
    };

    cases.iter().map(case).collect()
}

fn case(node: &MarkedYaml<'_>) -> Result<Case, CorpusError> {
    let line = node.span.start.line();
    let YamlData::Mapping(entries) = &node.data else {
        return Err(error(line, "expected a case mapping"));
    };

    let mut case = Case {
        name: String::new(),
        line,
        code: String::new(),
        config: None,
        slots: false,
        report_validation: true,
        renderer: Renderer::Tree,
        expected: None,
        expected_error: None,
    };
    let mut seen_name = false;
    let mut seen_code = false;

    for (key_node, value) in entries {
        let YamlData::Value(Scalar::String(key)) = &key_node.data else {
            return Err(error(key_node.span.start.line(), "expected a string key"));
        };
        let at = value.span.start.line();
        match key.as_ref() {
            "name" => {
                case.name = string(value).ok_or_else(|| error(at, "name must be a string"))?;
                seen_name = true;
            }
            "code" => {
                case.code = string(value).ok_or_else(|| error(at, "code must be a string"))?;
                seen_code = true;
            }
            "config" => case.config = Some(value_of(value)?),
            "expected" => case.expected = Some(value_of(value)?),
            "expectedError" => {
                case.expected_error =
                    Some(string(value).ok_or_else(|| error(at, "expectedError must be a string"))?);
            }
            "slots" => {
                case.slots = boolean(value).ok_or_else(|| error(at, "slots must be a boolean"))?;
            }
            "validation" => {
                case.report_validation =
                    boolean(value).ok_or_else(|| error(at, "validation must be a boolean"))?;
            }
            "renderer" => {
                case.renderer = match string(value).as_deref() {
                    Some("html") => Renderer::Html,
                    Some("react") => Renderer::Tree,
                    other => {
                        return Err(error(
                            at,
                            format!("unknown renderer {other:?}; expected \"html\" or \"react\""),
                        ));
                    }
                };
            }
            unknown => {
                return Err(error(
                    key_node.span.start.line(),
                    format!(
                        "unknown key {unknown:?}. The corpus gained a key this harness does not \
                         grade: teach the loader about it, or the cases using it are graded \
                         against half their definition."
                    ),
                ));
            }
        }
    }

    if !seen_name {
        return Err(error(line, "case has no name"));
    }
    if !seen_code {
        return Err(error(line, "case has no code"));
    }
    if case.expected.is_none() && case.expected_error.is_none() {
        return Err(error(
            line,
            "case has neither expected nor expectedError, so nothing grades it",
        ));
    }
    Ok(case)
}

fn string(node: &MarkedYaml<'_>) -> Option<String> {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn boolean(node: &MarkedYaml<'_>) -> Option<bool> {
    match &node.data {
        YamlData::Value(Scalar::Boolean(b)) => Some(*b),
        _ => None,
    }
}

fn value_of(node: &MarkedYaml<'_>) -> Result<Value, CorpusError> {
    let line = node.span.start.line();
    match &node.data {
        YamlData::Value(Scalar::Null) => Ok(Value::Null),
        YamlData::Value(Scalar::Boolean(b)) => Ok(Value::Bool(*b)),
        YamlData::Value(Scalar::Integer(i)) => Ok(Value::Int(*i)),
        YamlData::Value(Scalar::FloatingPoint(f)) => Ok(Value::Float(f.into_inner())),
        YamlData::Value(Scalar::String(s)) => Ok(Value::Str(s.to_string())),
        YamlData::Sequence(items) => items
            .iter()
            .map(value_of)
            .collect::<Result<_, _>>()
            .map(Value::Seq),
        YamlData::Mapping(entries) => {
            let mut out = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let key = match &key.data {
                    YamlData::Value(Scalar::String(s)) => s.to_string(),
                    // A mapping key that is not a string has no JSON spelling,
                    // and the corpus has none. Refusing is better than inventing
                    // one.
                    _ => {
                        return Err(error(
                            key.span.start.line(),
                            "expected a string mapping key",
                        ))
                    }
                };
                out.push((key, value_of(value)?));
            }
            Ok(Value::Map(out))
        }
        YamlData::Representation(..) => Err(error(line, "unresolved scalar representation")),
        YamlData::Tagged(..) => Err(error(line, "tagged nodes are not part of the corpus")),
        YamlData::Alias(_) => Err(error(line, "aliases are not part of the corpus")),
        YamlData::BadValue => Err(error(line, "malformed value")),
    }
}
