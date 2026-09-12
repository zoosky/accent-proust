//! The host's half of a configuration: a file, a directory of partials, and
//! variables from the command line.
//!
//! The library reads no files and loads no configuration; this is where a
//! command-line host does both. The vocabulary -- which keys a schema
//! declaration may carry and what each means -- is `accent-proust-schema-config`'s
//! and shared with the WebAssembly host. What is this host's is the reading:
//! a YAML document as a [`Declaration`], `NAME=VALUE` as a typed variable, and
//! a directory as the partials map the library wants filled with parsed
//! nodes.
//!
//! # Why YAML, and why `saphyr`
//!
//! A documentation repository already has YAML, and every JSON file is a YAML
//! file, so one reader serves both. `saphyr` is the reader the library's own
//! conformance harness uses: pure Rust, no serde, and a lattice that maps onto
//! the vocabulary's seven questions directly. A file holds one document; a
//! second one after `---` is refused rather than dropped, because a
//! configuration that half arrives is the failure the vocabulary exists to
//! prevent.
//!
//! # Why the sources are read first
//!
//! A `Config` borrows its partials' sources, because the library parses a
//! partial and keeps the tree rather than the text. So the partial files are
//! read into [`Sources`] before the config is built, and the config borrows
//! them for as long as it is used. The obvious loop -- read a file, build,
//! process, next -- does not compile once partials are shared across inputs,
//! and this is the shape that does.

use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use accent_proust::ast::Value;
use accent_proust::builtins;
use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};
use accent_proust::validate::{Config, MapSchemaSource, Variables};
use accent_proust_schema_config::{Declaration, Error, ErrorKind, Path, Shape, declare};
use clap::Args;
use indexmap::IndexMap;
use saphyr::{LoadableYamlNode, ScalarOwned, YamlOwned};

use crate::exit::report;

/// Where the configuration comes from.
#[derive(Args, Debug)]
pub struct HostArgs {
    /// A YAML or JSON file declaring tags, nodes and variables.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// A directory of partials. `{% partial file="x.md" %}` finds `x.md` in
    /// it, by path relative to the directory.
    #[arg(long, value_name = "DIR")]
    pub partials: Option<PathBuf>,

    /// A variable, as NAME=VALUE. VALUE is read as YAML, so `count=3` is a
    /// number, `debug=true` a boolean and `name=production` a string; quote
    /// to force a string. Overrides a variable the configuration declared.
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,
}

/// The texts a configuration borrows: every partial, keyed as `{% partial %}`
/// names it.
pub struct Sources {
    partials: Vec<(String, String)>,
}

/// Read the sources, reporting under `command` if that fails.
#[must_use]
pub fn load(command: &str, args: &HostArgs) -> Option<Sources> {
    match Sources::read(args) {
        Ok(sources) => Some(sources),
        Err(message) => {
            report(command, &message);
            None
        }
    }
}

/// Build the configuration, reporting under `command` if that fails.
#[must_use]
pub fn configured<'a>(command: &str, args: &HostArgs, sources: &'a Sources) -> Option<Config<'a>> {
    match config(args, sources) {
        Ok(config) => Some(config),
        Err(message) => {
            report(command, &message);
            None
        }
    }
}

impl Sources {
    /// Read the partials directory, if one was named.
    ///
    /// Every UTF-8 text file under it, at any depth, keyed by its path
    /// relative to the directory with `/` separators -- `header.md`,
    /// `sections/intro.md` -- which is what the `file` attribute is written
    /// as. A file that is not UTF-8 is not a partial and is passed over: an
    /// image beside the partials is not an error, and a document that names
    /// it in `{% partial %}` is told so where it does. A symbolic link to a
    /// file is read; one to a directory is not followed, because a link back
    /// up the tree would otherwise be walked until the file system gave up.
    ///
    /// # Errors
    ///
    /// A directory that cannot be listed or a file that cannot be read,
    /// named.
    pub fn read(args: &HostArgs) -> Result<Sources, String> {
        let mut partials = Vec::new();
        if let Some(root) = &args.partials {
            collect(root, &mut partials)?;
        }
        Ok(Sources { partials })
    }
}

