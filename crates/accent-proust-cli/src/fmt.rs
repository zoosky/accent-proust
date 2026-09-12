//! `accent-proust fmt`: reprint Markdoc source in canonical form.
//!
//! The first command, and the one that needs no configuration. `format` is a
//! pure function of the parse, and `parse(format(ast))` returns the same tree,
//! tested in the library. `format(parse(s))` settles in one pass on every
//! document but one shape the library documents rather than hides
//! (`tests/formatter.rs`, "the escape set is upstream's three starters"): a
//! paragraph that begins with a fence marker reprints as itself and re-parses
//! as a fence, so the second pass differs from the first. A formatter a CI
//! pipeline runs has to settle whatever the shape, so this one reformats its
//! own output until it stops changing -- one extra pass to confirm, on every
//! document -- and refuses with exit 2 if [`MAX_PASSES`] are not enough. That
//! is the contract `--check` and `--write` need: write, then check, is clean.
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
//!
//! # Line endings
//!
//! The formatter writes `\n`. A CRLF file is read with its endings normalised
//! first, so the tag grammar never sees a `\r`; it is then reported as changed
//! by name -- not by a diff in which every line is removed and added back
//! looking identical -- and rewritten with LF by `--write`.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use accent_proust::format::{FormatOptions, OrderedListMode, format_with};
use accent_proust::parse;
use clap::{Args, ValueEnum};

/// Passes after which a document that is still changing is given up on.
///
/// The known unstable shape settles on the second pass, and a third confirms
/// it. Four is room for one more the library has not documented yet, which is
/// a bug to report rather than a document to keep chasing.
const MAX_PASSES: usize = 4;

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

/// What the command does with a canonical document. Decided once, from the
/// flags, so that each input asks one question rather than two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Print it.
    Print,
    /// Say whether it differs, and how.
    Check,
    /// Put it back where the source came from.
    Write,
}

impl Mode {
    /// The mode the flags ask for. clap has already refused both at once.
    fn of(args: &FmtArgs) -> Mode {
        if args.check {
            Mode::Check
        } else if args.write {
            Mode::Write
        } else {
            Mode::Print
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
    /// Could not be read, settled, written or printed. Reported on stderr as
    /// it happened; the run goes on to the next input so that one bad path
    /// does not hide the rest.
    Failed,
}

/// A document brought to rest.
struct Canonical {
    /// The settled text, LF-terminated.
    text: String,
    /// Whether it differs from the source as read, line endings included.
    changed: bool,
}

/// Run `fmt`.
///
/// The exit code: 2 if any input failed, otherwise 1 if `--check` found a
/// change, otherwise 0. `--write` exits 0 when it changed something, because
/// changing it was the request.
#[must_use]
pub fn run(args: &FmtArgs) -> ExitCode {
    let options = options(args);
    let mode = Mode::of(args);
    let outcomes: Vec<Outcome> = if args.paths.is_empty() {
        vec![from_stdin(mode, &options)]
    } else {
        args.paths
            .iter()
            .map(|path| from_file(mode, &options, path))
            .collect()
    };
    code(&outcomes, mode)
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

/// Format stdin. `--write` is refused here, because there is nothing to
/// write back to, and so the file-only mode never reaches the code below.
fn from_stdin(mode: Mode, options: &FormatOptions) -> Outcome {
    const LABEL: &str = "<stdin>";
    if mode == Mode::Write {
        report(
            LABEL,
            "--write needs a file; stdin has nowhere to be written back to",
        );
        return Outcome::Failed;
    }
    let source = match io::read_to_string(io::stdin()) {
        Ok(source) => source,
        Err(error) => {
            report(LABEL, &error.to_string());
            return Outcome::Failed;
        }
    };
    let Some(canonical) = settle(LABEL, &source, options) else {
        return Outcome::Failed;
    };
    if mode == Mode::Check {
        check(LABEL, &source, &canonical)
    } else {
        print(&canonical)
    }
}

/// Format one file, in whichever mode.
fn from_file(mode: Mode, options: &FormatOptions, path: &Path) -> Outcome {
    let label = path.display().to_string();
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            report(&label, &error.to_string());
            return Outcome::Failed;
        }
    };
    let Some(canonical) = settle(&label, &source, options) else {
        return Outcome::Failed;
    };
    match mode {
        Mode::Print => print(&canonical),
        Mode::Check => check(&label, &source, &canonical),
        Mode::Write => write(&label, path, &canonical),
    }
}

/// Format `source` until it stops changing, or give up.
///
/// `None` has been reported: the document is still changing after
/// [`MAX_PASSES`], which is a formatter bug and not a document to keep
/// chasing.
fn settle(label: &str, source: &str, options: &FormatOptions) -> Option<Canonical> {
    let normalised = source.replace("\r\n", "\n");
    let mut text = format_with(&parse::parse(&normalised), options);
    for _ in 1..MAX_PASSES {
        let again = format_with(&parse::parse(&text), options);
        if again == text {
            let changed = text != source;
            return Some(Canonical { text, changed });
        }
        text = again;
    }
    report(
        label,
        &format!(
            "still changing after {MAX_PASSES} passes; the formatter does not settle on this document"
        ),
    );
    None
}

/// Write the canonical text to stdout.
fn print(canonical: &Canonical) -> Outcome {
    match io::stdout().lock().write_all(canonical.text.as_bytes()) {
        Ok(()) if canonical.changed => Outcome::Changed,
        Ok(()) => Outcome::Unchanged,
        Err(error) => {
            report("stdout", &error.to_string());
            Outcome::Failed
        }
    }
}

/// Say what would change, on stdout.
///
/// CRLF endings are named rather than diffed. The diff is over the normalised
/// source, so a CRLF file that also needs reformatting shows the reformatting
/// and not every line of the file.
fn check(label: &str, source: &str, canonical: &Canonical) -> Outcome {
    if !canonical.changed {
        return Outcome::Unchanged;
    }
    let normalised = source.replace("\r\n", "\n");
    let mut text = String::new();
    if normalised.len() != source.len() {
        // Writing to a `String` cannot fail; the `Result` is the trait's.
        let _ = writeln!(text, "{label}: CRLF line endings; fmt writes LF");
    }
    if canonical.text != normalised {
        let diff = similar::TextDiff::from_lines(normalised.as_str(), canonical.text.as_str());
        text.push_str(
            &diff
                .unified_diff()
                .header(&format!("a/{label}"), &format!("b/{label}"))
                .to_string(),
        );
    }
    match io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Outcome::Changed,
        Err(error) => {
            report("stdout", &error.to_string());
            Outcome::Failed
        }
    }
}

/// Put the canonical text back in `path`, if it differs.
fn write(label: &str, path: &Path, canonical: &Canonical) -> Outcome {
    if !canonical.changed {
        return Outcome::Unchanged;
    }
    match std::fs::write(path, &canonical.text) {
        Ok(()) => Outcome::Changed,
        Err(error) => {
            report(label, &error.to_string());
            Outcome::Failed
        }
    }
}

/// The exit code for a set of outcomes. See [`run`]. The one place the policy
/// lives; clap's own usage errors exit 2 on their own.
fn code(outcomes: &[Outcome], mode: Mode) -> ExitCode {
    if outcomes.contains(&Outcome::Failed) {
        ExitCode::from(2)
    } else if mode == Mode::Check && outcomes.contains(&Outcome::Changed) {
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
