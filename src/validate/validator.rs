//! Checking a document against a config.
//!
//! A transliteration of upstream `src/validator.ts`. The checks run in
//! upstream's order and produce upstream's ids and messages, because the order
//! is observable -- a document with several problems reports one sequence, and a
//! tool diffing that sequence sees a reordering as a change -- and the ids are
//! what external tooling binds to.
//!
//! # Errors are data
//!
//! Nothing here returns `Result`. [`validate_tree`] returns every problem it
//! found, so an editor can show them all at once, and a document full of them is
//! still a document. `Result::Err` stays reserved for invariants this crate
//! would have broken itself.
//!
//! # The walk is iterative
//!
//! Upstream's `walkWithParents` is a recursive generator. Nesting depth is
//! attacker-controlled -- `{% a %}` repeated is one level per line -- and a
//! recursive walk would make tree depth a stack-overflow budget, which aborts
//! rather than raising. [`walk_with_parents`] keeps an explicit stack for the
//! same reason [`Node::walk`](crate::ast::Node::walk) and `Node`'s `Drop` do.
//!
//! # What the validator does not know
//!
//! Which schemas exist. Upstream's `Markdoc.validate` merges its built-in node
//! and tag schemas into the caller's config before validating; here that merge
//! is the transform stage's, because the built-ins are schema *content*. A
//! config with no `document` schema really does report `Undefined node:
//! 'document'`, and that is the correct answer rather than a degenerate one.

use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use crate::ast::{
    ErrorLevel, Function, Location, Node, NodeType, PathSegment, ValidationError, Value, Variable,
};
use crate::validate::schema_types::{Class, Id};
use crate::validate::{
    Config, RenderPolicy, Schema, SchemaAttribute, SchemaMatches, ValidationType, Variables,
    type_to_string,
};

/// One problem, with the node and the lines it belongs to.
///
/// Mirrors upstream's `ValidateError`, which is what `validate` returns and
/// what a host reports. It is [`ValidationError`] plus the context needed to
/// point at the document: an error may carry a location of its own, and when it
/// does that location wins over the node's.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidateError<'a> {
    /// The type of the node the problem was found on.
    pub node_type: NodeType,
    /// The source lines to blame, as `[start, end]`.
    pub lines: Vec<usize>,
    /// Where to point, when anything knows.
    pub location: Option<Location<'a>>,
    /// The problem itself.
    pub error: ValidationError<'a>,
}

/// The attributes every tag and node has, whatever its schema says.
///
/// Upstream keeps these in `transformer.ts` and spreads them under each
/// schema's own. They live here because both stages need them and their types
/// do: [`Class`] and [`Id`] are attribute types, and an attribute type is the
/// validator's vocabulary.
///
/// Built once. Upstream rebuilds the merged map per node, which is free in
/// JavaScript and is not free here.
#[must_use]
pub fn global_attributes() -> &'static IndexMap<String, SchemaAttribute> {
    static GLOBAL: OnceLock<IndexMap<String, SchemaAttribute>> = OnceLock::new();
    GLOBAL.get_or_init(|| {
        let mut attributes = IndexMap::new();
        attributes.insert(
            "class".to_string(),
            SchemaAttribute {
                attribute_type: Some(ValidationType::Custom(Arc::new(Class))),
                render: RenderPolicy::Named,
                ..SchemaAttribute::default()
            },
        );
        attributes.insert(
            "id".to_string(),
            SchemaAttribute {
                attribute_type: Some(ValidationType::Custom(Arc::new(Id))),
                render: RenderPolicy::Named,
                ..SchemaAttribute::default()
            },
        );
        attributes
    })
}

/// What checking a value against a type produced.
///
/// Upstream's `validateType` returns `boolean | ValidationError[]`, and all
/// three answers are used: `false` becomes an `attribute-type-invalid` written
/// by the caller, an array is the custom type's own errors passed through, and
/// an *empty* array is a custom type saying the value is fine. Collapsing the
/// last two into "no errors" would be the same value; collapsing either into
/// `Valid` would lose the custom type's message.
#[derive(Debug)]
#[non_exhaustive]
pub enum TypeCheck<'a> {
    /// The value has the type.
    Valid,
    /// It does not, and the caller writes the message.
    Invalid,
    /// A custom type answered for itself, possibly with nothing to say.
    Errors(Vec<ValidationError<'a>>),
}

