//! The ported oracle: `reference/src/grammar/tag.test.ts`.

use super::*;

#[test]
fn a_simple_opening_tag() {
    assert_eq!(
        parse_tag("foo").expect("parses"),
        TagItem::TagOpen {
            name: "foo".to_string(),
            attributes: Vec::new(),
            self_closing: false,
        }
    );
}
