//! `accent-proust fmt`, driven as a user drives it: the built binary, real
//! files, real exit codes.
//!
//! The exit codes are the contract a CI pipeline binds to, so every test
//! asserts one and not only the output. Nothing here unwraps: a test that
//! panics on its own fixture reads as a failure in the code under test.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A tag opening with the spacing the formatter normalises.
const MESSY: &str = "{% callout   type=\"note\"  %}\nBody\n{% /callout %}\n";
/// The same document, canonical.
const CLEAN: &str = "{% callout type=\"note\" %}\nBody\n{% /callout %}\n";

/// A fixture beside this file.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Run the binary with `args`, feeding `stdin` if given.
fn run(args: &[&str], stdin: Option<&str>) -> Result<Output, Box<dyn Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_accent-proust"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Taken either way, so a command that does read stdin sees EOF: the pipe
    // drops at the end of the `if let` whether or not the pattern matched.
    if let (Some(mut pipe), Some(input)) = (child.stdin.take(), stdin) {
        pipe.write_all(input.as_bytes())?;
    }
    Ok(child.wait_with_output()?)
}

/// Bytes as text, for the assertions.
fn text(bytes: Vec<u8>) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(bytes)?)
}

/// A directory of this test's own, for `--write`.
fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("accent-proust-cli-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn the_fixtures_are_what_the_tests_say_they_are() -> Result<(), Box<dyn Error>> {
    assert_eq!(fs::read_to_string(fixture("messy.md"))?, MESSY);
    assert_eq!(fs::read_to_string(fixture("clean.md"))?, CLEAN);
    Ok(())
}

#[test]
fn stdin_is_formatted_to_stdout() -> Result<(), Box<dyn Error>> {
    let output = run(&["fmt"], Some(MESSY))?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(output.stdout)?, CLEAN);
    Ok(())
}

#[test]
fn files_are_formatted_to_stdout_in_order() -> Result<(), Box<dyn Error>> {
    let messy = fixture("messy.md");
    let clean = fixture("clean.md");
    let output = run(
        &["fmt", &messy.to_string_lossy(), &clean.to_string_lossy()],
        None,
    )?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(output.stdout)?, format!("{CLEAN}{CLEAN}"));
    Ok(())
}

#[test]
fn a_clean_file_passes_check_silently() -> Result<(), Box<dyn Error>> {
    let clean = fixture("clean.md");
    let output = run(&["fmt", "--check", &clean.to_string_lossy()], None)?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(output.stdout)?, "");
    assert_eq!(text(output.stderr)?, "");
    Ok(())
}

#[test]
fn a_messy_file_fails_check_with_a_unified_diff() -> Result<(), Box<dyn Error>> {
    let messy = fixture("messy.md");
    let path = messy.to_string_lossy().into_owned();
    let output = run(&["fmt", "--check", &path], None)?;
    assert_eq!(output.status.code(), Some(1));
    let diff = text(output.stdout)?;
    assert!(diff.contains(&format!("--- a/{path}")), "{diff}");
    assert!(diff.contains(&format!("+++ b/{path}")), "{diff}");
    assert!(diff.contains("-{% callout   type=\"note\"  %}"), "{diff}");
    assert!(diff.contains("+{% callout type=\"note\" %}"), "{diff}");
    // Not written: `--check` looks and reports.
    assert_eq!(fs::read_to_string(&messy)?, MESSY);
    Ok(())
}

#[test]
fn stdin_is_labelled_in_a_check_diff() -> Result<(), Box<dyn Error>> {
    let output = run(&["fmt", "--check"], Some(MESSY))?;
    assert_eq!(output.status.code(), Some(1));
    let diff = text(output.stdout)?;
    assert!(diff.contains("--- a/<stdin>"), "{diff}");
    Ok(())
}

#[test]
fn write_rewrites_in_place_and_settles() -> Result<(), Box<dyn Error>> {
    let dir = scratch("write")?;
    let page = dir.join("page.md");
    fs::write(&page, MESSY)?;
    let path = page.to_string_lossy().into_owned();

    let output = run(&["fmt", "--write", &path], None)?;
    assert_eq!(output.status.code(), Some(0), "{}", text(output.stderr)?);
    assert_eq!(fs::read_to_string(&page)?, CLEAN);

    // Idempotent: the rewritten file passes `--check`, and a second `--write`
    // changes nothing.
    let output = run(&["fmt", "--check", &path], None)?;
    assert_eq!(output.status.code(), Some(0));
    let output = run(&["fmt", "--write", &path], None)?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&page)?, CLEAN);

    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn write_reports_the_file_it_cannot_write_and_goes_on() -> Result<(), Box<dyn Error>> {
    // A directory cannot be read as a file: the report names it, the exit code
    // is 2, and the other file was still formatted.
    let dir = scratch("write-failure")?;
    let page = dir.join("page.md");
    fs::write(&page, MESSY)?;
    let output = run(
        &[
            "fmt",
            "--write",
            &dir.to_string_lossy(),
            &page.to_string_lossy(),
        ],
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains(&dir.to_string_lossy().into_owned()));
    assert_eq!(fs::read_to_string(&page)?, CLEAN);
    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn write_without_a_path_is_refused() -> Result<(), Box<dyn Error>> {
    let output = run(&["fmt", "--write"], Some(MESSY))?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("--write"));
    assert_eq!(text(output.stdout)?, "");
    Ok(())
}

#[test]
fn check_and_write_are_mutually_exclusive() -> Result<(), Box<dyn Error>> {
    let clean = fixture("clean.md");
    let output = run(
        &["fmt", "--check", "--write", &clean.to_string_lossy()],
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

#[test]
fn an_unreadable_path_exits_two_and_names_it() -> Result<(), Box<dyn Error>> {
    let output = run(&["fmt", "--check", "no/such/page.md"], None)?;
    assert_eq!(output.status.code(), Some(2));
    assert!(text(output.stderr)?.contains("no/such/page.md"));
    Ok(())
}

#[test]
fn ordered_list_mode_increment_renumbers() -> Result<(), Box<dyn Error>> {
    let list = "1. a\n1. b\n1. c\n";
    let output = run(&["fmt"], Some(list))?;
    let repeated = text(output.stdout)?;
    assert!(repeated.contains("1. b"), "{repeated}");
    assert!(!repeated.contains("2. b"), "{repeated}");

    let output = run(&["fmt", "--ordered-list-mode", "increment"], Some(list))?;
    let incremented = text(output.stdout)?;
    assert!(incremented.contains("2. b"), "{incremented}");
    assert!(incremented.contains("3. c"), "{incremented}");
    Ok(())
}

#[test]
fn max_tag_opening_width_breaks_a_long_opening() -> Result<(), Box<dyn Error>> {
    let wide = "{% callout type=\"note\" title=\"A title long enough to matter\" id=\"x\" %}\nBody\n{% /callout %}\n";
    let output = run(&["fmt"], Some(wide))?;
    let default = text(output.stdout)?;
    assert_eq!(default.lines().count(), 3, "{default}");

    let output = run(&["fmt", "--max-tag-opening-width", "20"], Some(wide))?;
    let narrow = text(output.stdout)?;
    assert!(narrow.lines().count() > 3, "{narrow}");
    Ok(())
}