/// Check one value against one type.
///
/// Upstream's exported `validateType`, in its order, which matters twice:
///
/// - A **function** is checked against its declared return type *before* a
///   union is expanded, so a function feeding an attribute typed
///   `[String, Number]` is compared against the union itself and never matches
///   it. That is upstream's behaviour, not an oversight to fix here; see
///   [`ValidationType::is_same_type`].
/// - Any **other AST value** -- a variable, or a function when function
///   validation is off -- is accepted for every type, because its type is not
///   knowable until it resolves.
#[must_use]
pub fn validate_type<'a>(
    value_type: &ValidationType,
    value: &Value,
    config: &Config<'a>,
    key: &str,
) -> TypeCheck<'a> {
    if let Value::Function(function) = value
        && config.validation.validate_functions
    {
        let Some(schema) = config.functions.get(function.name.as_str()) else {
            return TypeCheck::Valid;
        };
        let Some(returns) = &schema.returns else {
            return TypeCheck::Valid;
        };
        let matched = match returns {
            ValidationType::Union(members) => {
                members.iter().any(|member| member.is_same_type(value_type))
            }
            single => single.is_same_type(value_type),
        };
        return if matched {
            TypeCheck::Valid
        } else {
            TypeCheck::Invalid
        };
    }

    if matches!(value, Value::Function(_) | Value::Variable(_)) {
        return TypeCheck::Valid;
    }

    match value_type {
        ValidationType::Union(members) => {
            // Upstream is `type.some(t => validateType(t, ...))`, and `some`
            // coerces its callback's result to a boolean. Every array is truthy
            // in JavaScript, the empty one included, so a union member that is a
            // custom type satisfies the union whatever that type said -- its
            // errors are discarded rather than propagated. Reproducing that is
            // the difference between a faithful port and a stricter one.
            let satisfied = members.iter().any(|member| {
                !matches!(
                    validate_type(member, value, config, key),
                    TypeCheck::Invalid
                )
            });
            if satisfied {
                TypeCheck::Valid
            } else {
                TypeCheck::Invalid
            }
        }
        ValidationType::Custom(custom) => match custom.validate(value, config, key) {
            Some(errors) => TypeCheck::Errors(errors),
            // A custom type with no validation falls through to upstream's
            // `value.constructor === type`, which compares the value against
            // the class and is false for everything a document can hold.
            None => TypeCheck::Invalid,
        },
        primitive => {
            if primitive.accepts_shape(value) {
                TypeCheck::Valid
            } else {
                TypeCheck::Invalid
            }
        }
    }
}

/// Check one function call against its declaration.
///
/// Reached only when [`ValidationOptions::validate_functions`](crate::validate::ValidationOptions::validate_functions)
/// is on, because a document may legitimately call functions a validating tool
/// has never heard of.
fn validate_function<'a>(function: &Function, config: &Config<'a>) -> Vec<ValidationError<'a>> {
    let mut errors = Vec::new();

    let Some(schema) = config.functions.get(function.name.as_str()) else {
        return vec![ValidationError::new(
            "function-undefined",
            ErrorLevel::Critical,
            format!("Undefined function: '{}'", function.name),
        )];
    };

    if let Some(hook) = &schema.validate {
        errors.extend(hook(function, config));
    }

    if let Some(parameters) = &schema.parameters {
        for (key, value) in &function.parameters {
            let Some(parameter) = parameters.get(key.as_str()) else {
                errors.push(ValidationError::new(
                    "parameter-undefined",
                    ErrorLevel::Error,
                    format!("Invalid parameter: '{key}'"),
                ));
                continue;
            };

            // A variable resolves later, so its type is not knowable now. A
            // nested call is knowable, and is checked against its own return
            // type by `validate_type`.
            if matches!(value, Value::Variable(_)) {
                continue;
            }

            if let Some(value_type) = &parameter.attribute_type {
                match validate_type(value_type, value, config, key) {
                    TypeCheck::Valid => {}
                    TypeCheck::Invalid => errors.push(ValidationError::new(
                        "parameter-type-invalid",
                        ErrorLevel::Error,
                        format!(
                            "Parameter '{key}' of '{}' must be type of '{}'",
                            function.name,
                            type_to_string(value_type)
                        ),
                    )),
                    TypeCheck::Errors(found) => errors.extend(found),
                }
            }
        }
    }

    for (key, parameter) in schema.parameters.iter().flatten() {
        if parameter.required && !function.parameters.contains_key(key.as_str()) {
            errors.push(ValidationError::new(
                "parameter-missing-required",
                ErrorLevel::Error,
                format!("Missing required parameter: '{key}'"),
            ));
        }
    }

    errors
}

