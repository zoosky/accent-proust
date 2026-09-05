//! Byte offsets, as the code-unit offsets JavaScript counts in.
//!
//! The engine measures a document in UTF-8 bytes, because that is what a `&str`
//! is. JavaScript measures a string in UTF-16 code units, and so does every
//! editor built on one: a CodeMirror position, a Monaco position and an LSP
//! `character` are all counts of UTF-16 code units, never bytes.
//!
//! The two agree exactly while a document stays in ASCII, which is why getting
//! this wrong survives every test written in English and then puts the
//! underline in the wrong place the first time an author writes `Grüße`. It is
//! converted here, once, rather than left for each host to rediscover.
//!
//! # Cost
//!
//! Building the index is one pass over the document, and it allocates only for
//! characters outside ASCII. An all-ASCII document -- the overwhelming majority
//! -- allocates nothing and every lookup is the identity.

/// A mapping from byte offset to UTF-16 code-unit offset for one document.
pub(crate) struct Utf16Index {
    /// `(byte, utf16)` immediately after each non-ASCII character.
    ///
    /// Between two consecutive checkpoints every character is ASCII, so one
    /// byte is one code unit there and a lookup is the nearest checkpoint plus
    /// the remaining byte distance. Empty for an ASCII document, which makes
    /// [`Utf16Index::at`] the identity without a special case.
    checkpoints: Vec<(usize, usize)>,
}

impl Utf16Index {
    /// Index a document.
    pub(crate) fn new(source: &str) -> Utf16Index {
        let mut checkpoints = Vec::new();
        // `utf16` trails `byte` by the number of code units the non-ASCII
        // characters so far have cost or saved: a 2- or 3-byte character is one
        // code unit, and a 4-byte character is a surrogate pair, so two.
        let mut utf16 = 0usize;
        for (byte, character) in source.char_indices() {
            let len = character.len_utf8();
            utf16 += character.len_utf16();
            if len != 1 {
                checkpoints.push((byte + len, utf16));
            }
        }
        Utf16Index { checkpoints }
    }

    /// The UTF-16 offset for a byte offset.
    ///
    /// A byte offset the engine produced is always a character boundary. One
    /// that is not lands on the checkpoint below it, which is the closest
    /// meaningful answer and is never out of range.
    pub(crate) fn at(&self, byte: usize) -> usize {
        // `partition_point` is a binary search for the first checkpoint after
        // `byte`; the one before it is the base to count ASCII from.
        let index = self.checkpoints.partition_point(|(at, _)| *at <= byte);
        match index.checked_sub(1).and_then(|i| self.checkpoints.get(i)) {
            Some((at, units)) => units + byte.saturating_sub(*at),
            // Before the first non-ASCII character, bytes and code units agree.
            None => byte,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Utf16Index;

    /// What JavaScript would say, for comparison.
    fn expected(source: &str, byte: usize) -> usize {
        source[..byte].encode_utf16().count()
    }

    #[test]
    fn ascii_is_the_identity() {
        let source = "hello world";
        let index = Utf16Index::new(source);
        assert!(
            index.checkpoints.is_empty(),
            "ASCII should allocate nothing"
        );
        for byte in 0..=source.len() {
            assert_eq!(index.at(byte), byte);
        }
    }

    #[test]
    fn agrees_with_javascript_on_every_boundary() {
        // Two bytes one unit, three bytes one unit, four bytes a surrogate
        // pair, and ASCII between them so the checkpoint arithmetic is
        // exercised rather than just the checkpoints.
        let source = "a ü b 日 c 🦀 d";
        let index = Utf16Index::new(source);
        for (byte, _) in source.char_indices().chain([(source.len(), ' ')]) {
            assert_eq!(
                index.at(byte),
                expected(source, byte),
                "byte {byte} of {source:?}"
            );
        }
    }

    #[test]
    fn a_surrogate_pair_costs_two_units() {
        let source = "🦀";
        let index = Utf16Index::new(source);
        assert_eq!(source.len(), 4);
        assert_eq!(index.at(4), 2);
    }

    #[test]
    fn an_offset_past_the_end_is_clamped_to_the_last_checkpoint() {
        let index = Utf16Index::new("ü");
        assert_eq!(index.at(2), 1);
        // Not reachable from engine output; it must still not panic.
        assert_eq!(index.at(99), 98);
    }
}
