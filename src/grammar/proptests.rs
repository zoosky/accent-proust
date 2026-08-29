//! Panic-freedom, asserted against generated input.

use proptest::prelude::*;

use super::parse_tag;

proptest! {
    #[test]
    fn never_panics_on_arbitrary_input(input in ".*") {
        let _ = parse_tag(&input);
    }
}