/// Check one node against its schema.
///
/// Upstream's default export. It starts from the node's own errors -- the
/// parser's, which are already there -- and adds what the schema says. A node
/// with no schema stops after one error: nothing else can be checked without
/// one, and a list of consequential complaints would bury the cause.
#[must_use]
pub fn validator<'a>(node: &'a Node<'a>, config: &Config<'a>) -> Vec<ValidationError<'a>> {
    let mut errors: Vec<ValidationError<'a>> = node.errors.clone();

    let Some(schema) = config.find_schema(node) else {
        errors.push(match &node.tag {
            Some(tag) => ValidationError::new(
                "tag-undefined",
                ErrorLevel::Critical,
                format!("Undefined tag: '{tag}'"),
            ),
            None => ValidationError::new(
                "node-undefined",
                ErrorLevel::Critical,
                format!("Undefined node: '{}'", node.node_type),
            ),
        });
        return errors;
    };

    if let Some(inline) = schema.inline
        && node.inline != inline
    {
        errors.push(ValidationError::new(
            "tag-placement-invalid",
            ErrorLevel::Critical,
            format!(
                "'{}' tag should be {}",
                node.tag.as_deref().unwrap_or_default(),
                if inline { "inline" } else { "block" }
            ),
        ));
    }

    if schema.self_closing && !node.children.is_empty() {
        errors.push(ValidationError::new(
            "tag-selfclosing-has-children",
            ErrorLevel::Critical,
            format!(
                "'{}' tag should be self-closing",
                node.tag.as_deref().unwrap_or_default()
            ),
        ));
    }

    let attributes = merged_attributes(schema);

    for key in node.slots.keys() {
        if !schema.slots.contains_key(key.as_str()) {
            errors.push(ValidationError::new(
                "slot-undefined",
                ErrorLevel::Error,
                format!("Invalid slot: '{key}'"),
            ));
        }
    }

    for (key, value) in &node.attributes {
        validate_attribute(&attributes, key, value, config, &mut errors);
    }

    for (key, attribute) in &attributes {
        if attribute.required && !node.attributes.contains_key(key.as_str()) {
            errors.push(ValidationError::new(
                "attribute-missing-required",
                ErrorLevel::Error,
                format!("Missing required attribute: '{key}'"),
            ));
        }
    }

    for (key, slot) in &schema.slots {
        if slot.required && !node.slots.contains_key(key.as_str()) {
            errors.push(ValidationError::new(
                "slot-missing-required",
                ErrorLevel::Error,
                format!("Missing required slot: '{key}'"),
            ));
        }
    }

    if let Some(allowed) = &schema.children {
        for child in &node.children {
            // An `error` child is never reported: the parser has already said
            // what is wrong with it, and a complaint about its placement on top
            // of that is noise.
            if child.node_type != NodeType::Error && !allowed.contains(&child.node_type) {
                errors.push(ValidationError::new(
                    "child-invalid",
                    ErrorLevel::Warning,
                    format!("Can't nest '{}' in '{}'", child.node_type, node.name()),
                ));
            }
        }
    }

    if let Some(hook) = &schema.validate {
        errors.extend(hook(node, config));
    }

    errors
}

/// The schema's attributes with the global ones underneath.
///
/// Upstream's `{...globalAttributes, ...schema.attributes}`. Object spread
/// keeps a repeated key's *first* position and *last* value, which
/// [`IndexMap::insert`] also does -- so a schema redeclaring `id` overrides the
/// global one without moving it, and the required-attribute loop below reports
/// in the same order upstream does.
fn merged_attributes(schema: &Schema) -> IndexMap<String, SchemaAttribute> {
    let mut merged = global_attributes().clone();
    for (key, attribute) in &schema.attributes {
        merged.insert(key.clone(), attribute.clone());
    }
    merged
}