/// Walk `root` without recursing: a worklist of directories, entries sorted so
/// that the order is the file system's name order and not its inode order.
fn collect(root: &FsPath, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let mut found = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("{}: {error}", entry.path().display()))?;
            found.push((entry.path(), kind));
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, kind) in found {
            let is_dir = if kind.is_symlink() {
                // Follow a link to a file; do not descend through a link to
                // a directory. `metadata` follows, `file_type` did not.
                match fs::metadata(&path) {
                    Ok(target) if target.is_dir() => continue,
                    Ok(_) => false,
                    Err(error) => return Err(format!("{}: {error}", path.display())),
                }
            } else {
                kind.is_dir()
            };
            if is_dir {
                pending.push(path);
                continue;
            }
            let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let key = relative
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((key, text));
        }
    }
    Ok(())
}

/// Build the configuration: the built-ins, the file's declarations over them,
/// the command line's variables over the file's, and the partials parsed.
///
/// # Errors
///
/// A file that cannot be read or is not one YAML document, a declaration the
/// vocabulary refuses (with its path), or a `--var` that is not `NAME=VALUE`.
pub fn config<'a>(args: &HostArgs, sources: &'a Sources) -> Result<Config<'a>, String> {
    let mut schemas = MapSchemaSource::builtin();
    let mut variables: Option<Variables> = None;

    if let Some(path) = &args.config {
        let label = path.display().to_string();
        let text = fs::read_to_string(path).map_err(|error| format!("{label}: {error}"))?;
        let root = document(&text).map_err(|error| format!("{label}: {error}"))?;
        let declared =
            declare(&Yaml(&root)).map_err(|error| format!("{label}: {}", explain(error)))?;
        variables = declared.apply(&mut schemas);
    }

    for var in &args.vars {
        let (name, text) = match var.split_once('=') {
            Some((name, text)) if !name.is_empty() => (name, text),
            // An empty name is what `--var $NAME=3` becomes when `NAME` is
            // unset in the shell: a variable called nothing, refused by name.
            _ => return Err(format!("--var {var}: expected NAME=VALUE")),
        };
        // The same reader as the configuration file, so the two can never
        // disagree about what `3` is. The path is the variable's, whichever
        // way it arrived.
        let at = Path::root().child("variables").child(name);
        let node = document(text).map_err(|error| format!("--var {name}: {error}"))?;
        let value = Yaml(&node)
            .to_value(&at)
            .map_err(|error| format!("--var {name}: {}", explain(error)))?;
        variables
            .get_or_insert_with(Variables::new)
            .insert(name.to_owned(), value);
    }

    let mut config = builtins::config_with(Arc::new(schemas));
    config.variables = variables;

    let tokenizer = PulldownTokenizer::new();
    for (key, text) in &sources.partials {
        let options = ParseOptions::new().file(key).location(true);
        let node = parse_with(text, &tokenizer, &options);
        config.partials_mut().insert(key.clone(), node);
    }

    Ok(config)
}

/// The one YAML document in `text`, or `null` when there is none -- an empty
/// `--var x=` is `null`, as it would be in the file.
///
/// A second document is refused rather than dropped: a `---` in a
/// configuration file is either a mistake or a configuration in two halves,
/// and reading one half silently is the failure the vocabulary exists to
/// prevent.
fn document(text: &str) -> Result<YamlOwned, String> {
    let mut documents =
        YamlOwned::load_from_str(text).map_err(|error| format!("not valid YAML: {error}"))?;
    match documents.len() {
        0 => Ok(YamlOwned::Value(ScalarOwned::Null)),
        1 => Ok(documents.swap_remove(0)),
        count => Err(format!(
            "expected one YAML document, found {count}; remove the `---` separators"
        )),
    }
}

/// The vocabulary's message, with this host's reason where it has one.
///
/// The same seam the WebAssembly host uses, with the reasons that are a
/// file's: a hook cannot be written in YAML, and partials come from a
/// directory here rather than from the configuration.
fn explain(error: Error) -> String {
    let why = match &error.kind {
        ErrorKind::UnknownKey { key, .. } => match key.as_str() {
            "transform" | "validate" => Some(
                "a hook is code, and a configuration file cannot hold code; declare what \
                 you can here and keep the hook in a Rust host, which sees the whole document",
            ),
            "functions" => Some(
                "a function is code, and a configuration file cannot hold code. Markdoc's \
                 own functions are already present",
            ),
            "partials" => Some(
                "partials come from --partials, a directory of files, not from the \
                 configuration",
            ),
            _ => None,
        },
        ErrorKind::MatchesNotAList => Some("write the acceptable values out as a list"),
        _ => None,
    };
    match why {
        Some(why) => error.explained(why).to_string(),
        None => error.to_string(),
    }
}

