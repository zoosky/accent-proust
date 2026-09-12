//! The documents a command reads: the files named on the command line, or
//! stdin.
//!
//! Every command but `fmt` reads its inputs the same way, so the reading is
//! written once. A file that cannot be read is reported in its place rather
//! than stopping the run, so one bad path does not hide the rest -- the
//! exit code says a path failed, and the other inputs are still answered.

use std::io;
use std::path::PathBuf;

use clap::Args;

/// Where the documents come from, and what to call stdin.
#[derive(Args, Debug)]
pub struct InputArgs {
    /// Files to read. With none, reads stdin.
    pub paths: Vec<PathBuf>,

    /// The name to report stdin under in diagnostics. Default: `<stdin>`.
    #[arg(long, value_name = "LABEL")]
    pub file: Option<String>,
}

/// One document, with the name it is reported under.
///
/// The label is a file's path as given, or `--file` for stdin. The library
/// borrows both -- a location carries its file label, and a node its source
/// -- so an input owns both for as long as its document is in use.
pub struct Input {
    /// What diagnostics call it.
    pub label: String,
    /// The text.
    pub source: String,
}

/// Read every input, in order. An input that could not be read is an `Err`
/// carrying its report, in its place.
#[must_use]
pub fn read(args: &InputArgs) -> Vec<Result<Input, String>> {
    if args.paths.is_empty() {
        let label = args.file.clone().unwrap_or_else(|| "<stdin>".to_owned());
        return vec![match io::read_to_string(io::stdin()) {
            Ok(source) => Ok(Input { label, source }),
            Err(error) => Err(format!("{label}: {error}")),
        }];
    }
    args.paths
        .iter()
        .map(|path| {
            let label = path.display().to_string();
            match std::fs::read_to_string(path) {
                Ok(source) => Ok(Input { label, source }),
                Err(error) => Err(format!("{label}: {error}")),
            }
        })
        .collect()
}
