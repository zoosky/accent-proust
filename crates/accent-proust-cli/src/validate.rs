//! `accent-proust validate`: report what a schema says is wrong.
//!
//! The command that makes a documentation repository a CI gate. Every input
//! is parsed with locations on and validated against the configuration; the
//! errors go to stdout, and the exit code says whether any of them matter.
//!
//! # What exits 1
//!
//! An error at level `error` or `critical`. A `warning`, `info` or `debug` is
//! printed and does not fail the run, which is how a schema ships a rule it
//! wants surfaced but not enforced yet -- the library's own `error_level`
//! exists for that, and a command that failed on it would take the option
//! away.
//!
//! # Two formats
//!
//! `human` is one line per error, `path:line:column: level[id]: message`,
//! lines and columns counted from one and the column in characters, because
//! that is what an editor shows. `json` is one object per input, one per
//! line, each carrying the file and its errors in the shape the WebAssembly
//! bindings return, positions included -- `character` and `offset` in UTF-16
//! code units and `byteOffset` in bytes, as the bindings write them -- so a
//! consumer written against either host reads the other. Error ids are
//! upstream's, so a consumer written against Markdoc's codes reads either
//! format unchanged.

use std::fmt::Write as _;
use std::process::ExitCode;

use accent_proust::ast::ErrorLevel;
use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};
use accent_proust::validate::{ValidateError, validate_tree};
use clap::{Args, ValueEnum};

use crate::exit::{Exit, emit, report};
use crate::host::{HostArgs, configured, load};
use crate::input::{InputArgs, read};
use crate::json;

/// Arguments to `validate`.
#[derive(Args, Debug)]
pub struct ValidateArgs {
    #[command(flatten)]
    pub input: InputArgs,

    #[command(flatten)]
    pub host: HostArgs,

    /// How to print the errors.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub format: Format,
}

/// The output format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// One line per error: `path:line:column: level[id]: message`.
    Human,
    /// One object per input, one per line, in the bindings' shape.
    Json,
}

/// Run `validate`.
#[must_use]
pub fn run(args: &ValidateArgs) -> ExitCode {
    let Some(sources) = load("validate", &args.host) else {
        return Exit::Failure.code();
    };
    let Some(config) = configured("validate", &args.host, &sources) else {
        return Exit::Failure.code();
    };

    let tokenizer = PulldownTokenizer::new();
    let mut exit = Exit::Success;
    let mut out = String::new();

    for input in read(&args.input) {
        let input = match input {
            Ok(input) => input,
            Err(message) => {
                report("validate", &message);
                exit = exit.and(Exit::Failure);
                continue;
            }
        };
        let options = ParseOptions::new().file(&input.label).location(true);
        let document = parse_with(&input.source, &tokenizer, &options);
        let errors = validate_tree(&document, &config);
        if errors
            .iter()
            .any(|found| matches!(found.error.level, ErrorLevel::Error | ErrorLevel::Critical))
        {
            exit = exit.and(Exit::Problem);
        }
        match args.format {
            Format::Human => human(&mut out, &input.label, &input.source, &errors),
            Format::Json => {
                json::diagnostics(&mut out, &input.label, &input.source, &errors);
                out.push('\n');
            }
        }
    }

    exit.and(emit("validate", &out)).code()
}

/// One line per error, for a person.
fn human(out: &mut String, label: &str, source: &str, errors: &[ValidateError<'_>]) {
    for found in errors {
        let (line, column) = position(source, found);
        // Writing to a `String` cannot fail; the `Result` is the trait's.
        let _ = writeln!(
            out,
            "{label}:{line}:{column}: {}[{}]: {}",
            found.error.level.as_str(),
            found.error.id,
            found.error.message
        );
    }
}

/// Where to point, counted from one, the column in characters. The error's
/// own location first, the node's second, the node's first line third, and
/// the top of the file when nothing knows.
///
/// The library's column is a byte count from the start of the line; the
/// characters before it are counted from the source, because an editor
/// counts characters and `éé` is two of them and four bytes.
fn position(source: &str, found: &ValidateError<'_>) -> (usize, usize) {
    found.error.location.or(found.location).map_or_else(
        || (found.lines.first().map_or(1, |line| line + 1), 1),
        |spot| {
            let line_start = spot.start.offset.saturating_sub(spot.start.column);
            let characters = source
                .get(line_start..spot.start.offset)
                .map_or(spot.start.column, |prefix| prefix.chars().count());
            (spot.start.line + 1, characters + 1)
        },
    )
}
