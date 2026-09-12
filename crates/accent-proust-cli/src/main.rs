//! `accent-proust`, the command-line host for the library of the same name.
//!
//! The library reads no files, decides no HTML policy and loads no
//! configuration; a host does those, and this binary is the second one beside
//! the WebAssembly bindings. Five commands, one per stage the library exposes:
//! [`fmt`] needs no configuration and is useful to anyone with a Markdoc file;
//! [`parse`] shows what the parser made of one; [`validate`], [`render`] and
//! [`transform`] read a configuration -- a YAML or JSON file, a directory of
//! partials, variables on the command line -- through [`host`], and are what
//! make a documentation repository a CI gate.
//!
//! # Exit codes
//!
//! | Code | Means |
//! |---|---|
//! | 0 | Success |
//! | 1 | The document has a problem: `fmt --check` found a change, `validate` found an error |
//! | 2 | The invocation or an input has: a usage error, an unreadable file, a configuration that does not declare |
//!
//! Keeping 1 apart from 2 is what lets a CI pipeline tell "the docs are wrong"
//! from "the tool is misconfigured", which are different alerts. clap exits 2
//! on a usage error of its own, which is why 2 is the code for ours.

mod exit;
mod fmt;
mod host;
mod input;
mod json;
mod parse;
mod render;
mod transform;
mod validate;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// The command line: one subcommand per stage the library exposes.
#[derive(Parser)]
#[command(name = "accent-proust", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The subcommands, one per stage.
#[derive(Subcommand)]
enum Command {
    /// Reprint Markdoc source in canonical form.
    Fmt(fmt::FmtArgs),
    /// Report what a schema says is wrong with a document.
    Validate(validate::ValidateArgs),
    /// Render a document to HTML.
    Render(render::RenderArgs),
    /// Print the renderable tree as JSON, one value per input.
    Transform(transform::TransformArgs),
    /// Print the syntax tree as JSON, one value per input.
    Parse(parse::ParseArgs),
}

/// Parse the arguments and run the command. The exit code is the command's.
fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Fmt(args) => fmt::run(&args),
        Command::Validate(args) => validate::run(&args),
        Command::Render(args) => render::run(&args),
        Command::Transform(args) => transform::run(&args),
        Command::Parse(args) => parse::run(&args),
    }
}
