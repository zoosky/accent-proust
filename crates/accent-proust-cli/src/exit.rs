//! The three exit codes, and the rule that the worst one wins.
//!
//! Every command reports the same way: 0 when nothing was wrong, 1 when a
//! document was, 2 when the invocation or an input was. A run over many
//! inputs answers with the worst of them, so that CI sees one number and
//! it is the one that matters.

use std::io::{self, Write};
use std::process::ExitCode;

/// What a run found, worst last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exit {
    /// Nothing wrong.
    Success,
    /// A document has a problem: validation errors, a `--check` that would
    /// change something.
    Problem,
    /// The invocation or an input has: a usage error, an unreadable file, a
    /// configuration that does not declare.
    Failure,
}

impl Exit {
    /// The process exit code.
    #[must_use]
    pub fn code(self) -> ExitCode {
        match self {
            Exit::Success => ExitCode::SUCCESS,
            Exit::Problem => ExitCode::from(1),
            Exit::Failure => ExitCode::from(2),
        }
    }

    /// The worse of two.
    #[must_use]
    pub fn and(self, other: Exit) -> Exit {
        self.max(other)
    }
}

/// One line on stderr, naming the command.
///
/// A failure to write the report is not itself reported: there is nowhere
/// left to say so, and the exit code carries the news.
pub fn report(command: &str, message: &str) {
    let _ = writeln!(io::stderr().lock(), "accent-proust {command}: {message}");
}

/// Everything a command printed, to stdout at once.
///
/// One write rather than one per input, so a broken pipe is one failure and
/// the output of a run is never half of it.
#[must_use]
pub fn emit(command: &str, text: &str) -> Exit {
    match io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Exit::Success,
        Err(error) => {
            report(command, &format!("stdout: {error}"));
            Exit::Failure
        }
    }
}