/// A YAML node, read as a declaration.
///
/// A newtype because the trait and the node are both foreign here, and a
/// borrow because a declaration is read, not kept: `get` and `items` hand
/// back references into the document rather than copies of its subtrees.
struct Yaml<'y>(&'y YamlOwned);

/// The node under any tags: `!!str 3` is the string `3`, read as one.
///
/// A loop rather than a recursion, though a tag on a tag does not occur in
/// practice, because the depth would otherwise be the file's to choose.
fn untagged(mut node: &YamlOwned) -> &YamlOwned {
    while let YamlOwned::Tagged(_, inner) = node {
        node = inner;
    }
    node
}

/// A mapping key as text, for `keys`.
///
/// A scalar key is its text, so `2024:` and `true:` are the keys `2024` and
/// `true`, as they would be as JavaScript object keys. A key that is a list
/// or a mapping has no text; it is named as such here, and refused by
/// [`Declaration::get`] when reached, so that nothing under it is read as
/// something else.
fn key_text(key: &YamlOwned) -> Option<String> {
    match untagged(key) {
        YamlOwned::Value(ScalarOwned::String(text)) | YamlOwned::Representation(text, _, _) => {
            Some(text.clone())
        }
        YamlOwned::Value(ScalarOwned::Integer(number)) => Some(number.to_string()),
        YamlOwned::Value(ScalarOwned::FloatingPoint(number)) => {
            Some(number.into_inner().to_string())
        }
        YamlOwned::Value(ScalarOwned::Boolean(flag)) => Some(flag.to_string()),
        YamlOwned::Value(ScalarOwned::Null) => Some("null".to_owned()),
        _ => None,
    }
}

/// The name a key without text is listed under.
const UNNAMEABLE: &str = "<non-scalar key>";

