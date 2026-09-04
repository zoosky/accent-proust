//! The README's Rust blocks, compiled and run.
//!
//! A README full of assertions that nothing executes is a README that rots.
//! This example is the executable copy, and `tests/readme.rs` asserts that
//! every Rust block in README.md appears here verbatim -- so editing one
//! without the other fails CI rather than shipping a lie.
//!
//! Keep the blocks in document order, and copy them exactly.

// A README block carries its own `use` line, so a reader can paste one block
// and have it compile. Copied verbatim into one function, those lines land
// after statements. The lint is right about ordinary code and wrong here:
// moving the imports to the top would break the property this file exists for.
#![allow(clippy::items_after_statements)]

/// Runs every block in order, panicking if the README claims a wrong result.
fn main() {
    use accent_proust::{builtins, parse, render, transform};

    let document = parse::parse("# Title\n\nSome *text*.\n");
    let tree = transform::transform(&document, &builtins::config());

    assert_eq!(
        render::render_all(&tree.into_vec()),
        "<article><h1>Title</h1><p>Some <em>text</em>.</p></article>"
    );

    use accent_proust::validate::{self, Schema, SchemaAttribute, ValidationType};

    let mut config = builtins::config();
    config.tags_mut().insert(
        "callout".to_string(),
        Schema::new().render("div").attribute(
            "type",
            SchemaAttribute {
                attribute_type: Some(ValidationType::String),
                required: true,
                ..SchemaAttribute::default()
            },
        ),
    );

    let document = parse::parse("{% callout type=\"note\" %}\nBody\n{% /callout %}\n");
    assert!(validate::validate_tree(&document, &config).is_empty());

    let tree = transform::transform(&document, &config);
    assert_eq!(
        render::render_all(&tree.into_vec()),
        "<article><div type=\"note\"><p>Body</p></div></article>"
    );

    let document = parse::parse("{% callout %}\nBody\n{% /callout %}\n");

    for error in validate::validate_tree(&document, &config) {
        println!("{}: {}", error.error.id, error.error.message);
        // attribute-missing-required: Missing required attribute: 'type'
    }

    use accent_proust::format;

    let document = parse::parse("{% callout   type=\"note\"  %}\nBody\n{% /callout %}\n");
    assert_eq!(
        format::format(&document),
        "{% callout type=\"note\" %}\nBody\n{% /callout %}\n"
    );

    println!("every README assertion holds");
}
