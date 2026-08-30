//! The parsing cursor: position, backtracking, terminals, and the bookkeeping
//! that lets a failure explain itself.
//!
//! Recursive descent over a PEG is mechanical, and the productions in
//! `tag.rs` and `value.rs` read as such. Everything that is *not* mechanical
//! lives here:
//!
//! - **Backtracking is explicit.** A production that fails restores the
//!   position it started at. PEG ordered choice depends on this: the next
//!   alternative must see exactly what the failed one saw.
//! - **Expectations are recorded at the furthest position reached**, and a
//!   named rule records its own name and then silences everything inside it.
//!   This is peggy's `peg$expect` / `peg$silentFails` pair, and it is what
//!   makes the messages in `error.rs` come out identical to upstream's.
//! - **Value nesting is depth-limited.** See [`MAX_VALUE_DEPTH`].

use super::error::{build_message, Expectation, TagError};

/// The deepest a value may nest before the parser gives up.
///
/// Upstream has no limit and dies at whatever depth exhausts the JavaScript
/// stack, throwing a `RangeError` that its own tokenizer does not catch. A
/// recursive-descent parser in Rust would instead overflow the stack, which
/// aborts the process and cannot be caught at all -- so the crate's
/// panic-freedom promise is only true with a bound here. See `DIVERGENCES.md`.
///
/// The value is far above anything an author writes: an attribute nested 64
/// deep is not a document, it is an attack.
pub const MAX_VALUE_DEPTH: usize = 64;

