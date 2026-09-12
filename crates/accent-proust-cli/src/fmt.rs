//! `accent-proust fmt`: reprint Markdoc source in canonical form.
//!
//! The first command, and the one that needs no configuration. `format` is a
//! pure function of the parse; `format(parse(s))` is idempotent and
//! `parse(format(ast))` returns the same tree, both tested in the library.
//! That is exactly the contract `--check` and `--write` need: a formatter that
//! could change a document's meaning, or that never settled, could not be run
//! in CI.
//!
//! # Three modes
//!
//! - Neither flag: the formatted source goes to stdout, files concatenated in
//!   argument order. What `cat` would print, formatted.
//! - `--check`: nothing is written. A unified diff of what would change goes to
//!   stdout, one per file, and the exit code says whether anything would.
//!   rustfmt's behaviour rather than prettier's: a list of files answers
//!   "which", a diff also answers "what is wrong with them", and the list is
//!   recoverable from the diff headers while the diff is not recoverable from
//!   the list.
//! - `--write`: each file that would change is rewritten in place. A file that
//!   would not change is not touched, so its timestamp stays honest.
//!
//! With no paths the source is stdin, labelled `<stdin>` in a diff. `--write`
//! then has nowhere to write back to and is refused.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use accent_proust::format::{FormatOptions, OrderedListMode, format_with};
use accent_proust::parse;
use clap::{Args, ValueEnum};

/// Arguments to `fmt`.
#[derive(Args, Debug)]
pub struct FmtArgs {
    /// Files to format. With none, reads stdin and writes stdout.
    pub paths: Vec<PathBuf>,

    /// Print a unified diff of what would change, and exit 1 if anything would.
    #[arg(long, conflicts_with = "write")]
    pub check: bool,

    /// Rewrite each file in place. Needs paths: stdin cannot be written back.
    #[arg(long)]
    pub write: bool,

    /// The width past which a block tag's opening breaks across lines.
    /// Default: the library's, 80.
    #[arg(long, value_name = "COLUMNS")]
    pub max_tag_opening_width: Option<usize>,

    /// Whether a numbered list reprints its numbers. Default: the library's,
    /// repeat.
    #[arg(long, value_enum, value_name = "MODE")]
    pub ordered_list_mode: Option<ListMode>,
}

/// `--ordered-list-mode`, as the command line spells the library's
/// `OrderedListMode`. A separate type so that clap's derive never reaches
/// into the library's enum, which is `#[non_exhaustive]` and not clap's to
/// name.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ListMode {
    /// The list's start number on the first item and `1` on the rest.
    Repeat,
    /// Consecutive numbers, counting up from the list's start.
    Increment,
}

impl From<ListMode> for OrderedListMode {
    fn from(mode: ListMode) -> OrderedListMode {
        match mode {
            ListMode::Repeat => OrderedListMode::Repeat,
            ListMode::Increment => OrderedListMode::Increment,
        }
    }
}

/// What happened to one input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// Already canonical.
    Unchanged,
    /// Would change, or did.
    Changed,
    /// Could not be read, written or printed. Reported on stderr as it
    /// happened; the run goes on to the next input so that one bad path does
    /// not hide the rest.
    Failed,
}

/// Run `fmt`.
///
/// The exit code: 2 if any input failed, otherwise 1 if `--check` found a
/// change, otherwise 0. `--write` exits 0 when it changed something, because
/// changing it was the request.
#[must_use]
pub fn run(args: &FmtArgs) -> ExitCode {
    let options = options(args);

    if args.paths.is_empty() {
        if args.write {
            report(
                "<stdin>",
                "--write needs a file; stdin has nowhere to be written back to",
            );
            return ExitCode::from(2);
        }
        let outcome = match io::read_to_string(io::stdin()) {
            Ok(source) => one(args, &options, "<stdin>", &source, None),
            Err(error) => {
                report("<stdin>", &error.to_string());
                Outcome::Failed
            }
        };
        return code(&[outcome], args.check);
    }

    let outcomes: Vec<Outcome> = args
        .paths
        .iter()
        .map(|path| {
            let label = path.display().to_string();
            match std::fs::read_to_string(path) {
                Ok(source) => one(args, &options, &label, &source, Some(path)),
                Err(error) => {
                    report(&label, &error.to_string());
                    Outcome::Failed
                }
            }
        })
        .collect();
    code(&outcomes, args.check)
}

/// The library's defaults, with only what the command line said applied.
///
/// The flags are optional rather than defaulted here so that the command line
/// never restates a default the library owns; if the library's width moved,
/// this would move with it.
fn options(args: &FmtArgs) -> FormatOptions {
    let mut options = FormatOptions::new();
    if let Some(width) = args.max_tag_opening_width {
        options = options.max_tag_opening_width(width);
    }
    if let Some(mode) = args.ordered_list_mode {
        options = options.ordered_list_mode(mode.into());
    }
    options
}

/// Format one input and do what the mode asks with the result.
///
/// `path` is `None` for stdin, which only `--write` cares about, and it is
/// refused before this is reached.
fn one(
    args: &FmtArgs,
    options: &FormatOptions,
    label: &str,
    source: &str,
    path: Option<&Path>,
) -> Outcome {
    let formatted = format_with(&parse::parse(source), options);
    let changed = formatted != source;

    if args.check {
        if !changed {
            return Outcome::Unchanged;
        }
        let diff = similar::TextDiff::from_lines(source, &formatted);
        let text = diff
            .unified_diff()
            .header(&format!("a/{label}"), &format!("b/{label}"))
            .to_string();
        return match io::stdout().lock().write_all(text.as_bytes()) {
            Ok(()) => Outcome::Changed,
            Err(error) => {
                report("stdout", &error.to_string());
                Outcome::Failed
            }
        };
    }

    if args.write {
        if !changed {
            return Outcome::Unchanged;
        }
        let Some(path) = path else {
            report(label, "nothing to write to");
            return Outcome::Failed;
        };
        return match std::fs::write(path, &formatted) {
            Ok(()) => Outcome::Changed,
            Err(error) => {
                report(label, &error.to_string());
                Outcome::Failed
            }
        };
    }

    match io::stdout().lock().write_all(formatted.as_bytes()) {
        Ok(()) if changed => Outcome::Changed,
        Ok(()) => Outcome::Unchanged,
        Err(error) => {
            report("stdout", &error.to_string());
            Outcome::Failed
        }
    }
}

/// The exit code for a set of outcomes. See [`run`].
fn code(outcomes: &[Outcome], check: bool) -> ExitCode {
    if outcomes.contains(&Outcome::Failed) {
        ExitCode::from(2)
    } else if check && outcomes.contains(&Outcome::Changed) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// One line on stderr, naming the input.
///
/// A failure to write the report is not itself reported: there is nowhere
/// left to say so, and the exit code carries the news.
fn report(label: &str, message: &str) {
    let _ = writeln!(io::stderr().lock(), "accent-proust fmt: {label}: {message}");
}
