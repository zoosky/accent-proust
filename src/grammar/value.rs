//! The `Value` productions: literals, arrays, hashes, function calls, and
//! variable references.
//!
//! Read this beside `reference/src/grammar/tag.pegjs`. The alternation order in
//! [`Cursor::value`] is the grammar's, and it is the whole disambiguation
//! strategy -- there is no lexer to consult and no lookahead beyond what
//! backtracking gives you:
//!
//! - `null`, `true` and `false` are matched before an identifier could swallow
//!   them.
//! - A number is matched before a function call or a variable, so `1a` fails
//!   as a trailing-input error rather than parsing as an identifier.
//! - A function call is tried before a variable, and both begin with an
//!   identifier-shaped token. A PEG backtracks between them; so does this.

use indexmap::IndexMap;

use super::cursor::Cursor;
use crate::ast::{Function, PathSegment, Value, Variable};

impl Cursor<'_> {
    /// `Value = ValueNull / ValueBoolean / ValueString / ValueNumber /
    /// ValueArray / ValueHash / Function / Variable`
    ///
    /// Every nested value goes through here, which is where the depth bound
    /// applies. Exceeding it fails the value rather than unwinding the stack;
    /// see `MAX_VALUE_DEPTH`.
    pub(crate) fn value(&mut self) -> Option<Value> {
        if !self.enter_value() {
            return None;
        }
        let value = self.value_alternatives();
        self.leave_value();
        value
    }

    fn value_alternatives(&mut self) -> Option<Value> {
        if let Some(value) = self.value_null() {
            return Some(value);
        }
        if let Some(value) = self.value_boolean() {
            return Some(value);
        }
        if let Some(value) = self.value_string() {
            return Some(value);
        }
        if let Some(value) = self.value_number() {
            return Some(value);
        }
        if let Some(value) = self.value_array() {
            return Some(value);
        }
        if let Some(value) = self.value_hash() {
            return Some(value);
        }
        if let Some(value) = self.function() {
            return Some(value);
        }
        self.variable()
    }

    /// `ValueNull 'null' = 'null'`
    ///
    /// A prefix match, as in the grammar: `nullish` parses as `null` followed
    /// by unconsumed input, and the trailing text is what fails the parse.
    fn value_null(&mut self) -> Option<Value> {
        self.enter_named("null");
        let matched = self.literal("null");
        self.leave_named();
        matched.then_some(Value::Null)
    }

    /// `ValueBoolean 'boolean' = 'true' / 'false'`
    fn value_boolean(&mut self) -> Option<Value> {
        self.enter_named("boolean");
        let value = if self.literal("true") {
            Some(Value::Boolean(true))
        } else if self.literal("false") {
            Some(Value::Boolean(false))
        } else {
            None
        };
        self.leave_named();
        value
    }

    /// `ValueString 'string' = '"' ValueStringChars* '"'`
    fn value_string(&mut self) -> Option<Value> {
        self.string_literal().map(Value::String)
    }

    /// The string rule, returning the unescaped text.
    ///
    /// Separate from [`Cursor::value_string`] because `ValueHashItem` uses a
    /// string as a *key*, where a [`Value`] wrapper would only have to be
    /// unwrapped again.
    fn string_literal(&mut self) -> Option<String> {
        let start = self.pos();
        self.enter_named("string");
        let text = self.string_literal_body();
        self.leave_named();
        if text.is_none() {
            self.reset(start);
        }
        text
    }

    fn string_literal_body(&mut self) -> Option<String> {
        if !self.literal("\"") {
            return None;
        }
        let mut text = String::new();
        while let Some(ch) = self.string_char() {
            text.push(ch);
        }
        if !self.literal("\"") {
            return None;
        }
        Some(text)
    }

    /// `ValueStringChars = [^\0-\x1F\x22\x5C] / ValueStringEscapes`
    ///
    /// The unescaped class excludes the C0 control characters, the quote and
    /// the backslash, and nothing else -- `\x7F` and every non-ASCII character
    /// are ordinary string content. The escape set is exactly `\"`, `\\`, `\n`,
    /// `\r` and `\t`; there is no `\u`, and a backslash before anything else is
    /// not an escape but a failed character, which ends the string body and
    /// then fails the closing quote.
    fn string_char(&mut self) -> Option<char> {
        match self.peek()? {
            '\\' => {
                let start = self.pos();
                self.advance();
                let unescaped = match self.peek() {
                    Some('"') => Some('"'),
                    Some('\\') => Some('\\'),
                    Some('n') => Some('\n'),
                    Some('r') => Some('\r'),
                    Some('t') => Some('\t'),
                    _ => None,
                };
                if let Some(ch) = unescaped {
                    self.advance();
                    return Some(ch);
                }
                self.reset(start);
                None
            }
            '"' => None,
            ch if (ch as u32) <= 0x1f => None,
            ch => {
                self.advance();
                Some(ch)
            }
        }
    }

    /// `ValueNumber 'number' = '-'? [0-9]+ ('.' [0-9]+)?`
    ///
    /// No exponent, no leading `+`, and no bare `.5`. A `.` not followed by a
    /// digit is left unconsumed, so `$a[1].b` still sees its `.`.
    fn value_number(&mut self) -> Option<Value> {
        let start = self.pos();
        self.enter_named("number");
        let matched = self.value_number_body();
        self.leave_named();
        if !matched {
            self.reset(start);
            return None;
        }
        // Upstream is `parseFloat(text())`. Rust's `f64` parser accepts the
        // same shape with the same rounding, and overflows to infinity the way
        // `parseFloat` does.
        let text = self.slice(start, self.pos());
        if let Ok(number) = text.parse::<f64>() {
            return Some(Value::Number(number));
        }
        self.reset(start);
        None
    }

    fn value_number_body(&mut self) -> bool {
        self.literal("-");
        if !self.digit() {
            return false;
        }
        while self.digit() {}

        let mark = self.pos();
        if self.literal(".") {
            if !self.digit() {
                self.reset(mark);
                return true;
            }
            while self.digit() {}
        }
        true
    }

    /// `ValueArray = '[' _* (Value ValueArrayTail* TrailingComma)? _* ']'`
    fn value_array(&mut self) -> Option<Value> {
        let start = self.pos();
        if !self.literal("[") {
            self.reset(start);
            return None;
        }
        self.whitespace_star();

        let mut items = Vec::new();
        let mark = self.pos();
        if let Some(head) = self.value() {
            items.push(head);
            loop {
                let tail = self.pos();
                self.whitespace_star();
                if !self.literal(",") {
                    self.reset(tail);
                    break;
                }
                self.whitespace_star();
                let Some(item) = self.value() else {
                    self.reset(tail);
                    break;
                };
                items.push(item);
            }
            self.trailing_comma();
        } else {
            self.reset(mark);
        }

        self.whitespace_star();
        if !self.literal("]") {
            self.reset(start);
            return None;
        }
        Some(Value::Array(items))
    }

    /// `ValueHash = '{' _* (ValueHashItem ValueHashTail* TrailingComma)? _* '}'`
    fn value_hash(&mut self) -> Option<Value> {
        let start = self.pos();
        if !self.literal("{") {
            self.reset(start);
            return None;
        }
        self.whitespace_star();

        let mut hash = IndexMap::new();
        let mark = self.pos();
        if let Some(head) = self.value_hash_item() {
            if let Some((key, value)) = head {
                hash.insert(key, value);
            }
            loop {
                let tail = self.pos();
                self.whitespace_star();
                if !self.literal(",") {
                    self.reset(tail);
                    break;
                }
                self.whitespace_star();
                let Some(item) = self.value_hash_item() else {
                    self.reset(tail);
                    break;
                };
                if let Some((key, value)) = item {
                    hash.insert(key, value);
                }
            }
            self.trailing_comma();
        } else {
            self.reset(mark);
        }

        self.whitespace_star();
        if !self.literal("}") {
            self.reset(start);
            return None;
        }
        Some(Value::Hash(hash))
    }

    /// `ValueHashItem = (Identifier / ValueString) ':' _* Value`
    ///
    /// Returns `Some(None)` for a `$$mdtype` key: upstream's action returns an
    /// empty object for that one key, so the entry parses and then vanishes.
    /// That is a guard, not an accident -- `$$mdtype` is how upstream tags its
    /// own AST classes at runtime, and letting authored content set it would
    /// let a document forge a `Variable` or a `Function` out of a hash literal.
    /// Dropping the guard as "dead JavaScript" would reintroduce the hole.
    ///
    /// Note the asymmetry the grammar insists on: no whitespace before the
    /// colon, any amount after it. `{a : 1}` is a syntax error.
    #[allow(
        clippy::option_option,
        reason = "outer None is a parse failure, \
        inner None is a `$$mdtype` entry that parsed and was discarded; \
        collapsing them would lose the difference between the two"
    )]
    fn value_hash_item(&mut self) -> Option<Option<(String, Value)>> {
        let start = self.pos();
        let key = if let Some(identifier) = self.identifier() {
            identifier.to_string()
        } else if let Some(key) = self.string_literal() {
            key
        } else {
            self.reset(start);
            return None;
        };
        if !self.literal(":") {
            self.reset(start);
            return None;
        }
        self.whitespace_star();
        let Some(value) = self.value() else {
            self.reset(start);
            return None;
        };

        if key == "$$mdtype" {
            return Some(None);
        }
        Some(Some((key, value)))
    }

    /// `TrailingComma = (_* ',')?`
    ///
    /// Permitted in arrays and hashes. Not in a function's parameter list,
    /// which has no such rule: `f(1,)` is a syntax error.
    fn trailing_comma(&mut self) {
        let start = self.pos();
        self.whitespace_star();
        if !self.literal(",") {
            self.reset(start);
        }
    }

    /// `Function = Identifier '(' _* (FunctionParameter? FunctionParameterTail*)? ')'`
    ///
    /// Two details of the shape are easy to lose and both are load-bearing:
    ///
    /// - There is `_*` after the opening parenthesis but none before the
    ///   closing one, so `f(1 )` does not parse while `f( 1)` does.
    /// - When the first parameter fails but the tail still matches, upstream's
    ///   action returns an empty list and keeps the input the tail consumed.
    ///   `f(,1)` is therefore a call with no parameters, not an error.
    pub(crate) fn function(&mut self) -> Option<Value> {
        let start = self.pos();
        let Some(name) = self.identifier() else {
            self.reset(start);
            return None;
        };
        if !self.literal("(") {
            self.reset(start);
            return None;
        }
        self.whitespace_star();

        let head = self.function_parameter();
        let mut tail = Vec::new();
        loop {
            let mark = self.pos();
            self.whitespace_star();
            if !self.literal(",") {
                self.reset(mark);
                break;
            }
            self.whitespace_star();
            let Some(parameter) = self.function_parameter() else {
                self.reset(mark);
                break;
            };
            tail.push(parameter);
        }

        if !self.literal(")") {
            self.reset(start);
            return None;
        }

        let mut parameters = IndexMap::new();
        if let Some(head) = head {
            for (index, (name, value)) in std::iter::once(head).chain(tail).enumerate() {
                let key = name.unwrap_or_else(|| Function::positional_key(index));
                parameters.insert(key, value);
            }
        }

        Some(Value::Function(Function::new(name.to_string(), parameters)))
    }

    /// `FunctionParameter = (Identifier '=')? Value`
    fn function_parameter(&mut self) -> Option<(Option<String>, Value)> {
        let start = self.pos();
        let name = {
            let mark = self.pos();
            match self.identifier() {
                Some(identifier) if self.literal("=") => Some(identifier.to_string()),
                _ => {
                    self.reset(mark);
                    None
                }
            }
        };
        let Some(value) = self.value() else {
            self.reset(start);
            return None;
        };
        Some((name, value))
    }

    /// `Variable 'variable' = [$@] Identifier VariableTail*`
    ///
    /// The prefix decides the shape of the result, and the two are not the same
    /// type: `$foo.bar` is a [`Value::Variable`], while `@foo.bar` is a plain
    /// [`Value::Array`] of its path steps. Upstream does exactly this -- the
    /// `@` branch returns a bare JavaScript array rather than constructing a
    /// `Variable` -- and both reach the same `Top` alternative.
    pub(crate) fn variable(&mut self) -> Option<Value> {
        let start = self.pos();
        self.enter_named("variable");
        let value = self.variable_body();
        self.leave_named();
        if value.is_none() {
            self.reset(start);
        }
        value
    }

    fn variable_body(&mut self) -> Option<Value> {
        let Some(prefix @ ('$' | '@')) = self.peek() else {
            return None;
        };
        self.advance();

        let head = self.identifier()?;
        let mut path = vec![PathSegment::Key(head.to_string())];
        while let Some(segment) = self.variable_tail() {
            path.push(segment);
        }

        if prefix == '@' {
            let steps = path
                .into_iter()
                .map(|segment| match segment {
                    PathSegment::Key(key) => Value::String(key),
                    PathSegment::Index(index) => Value::Number(index),
                })
                .collect();
            return Some(Value::Array(steps));
        }
        Some(Value::Variable(Variable::new(path)))
    }

    /// `VariableTail = '.' Identifier / '[' (ValueNumber / ValueString) ']'`
    ///
    /// An index step is a number or a string and nothing else, so `$foo[bar]`
    /// does not parse -- the tail fails, the variable ends at `$foo`, and the
    /// leftover `[bar]` fails the end-of-input check. The error you get names
    /// the leftover, which is upstream's behaviour too.
    fn variable_tail(&mut self) -> Option<PathSegment> {
        let start = self.pos();

        if self.literal(".") {
            if let Some(name) = self.identifier() {
                return Some(PathSegment::Key(name.to_string()));
            }
            self.reset(start);
            return None;
        }

        if !self.literal("[") {
            self.reset(start);
            return None;
        }
        let segment = if let Some(Value::Number(index)) = self.value_number() {
            PathSegment::Index(index)
        } else if let Some(key) = self.string_literal() {
            PathSegment::Key(key)
        } else {
            self.reset(start);
            return None;
        };
        if !self.literal("]") {
            self.reset(start);
            return None;
        }
        Some(segment)
    }
}
