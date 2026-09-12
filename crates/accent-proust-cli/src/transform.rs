//! `accent-proust transform`: the renderable tree, as JSON.
//!
//! One JSON value per input, one per line, in the shape the WebAssembly
//! bindings return and upstream's renderers expect: a tag is an object
//! carrying `$$mdtype: "Tag"`, `name`, `attributes` and `children`, and a
//! scalar is itself. One value per line rather than one array for the run,
//! so the output composes with `jq` and with a pipeline that reads a line at
//! a time.

use std::process::ExitCode;

use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};
use accent_proust::transform::transform;
use clap::Args;

use crate::exit::{Exit, emit, report};
use crate::host::{HostArgs, Sources};
use crate::input::{InputArgs, read};
use crate::json;

/// Arguments to `transform`.
#[derive(Args, Debug)]
pub struct TransformArgs {
    #[command(flatten)]
    pub input: InputArgs,

    #[command(flatten)]
    pub host: HostArgs,
}

/// Run `transform`.
#[must_use]
pub fn run(args: &TransformArgs) -> ExitCode {
    let sources = match Sources::read(&args.host) {
        Ok(sources) => sources,
        Err(message) => {
            report("transform", &message);
            return Exit::Failure.code();
        }
    };
    let config = match crate::host::config(&args.host, &sources) {
        Ok(config) => config,
        Err(message) => {
            report("transform", &message);
            return Exit::Failure.code();
        }
    };

    let tokenizer = PulldownTokenizer::new();
    let mut exit = Exit::Success;
    let mut out = String::new();

    for input in read(&args.input) {
        let input = match input {
            Ok(input) => input,
            Err(message) => {
                report("transform", &message);
                exit = exit.and(Exit::Failure);
                continue;
            }
        };
        let options = ParseOptions::new().file(&input.label).location(true);
        let document = parse_with(&input.source, &tokenizer, &options);
        let nodes = transform(&document, &config).into_vec();
        json::renderable(&mut out, &nodes);
        out.push('\n');
    }

    exit.and(emit("transform", &out)).code()
}