/// Check one authored attribute against its declaration.
fn validate_attribute<'a>(
    attributes: &IndexMap<String, SchemaAttribute>,
    key: &str,
    value: &Value,
    config: &Config<'a>,
    errors: &mut Vec<ValidationError<'a>>,
) {
    let Some(attribute) = attributes.get(key) else {
        errors.push(ValidationError::new(
            "attribute-undefined",
            ErrorLevel::Error,
            format!("Invalid attribute: '{key}'"),
        ));
        return;
    };

    // An unresolved reference is checked as a reference, and only then as a
    // value. A variable with no `variables` config, or a function with function
    // validation off, is not checked at all -- upstream's `else continue`.
    match value {
        Value::Function(function) if config.validation.validate_functions => {
            errors.extend(validate_function(function, config));
        }
        Value::Variable(variable) => match &config.variables {
            Some(variables) => errors.extend(undefined_variable(variable, variables)),
            None => return,
        },
        Value::Function(_) => return,
        _ => {}
    }

    let level = attribute.error_level.unwrap_or(ErrorLevel::Error);

    if let Some(value_type) = &attribute.attribute_type {
        match validate_type(value_type, value, config, key) {
            TypeCheck::Valid => {}
            TypeCheck::Invalid => errors.push(ValidationError::new(
                "attribute-type-invalid",
                level,
                format!(
                    "Attribute '{key}' must be type of '{}'",
                    type_to_string(value_type)
                ),
            )),
            TypeCheck::Errors(found) => errors.extend(found),
        }
    }

    if let Some(matches) = resolve_matches(attribute.matches.as_ref(), config) {
        match matches {
            SchemaMatches::Values(allowed) => {
                let member = matches!(value, Value::String(text) if allowed.contains(text));
                if !member {
                    errors.push(ValidationError::new(
                        "attribute-value-invalid",
                        level,
                        format!(
                            "Attribute '{key}' must match one of {}. Got '{}' instead.",
                            display_matches(&allowed, 8),
                            js_string(value)
                        ),
                    ));
                }
            }
            SchemaMatches::Pattern(pattern) => {
                if !pattern.is_match(&js_string(value)) {
                    errors.push(ValidationError::new(
                        "attribute-value-invalid",
                        level,
                        format!(
                            "Attribute '{key}' must match {}. Got '{}' instead.",
                            pattern.display(),
                            js_string(value)
                        ),
                    ));
                }
            }
            // A hook that resolved to another hook is neither an array nor a
            // pattern, so upstream runs no check. Neither does this.
            SchemaMatches::Dynamic(_) => {}
        }
    }

    if let Some(hook) = &attribute.validate {
        errors.extend(hook(value, config, key));
    }
}

/// Resolve a dynamic `matches` once, as upstream does.
fn resolve_matches(matches: Option<&SchemaMatches>, config: &Config<'_>) -> Option<SchemaMatches> {
    match matches? {
        SchemaMatches::Dynamic(hook) => hook(config),
        other => Some(other.clone()),
    }
}

/// Walk a variable's path through the configured variables.
///
/// Upstream descends one segment at a time with `hasOwnProperty`, so the first
/// segment that is not there stops the walk and the *whole* path is reported --
/// `$a.b.c` against `{a: {}}` says `Undefined variable: 'a.b.c'`, not `'a.b'`.
fn undefined_variable<'a>(
    variable: &Variable,
    variables: &Variables,
) -> Option<ValidationError<'a>> {
    let mut current: Option<&Value> = None;
    for segment in &variable.path {
        let found = match current {
            None => match segment {
                PathSegment::Key(key) => variables.get(key.as_str()),
                PathSegment::Index(index) => variables.get(index_key(*index).as_str()),
            },
            Some(Value::Hash(entries)) => match segment {
                PathSegment::Key(key) => entries.get(key.as_str()),
                PathSegment::Index(index) => entries.get(index_key(*index).as_str()),
            },
            Some(Value::Array(items)) => array_element(items, segment),
            // A primitive has no own properties a path can descend into.
            Some(_) => None,
        };
        let Some(found) = found else {
            return Some(ValidationError::new(
                "variable-undefined",
                ErrorLevel::Error,
                format!("Undefined variable: '{}'", path_to_string(&variable.path)),
            ));
        };
        current = Some(found);
    }
    None
}

