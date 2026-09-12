//! `accent-proust`, the command-line host for the library of the same name.
//!
//! The library reads no files, decides no HTML policy and loads no
//! configuration; a host does those, and this binary is the second one beside
//! the WebAssembly bindings. It ships one command at a time, in the order
//! `specs/features/cli-and-host-seams.md` sequences them: [`fmt`] first,
//! because it needs no configuration and is useful on day one to anyone with
//! a Markdoc file.
//!
//! # Exit codes
//!
//! | Code | Means |
//! |---|---|
//! | 0 | Success |
//! | 1 | The document has a problem: `fmt --check` found a change |
//! | 2 | The invocation or an input has: a usage error, an unreadable file |
//!
//! Keeping 1 apart from 2 is what lets a CI pipeline tell "the docs are wrong"
//! from "the tool is misconfigured", which are different alerts. clap exits 2
//! on a usage error of its own, which is why 2 is the code for ours.

mod fmt;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// The command line: one subcommand per stage the library exposes.
#[derive(Parser)]
#[command(name = "accent-proust", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The subcommands. One so far; the rest follow the specification's steps.
#[derive(Subcommand)]
enum Command {
    /// Reprint Markdoc source in canonical form.
    Fmt(fmt::FmtArgs),
}

/// Parse the arguments and run the command. The exit code is the command's.
fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Fmt(args) => fmt::run(&args),
    }
}
