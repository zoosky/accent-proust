//! `validate`, `render`, `transform` and `parse`, driven as a user drives
//! them: the built binary, real files, real exit codes.
//!
//! The configuration fixtures are the vocabulary the WebAssembly host reads
//! too, in YAML and in JSON, and a partials directory with a nested file.
//! Every test asserts an exit code, not only the output, because the exit
//! codes are the contract a CI pipeline binds to. Nothing here unwraps.

use std::error::Error;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

type Outcome = Result<(), Box<dyn Error>>;

/// A fixture beside this file, as text for the command line.
fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// Run the binary with `args`, feeding `stdin` if given.
fn run(args: &[&str], stdin: Option<&str>) -> Result<Output, Box<dyn Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_accent-proust"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(mut pipe), Some(input)) = (child.stdin.take(), stdin) {
        match pipe.write_all(input.as_bytes()) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::BrokenPipe => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(child.wait_with_output()?)
}

fn text(bytes: Vec<u8>) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(bytes)?)
}

// --- validate ---------------------------------------------------------------

#[test]
fn validate_without_a_config_reports_the_tag_as_undefined() -> Outcome {
    let callout = fixture("callout.md");
    let output = run(&["validate", &callout], None)?;
    assert_eq!(output.status.code(), Some(1));
    let report = text(output.stdout)?;
    // Upstream's level for an undefined tag is `critical`.
    assert!(
        report.contains(&format!("{callout}:1:1: critical[tag-undefined]")),
        "{report}"
    );
    Ok(())
}