/// One element of an array, addressed by a path segment.
///
/// A JavaScript array's own properties are its indices, so a non-integral or
/// out-of-range segment finds nothing.
fn array_element<'v>(items: &'v [Value], segment: &PathSegment) -> Option<&'v Value> {
    let index = match segment {
        PathSegment::Index(index) => *index,
        PathSegment::Key(key) => key.parse::<f64>().ok()?,
    };
    if !(0.0..=f64::from(u32::MAX)).contains(&index) || index.fract() != 0.0 {
        return None;
    }
    // Bounded above by `u32::MAX` and integral, so the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = index as usize;
    items.get(index)
}

/// A numeric path segment as JavaScript would key an object with it.
fn index_key(index: f64) -> String {
    js_number(index)
}

/// A variable path as upstream joins it for the message: `Array.join('.')`.
fn path_to_string(path: &[PathSegment]) -> String {
    path.iter()
        .map(|segment| match segment {
            PathSegment::Key(key) => key.clone(),
            PathSegment::Index(index) => js_number(*index),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Upstream's `displayMatches`: the whole list as JSON, or the first `n` with a
/// count of the rest.
///
/// The elision is not cosmetic -- a schema whose `matches` is every page slug on
/// a site would otherwise print the site into one error message.
fn display_matches(matches: &[String], n: usize) -> String {
    if matches.len() <= n {
        return format!(
            "[{}]",
            matches
                .iter()
                .map(|item| json_string(item))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    let shown = matches
        .iter()
        .take(n)
        .map(|item| json_string(item))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{shown}, ... {} more]", matches.len() - n)
}

/// A string as `JSON.stringify` writes it.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                // `write!` into a `String` cannot fail, and the alternative
                // clippy objects to is a second allocation per control byte.
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A value as JavaScript's `String(value)` writes it.
///
/// Error messages interpolate the offending value, so the spelling is part of
/// the message and the message is compared character for character by the
/// conformance runner. An object prints as `[object Object]` here for the same
/// reason it does there: the alternative is a nicer message that no longer
/// matches.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Number(number) => js_number(*number),
        Value::String(text) => text.clone(),
        // `Array.prototype.toString` joins with a comma and writes null and
        // undefined as the empty string.
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Hash(_) | Value::Function(_) | Value::Variable(_) => "[object Object]".to_string(),
    }
}

/// A number as JavaScript writes it.
///
/// Rust's `f64` display already drops a trailing `.0`, so `1` prints as `1` and
/// `1.5` as `1.5`, which is what matters for a message quoting an authored
/// literal.
fn js_number(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_string();
    }
    format!("{number}")
}

/// Visit every node of a tree, depth first, with the path that reached it.
///
/// Upstream's `walkWithParents`, as a callback rather than a generator: the
/// path is a slice the walk owns, so a visitor that does not need it pays
/// nothing, and one that does gets it without a copy per node.
///
/// The order is slots before children, which is the order
/// [`Node::walk`](crate::ast::Node::walk) uses and which upstream's `node.test.ts`
/// fixes. It is load-bearing: it decides the order problems are reported in.
pub fn walk_with_parents<'n, 'a, F>(node: &'n Node<'a>, mut visit: F)
where
    F: FnMut(&'n Node<'a>, &[&'n Node<'a>]),
{
    enum Step<'n, 'a> {
        Visit(&'n Node<'a>),
        Leave,
    }

    let mut stack = vec![Step::Visit(node)];
    let mut path: Vec<&'n Node<'a>> = Vec::new();
    while let Some(step) = stack.pop() {
        match step {
            Step::Leave => {
                path.pop();
            }
            Step::Visit(current) => {
                visit(current, &path);
                let descendants: Vec<&'n Node<'a>> = current
                    .slots
                    .values()
                    .chain(current.children.iter())
                    .collect();
                if !descendants.is_empty() {
                    stack.push(Step::Leave);
                    path.push(current);
                    stack.extend(descendants.into_iter().rev().map(Step::Visit));
                }
            }
        }
    }
}

/// Validate a whole document.
///
/// This is the entry point. Upstream's `Markdoc.validate` is this plus a merge
/// of its built-in schemas, which is the transform stage's to supply here.
///
/// The config is cloned once, not per node, so that
/// [`ValidationOptions::parents`](crate::validate::ValidationOptions::parents)
/// can be set as the walk descends without the caller's config being touched.
/// Upstream builds a fresh config object per node; one clone for the document is
/// the same behaviour at a cost that does not scale with the tree.
#[must_use]
pub fn validate_tree<'a>(content: &'a Node<'a>, config: &Config<'a>) -> Vec<ValidateError<'a>> {
    let mut scoped = config.clone();
    let mut output = Vec::new();
    walk_with_parents(content, |node, parents| {
        scoped.validation.parents.clear();
        scoped.validation.parents.extend_from_slice(parents);
        for error in validator(node, &scoped) {
            output.push(to_validate_error(node, error));
        }
    });
    output
}

