//! `accent-proust render`: a document as HTML.
//!
//! Parse, transform against the configuration, render with the library's
//! HTML renderer, print. Inputs are concatenated in argument order, as `fmt`
//! concatenates them. Validation is not run: `render` prints what the tree
//! is, and `validate` says what is wrong with it, and a pipeline that wants
//! both runs both.

use std::process::ExitCode;

use accent_proust::parse::{ParseOptions, PulldownTokenizer, parse_with};
use accent_proust::render::render_all;
use accent_proust::transform::transform;
use clap::Args;

use crate::exit::{Exit, emit, report};
use crate::host::{HostArgs, configured, load};
use crate::input::{InputArgs, read};

/// Arguments to `render`.
#[derive(Args, Debug)]
pub struct RenderArgs {
    #[command(flatten)]
    pub input: InputArgs,

    #[command(flatten)]
    pub host: HostArgs,
}

/// Run `render`.
#[must_use]
pub fn run(args: &RenderArgs) -> ExitCode {
    let Some(sources) = load("render", &args.host) else {
        return Exit::Failure.code();
    };
    let Some(config) = configured("render", &args.host, &sources) else {
        return Exit::Failure.code();
    };

    let tokenizer = PulldownTokenizer::new();
    let mut exit = Exit::Success;
    let mut out = String::new();

    for input in read(&args.input) {
        let input = match input {
            Ok(input) => input,
            Err(message) => {
                report("render", &message);
                exit = exit.and(Exit::Failure);
                continue;
            }
        };
        let options = ParseOptions::new().file(&input.label).location(true);
        let document = parse_with(&input.source, &tokenizer, &options);
        let nodes = transform(&document, &config).into_vec();
        out.push_str(&render_all(&nodes));
    }

    exit.and(emit("render", &out)).code()
}
