//! Keeps README.md and `examples/readme.rs` from drifting apart.
//!
//! The example is compiled and run by CI, so the README's code is known to
//! work -- but only the copy in the example. Nothing stops someone editing a
//! block in the README and leaving the example behind, which puts CI's green
//! tick behind code the reader never sees. This test closes that gap: every
//! Rust block in the README must appear verbatim in the example.
//!
//! It compares text, not behaviour. That is the point: behaviour is what
//! running the example already proves.

/// The README, as shipped.
const README: &str = include_str!("../README.md");

/// The executable copy of its Rust blocks.
const EXAMPLE: &str = include_str!("../examples/readme.rs");

/// Every fenced `rust` block in the README, in document order.
fn rust_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;

    for line in markdown.lines() {
        match (line.trim_end(), current.as_mut()) {
            ("```rust", None) => current = Some(Vec::new()),
            ("```", Some(_)) => {
                if let Some(lines) = current.take() {
                    blocks.push(lines.join("\n"));
                }
            }
            (_, Some(lines)) => lines.push(line),
            (_, None) => {}
        }
    }

    blocks
}

/// The example with one level of function-body indentation removed, so a block
/// written at the left margin in the README matches its indented copy here.
fn dedented(source: &str) -> String {
    source
        .lines()
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_readme_rust_block_appears_in_the_example() {
    let blocks = rust_blocks(README);
    assert!(
        !blocks.is_empty(),
        "found no ```rust blocks in README.md; the extractor is broken, \
         not the README"
    );

    let example = dedented(EXAMPLE);

    for (index, block) in blocks.iter().enumerate() {
        assert!(
            example.contains(block.as_str()),
            "README.md block {} is not in examples/readme.rs verbatim.\n\n\
             Copy it across, keeping document order, so the example still \
             proves what the README claims.\n\n--- block ---\n{}",
            index + 1,
            block
        );
    }
}