/// A position in the tag body, plus everything needed to explain a failure at
/// it.
pub(crate) struct Cursor<'a> {
    input: &'a str,
    pos: usize,
    /// The furthest position any expectation has been recorded at. peggy calls
    /// this `peg$expected[0].pos`; the failure is reported here, not wherever
    /// the parse happened to stop.
    furthest: usize,
    variants: Vec<Expectation>,
    /// Nesting depth of named rules. Non-zero means expectations are dropped,
    /// because the enclosing named rule has already spoken for them.
    silent: u32,
    depth: usize,
    /// Where [`MAX_VALUE_DEPTH`] was first exceeded, if it was.
    depth_exhausted: Option<usize>,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor over a tag body.
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            furthest: 0,
            variants: Vec::new(),
            silent: 0,
            depth: 0,
            depth_exhausted: None,
        }
    }

    /// Returns the current byte offset. Always on a character boundary.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// Backtracks to a position a production saved earlier.
    pub(crate) fn reset(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Reports whether the whole input has been consumed.
    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Returns the unconsumed input.
    fn rest(&self) -> &'a str {
        self.input.get(self.pos..).unwrap_or_default()
    }

    /// Returns the next character without consuming it.
    pub(crate) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// Consumes the next character, if there is one.
    pub(crate) fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    /// Records that the parser would have accepted `expectation` here.
    ///
    /// An expectation further along the input replaces every earlier one: the
    /// parse that got furthest is the one worth reporting. An expectation
    /// inside a named rule is dropped, because the rule recorded its name on
    /// the way in.
    fn expect(&mut self, expectation: Expectation) {
        if self.silent > 0 || self.pos < self.furthest {
            return;
        }
        if self.pos > self.furthest {
            self.furthest = self.pos;
            self.variants.clear();
        }
        self.variants.push(expectation);
    }

    /// Records the end of the input as the only acceptable thing here.
    ///
    /// Called once, when a production matched but left input behind. Upstream
    /// does the same and for the same reason: the start rule must consume the
    /// whole body, so trailing text is a syntax error rather than a signal to
    /// try another alternative.
    pub(crate) fn expect_end_of_input(&mut self) {
        self.expect(Expectation::EndOfInput);
    }

    /// Enters a named rule: record its name, then silence its internals.
    pub(crate) fn enter_named(&mut self, name: &'static str) {
        self.expect(Expectation::Named(name));
        self.silent += 1;
    }

    /// Leaves a named rule.
    pub(crate) fn leave_named(&mut self) {
        self.silent = self.silent.saturating_sub(1);
    }

    /// Matches a literal, recording it as an expectation first.
    pub(crate) fn literal(&mut self, text: &'static str) -> bool {
        self.expect(Expectation::Literal(text));
        if self.rest().starts_with(text) {
            self.pos += text.len();
            true
        } else {
            false
        }
    }

    /// Matches upstream's `_` rule: one space, newline or tab.
    ///
    /// Carriage return is not whitespace to this grammar, which matters for
    /// documents with CRLF line endings: a tag body ending in `\r` does not
    /// parse. Upstream's tokenizer trims the body before parsing it, so the
    /// case does not arise there and must not be "fixed" here.
    pub(crate) fn whitespace(&mut self) -> bool {
        self.expect(Expectation::Named("whitespace"));
        match self.peek() {
            Some(' ' | '\n' | '\t') => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    /// Matches `_*`.
    pub(crate) fn whitespace_star(&mut self) {
        while self.whitespace() {}
    }

    /// Matches `_+`.
    pub(crate) fn whitespace_plus(&mut self) -> bool {
        if !self.whitespace() {
            return false;
        }
        self.whitespace_star();
        true
    }

    /// Matches upstream's `Identifier` rule: `[a-zA-Z0-9_-]+`.
    ///
    /// Digits and hyphens are ordinary identifier characters, and a leading one
    /// is legal, so `-` cannot be used as a lookahead-free discriminator
    /// between a number and an identifier. `1a` is one identifier and `-1` is
    /// another; whether either is a *value* is decided by `Value`'s
    /// alternation order, not here.
    pub(crate) fn identifier(&mut self) -> Option<&'a str> {
        self.enter_named("identifier");
        let start = self.pos;
        let end = start
            + self
                .rest()
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
                .count();
        self.leave_named();

        if end == start {
            return None;
        }
        self.pos = end;
        self.input.get(start..end)
    }

    /// Matches one ASCII digit.
    pub(crate) fn digit(&mut self) -> bool {
        if self.rest().starts_with(|c: char| c.is_ascii_digit()) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Returns the text between two positions this cursor produced.
    ///
    /// Upstream's `text()`, used by `ValueNumber` to hand the matched span to
    /// `parseFloat`.
    pub(crate) fn slice(&self, start: usize, end: usize) -> &'a str {
        self.input.get(start..end).unwrap_or_default()
    }

    /// Enters a nested value, or reports that the nesting limit is reached.
    ///
    /// Returns `false` once [`MAX_VALUE_DEPTH`] is exceeded, and remembers
    /// where, so the error says what actually happened instead of blaming the
    /// innermost bracket.
    pub(crate) fn enter_value(&mut self) -> bool {
        if self.depth >= MAX_VALUE_DEPTH {
            if self.depth_exhausted.is_none() {
                self.depth_exhausted = Some(self.pos);
            }
            return false;
        }
        self.depth += 1;
        true
    }

    /// Leaves a nested value.
    pub(crate) fn leave_value(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Turns the recorded expectations into the error upstream would raise.
    pub(crate) fn into_error(self) -> TagError {
        if let Some(offset) = self.depth_exhausted {
            let end = self
                .input
                .get(offset..)
                .and_then(|rest| rest.chars().next())
                .map_or(offset, |ch| offset + ch.len_utf8());
            return TagError::new(
                format!("Value nesting exceeds the maximum depth of {MAX_VALUE_DEPTH}."),
                offset,
                end,
            );
        }

        let found = self
            .input
            .get(self.furthest..)
            .and_then(|rest| rest.chars().next());
        let end = found.map_or(self.furthest, |ch| self.furthest + ch.len_utf8());
        TagError::new(build_message(&self.variants, found), self.furthest, end)
    }
}