/// Attach a node's context to one error.
///
/// An error that carries its own location keeps it and contributes its lines;
/// upstream also fills in the node's file when the error's location did not name
/// one, which is how a parser error raised inside a partial still says which
/// file it came from.
fn to_validate_error<'a>(node: &Node<'a>, error: ValidationError<'a>) -> ValidateError<'a> {
    match error.location {
        Some(location) => {
            let file = location.file.or_else(|| node.location.and_then(|l| l.file));
            ValidateError {
                node_type: node.node_type,
                lines: vec![location.start.line, location.end.line],
                location: Some(Location { file, ..location }),
                error,
            }
        }
        None => ValidateError {
            node_type: node.node_type,
            lines: node.lines.clone(),
            location: node.location,
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_prints_as_javascript_prints_it() {
        assert_eq!(js_string(&Value::Null), "null");
        assert_eq!(js_string(&Value::Boolean(true)), "true");
        assert_eq!(js_string(&Value::Number(1.0)), "1");
        assert_eq!(js_string(&Value::Number(1.5)), "1.5");
        assert_eq!(
            js_string(&Value::Array(vec![
                Value::Null,
                Value::Number(1.0),
                Value::String("a".into())
            ])),
            ",1,a"
        );
        assert_eq!(
            js_string(&Value::Variable(Variable::default())),
            "[object Object]"
        );
    }

    #[test]
    fn matches_are_elided_the_way_upstream_elides_them() {
        let short: Vec<String> = ["bar", "baz", "bat"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(display_matches(&short, 8), r#"["bar","baz","bat"]"#);

        let long: Vec<String> = "foobarbazqux".chars().map(|c| c.to_string()).collect();
        assert_eq!(
            display_matches(&long, 8),
            r#"["f","o","o","b","a","r","b","a", ... 4 more]"#
        );
    }

    #[test]
    fn json_strings_escape_what_json_escapes() {
        assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(json_string("a\nb"), r#""a\nb""#);
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn the_walk_is_iterative_and_carries_the_path() {
        let mut node = Node::new(NodeType::Document);
        for _ in 0..50_000 {
            node = Node::with(NodeType::Tag, IndexMap::new(), vec![node], None);
        }
        let mut deepest = 0;
        let mut visited = 0;
        walk_with_parents(&node, |_, parents| {
            visited += 1;
            deepest = deepest.max(parents.len());
        });
        assert_eq!(visited, 50_001);
        assert_eq!(deepest, 50_000);
    }

    #[test]
    fn the_walk_visits_slots_before_children() {
        let mut tag = Node::with(
            NodeType::Tag,
            IndexMap::new(),
            vec![Node::new(NodeType::Heading)],
            Some("example".into()),
        );
        tag.slots
            .insert("foo".to_string(), Node::new(NodeType::Paragraph));
        let document = Node::with(NodeType::Document, IndexMap::new(), vec![tag], None);

        let mut seen = Vec::new();
        walk_with_parents(&document, |node, _| seen.push(node.name().to_string()));
        assert_eq!(seen, ["document", "example", "paragraph", "heading"]);
    }
}