#[test]
fn validate_with_a_config_passes_a_correct_document() -> Outcome {
    let output = run(
        &[
            "validate",
            "--config",
            &fixture("config.yaml"),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    assert_eq!(text(output.stdout)?, "");
    Ok(())
}

#[test]
fn validate_reads_json_configuration_as_yaml() -> Outcome {
    let output = run(
        &[
            "validate",
            "--config",
            &fixture("config.json"),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    Ok(())
}

#[test]
fn validate_reports_a_missing_required_attribute_by_line_and_column() -> Outcome {
    let bad = fixture("bad-callout.md");
    let output = run(
        &["validate", "--config", &fixture("config.yaml"), &bad],
        None,
    )?;
    assert_eq!(output.status.code(), Some(1));
    let report = text(output.stdout)?;
    assert!(
        report.contains("error[attribute-missing-required]"),
        "{report}"
    );
    assert!(report.contains(&format!("{bad}:1:")), "{report}");
    assert!(report.contains("'type'"), "{report}");
    Ok(())
}

#[test]
fn validate_json_is_one_object_per_input_in_the_bindings_shape() -> Outcome {
    let bad = fixture("bad-callout.md");
    let good = fixture("callout.md");
    let output = run(
        &[
            "validate",
            "--format",
            "json",
            "--config",
            &fixture("config.yaml"),
            &bad,
            &good,
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(1));
    let report = text(output.stdout)?;
    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(lines.len(), 2, "{report}");
    let first = lines.first().copied().unwrap_or_default();
    assert!(first.starts_with("{\"file\":"), "{first}");
    assert!(
        first.contains("\"id\":\"attribute-missing-required\""),
        "{first}"
    );
    assert!(first.contains("\"level\":\"error\""), "{first}");
    assert!(first.contains("\"location\":{"), "{first}");
    let second = lines.get(1).copied().unwrap_or_default();
    assert!(second.contains("\"errors\":[]"), "{second}");
    Ok(())
}

#[test]
fn validate_labels_stdin_as_file_says() -> Outcome {
    let output = run(
        &["validate", "--file", "page.md"],
        Some("{% nope %}x{% /nope %}\n"),
    )?;
    assert_eq!(output.status.code(), Some(1));
    assert!(text(output.stdout)?.starts_with("page.md:1:1: critical[tag-undefined]"));
    Ok(())
}

#[test]
fn validate_goes_on_past_an_unreadable_input() -> Outcome {
    let output = run(
        &[
            "validate",
            "--config",
            &fixture("config.yaml"),
            "no/such.md",
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("no/such.md"));
    Ok(())
}

// --- configuration ----------------------------------------------------------

#[test]
fn a_hook_in_the_config_is_refused_with_its_path_and_the_hosts_reason() -> Outcome {
    let output = run(
        &[
            "validate",
            "--config",
            &fixture("hooked.yaml"),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    let report = text(output.stderr)?;
    assert!(report.contains("config.tags.callout.validate"), "{report}");
    assert!(report.contains("cannot hold code"), "{report}");
    Ok(())
}

#[test]
fn a_missing_config_file_exits_two_and_names_it() -> Outcome {
    let output = run(
        &[
            "validate",
            "--config",
            "no/such.yaml",
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("no/such.yaml"));
    Ok(())
}

#[test]
fn a_var_that_is_not_name_equals_value_exits_two() -> Outcome {
    let output = run(&["render", "--var", "novalue", &fixture("vars.md")], None)?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("NAME=VALUE"));
    Ok(())
}

// --- render -----------------------------------------------------------------

#[test]
fn render_uses_the_configured_element() -> Outcome {
    let output = run(
        &[
            "render",
            "--config",
            &fixture("config.yaml"),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let html = text(output.stdout)?;
    assert!(html.contains("<aside type=\"note\">"), "{html}");
    Ok(())
}

#[test]
fn render_inlines_partials_from_the_directory_at_any_depth() -> Outcome {
    let output = run(
        &[
            "render",
            "--partials",
            &fixture("partials"),
            &fixture("uses-partials.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let html = text(output.stdout)?;
    assert!(html.contains("<h1>Welcome</h1>"), "{html}");
    assert!(html.contains("Deep text."), "{html}");
    Ok(())
}

#[test]
fn vars_are_typed_and_override_the_config() -> Outcome {
    // `n=3` is a number, so `equals($n, 3)` holds; `name=World` is a string;
    // and the command line's `greeting` wins over the file's `hello`.
    let output = run(
        &[
            "render",
            "--config",
            &fixture("config.yaml"),
            "--var",
            "name=World",
            "--var",
            "n=3",
            "--var",
            "greeting=hi",
            &fixture("vars.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let html = text(output.stdout)?;
    assert!(html.contains("Hello World!"), "{html}");
    assert!(html.contains("three"), "{html}");
    assert!(html.contains("hi"), "{html}");
    assert!(!html.contains("hello"), "{html}");
    Ok(())
}

#[test]
fn a_quoted_var_is_a_string_and_an_unquoted_number_is_not() -> Outcome {
    let output = run(
        &[
            "render",
            "--var",
            "n=\"3\"",
            "--var",
            "name=x",
            &fixture("vars.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    // The string "3" is not the number 3.
    assert!(!text(output.stdout)?.contains("three"));
    Ok(())
}

#[test]
fn render_concatenates_inputs_in_order() -> Outcome {
    let output = run(
        &[
            "render",
            "--config",
            &fixture("config.yaml"),
            &fixture("callout.md"),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(output.stdout)?.matches("<aside").count(), 2);
    Ok(())
}

// --- transform and parse ----------------------------------------------------

#[test]
fn transform_prints_a_tag_object_per_input() -> Outcome {
    let output = run(
        &[
            "transform",
            "--config",
            &fixture("config.yaml"),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let json = text(output.stdout)?;
    assert_eq!(json.lines().count(), 1, "{json}");
    assert!(json.contains("\"$$mdtype\":\"Tag\""), "{json}");
    assert!(json.contains("\"name\":\"aside\""), "{json}");
    assert!(
        json.contains("\"attributes\":{\"type\":\"note\"}"),
        "{json}"
    );
    // One array per input, as the WebAssembly `transform` returns one.
    assert!(
        json.starts_with("[{\"$$mdtype\":\"Tag\",\"name\":\"article\""),
        "{json}"
    );
    Ok(())
}

#[test]
fn parse_prints_the_document_node() -> Outcome {
    let output = run(&["parse"], Some("# Title\n"))?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let json = text(output.stdout)?;
    assert!(
        json.starts_with("{\"$$mdtype\":\"Node\",\"type\":\"document\""),
        "{json}"
    );
    assert!(json.contains("\"type\":\"heading\""), "{json}");
    assert!(json.contains("\"attributes\":{\"level\":1}"), "{json}");
    assert!(
        json.contains("\"location\":{\"file\":\"<stdin>\""),
        "{json}"
    );
    Ok(())
}

#[test]
fn parse_escapes_what_json_must() -> Outcome {
    let output = run(&["parse"], Some("say \"hi\"\\\n"))?;
    assert_eq!(output.status.code(), Some(0));
    let json = text(output.stdout)?;
    assert!(json.contains("say \\\"hi\\\"\\\\"), "{json}");
    Ok(())
}