impl<'y> Declaration for Yaml<'y> {
    fn shape(&self) -> Shape {
        match untagged(self.0) {
            YamlOwned::Value(ScalarOwned::Null) => Shape::Null,
            YamlOwned::Value(ScalarOwned::Boolean(_)) => Shape::Boolean,
            YamlOwned::Value(ScalarOwned::Integer(_) | ScalarOwned::FloatingPoint(_)) => {
                Shape::Number
            }
            YamlOwned::Value(ScalarOwned::String(_)) | YamlOwned::Representation(..) => {
                Shape::String
            }
            YamlOwned::Sequence(_) => Shape::List,
            YamlOwned::Mapping(_) => Shape::Object,
            YamlOwned::Alias(_) => Shape::Other("alias"),
            YamlOwned::BadValue => Shape::Other("malformed value"),
            // `untagged` strips these; the arm is for the compiler.
            YamlOwned::Tagged(..) => Shape::Other("tagged value"),
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match untagged(self.0) {
            YamlOwned::Value(ScalarOwned::Boolean(flag)) => Some(*flag),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<String> {
        match untagged(self.0) {
            YamlOwned::Value(ScalarOwned::String(text)) | YamlOwned::Representation(text, _, _) => {
                Some(text.clone())
            }
            _ => None,
        }
    }

    fn keys(&self) -> Vec<String> {
        match untagged(self.0) {
            YamlOwned::Mapping(map) => map
                .keys()
                .map(|key| key_text(key).unwrap_or_else(|| UNNAMEABLE.to_owned()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn get(&self, key: &str, at: &Path) -> Result<Option<Yaml<'y>>, Error> {
        let YamlOwned::Mapping(map) = untagged(self.0) else {
            return Ok(None);
        };
        // Matched by text, so that `2024:` is found under `"2024"` and a
        // tagged key under its plain spelling.
        for (found, value) in map {
            match key_text(found) {
                Some(text) if text == key => return Ok(Some(Yaml(value))),
                None if key == UNNAMEABLE => {
                    return Err(Error::new(
                        at.clone(),
                        ErrorKind::Expected {
                            what: "a scalar key",
                            got: Yaml(found).shape(),
                        },
                    ));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    fn items(&self) -> Vec<Yaml<'y>> {
        match untagged(self.0) {
            YamlOwned::Sequence(items) => items.iter().map(Yaml).collect(),
            _ => Vec::new(),
        }
    }

    /// Iterative, for the reason every walk over a document's own structure
    /// is: a configuration is a file, and a file's nesting is its author's.
    #[allow(
        clippy::cast_precision_loss,
        reason = "Markdoc has one numeric type, f64, and an integer past 2^53 loses precision exactly as upstream's parseFloat does on the same text"
    )]
    fn to_value(&self, at: &Path) -> Result<Value, Error> {
        enum Step<'y> {
            Read(&'y YamlOwned, Path),
            Array(usize),
            Object(Vec<String>),
        }

        let mut steps = vec![Step::Read(self.0, at.clone())];
        let mut values: Vec<Value> = Vec::new();

        while let Some(step) = steps.pop() {
            match step {
                Step::Read(node, path) => match untagged(node) {
                    YamlOwned::Value(ScalarOwned::Null) => values.push(Value::Null),
                    YamlOwned::Value(ScalarOwned::Boolean(flag)) => {
                        values.push(Value::Boolean(*flag));
                    }
                    YamlOwned::Value(ScalarOwned::Integer(number)) => {
                        values.push(Value::Number(*number as f64));
                    }
                    YamlOwned::Value(ScalarOwned::FloatingPoint(number)) => {
                        values.push(Value::Number(number.into_inner()));
                    }
                    YamlOwned::Value(ScalarOwned::String(text))
                    | YamlOwned::Representation(text, _, _) => {
                        values.push(Value::String(text.clone()));
                    }
                    YamlOwned::Sequence(items) => {
                        steps.push(Step::Array(items.len()));
                        for (index, item) in items.iter().enumerate().rev() {
                            steps.push(Step::Read(item, path.index(index)));
                        }
                    }
                    YamlOwned::Mapping(map) => {
                        let mut keys = Vec::with_capacity(map.len());
                        for key in map.keys() {
                            match key_text(key) {
                                Some(text) => keys.push(text),
                                None => {
                                    return Err(Error::new(
                                        path,
                                        ErrorKind::Expected {
                                            what: "scalar keys",
                                            got: Yaml(key).shape(),
                                        },
                                    ));
                                }
                            }
                        }
                        steps.push(Step::Object(keys.clone()));
                        for (key, value) in keys.iter().zip(map.values()).rev() {
                            steps.push(Step::Read(value, path.child(key)));
                        }
                    }
                    YamlOwned::Alias(_) => {
                        return Err(Error::new(
                            path,
                            ErrorKind::NoCounterpart("alias".to_owned()),
                        ));
                    }
                    YamlOwned::BadValue => {
                        return Err(Error::new(
                            path,
                            ErrorKind::NoCounterpart("malformed value".to_owned()),
                        ));
                    }
                    YamlOwned::Tagged(..) => {
                        return Err(Error::new(
                            path,
                            ErrorKind::NoCounterpart("tagged value".to_owned()),
                        ));
                    }
                },
                Step::Array(len) => {
                    let items = take(&mut values, len);
                    values.push(Value::Array(items));
                }
                Step::Object(keys) => {
                    let items = take(&mut values, keys.len());
                    let map: IndexMap<String, Value> = keys.into_iter().zip(items).collect();
                    values.push(Value::Hash(map));
                }
            }
        }

        Ok(values.pop().unwrap_or(Value::Null))
    }
}

/// Take the last `count` values, oldest first.
///
/// A short stack is impossible -- every container schedules exactly the steps
/// it later consumes -- but it is handled rather than indexed, because
/// `indexing_slicing` is denied here for the reason it is denied in the
/// library: a proof that holds today is not a promise.
fn take(values: &mut Vec<Value>, count: usize) -> Vec<Value> {
    match values.len().checked_sub(count) {
        Some(start) => values.split_off(start),
        None => std::mem::take(values),
    }
}
