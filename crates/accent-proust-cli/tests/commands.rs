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
    // Upstream's field order: `attributes` first after the marker.
    assert!(
        json.starts_with("{\"$$mdtype\":\"Node\",\"attributes\":{"),
        "{json}"
    );
    assert!(json.contains("\"type\":\"document\""), "{json}");
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

// --- what the review of this step found unpinned ----------------------------

/// A directory of this test's own, removed when dropped.
fn scratch() -> Result<tempfile::TempDir, Box<dyn Error>> {
    Ok(tempfile::Builder::new()
        .prefix("accent-proust-cli-")
        .tempdir()?)
}

#[test]
fn a_non_text_file_beside_the_partials_is_passed_over() -> Outcome {
    // An image in the partials directory is not a partial and not an error;
    // a document that named it would be told so where it did.
    let dir = scratch()?;
    std::fs::write(dir.path().join("header.md"), "# Welcome\n")?;
    std::fs::write(dir.path().join("logo.png"), [0xFF, 0xFE, 0x00, 0x80])?;
    let page = dir.path().join("page.md");
    std::fs::write(&page, "{% partial file=\"header.md\" /%}\n")?;

    let output = run(
        &[
            "render",
            "--partials",
            &dir.path().to_string_lossy(),
            &page.to_string_lossy(),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    assert!(text(output.stdout)?.contains("<h1>Welcome</h1>"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_directory_symlink_under_partials_is_not_followed() -> Outcome {
    // A link back up the tree would otherwise be walked until the file
    // system gave up. A link to a file is still read.
    let dir = scratch()?;
    let partials = dir.path().join("partials");
    std::fs::create_dir_all(partials.join("sub"))?;
    std::fs::write(partials.join("header.md"), "# Welcome\n")?;
    std::os::unix::fs::symlink("..", partials.join("sub").join("loop"))?;
    std::os::unix::fs::symlink("../header.md", partials.join("sub").join("again.md"))?;
    let page = dir.path().join("page.md");
    std::fs::write(
        &page,
        "{% partial file=\"header.md\" /%}\n{% partial file=\"sub/again.md\" /%}\n",
    )?;

    let output = run(
        &[
            "render",
            "--partials",
            &partials.to_string_lossy(),
            &page.to_string_lossy(),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    assert_eq!(text(output.stdout)?.matches("<h1>Welcome</h1>").count(), 2);
    Ok(())
}

#[test]
fn a_numeric_yaml_key_is_its_text() -> Outcome {
    // `2024:` is the key `2024`, as it would be as a JavaScript object key,
    // and the grammar admits a tag by that name.
    let dir = scratch()?;
    let config = dir.path().join("config.yaml");
    std::fs::write(&config, "tags:\n  2024:\n    render: b\n")?;
    let page = dir.path().join("page.md");
    std::fs::write(&page, "{% 2024 %}x{% /2024 %}\n")?;

    let output = run(
        &[
            "render",
            "--config",
            &config.to_string_lossy(),
            &page.to_string_lossy(),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let html = text(output.stdout)?;
    assert!(html.contains("<b>"), "{html}");
    Ok(())
}

#[test]
fn a_second_yaml_document_is_refused_rather_than_dropped() -> Outcome {
    let dir = scratch()?;
    let config = dir.path().join("config.yaml");
    std::fs::write(
        &config,
        "tags:\n  callout:\n    render: aside\n---\ntags:\n  other:\n    render: div\n",
    )?;
    let output = run(
        &[
            "validate",
            "--config",
            &config.to_string_lossy(),
            &fixture("callout.md"),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("one YAML document"));
    Ok(())
}

#[test]
fn an_empty_var_name_is_refused() -> Outcome {
    // What `--var $NAME=3` becomes when `NAME` is unset in the shell.
    let output = run(&["render", "--var", "=3", &fixture("vars.md")], None)?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("NAME=VALUE"));
    Ok(())
}

#[test]
fn parse_spells_numbers_as_ecmascript_and_omits_absent_fields() -> Outcome {
    let output = run(
        &["parse"],
        Some("{% x n=1000000000000000000000 m=0.0000001 %}{% /x %}\n"),
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    let json = text(output.stdout)?;
    // `JSON.stringify(Markdoc.parse(source))`: field order, spelling, and no
    // `tag: null` on a node that has no tag.
    assert!(
        json.starts_with("{\"$$mdtype\":\"Node\",\"attributes\":{"),
        "{json}"
    );
    assert!(json.contains("\"n\":1e+21"), "{json}");
    assert!(json.contains("\"m\":1e-7"), "{json}");
    assert!(json.contains("\"tag\":\"x\""), "{json}");
    assert!(!json.contains("\"tag\":null"), "{json}");
    assert!(!json.contains("\"location\":null"), "{json}");
    Ok(())
}

#[test]
fn parse_escapes_controls_as_json_stringify_does() -> Outcome {
    let output = run(&["parse"], Some("a\u{8}b\u{1}c\n"))?;
    assert_eq!(output.status.code(), Some(0));
    let json = text(output.stdout)?;
    assert!(json.contains("a\\bb\\u0001c"), "{json}");
    Ok(())
}

#[test]
fn validate_json_positions_are_the_bindings_in_utf16_units() -> Outcome {
    // `éé` is two characters, two code units, and four bytes.
    let output = run(
        &["validate", "--format", "json", "--file", "p.md"],
        Some("éé{% nope %}x{% /nope %}\n"),
    )?;
    assert_eq!(output.status.code(), Some(1));
    let json = text(output.stdout)?;
    assert!(
        json.contains("\"start\":{\"line\":0,\"character\":2,\"offset\":2,\"byteOffset\":4}"),
        "{json}"
    );
    Ok(())
}

#[test]
fn validate_human_column_counts_characters() -> Outcome {
    let output = run(
        &["validate", "--file", "p.md"],
        Some("éé{% nope %}x{% /nope %}\n"),
    )?;
    assert_eq!(output.status.code(), Some(1));
    assert!(text(output.stdout)?.starts_with("p.md:1:3: critical[tag-undefined]"));
    Ok(())
}
