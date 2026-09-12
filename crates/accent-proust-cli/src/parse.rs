//! `accent-proust parse`: the syntax tree, as JSON.
//!
//! For seeing what the parser made of a document before any schema touched
//! it: every node with its type, tag, attributes, children, slots, lines,
//! location, annotations and the errors the parser itself reported. One JSON
//! value per input, one per line, as `transform` prints. No configuration is
//! read, because parsing needs none.

use std::process::ExitCode;

use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};
use clap::Args;

use crate::exit::{Exit, emit, report};
use crate::input::{InputArgs, read};
use crate::json;

/// Arguments to `parse`.
#[derive(Args, Debug)]
pub struct ParseArgs {
    #[command(flatten)]
    pub input: InputArgs,
}

/// Run `parse`.
#[must_use]
pub fn run(args: &ParseArgs) -> ExitCode {
    let tokenizer = PulldownTokenizer::new();
    let mut exit = Exit::Success;
    let mut out = String::new();

    for input in read(&args.input) {
        let input = match input {
            Ok(input) => input,
            Err(message) => {
                report("parse", &message);
                exit = exit.and(Exit::Failure);
                continue;
            }
        };
        let options = ParseOptions::new().file(&input.label).location(true);
        let document = parse_with(&input.source, &tokenizer, &options);
        json::node(&mut out, &document);
        out.push('\n');
    }

    exit.and(emit("parse", &out)).code()
}
