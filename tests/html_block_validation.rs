//! A document with block-level HTML passes the built-in nesting rules.
//!
//! Before the parser gave an HTML block the paragraph shape markdown-it gives
//! it, the block reached the validator as a bare `text` child of `document`,
//! and every such document failed with "Can't nest 'text' in 'document'" at
//! line 1. A host that validates whole pages -- Accent's `accent validate`
//! over a documentation site, where every page carries HTML comments -- saw
//! hundreds of these.

use std::sync::Arc;

use accent_proust::parse::parse;
use accent_proust::validate::{Config, MapSchemaSource, nodes, validate_tree};

fn builtin_config() -> Config<'static> {
    let mut schemas = MapSchemaSource::new();
    schemas.nodes_mut().extend(nodes::builtin());
    Config::new().with_schemas(Arc::new(schemas))
}

#[test]
fn html_blocks_are_valid_children_of_the_document() {
    let source = "Intro.\n\n<!-- A comment -->\n\n<div>\nraw\n</div>\n\n```\n{% x %}\n```\n<!-- after a fence -->\n";
    let document = parse(source);
    let config = builtin_config();
    let ids: Vec<&str> = validate_tree(&document, &config)
        .into_iter()
        .map(|found| found.error.id)
        .collect();
    assert!(!ids.contains(&"child-invalid"), "{ids:?}");
}
