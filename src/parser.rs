use indexmap::IndexMap;
use crate::error::ParseError;
use crate::lexer::{Lexer, Token};
use crate::value::Value;

// ---------------------------------------------------------------------------
// Task 6.1 – Internal data structures
// ---------------------------------------------------------------------------

/// Tracks whether a table entry was explicitly defined or implicitly created.
enum TrackedValue {
    /// Fully defined, immutable.
    Defined(Value),
    /// Created by a dotted key or implicit path – extensible until promoted.
    /// The bool indicates whether it has been opened as a standard table header
    /// (true = opened, cannot be re-opened).
    ImplicitTable(IndexMap<String, TrackedValue>, bool),
    /// [[header]] array of tables.
    ArrayOfTables(Vec<IndexMap<String, TrackedValue>>),
}

#[allow(dead_code)]
enum CurrentTableKind {
    Root,
    StandardTable,
    ArrayOfTablesElement,
}

struct TableTracker {
    root: IndexMap<String, TrackedValue>,
    current_path: Vec<String>,
    current_kind: CurrentTableKind,
}

// ---------------------------------------------------------------------------
// Helper: navigate to the map at `path` starting from `root`.
// Handles ArrayOfTables by going to the last element.
// ---------------------------------------------------------------------------
fn navigate_to<'m>(
    root: &'m mut IndexMap<String, TrackedValue>,
    path: &[String],
    line: u32,
    col: u32,
) -> Result<&'m mut IndexMap<String, TrackedValue>, ParseError> {
    let mut current = root;
    for seg in path {
        let entry = current.get_mut(seg.as_str()).ok_or_else(|| {
            ParseError::new(format!("internal: path segment '{}' not found", seg), line, col)
        })?;
        current = match entry {
            TrackedValue::ImplicitTable(m, _) => m,
            TrackedValue::ArrayOfTables(arr) => {
                arr.last_mut().ok_or_else(|| {
                    ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                })?
            }
            TrackedValue::Defined(_) => {
                return Err(ParseError::new(
                    format!("'{}' is already defined as a non-table value", seg),
                    line, col,
                ));
            }
        };
    }
    Ok(current)
}

impl TableTracker {
    fn new() -> Self {
        TableTracker {
            root: IndexMap::new(),
            current_path: Vec::new(),
            current_kind: CurrentTableKind::Root,
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.2 – Insert a key/value pair into the current table.
    // -----------------------------------------------------------------------
    fn insert_keyval(
        &mut self,
        segments: Vec<String>,
        value: Value,
        line: u32,
        col: u32,
    ) -> Result<(), ParseError> {
        let current = navigate_to(&mut self.root, &self.current_path, line, col)?;

        if segments.len() == 1 {
            let key = &segments[0];
            if current.contains_key(key.as_str()) {
                return Err(ParseError::new(
                    format!("duplicate key '{}'", key),
                    line, col,
                ));
            }
            current.insert(key.clone(), TrackedValue::Defined(value));
        } else {
            // Multi-segment dotted key: walk intermediate segments, creating
            // ImplicitTable entries as needed.
            let (last, intermediates) = segments.split_last().unwrap();
            let mut map = current;
            for seg in intermediates {
                if !map.contains_key(seg.as_str()) {
                    map.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new(), false));
                }
                let entry = map.get_mut(seg.as_str()).unwrap();
                map = match entry {
                    TrackedValue::ImplicitTable(m, _) => m,
                    TrackedValue::Defined(_) => {
                        return Err(ParseError::new(
                            format!("'{}' is already defined as a non-table value", seg),
                            line, col,
                        ));
                    }
                    TrackedValue::ArrayOfTables(arr) => {
                        arr.last_mut().ok_or_else(|| {
                            ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                        })?
                    }
                };
            }
            if map.contains_key(last.as_str()) {
                return Err(ParseError::new(
                    format!("duplicate key '{}'", last),
                    line, col,
                ));
            }
            map.insert(last.clone(), TrackedValue::Defined(value));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Task 6.3 – Process a [standard table] header.
    // -----------------------------------------------------------------------
    fn set_standard_table(
        &mut self,
        path: Vec<String>,
        line: u32,
        col: u32,
    ) -> Result<(), ParseError> {
        if path.is_empty() {
            return Err(ParseError::new("empty table header", line, col));
        }
        let (last, intermediates) = path.split_last().unwrap();

        // Walk intermediate segments from root.
        let mut current = &mut self.root;
        for seg in intermediates {
            if !current.contains_key(seg.as_str()) {
                current.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new(), false));
            }
            let entry = current.get_mut(seg.as_str()).unwrap();
            current = match entry {
                TrackedValue::ImplicitTable(m, _) => m,
                TrackedValue::ArrayOfTables(arr) => {
                    arr.last_mut().ok_or_else(|| {
                        ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                    })?
                }
                TrackedValue::Defined(_) => {
                    return Err(ParseError::new(
                        format!("'{}' is already defined as a non-table value", seg),
                        line, col,
                    ));
                }
            };
        }

        // Handle the final segment.
        match current.get_mut(last.as_str()) {
            None => {
                // Create a new standard table (opened = true).
                current.insert(last.clone(), TrackedValue::ImplicitTable(IndexMap::new(), true));
            }
            Some(TrackedValue::ImplicitTable(_, opened)) => {
                if *opened {
                    return Err(ParseError::new(
                        format!("table '{}' defined more than once", last),
                        line, col,
                    ));
                }
                // Promote implicit table to standard table.
                *opened = true;
            }
            Some(TrackedValue::Defined(_)) => {
                return Err(ParseError::new(
                    format!("table '{}' already defined", last),
                    line, col,
                ));
            }
            Some(TrackedValue::ArrayOfTables(_)) => {
                return Err(ParseError::new(
                    format!("'{}' is an array of tables, not a standard table", last),
                    line, col,
                ));
            }
        }

        self.current_path = path;
        self.current_kind = CurrentTableKind::StandardTable;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Task 6.4 – Process a [[array of tables]] header.
    // -----------------------------------------------------------------------
    fn append_aot(
        &mut self,
        path: Vec<String>,
        line: u32,
        col: u32,
    ) -> Result<(), ParseError> {
        if path.is_empty() {
            return Err(ParseError::new("empty array-of-tables header", line, col));
        }
        let (last, intermediates) = path.split_last().unwrap();

        // Walk intermediate segments from root.
        let mut current = &mut self.root;
        for seg in intermediates {
            if !current.contains_key(seg.as_str()) {
                current.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new(), false));
            }
            let entry = current.get_mut(seg.as_str()).unwrap();
            current = match entry {
                TrackedValue::ImplicitTable(m, _) => m,
                TrackedValue::ArrayOfTables(arr) => {
                    arr.last_mut().ok_or_else(|| {
                        ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                    })?
                }
                TrackedValue::Defined(_) => {
                    return Err(ParseError::new(
                        format!("'{}' is already defined as a non-table value", seg),
                        line, col,
                    ));
                }
            };
        }

        // Handle the final segment.
        match current.get_mut(last.as_str()) {
            None => {
                current.insert(
                    last.clone(),
                    TrackedValue::ArrayOfTables(vec![IndexMap::new()]),
                );
            }
            Some(TrackedValue::ArrayOfTables(arr)) => {
                arr.push(IndexMap::new());
            }
            Some(TrackedValue::Defined(_)) => {
                return Err(ParseError::new(
                    format!("'{}' is already defined; cannot use as array of tables", last),
                    line, col,
                ));
            }
            Some(TrackedValue::ImplicitTable(_, _)) => {
                return Err(ParseError::new(
                    format!("'{}' is already defined as a table; cannot use as array of tables", last),
                    line, col,
                ));
            }
        }

        self.current_path = path;
        self.current_kind = CurrentTableKind::ArrayOfTablesElement;
        Ok(())
    }

    /// Convert the root map into a `Value::Table`.
    fn into_value(self) -> Value {
        Value::Table(convert_map(self.root))
    }
}

/// Recursively convert `IndexMap<String, TrackedValue>` → `IndexMap<String, Value>`.
fn convert_map(map: IndexMap<String, TrackedValue>) -> IndexMap<String, Value> {
    let mut out = IndexMap::new();
    for (k, v) in map {
        out.insert(k, convert_tracked(v));
    }
    out
}

fn convert_tracked(tv: TrackedValue) -> Value {
    match tv {
        TrackedValue::Defined(v) => v,
        TrackedValue::ImplicitTable(m, _) => Value::Table(convert_map(m)),
        TrackedValue::ArrayOfTables(arr) => {
            Value::Array(arr.into_iter().map(|m| Value::Table(convert_map(m))).collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Task 6.5 – Parser struct and top-level parse_document loop
// ---------------------------------------------------------------------------

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    tracker: TableTracker,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Parser {
            lexer: Lexer::new(input),
            tracker: TableTracker::new(),
        }
    }

    // Task 6.12 – Wire parse to call parse_document and return Value::Table.
    pub fn parse(mut self) -> Result<Value, ParseError> {
        self.parse_document()?;
        Ok(self.tracker.into_value())
    }

    fn parse_document(&mut self) -> Result<(), ParseError> {
        loop {
            // Set key_mode before any peeking so that digit-only dotted keys
            // (e.g. `3.14159`) are not lexed as floats. Newlines are single
            // characters and are unaffected by key_mode.
            self.lexer.key_mode = true;

            // Skip newlines.
            while matches!(self.lexer.peek_token()?, Token::Newline) {
                self.lexer.next_token()?;
            }

            match self.lexer.peek_token()? {
                Token::Eof => {
                    self.lexer.key_mode = false;
                    break;
                }
                Token::LBracket => {
                    self.lexer.key_mode = false;
                    self.lexer.next_token()?; // consume LBracket
                    self.parse_table_header()?;
                }
                Token::DoubleLBracket => {
                    self.lexer.key_mode = false;
                    self.lexer.next_token()?; // consume DoubleLBracket
                    self.parse_aot_header()?;
                }
                _ => {
                    // key_mode remains true; parse_key will reset it after consuming the key.
                    self.parse_keyval()?;
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Task 6.6 – parse_key
    // -----------------------------------------------------------------------
    fn parse_key(&mut self) -> Result<Vec<String>, ParseError> {
        self.lexer.key_mode = true;
        let first = self.parse_simple_key();
        // key_mode stays true while we consume dot-separated segments
        let first = first?;
        let mut segments = vec![first];
        while matches!(self.lexer.peek_token()?, Token::Dot) {
            self.lexer.next_token()?; // consume Dot
            segments.push(self.parse_simple_key()?);
        }
        self.lexer.key_mode = false;
        Ok(segments)
    }

    fn parse_simple_key(&mut self) -> Result<String, ParseError> {
        let tok = self.lexer.next_token()?;
        match tok {
            Token::BareKey(s) => Ok(s.to_string()),
            Token::BasicString(s) => Ok(s),
            Token::LiteralString(s) => Ok(s.to_string()),
            // Digit-only bare key segments (e.g. `3` or `14159` in `3.14159 = "pi"`)
            // are lexed as Integer tokens when key_mode is active.
            Token::Integer(n) => Ok(n.to_string()),
            // Integer tokens that look like bare keys (e.g. digit-only dotted keys)
            // are not valid simple keys; fall through to error.
            other => {
                Err(ParseError::new(
                    format!("expected key, found {:?}", other),
                    1, 1,
                ))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.7 – parse_value
    // -----------------------------------------------------------------------
    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let tok = self.lexer.next_token()?;
        match tok {
            Token::BasicString(s) => Ok(Value::String(s)),
            Token::LiteralString(s) => Ok(Value::String(s.to_string())),
            Token::MlBasicString(s) => Ok(Value::String(s)),
            Token::MlLiteralString(s) => Ok(Value::String(s.to_string())),
            Token::Integer(n) => Ok(Value::Integer(n)),
            Token::Float(f) => Ok(Value::Float(f)),
            Token::Boolean(b) => Ok(Value::Boolean(b)),
            Token::OffsetDateTimeToken(dt) => Ok(Value::OffsetDateTime(dt)),
            Token::LocalDateTimeToken(dt) => Ok(Value::LocalDateTime(dt)),
            Token::LocalDateToken(d) => Ok(Value::LocalDate(d)),
            Token::LocalTimeToken(t) => Ok(Value::LocalTime(t)),
            Token::LBracket => self.parse_array(),
            // `[[` is lexed as DoubleLBracket; inside a value context the first `[`
            // starts a nested array, so push the second `[` back and parse normally.
            Token::DoubleLBracket => {
                self.lexer.push_back(Token::LBracket);
                self.parse_array()
            }
            Token::LBrace => self.parse_inline_table(),
            other => Err(ParseError::new(
                format!("expected value, found {:?}", other),
                1, 1,
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.8 – parse_array
    // -----------------------------------------------------------------------
    fn parse_array(&mut self) -> Result<Value, ParseError> {
        // LBracket already consumed by parse_value.
        let mut elements = Vec::new();
        loop {
            // Skip newlines and comments between elements.
            while matches!(self.lexer.peek_token()?, Token::Newline) {
                self.lexer.next_token()?;
            }
            // `]]` is lexed as DoubleRBracket; inside an array the first `]`
            // closes this array, so push the second `]` back.
            if matches!(self.lexer.peek_token()?, Token::DoubleRBracket) {
                self.lexer.next_token()?; // consume DoubleRBracket
                self.lexer.push_back(Token::RBracket);
                return Ok(Value::Array(elements));
            }
            if matches!(self.lexer.peek_token()?, Token::RBracket) {
                self.lexer.next_token()?; // consume ]
                return Ok(Value::Array(elements));
            }
            elements.push(self.parse_value()?);
            // Skip newlines after value.
            while matches!(self.lexer.peek_token()?, Token::Newline) {
                self.lexer.next_token()?;
            }
            // Peek to decide: comma, closing bracket, or error.
            // Clone the discriminant to avoid holding the borrow.
            let is_comma = matches!(self.lexer.peek_token()?, Token::Comma);
            let is_rbracket = matches!(self.lexer.peek_token()?, Token::RBracket);
            // `]]` at end of nested array: first `]` closes this array.
            let is_double_rbracket = matches!(self.lexer.peek_token()?, Token::DoubleRBracket);
            if is_comma {
                self.lexer.next_token()?; // consume comma
                // trailing comma is fine – loop will hit RBracket next
            } else if is_rbracket {
                self.lexer.next_token()?; // consume ]
                return Ok(Value::Array(elements));
            } else if is_double_rbracket {
                self.lexer.next_token()?; // consume ]]
                self.lexer.push_back(Token::RBracket); // put back second ]
                return Ok(Value::Array(elements));
            } else {
                return Err(ParseError::new(
                    "expected ',' or ']' in array",
                    1, 1,
                ));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.9 – parse_inline_table
    // -----------------------------------------------------------------------
    fn parse_inline_table(&mut self) -> Result<Value, ParseError> {
        // LBrace already consumed by parse_value.
        let mut pairs: IndexMap<String, TrackedValue> = IndexMap::new();

        if matches!(self.lexer.peek_token()?, Token::RBrace) {
            self.lexer.next_token()?; // consume }
            return Ok(Value::Table(IndexMap::new()));
        }

        loop {
            let key_segs = self.parse_key()?;
            self.expect_equals()?;
            let value = self.parse_value()?;

            // Insert into pairs, handling dotted keys.
            insert_into_inline(&mut pairs, key_segs, value, 1, 1)?;

            let is_comma = matches!(self.lexer.peek_token()?, Token::Comma);
            let is_rbrace = matches!(self.lexer.peek_token()?, Token::RBrace);
            let is_newline = matches!(self.lexer.peek_token()?, Token::Newline);

            if is_comma {
                self.lexer.next_token()?; // consume comma
                // Trailing comma check: next must not be }
                if matches!(self.lexer.peek_token()?, Token::RBrace) {
                    return Err(ParseError::new(
                        "trailing comma not allowed in inline table",
                        1, 1,
                    ));
                }
            } else if is_rbrace {
                self.lexer.next_token()?; // consume }
                return Ok(Value::Table(convert_map(pairs)));
            } else if is_newline {
                return Err(ParseError::new(
                    "newlines not allowed inside inline table",
                    1, 1,
                ));
            } else {
                return Err(ParseError::new(
                    "expected ',' or '}' in inline table",
                    1, 1,
                ));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.10 – parse_table_header / parse_aot_header
    // -----------------------------------------------------------------------
    fn parse_table_header(&mut self) -> Result<(), ParseError> {
        // LBracket already consumed by parse_document.
        let key = self.parse_key()?;
        self.expect_rbracket()?;
        self.require_newline_or_eof()?;
        self.tracker.set_standard_table(key, 1, 1)
    }

    fn parse_aot_header(&mut self) -> Result<(), ParseError> {
        // DoubleLBracket already consumed by parse_document.
        let key = self.parse_key()?;
        self.expect_double_rbracket()?;
        self.require_newline_or_eof()?;
        self.tracker.append_aot(key, 1, 1)
    }

    // -----------------------------------------------------------------------
    // Task 6.11 – parse_keyval
    // -----------------------------------------------------------------------
    fn parse_keyval(&mut self) -> Result<(), ParseError> {
        let key = self.parse_key()?;
        self.expect_equals()?;
        let value = self.parse_value()?;
        self.require_newline_or_eof()?;
        self.tracker.insert_keyval(key, value, 1, 1)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn expect_equals(&mut self) -> Result<(), ParseError> {
        match self.lexer.next_token()? {
            Token::Equals => Ok(()),
            other => Err(ParseError::new(format!("expected '=', found {:?}", other), 1, 1)),
        }
    }

    fn expect_rbracket(&mut self) -> Result<(), ParseError> {
        match self.lexer.next_token()? {
            Token::RBracket => Ok(()),
            other => Err(ParseError::new(format!("expected ']', found {:?}", other), 1, 1)),
        }
    }

    fn expect_double_rbracket(&mut self) -> Result<(), ParseError> {
        match self.lexer.next_token()? {
            Token::DoubleRBracket => Ok(()),
            other => Err(ParseError::new(format!("expected ']]', found {:?}", other), 1, 1)),
        }
    }

    fn require_newline_or_eof(&mut self) -> Result<(), ParseError> {
        match self.lexer.peek_token()? {
            Token::Newline => {
                self.lexer.next_token()?;
                Ok(())
            }
            Token::Eof => Ok(()),
            other => Err(ParseError::new(
                format!("expected newline or EOF after value, found {:?}", other),
                1, 1,
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: insert into an inline table's TrackedValue map (handles dotted keys).
// ---------------------------------------------------------------------------
fn insert_into_inline(
    map: &mut IndexMap<String, TrackedValue>,
    segments: Vec<String>,
    value: Value,
    line: u32,
    col: u32,
) -> Result<(), ParseError> {
    if segments.len() == 1 {
        let key = &segments[0];
        if map.contains_key(key.as_str()) {
            return Err(ParseError::new(
                format!("duplicate key '{}' in inline table", key),
                line, col,
            ));
        }
        map.insert(key.clone(), TrackedValue::Defined(value));
    } else {
        let (last, intermediates) = segments.split_last().unwrap();
        let mut current = map;
        for seg in intermediates {
            if !current.contains_key(seg.as_str()) {
                current.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new(), false));
            }
            let entry = current.get_mut(seg.as_str()).unwrap();
            current = match entry {
                TrackedValue::ImplicitTable(m, _) => m,
                TrackedValue::Defined(_) => {
                    return Err(ParseError::new(
                        format!("'{}' is already defined", seg),
                        line, col,
                    ));
                }
                TrackedValue::ArrayOfTables(_) => {
                    return Err(ParseError::new(
                        format!("'{}' is an array of tables", seg),
                        line, col,
                    ));
                }
            };
        }
        if current.contains_key(last.as_str()) {
            return Err(ParseError::new(
                format!("duplicate key '{}' in inline table", last),
                line, col,
            ));
        }
        current.insert(last.clone(), TrackedValue::Defined(value));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Task 8.1 – Unit tests for all string types
// ---------------------------------------------------------------------------
#[cfg(test)]
mod string_tests {
    use crate::{parse, Value};

    // Helper: parse `key = <value_toml>` and extract the string value.
    fn parse_str(value_toml: &str) -> Result<String, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::String(s) => Ok(s),
                other => panic!("expected String, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 4.1 – Basic string escape sequences
    // -----------------------------------------------------------------------

    #[test]
    fn basic_string_escape_backspace() {
        assert_eq!(parse_str(r#""\b""#).unwrap(), "\u{0008}");
    }

    #[test]
    fn basic_string_escape_tab() {
        assert_eq!(parse_str(r#""\t""#).unwrap(), "\t");
    }

    #[test]
    fn basic_string_escape_newline() {
        assert_eq!(parse_str(r#""\n""#).unwrap(), "\n");
    }

    #[test]
    fn basic_string_escape_formfeed() {
        assert_eq!(parse_str(r#""\f""#).unwrap(), "\u{000C}");
    }

    #[test]
    fn basic_string_escape_carriage_return() {
        assert_eq!(parse_str(r#""\r""#).unwrap(), "\r");
    }

    #[test]
    fn basic_string_escape_quote() {
        assert_eq!(parse_str(r#""\"""#).unwrap(), "\"");
    }

    #[test]
    fn basic_string_escape_backslash() {
        assert_eq!(parse_str(r#""\\""#).unwrap(), "\\");
    }

    #[test]
    fn basic_string_escape_unicode_4digit() {
        // \u0041 = 'A'
        assert_eq!(parse_str(r#""\u0041""#).unwrap(), "A");
    }

    #[test]
    fn basic_string_escape_unicode_8digit() {
        // \U0001F600 = 😀
        assert_eq!(parse_str(r#""\U0001F600""#).unwrap(), "😀");
    }

    #[test]
    fn basic_string_all_escapes_combined() {
        assert_eq!(
            parse_str(r#""\b\t\n\f\r\"\\""#).unwrap(),
            "\u{0008}\t\n\u{000C}\r\"\\"
        );
    }

    // -----------------------------------------------------------------------
    // 4.2 – Unrecognized escape rejection
    // -----------------------------------------------------------------------

    #[test]
    fn basic_string_unrecognized_escape_rejected() {
        assert!(parse_str(r#""\a""#).is_err());
    }

    #[test]
    fn basic_string_unrecognized_escape_x_rejected() {
        assert!(parse_str(r#""\x41""#).is_err());
    }

    // -----------------------------------------------------------------------
    // 4.3 – Invalid Unicode scalar value rejection
    // -----------------------------------------------------------------------

    #[test]
    fn basic_string_invalid_unicode_surrogate_rejected() {
        // U+D800 is a surrogate, not a valid scalar value
        assert!(parse_str(r#""\uD800""#).is_err());
    }

    #[test]
    fn basic_string_invalid_unicode_too_large_rejected() {
        // U+110000 is beyond the Unicode range
        assert!(parse_str(r#""\U00110000""#).is_err());
    }

    // -----------------------------------------------------------------------
    // 4.4 – Control character rejection in basic strings
    // -----------------------------------------------------------------------

    #[test]
    fn basic_string_control_char_nul_rejected() {
        // U+0000 (NUL) embedded directly
        let input = "x = \"\u{0000}\"";
        assert!(parse(input).is_err());
    }

    #[test]
    fn basic_string_control_char_0x01_rejected() {
        let input = "x = \"\u{0001}\"";
        assert!(parse(input).is_err());
    }

    #[test]
    fn basic_string_control_char_del_rejected() {
        // U+007F (DEL)
        let input = "x = \"\u{007F}\"";
        assert!(parse(input).is_err());
    }

    #[test]
    fn basic_string_tab_allowed() {
        // Tab (U+0009) is explicitly allowed unescaped
        assert_eq!(parse_str("\"\t\"").unwrap(), "\t");
    }

    // -----------------------------------------------------------------------
    // 4.8 – Literal strings: raw content, no escape processing
    // -----------------------------------------------------------------------

    #[test]
    fn literal_string_raw_content() {
        assert_eq!(parse_str("'hello world'").unwrap(), "hello world");
    }

    #[test]
    fn literal_string_backslash_not_escaped() {
        // Backslash is literal in a literal string
        assert_eq!(parse_str(r"'C:\Users\tom'").unwrap(), r"C:\Users\tom");
    }

    #[test]
    fn literal_string_double_quote_allowed() {
        assert_eq!(parse_str("'say \"hi\"'").unwrap(), "say \"hi\"");
    }

    // -----------------------------------------------------------------------
    // 4.9 – Control character rejection in literal strings
    // -----------------------------------------------------------------------

    #[test]
    fn literal_string_control_char_rejected() {
        // U+0001 embedded directly
        let input = "x = '\u{0001}'";
        assert!(parse(input).is_err());
    }

    #[test]
    fn literal_string_del_rejected() {
        let input = "x = '\u{007F}'";
        assert!(parse(input).is_err());
    }

    #[test]
    fn literal_string_tab_allowed() {
        // Tab is allowed in literal strings
        assert_eq!(parse_str("'\t'").unwrap(), "\t");
    }

    // -----------------------------------------------------------------------
    // 4.5 – Multi-line basic string: leading newline trim
    // -----------------------------------------------------------------------

    #[test]
    fn ml_basic_string_leading_newline_trimmed() {
        // The newline immediately after """ is trimmed (covered by _direct test below)
        // This test verifies the same via the helper which wraps in key=value
        // Note: the helper adds "x = " prefix, so we pass the raw ml string
        let input = "x = \"\"\"\nhello\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_leading_newline_trimmed_direct() {
        let input = "x = \"\"\"\nhello\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_no_leading_newline_not_trimmed() {
        // If no leading newline, content starts immediately
        let input = "x = \"\"\"hello\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_crlf_leading_newline_trimmed() {
        // CRLF immediately after """ is also trimmed
        let input = "x = \"\"\"\r\nhello\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // 4.6 – Multi-line basic string: line-ending backslash
    // -----------------------------------------------------------------------

    #[test]
    fn ml_basic_string_line_ending_backslash() {
        // Backslash at end of line trims backslash + whitespace + newline
        let input = "x = \"\"\"\\\n    hello\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_line_ending_backslash_multiple_lines() {
        let input = "x = \"\"\"\\\n  \n  hello\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // 4.7 – Multi-line basic string: up to two unescaped quotes before closing
    // -----------------------------------------------------------------------

    #[test]
    fn ml_basic_string_one_quote_before_closing() {
        // Content ends with one quote before the closing """
        let input = "x = \"\"\"hello\"\"\"\"";
        // That's: content = hello", closing = """
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello\"".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_two_quotes_before_closing() {
        // Content ends with two quotes before the closing """
        // TOML: x = """hello"""""  (3 opening + hello + 2 extra + 3 closing = 5 quotes at end)
        let input = "x = \"\"\"hello\"\"\"\"\"\"";
        // That's: x = """ hello "" """  but we need exactly 5 quotes at end
        // Use a raw string to be unambiguous: """hello"""""
        let input2 = r#"x = """hello""""" "#;
        // Trim trailing space
        let input2 = input2.trim_end();
        let doc = parse(input2).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello\"\"".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_one_quote_inside() {
        // One unescaped quote in the middle of a multi-line basic string
        let input2 = "x = \"\"\"say \"hi\" there\"\"\"";
        let doc = parse(input2).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("say \"hi\" there".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_basic_string_escape_sequences_work() {
        let input = "x = \"\"\"\n\\t\\n\\\\\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("\t\n\\".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // 4.10 – Multi-line literal string: leading newline trim
    // -----------------------------------------------------------------------

    #[test]
    fn ml_literal_string_leading_newline_trimmed() {
        let input = "x = '''\nhello'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_literal_string_no_leading_newline_not_trimmed() {
        let input = "x = '''hello'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_literal_string_crlf_leading_newline_trimmed() {
        let input = "x = '''\r\nhello'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // 4.10 – Multi-line literal string: no escape processing
    // -----------------------------------------------------------------------

    #[test]
    fn ml_literal_string_no_escape_processing() {
        let input = "x = '''\n\\n\\t\\\\'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                // Backslashes are literal
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("\\n\\t\\\\".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // 4.11 – Multi-line literal string: up to two unescaped single quotes before closing
    // -----------------------------------------------------------------------

    #[test]
    fn ml_literal_string_one_quote_before_closing() {
        // Content ends with one single quote before the closing '''
        let input = "x = '''hello''''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello'".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_literal_string_two_quotes_before_closing() {
        // Content ends with two single quotes before the closing '''
        let input = "x = '''hello'''''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("hello''".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_literal_string_single_quote_inside() {
        let input = "x = '''it's a test'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("it's a test".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // 4.12 – Multi-line literal string: control character rejection
    // -----------------------------------------------------------------------

    #[test]
    fn ml_literal_string_control_char_rejected() {
        // U+0001 is not allowed (not tab, LF, or CR)
        let input = "x = '''\n\u{0001}'''";
        assert!(parse(input).is_err());
    }

    #[test]
    fn ml_literal_string_del_rejected() {
        let input = "x = '''\n\u{007F}'''";
        assert!(parse(input).is_err());
    }

    #[test]
    fn ml_literal_string_tab_allowed() {
        let input = "x = '''\n\t'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("\t".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_literal_string_lf_allowed() {
        // LF is allowed inside multi-line literal strings
        let input = "x = '''\nline1\nline2'''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("line1\nline2".to_string()));
            }
            _ => panic!(),
        }
    }

    // -----------------------------------------------------------------------
    // Additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn basic_string_empty() {
        assert_eq!(parse_str(r#""""#).unwrap(), "");
    }

    #[test]
    fn literal_string_empty() {
        assert_eq!(parse_str("''").unwrap(), "");
    }

    #[test]
    fn ml_basic_string_empty() {
        let input = "x = \"\"\"\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ml_literal_string_empty() {
        let input = "x = ''''''";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn basic_string_unicode_content() {
        // Direct Unicode content (non-ASCII) is allowed
        assert_eq!(parse_str("\"héllo\"").unwrap(), "héllo");
    }

    #[test]
    fn ml_basic_string_multiline_content() {
        let input = "x = \"\"\"\nline1\nline2\n\"\"\"";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut m) => {
                assert_eq!(m.shift_remove("x").unwrap(), Value::String("line1\nline2\n".to_string()));
            }
            _ => panic!(),
        }
    }
}

// ---------------------------------------------------------------------------
// Task 8.2 – Unit tests for integer parsing
// ---------------------------------------------------------------------------
#[cfg(test)]
mod integer_tests {
    use crate::{parse, Value};

    /// Helper: parse `x = <integer_toml>` and return the i64 value.
    fn parse_int(value_toml: &str) -> Result<i64, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::Integer(n) => Ok(n),
                other => panic!("expected Integer, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 5.1 – Decimal integers with optional sign
    // -----------------------------------------------------------------------

    #[test]
    fn decimal_positive() {
        assert_eq!(parse_int("42").unwrap(), 42);
    }

    #[test]
    fn decimal_negative() {
        assert_eq!(parse_int("-42").unwrap(), -42);
    }

    #[test]
    fn decimal_zero() {
        assert_eq!(parse_int("0").unwrap(), 0);
    }

    #[test]
    fn decimal_plus_zero() {
        assert_eq!(parse_int("+0").unwrap(), 0);
    }

    #[test]
    fn decimal_minus_zero() {
        assert_eq!(parse_int("-0").unwrap(), 0);
    }

    #[test]
    fn decimal_explicit_plus_sign() {
        assert_eq!(parse_int("+99").unwrap(), 99);
    }

    #[test]
    fn decimal_large_positive() {
        assert_eq!(parse_int("1000000").unwrap(), 1_000_000);
    }

    // -----------------------------------------------------------------------
    // 5.2 – Leading zero rejection
    // -----------------------------------------------------------------------

    #[test]
    fn decimal_leading_zero_rejected() {
        assert!(parse_int("01").is_err());
    }

    #[test]
    fn decimal_leading_zero_multi_rejected() {
        assert!(parse_int("007").is_err());
    }

    #[test]
    fn decimal_leading_zero_negative_rejected() {
        assert!(parse_int("-01").is_err());
    }

    // -----------------------------------------------------------------------
    // 5.3 – Hexadecimal integers (0x prefix, case-insensitive)
    // -----------------------------------------------------------------------

    #[test]
    fn hex_lowercase() {
        assert_eq!(parse_int("0xdeadbeef").unwrap(), 0xdeadbeef_i64);
    }

    #[test]
    fn hex_uppercase() {
        assert_eq!(parse_int("0xDEADBEEF").unwrap(), 0xDEADBEEF_i64);
    }

    #[test]
    fn hex_mixed_case() {
        assert_eq!(parse_int("0xDeAdBeEf").unwrap(), 0xDeAdBeEf_i64);
    }

    #[test]
    fn hex_zero() {
        assert_eq!(parse_int("0x0").unwrap(), 0);
    }

    #[test]
    fn hex_simple() {
        assert_eq!(parse_int("0xff").unwrap(), 255);
    }

    // -----------------------------------------------------------------------
    // 5.4 – Octal integers (0o prefix)
    // -----------------------------------------------------------------------

    #[test]
    fn octal_simple() {
        assert_eq!(parse_int("0o17").unwrap(), 15);
    }

    #[test]
    fn octal_zero() {
        assert_eq!(parse_int("0o0").unwrap(), 0);
    }

    #[test]
    fn octal_larger() {
        assert_eq!(parse_int("0o755").unwrap(), 0o755_i64);
    }

    // -----------------------------------------------------------------------
    // 5.5 – Binary integers (0b prefix)
    // -----------------------------------------------------------------------

    #[test]
    fn binary_simple() {
        assert_eq!(parse_int("0b1010").unwrap(), 10);
    }

    #[test]
    fn binary_zero() {
        assert_eq!(parse_int("0b0").unwrap(), 0);
    }

    #[test]
    fn binary_all_ones() {
        assert_eq!(parse_int("0b1111").unwrap(), 15);
    }

    // -----------------------------------------------------------------------
    // 5.6 – + prefix rejected on non-decimal integers
    // -----------------------------------------------------------------------

    #[test]
    fn hex_plus_prefix_rejected() {
        assert!(parse_int("+0xff").is_err());
    }

    #[test]
    fn octal_plus_prefix_rejected() {
        assert!(parse_int("+0o7").is_err());
    }

    #[test]
    fn binary_plus_prefix_rejected() {
        assert!(parse_int("+0b1").is_err());
    }

    // -----------------------------------------------------------------------
    // 5.7 – Leading zeros allowed in non-decimal digit portion (after prefix)
    // -----------------------------------------------------------------------

    #[test]
    fn hex_leading_zeros_in_digits_allowed() {
        assert_eq!(parse_int("0x00ff").unwrap(), 255);
    }

    #[test]
    fn octal_leading_zeros_in_digits_allowed() {
        assert_eq!(parse_int("0o007").unwrap(), 7);
    }

    #[test]
    fn binary_leading_zeros_in_digits_allowed() {
        assert_eq!(parse_int("0b0001").unwrap(), 1);
    }

    // -----------------------------------------------------------------------
    // 5.8 – Underscores as visual separators
    // -----------------------------------------------------------------------

    #[test]
    fn decimal_underscore_separator() {
        assert_eq!(parse_int("1_000_000").unwrap(), 1_000_000);
    }

    #[test]
    fn decimal_underscore_single() {
        assert_eq!(parse_int("1_0").unwrap(), 10);
    }

    #[test]
    fn hex_underscore_separator() {
        assert_eq!(parse_int("0xdead_beef").unwrap(), 0xdeadbeef_i64);
    }

    #[test]
    fn octal_underscore_separator() {
        assert_eq!(parse_int("0o7_5_5").unwrap(), 0o755_i64);
    }

    #[test]
    fn binary_underscore_separator() {
        assert_eq!(parse_int("0b1010_0101").unwrap(), 0b10100101_i64);
    }

    // -----------------------------------------------------------------------
    // 5.9 – Invalid underscore placement
    // -----------------------------------------------------------------------

    #[test]
    fn decimal_underscore_at_start_rejected() {
        assert!(parse_int("_1000").is_err());
    }

    #[test]
    fn decimal_underscore_at_end_rejected() {
        assert!(parse_int("1000_").is_err());
    }

    #[test]
    fn decimal_double_underscore_rejected() {
        assert!(parse_int("1__000").is_err());
    }

    #[test]
    fn hex_underscore_adjacent_to_prefix_rejected() {
        // underscore immediately after 0x prefix
        assert!(parse_int("0x_ff").is_err());
    }

    #[test]
    fn hex_underscore_at_end_rejected() {
        assert!(parse_int("0xff_").is_err());
    }

    #[test]
    fn hex_double_underscore_rejected() {
        assert!(parse_int("0xff__00").is_err());
    }

    #[test]
    fn octal_underscore_adjacent_to_prefix_rejected() {
        assert!(parse_int("0o_7").is_err());
    }

    #[test]
    fn binary_underscore_adjacent_to_prefix_rejected() {
        assert!(parse_int("0b_1").is_err());
    }

    // -----------------------------------------------------------------------
    // 5.10 / 5.11 – i64 range: overflow and boundary values
    // -----------------------------------------------------------------------

    #[test]
    fn i64_max_parses_successfully() {
        // i64::MAX = 9223372036854775807
        assert_eq!(parse_int("9223372036854775807").unwrap(), i64::MAX);
    }

    #[test]
    fn i64_min_parses_successfully() {
        // i64::MIN = -9223372036854775808
        assert_eq!(parse_int("-9223372036854775808").unwrap(), i64::MIN);
    }

    #[test]
    fn overflow_positive_rejected() {
        // i64::MAX + 1
        assert!(parse_int("9223372036854775808").is_err());
    }

    #[test]
    fn overflow_negative_rejected() {
        // i64::MIN - 1
        assert!(parse_int("-9223372036854775809").is_err());
    }

    #[test]
    fn overflow_very_large_rejected() {
        assert!(parse_int("99999999999999999999").is_err());
    }
}

// ---------------------------------------------------------------------------
// Task 8.3 – Unit tests for float parsing
// ---------------------------------------------------------------------------
#[cfg(test)]
mod float_tests {
    use crate::{parse, Value};

    /// Helper: parse `x = <float_toml>` and extract the f64 value.
    fn parse_float(value_toml: &str) -> Result<f64, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::Float(f) => Ok(f),
                other => panic!("expected Float, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 6.1 / 6.7 – Fractional floats
    // -----------------------------------------------------------------------

    #[test]
    fn fractional_one_point_zero() {
        assert_eq!(parse_float("1.0").unwrap(), 1.0_f64);
    }

    #[test]
    fn fractional_negative_one_point_five() {
        assert_eq!(parse_float("-1.5").unwrap(), -1.5_f64);
    }

    #[test]
    fn fractional_pi() {
        assert_eq!(parse_float("3.14").unwrap(), 3.14_f64);
    }

    // -----------------------------------------------------------------------
    // 6.1 / 6.7 – Exponent floats
    // -----------------------------------------------------------------------

    #[test]
    fn exponent_lowercase_e() {
        assert_eq!(parse_float("1e10").unwrap(), 1e10_f64);
    }

    #[test]
    fn exponent_uppercase_e() {
        assert_eq!(parse_float("1E10").unwrap(), 1e10_f64);
    }

    #[test]
    fn exponent_positive_sign() {
        assert_eq!(parse_float("1e+10").unwrap(), 1e10_f64);
    }

    #[test]
    fn exponent_negative_sign() {
        assert_eq!(parse_float("1e-10").unwrap(), 1e-10_f64);
    }

    // -----------------------------------------------------------------------
    // 6.3 – Combined fractional + exponent
    // -----------------------------------------------------------------------

    #[test]
    fn combined_fractional_and_exponent() {
        let f = parse_float("6.626e-34").unwrap();
        assert!((f - 6.626e-34_f64).abs() < 1e-40);
    }

    // -----------------------------------------------------------------------
    // 6.4 – Underscores between digits
    // -----------------------------------------------------------------------

    #[test]
    fn underscores_in_float() {
        let f = parse_float("9_224_617.445_991_228_313").unwrap();
        assert_eq!(f, 9_224_617.445_991_228_313_f64);
    }

    // -----------------------------------------------------------------------
    // 6.5 – Special values: inf, +inf, -inf, nan, +nan, -nan
    // -----------------------------------------------------------------------

    #[test]
    fn special_inf() {
        let f = parse_float("inf").unwrap();
        assert!(f.is_infinite() && f.is_sign_positive());
    }

    #[test]
    fn special_plus_inf() {
        let f = parse_float("+inf").unwrap();
        assert!(f.is_infinite() && f.is_sign_positive());
    }

    #[test]
    fn special_minus_inf() {
        let f = parse_float("-inf").unwrap();
        assert!(f.is_infinite() && f.is_sign_negative());
    }

    #[test]
    fn special_nan() {
        assert!(parse_float("nan").unwrap().is_nan());
    }

    #[test]
    fn special_plus_nan() {
        assert!(parse_float("+nan").unwrap().is_nan());
    }

    #[test]
    fn special_minus_nan() {
        assert!(parse_float("-nan").unwrap().is_nan());
    }

    // -----------------------------------------------------------------------
    // 6.6 – -0.0 and +0.0 (IEEE 754 representations)
    // -----------------------------------------------------------------------

    #[test]
    fn negative_zero() {
        let f = parse_float("-0.0").unwrap();
        assert_eq!(f, 0.0_f64);
        assert!(f.is_sign_negative());
    }

    #[test]
    fn positive_zero() {
        let f = parse_float("+0.0").unwrap();
        assert_eq!(f, 0.0_f64);
        assert!(f.is_sign_positive());
    }

    // -----------------------------------------------------------------------
    // 6.2 – Rejection: decimal point with no digit on one side
    // -----------------------------------------------------------------------

    #[test]
    fn reject_leading_decimal_point() {
        // ".5" – no digit before the decimal point
        assert!(parse_float(".5").is_err());
    }

    #[test]
    fn reject_trailing_decimal_point() {
        // "1." – no digit after the decimal point
        assert!(parse_float("1.").is_err());
    }

    // -----------------------------------------------------------------------
    // 6.1 – Rejection: exponent with no digits
    // -----------------------------------------------------------------------

    #[test]
    fn reject_exponent_no_digits() {
        assert!(parse_float("1e").is_err());
    }

    #[test]
    fn reject_exponent_sign_no_digits() {
        assert!(parse_float("1e+").is_err());
    }
}

// ---------------------------------------------------------------------------
// Task 8.4 – Unit tests for boolean parsing
// ---------------------------------------------------------------------------
#[cfg(test)]
mod boolean_tests {
    use crate::parse;
    use crate::Value;

    fn parse_bool(value_toml: &str) -> Result<bool, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::Boolean(b) => Ok(b),
                other => panic!("expected Boolean, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 7.1 – `true` parses to Value::Boolean(true)
    // -----------------------------------------------------------------------

    #[test]
    fn true_parses_to_boolean_true() {
        assert_eq!(parse_bool("true").unwrap(), true);
    }

    // -----------------------------------------------------------------------
    // 7.2 – `false` parses to Value::Boolean(false)
    // -----------------------------------------------------------------------

    #[test]
    fn false_parses_to_boolean_false() {
        assert_eq!(parse_bool("false").unwrap(), false);
    }

    // -----------------------------------------------------------------------
    // 7.3 – Case-sensitivity: mixed/upper-case variants are rejected
    // -----------------------------------------------------------------------

    #[test]
    fn true_capitalized_rejected() {
        assert!(parse_bool("True").is_err());
    }

    #[test]
    fn true_all_caps_rejected() {
        assert!(parse_bool("TRUE").is_err());
    }

    #[test]
    fn false_capitalized_rejected() {
        assert!(parse_bool("False").is_err());
    }

    #[test]
    fn false_all_caps_rejected() {
        assert!(parse_bool("FALSE").is_err());
    }

    #[test]
    fn true_mixed_case_rejected() {
        assert!(parse_bool("tRuE").is_err());
    }
}

// ---------------------------------------------------------------------------
// Task 8.5 – Unit tests for all four date/time types
// ---------------------------------------------------------------------------
#[cfg(test)]
mod datetime_tests {
    use crate::parse;
    use crate::value::{LocalDate, LocalDateTime, LocalTime, OffsetDateTime, UtcOffset};
    use crate::Value;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn parse_offset_dt(value_toml: &str) -> Result<OffsetDateTime, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::OffsetDateTime(dt) => Ok(dt),
                other => panic!("expected OffsetDateTime, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    fn parse_local_dt(value_toml: &str) -> Result<LocalDateTime, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::LocalDateTime(dt) => Ok(dt),
                other => panic!("expected LocalDateTime, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    fn parse_local_date(value_toml: &str) -> Result<LocalDate, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::LocalDate(d) => Ok(d),
                other => panic!("expected LocalDate, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    fn parse_local_time(value_toml: &str) -> Result<LocalTime, crate::ParseError> {
        let input = format!("x = {}", value_toml);
        let doc = parse(&input)?;
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::LocalTime(t) => Ok(t),
                other => panic!("expected LocalTime, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 8.1 – OffsetDateTime: Z offset
    // -----------------------------------------------------------------------

    #[test]
    fn offset_datetime_z_offset() {
        let dt = parse_offset_dt("1979-05-27T07:32:00Z").unwrap();
        assert_eq!(dt.date, LocalDate { year: 1979, month: 5, day: 27 });
        assert_eq!(dt.time, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 });
        assert_eq!(dt.offset, UtcOffset::Z);
    }

    #[test]
    fn offset_datetime_z_offset_lowercase() {
        // lowercase 'z' should also be accepted
        let dt = parse_offset_dt("1979-05-27T07:32:00z").unwrap();
        assert_eq!(dt.offset, UtcOffset::Z);
    }

    // -----------------------------------------------------------------------
    // 8.1 – OffsetDateTime: +HH:MM offset
    // -----------------------------------------------------------------------

    #[test]
    fn offset_datetime_positive_offset() {
        let dt = parse_offset_dt("1979-05-27T07:32:00+05:30").unwrap();
        assert_eq!(dt.date, LocalDate { year: 1979, month: 5, day: 27 });
        assert_eq!(dt.time, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 });
        // +05:30 = 5*60 + 30 = 330 minutes
        assert_eq!(dt.offset, UtcOffset::Minutes(330));
    }

    // -----------------------------------------------------------------------
    // 8.1 – OffsetDateTime: -HH:MM offset
    // -----------------------------------------------------------------------

    #[test]
    fn offset_datetime_negative_offset() {
        let dt = parse_offset_dt("1979-05-27T07:32:00-08:00").unwrap();
        // -08:00 = -(8*60) = -480 minutes
        assert_eq!(dt.offset, UtcOffset::Minutes(-480));
    }

    // -----------------------------------------------------------------------
    // 8.1 – OffsetDateTime: space separator instead of T
    // -----------------------------------------------------------------------

    #[test]
    fn offset_datetime_space_separator() {
        let dt = parse_offset_dt("1979-05-27 07:32:00Z").unwrap();
        assert_eq!(dt.date, LocalDate { year: 1979, month: 5, day: 27 });
        assert_eq!(dt.time, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 });
        assert_eq!(dt.offset, UtcOffset::Z);
    }

    // -----------------------------------------------------------------------
    // 8.2 – LocalDateTime: valid RFC 3339 without offset
    // -----------------------------------------------------------------------

    #[test]
    fn local_datetime_t_separator() {
        let dt = parse_local_dt("1979-05-27T07:32:00").unwrap();
        assert_eq!(dt.date, LocalDate { year: 1979, month: 5, day: 27 });
        assert_eq!(dt.time, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 });
    }

    // -----------------------------------------------------------------------
    // 8.2 – LocalDateTime: space separator
    // -----------------------------------------------------------------------

    #[test]
    fn local_datetime_space_separator() {
        let dt = parse_local_dt("1979-05-27 07:32:00").unwrap();
        assert_eq!(dt.date, LocalDate { year: 1979, month: 5, day: 27 });
        assert_eq!(dt.time, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 });
    }

    // -----------------------------------------------------------------------
    // 8.3 – LocalDate: valid YYYY-MM-DD
    // -----------------------------------------------------------------------

    #[test]
    fn local_date_valid() {
        let d = parse_local_date("1979-05-27").unwrap();
        assert_eq!(d, LocalDate { year: 1979, month: 5, day: 27 });
    }

    #[test]
    fn local_date_first_of_year() {
        let d = parse_local_date("2024-01-01").unwrap();
        assert_eq!(d, LocalDate { year: 2024, month: 1, day: 1 });
    }

    // -----------------------------------------------------------------------
    // 8.4 – LocalTime: valid HH:MM:SS
    // -----------------------------------------------------------------------

    #[test]
    fn local_time_no_fractional() {
        let t = parse_local_time("07:32:00").unwrap();
        assert_eq!(t, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 });
    }

    // -----------------------------------------------------------------------
    // 8.4 – LocalTime: with fractional seconds
    // -----------------------------------------------------------------------

    #[test]
    fn local_time_with_fractional() {
        let t = parse_local_time("07:32:00.999").unwrap();
        assert_eq!(t, LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 999_000_000 });
    }

    // -----------------------------------------------------------------------
    // 8.5 – Fractional seconds: 3 digits (ms), 6 digits (µs), 9 digits (ns)
    // -----------------------------------------------------------------------

    #[test]
    fn fractional_seconds_3_digits_ms() {
        let t = parse_local_time("00:00:00.123").unwrap();
        assert_eq!(t.nanosecond, 123_000_000);
    }

    #[test]
    fn fractional_seconds_6_digits_us() {
        let t = parse_local_time("00:00:00.123456").unwrap();
        assert_eq!(t.nanosecond, 123_456_000);
    }

    #[test]
    fn fractional_seconds_9_digits_ns() {
        let t = parse_local_time("00:00:00.123456789").unwrap();
        assert_eq!(t.nanosecond, 123_456_789);
    }

    #[test]
    fn fractional_seconds_9_digits_max() {
        let t = parse_local_time("07:32:00.999999999").unwrap();
        assert_eq!(t.nanosecond, 999_999_999);
    }

    // -----------------------------------------------------------------------
    // 8.6 – Truncation: more than 9 fractional digits truncated (not rounded)
    // -----------------------------------------------------------------------

    #[test]
    fn fractional_seconds_truncated_not_rounded() {
        // 10 digits: .1234567899 → truncate to 9 → 123456789 (not rounded to 123456790)
        let t = parse_local_time("07:32:00.1234567899").unwrap();
        assert_eq!(t.nanosecond, 123_456_789);
    }

    #[test]
    fn fractional_seconds_many_digits_truncated() {
        // Many extra digits: .1234567899999 → truncate to 9 → 123456789
        let t = parse_local_time("07:32:00.1234567899999").unwrap();
        assert_eq!(t.nanosecond, 123_456_789);
    }

    #[test]
    fn fractional_seconds_truncation_in_offset_datetime() {
        // Truncation also applies to OffsetDateTime
        let dt = parse_offset_dt("1979-05-27T07:32:00.9999999999Z").unwrap();
        assert_eq!(dt.time.nanosecond, 999_999_999);
    }

    #[test]
    fn fractional_seconds_truncation_in_local_datetime() {
        // Truncation also applies to LocalDateTime
        let dt = parse_local_dt("1979-05-27T07:32:00.9999999999").unwrap();
        assert_eq!(dt.time.nanosecond, 999_999_999);
    }

    // -----------------------------------------------------------------------
    // 8.7 – Invalid field ranges
    // -----------------------------------------------------------------------

    #[test]
    fn invalid_month_13() {
        assert!(parse_local_date("2024-13-01").is_err());
    }

    #[test]
    fn invalid_month_00() {
        assert!(parse_local_date("2024-00-01").is_err());
    }

    #[test]
    fn invalid_day_32() {
        assert!(parse_local_date("2024-01-32").is_err());
    }

    #[test]
    fn invalid_day_00() {
        assert!(parse_local_date("2024-01-00").is_err());
    }

    #[test]
    fn invalid_hour_24() {
        assert!(parse_local_time("24:00:00").is_err());
    }

    #[test]
    fn invalid_minute_60() {
        assert!(parse_local_time("00:60:00").is_err());
    }

    #[test]
    fn invalid_second_61() {
        // second > 60 is invalid (60 is allowed for leap seconds)
        assert!(parse_local_time("00:00:61").is_err());
    }

    // -----------------------------------------------------------------------
    // 8.8 – Invalid format: missing separators, wrong format
    // -----------------------------------------------------------------------

    #[test]
    fn invalid_date_missing_dashes() {
        // "20240101" looks like an integer, not a date
        let input = "x = 20240101";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut map) => {
                // Should parse as integer, not LocalDate
                assert!(matches!(map.shift_remove("x").unwrap(), Value::Integer(_)));
            }
            _ => panic!("expected Table"),
        }
    }

    #[test]
    fn invalid_time_missing_colons() {
        // "073200" looks like an integer, not a time
        let input = "x = 073200";
        assert!(parse(input).is_err()); // leading zero in integer is rejected
    }

    #[test]
    fn invalid_datetime_wrong_separator() {
        // Using 'X' instead of 'T' or space should not produce a datetime
        // The date part "1979-05-27" would be parsed as LocalDate, then "X07:32:00Z" is a parse error
        let input = "x = 1979-05-27X07:32:00Z";
        assert!(parse(input).is_err());
    }

    #[test]
    fn invalid_date_only_year() {
        // "1979" is an integer, not a date
        let input = "x = 1979";
        let doc = parse(input).unwrap();
        match doc {
            Value::Table(mut map) => {
                assert!(matches!(map.shift_remove("x").unwrap(), Value::Integer(_)));
            }
            _ => panic!("expected Table"),
        }
    }
}

// ---------------------------------------------------------------------------
// Task 8.6 – Unit tests for arrays
// ---------------------------------------------------------------------------
#[cfg(test)]
mod array_tests {
    use crate::{parse, Value};

    /// Helper: parse `x = <array_toml>` and return the Vec<Value>.
    fn parse_array(array_toml: &str) -> Vec<Value> {
        let input = format!("x = {}", array_toml);
        let doc = parse(&input).expect("parse failed");
        match doc {
            Value::Table(mut map) => match map.shift_remove("x").unwrap() {
                Value::Array(v) => v,
                other => panic!("expected Array, got {:?}", other),
            },
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 9.1 – Empty array
    // -----------------------------------------------------------------------

    #[test]
    fn empty_array() {
        let arr = parse_array("[]");
        assert!(arr.is_empty());
    }

    // -----------------------------------------------------------------------
    // 9.1 – Single element array
    // -----------------------------------------------------------------------

    #[test]
    fn single_integer_element() {
        let arr = parse_array("[42]");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], Value::Integer(42));
    }

    #[test]
    fn single_string_element() {
        let arr = parse_array(r#"["hello"]"#);
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], Value::String("hello".to_string()));
    }

    // -----------------------------------------------------------------------
    // 9.1 – Multiple elements of the same type
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_integers() {
        let arr = parse_array("[1, 2, 3]");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
        assert_eq!(arr[2], Value::Integer(3));
    }

    #[test]
    fn multiple_strings() {
        let arr = parse_array(r#"["a", "b", "c"]"#);
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::String("a".to_string()));
        assert_eq!(arr[1], Value::String("b".to_string()));
        assert_eq!(arr[2], Value::String("c".to_string()));
    }

    #[test]
    fn multiple_booleans() {
        let arr = parse_array("[true, false, true]");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::Boolean(true));
        assert_eq!(arr[1], Value::Boolean(false));
        assert_eq!(arr[2], Value::Boolean(true));
    }

    // -----------------------------------------------------------------------
    // 9.2 – Mixed types in the same array
    // -----------------------------------------------------------------------

    #[test]
    fn mixed_integer_and_string() {
        let arr = parse_array(r#"[1, "two", 3]"#);
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::String("two".to_string()));
        assert_eq!(arr[2], Value::Integer(3));
    }

    #[test]
    fn mixed_integer_float_boolean_string() {
        let arr = parse_array(r#"[1, 2.5, true, "hello"]"#);
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Float(2.5));
        assert_eq!(arr[2], Value::Boolean(true));
        assert_eq!(arr[3], Value::String("hello".to_string()));
    }

    // -----------------------------------------------------------------------
    // 9.3 – Trailing comma after last element
    // -----------------------------------------------------------------------

    #[test]
    fn trailing_comma_single_element() {
        let arr = parse_array("[1,]");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], Value::Integer(1));
    }

    #[test]
    fn trailing_comma_multiple_elements() {
        let arr = parse_array("[1, 2, 3,]");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
        assert_eq!(arr[2], Value::Integer(3));
    }

    // -----------------------------------------------------------------------
    // 9.5 – Nested arrays (arrays of arrays)
    // -----------------------------------------------------------------------

    #[test]
    fn nested_array_of_integers() {
        let arr = parse_array("[[1, 2], [3, 4]]");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], Value::Array(vec![Value::Integer(1), Value::Integer(2)]));
        assert_eq!(arr[1], Value::Array(vec![Value::Integer(3), Value::Integer(4)]));
    }

    #[test]
    fn nested_array_empty_inner() {
        let arr = parse_array("[[], []]");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], Value::Array(vec![]));
        assert_eq!(arr[1], Value::Array(vec![]));
    }

    #[test]
    fn deeply_nested_array() {
        let arr = parse_array("[[[1]]]");
        assert_eq!(arr.len(), 1);
        let inner1 = match &arr[0] {
            Value::Array(v) => v,
            _ => panic!("expected Array"),
        };
        assert_eq!(inner1.len(), 1);
        let inner2 = match &inner1[0] {
            Value::Array(v) => v,
            _ => panic!("expected Array"),
        };
        assert_eq!(inner2, &vec![Value::Integer(1)]);
    }

    // -----------------------------------------------------------------------
    // 9.4 – Multi-line arrays with newlines between elements
    // -----------------------------------------------------------------------

    #[test]
    fn multiline_array_newlines_between_elements() {
        let input = "x = [\n  1,\n  2,\n  3\n]";
        let arr = match parse(input).unwrap() {
            Value::Table(mut m) => match m.shift_remove("x").unwrap() {
                Value::Array(v) => v,
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
        assert_eq!(arr[2], Value::Integer(3));
    }

    #[test]
    fn multiline_array_trailing_comma_with_newline() {
        let input = "x = [\n  1,\n  2,\n]";
        let arr = match parse(input).unwrap() {
            Value::Table(mut m) => match m.shift_remove("x").unwrap() {
                Value::Array(v) => v,
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
    }

    // -----------------------------------------------------------------------
    // 9.4 – Arrays with comments between elements
    // -----------------------------------------------------------------------

    #[test]
    fn array_with_comment_between_elements() {
        let input = "x = [\n  1, # first\n  2, # second\n  3  # third\n]";
        let arr = match parse(input).unwrap() {
            Value::Table(mut m) => match m.shift_remove("x").unwrap() {
                Value::Array(v) => v,
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
        assert_eq!(arr[2], Value::Integer(3));
    }

    #[test]
    fn array_with_comment_after_trailing_comma() {
        let input = "x = [\n  1,\n  2, # last element\n]";
        let arr = match parse(input).unwrap() {
            Value::Table(mut m) => match m.shift_remove("x").unwrap() {
                Value::Array(v) => v,
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
    }

    #[test]
    fn array_comment_before_first_element() {
        let input = "x = [\n  # header comment\n  1,\n  2\n]";
        let arr = match parse(input).unwrap() {
            Value::Table(mut m) => match m.shift_remove("x").unwrap() {
                Value::Array(v) => v,
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], Value::Integer(1));
        assert_eq!(arr[1], Value::Integer(2));
    }
}

// ---------------------------------------------------------------------------
// Task 8.7 – Unit tests for standard tables
// ---------------------------------------------------------------------------
#[cfg(test)]
mod table_tests {
    use crate::{parse, Value};

    // Helper: extract a nested Value by a dot-separated path.
    fn get_path<'a>(val: &'a Value, path: &[&str]) -> &'a Value {
        let mut cur = val;
        for seg in path {
            match cur {
                Value::Table(m) => cur = m.get(*seg).unwrap_or_else(|| panic!("key '{}' not found", seg)),
                other => panic!("expected Table at '{}', got {:?}", seg, other),
            }
        }
        cur
    }

    // -----------------------------------------------------------------------
    // 10.1 – Basic table header: [table] creates a table with key/value pairs
    // -----------------------------------------------------------------------

    #[test]
    fn basic_table_header_creates_table() {
        let input = "[owner]\nname = \"Alice\"\nage = 30\n";
        let doc = parse(input).unwrap();
        let owner = get_path(&doc, &["owner"]);
        assert_eq!(get_path(owner, &["name"]), &Value::String("Alice".to_string()));
        assert_eq!(get_path(owner, &["age"]), &Value::Integer(30));
    }

    #[test]
    fn basic_table_header_multiple_tables() {
        let input = "[a]\nx = 1\n[b]\ny = 2\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["a", "x"]), &Value::Integer(1));
        assert_eq!(get_path(&doc, &["b", "y"]), &Value::Integer(2));
    }

    // -----------------------------------------------------------------------
    // 10.2 – Dotted table header: [a.b.c] creates nested tables
    // -----------------------------------------------------------------------

    #[test]
    fn dotted_table_header_creates_nested_tables() {
        let input = "[a.b.c]\nval = 42\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["a", "b", "c", "val"]), &Value::Integer(42));
    }

    #[test]
    fn dotted_table_header_two_levels() {
        let input = "[database.connection]\nhost = \"localhost\"\nport = 5432\n";
        let doc = parse(input).unwrap();
        assert_eq!(
            get_path(&doc, &["database", "connection", "host"]),
            &Value::String("localhost".to_string())
        );
        assert_eq!(
            get_path(&doc, &["database", "connection", "port"]),
            &Value::Integer(5432)
        );
    }

    // -----------------------------------------------------------------------
    // 10.3 – Whitespace in header: [ table ] and [ a . b ] are valid
    // -----------------------------------------------------------------------

    #[test]
    fn whitespace_around_key_in_header() {
        let input = "[ table ]\nkey = \"value\"\n";
        let doc = parse(input).unwrap();
        assert_eq!(
            get_path(&doc, &["table", "key"]),
            &Value::String("value".to_string())
        );
    }

    #[test]
    fn whitespace_around_dots_in_dotted_header() {
        let input = "[ a . b ]\nkey = 1\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["a", "b", "key"]), &Value::Integer(1));
    }

    // -----------------------------------------------------------------------
    // 10.9 – Super-table defined after sub-table
    // -----------------------------------------------------------------------

    #[test]
    fn super_table_after_sub_table() {
        let input = "[x.y.z.w]\ndeep = true\n[x]\nshallow = 1\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["x", "shallow"]), &Value::Integer(1));
        assert_eq!(
            get_path(&doc, &["x", "y", "z", "w", "deep"]),
            &Value::Boolean(true)
        );
    }

    #[test]
    fn super_table_after_sub_table_two_levels() {
        let input = "[a.b]\ninner = 2\n[a]\nouter = 1\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["a", "outer"]), &Value::Integer(1));
        assert_eq!(get_path(&doc, &["a", "b", "inner"]), &Value::Integer(2));
    }

    // -----------------------------------------------------------------------
    // 10.4 – Duplicate header rejection
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_table_header_rejected() {
        let input = "[table]\nkey = 1\n[table]\nkey = 2\n";
        assert!(parse(input).is_err());
    }

    #[test]
    fn duplicate_dotted_table_header_rejected() {
        let input = "[a.b]\nx = 1\n[a.b]\ny = 2\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 10.5 – Redefining an explicitly defined table returns Err
    // -----------------------------------------------------------------------

    #[test]
    fn redefine_explicit_table_rejected() {
        // [fruit] defined twice explicitly
        let input = "[fruit]\nname = \"apple\"\n[fruit]\nname = \"banana\"\n";
        assert!(parse(input).is_err());
    }

    #[test]
    fn redefine_table_via_direct_key_assignment_rejected() {
        // 'a' is first defined as a non-table value, then [a] tries to open it
        let input = "a = 1\n[a]\nkey = 2\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 10.6 – Implicit-table promotion: a table created implicitly via dotted
    //         keys can be opened with [header] once
    // -----------------------------------------------------------------------

    #[test]
    fn implicit_table_can_be_opened_once() {
        // 'a.b' is created implicitly by a dotted key, then [a] opens 'a'
        let input = "[a.b]\ninner = 1\n[a]\nouter = 2\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["a", "outer"]), &Value::Integer(2));
        assert_eq!(get_path(&doc, &["a", "b", "inner"]), &Value::Integer(1));
    }

    #[test]
    fn implicit_table_cannot_be_opened_twice() {
        // Opening [a] twice after it was implicitly created should fail
        let input = "[a.b]\ninner = 1\n[a]\nouter = 2\n[a]\nother = 3\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 10.8 – Root table key/value pairs before any header
    // -----------------------------------------------------------------------

    #[test]
    fn root_table_keyvals_before_header() {
        let input = "name = \"root\"\nversion = 1\n[section]\nkey = \"val\"\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["name"]), &Value::String("root".to_string()));
        assert_eq!(get_path(&doc, &["version"]), &Value::Integer(1));
        assert_eq!(
            get_path(&doc, &["section", "key"]),
            &Value::String("val".to_string())
        );
    }

    #[test]
    fn root_table_only_no_headers() {
        let input = "a = 1\nb = \"hello\"\nc = true\n";
        let doc = parse(input).unwrap();
        assert_eq!(get_path(&doc, &["a"]), &Value::Integer(1));
        assert_eq!(get_path(&doc, &["b"]), &Value::String("hello".to_string()));
        assert_eq!(get_path(&doc, &["c"]), &Value::Boolean(true));
    }

    // -----------------------------------------------------------------------
    // Additional: table structure is Value::Table(IndexMap)
    // -----------------------------------------------------------------------

    #[test]
    fn table_value_is_indexmap() {
        let input = "[t]\nx = 1\ny = 2\n";
        let doc = parse(input).unwrap();
        match get_path(&doc, &["t"]) {
            Value::Table(m) => {
                assert!(m.contains_key("x"));
                assert!(m.contains_key("y"));
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn table_preserves_insertion_order() {
        let input = "[t]\na = 1\nb = 2\nc = 3\n";
        let doc = parse(input).unwrap();
        match get_path(&doc, &["t"]) {
            Value::Table(m) => {
                let keys: Vec<&str> = m.keys().map(|s| s.as_str()).collect();
                assert_eq!(keys, vec!["a", "b", "c"]);
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Task 8.8 – Unit tests for inline tables
// ---------------------------------------------------------------------------
#[cfg(test)]
mod inline_table_tests {
    use crate::{parse, Value};

    // Helper: parse and extract a top-level key as Value.
    fn get_key(doc: &Value, key: &str) -> Value {
        match doc {
            Value::Table(m) => m.get(key).cloned().expect("key not found"),
            _ => panic!("expected Table at root"),
        }
    }

    // -----------------------------------------------------------------------
    // 11.1 – Valid inline table: single key/value pair
    // -----------------------------------------------------------------------

    #[test]
    fn valid_single_pair() {
        let doc = parse("t = {key = \"value\"}").unwrap();
        match get_key(&doc, "t") {
            Value::Table(m) => {
                assert_eq!(m.get("key"), Some(&Value::String("value".to_string())));
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 11.1 – Empty inline table
    // -----------------------------------------------------------------------

    #[test]
    fn empty_inline_table() {
        let doc = parse("t = {}").unwrap();
        match get_key(&doc, "t") {
            Value::Table(m) => assert!(m.is_empty()),
            other => panic!("expected empty Table, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 11.1 – Multiple key/value pairs
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_pairs() {
        let doc = parse("t = {a = 1, b = 2}").unwrap();
        match get_key(&doc, "t") {
            Value::Table(m) => {
                assert_eq!(m.get("a"), Some(&Value::Integer(1)));
                assert_eq!(m.get("b"), Some(&Value::Integer(2)));
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 11.2 – Trailing comma rejection
    // -----------------------------------------------------------------------

    #[test]
    fn trailing_comma_rejected() {
        assert!(parse("t = {a = 1,}").is_err());
    }

    // -----------------------------------------------------------------------
    // 11.3 – Newline rejection
    // -----------------------------------------------------------------------

    #[test]
    fn newline_inside_inline_table_rejected() {
        let input = "t = {a = 1,\nb = 2}";
        assert!(parse(input).is_err());
    }

    #[test]
    fn newline_before_closing_brace_rejected() {
        let input = "t = {a = 1\n}";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 11.4 – Duplicate key rejection
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_key_rejected() {
        assert!(parse("t = {a = 1, a = 2}").is_err());
    }

    // -----------------------------------------------------------------------
    // 11.5 – Extension rejection: [x] after x = {a = 1}
    // -----------------------------------------------------------------------

    #[test]
    fn extension_via_table_header_rejected() {
        let input = "x = {a = 1}\n[x]\nb = 2\n";
        assert!(parse(input).is_err());
    }

    #[test]
    fn extension_via_dotted_key_rejected() {
        let input = "x = {a = 1}\nx.b = 2\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 11.1 – Nested inline tables
    // -----------------------------------------------------------------------

    #[test]
    fn nested_inline_tables() {
        let doc = parse("t = {a = {b = 1}}").unwrap();
        match get_key(&doc, "t") {
            Value::Table(outer) => match outer.get("a") {
                Some(Value::Table(inner)) => {
                    assert_eq!(inner.get("b"), Some(&Value::Integer(1)));
                }
                other => panic!("expected inner Table, got {:?}", other),
            },
            other => panic!("expected outer Table, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 11.1 – Inline table value is Value::Table(IndexMap)
    // -----------------------------------------------------------------------

    #[test]
    fn inline_table_is_indexmap() {
        let doc = parse("t = {x = 1, y = 2}").unwrap();
        match get_key(&doc, "t") {
            Value::Table(m) => {
                let keys: Vec<&str> = m.keys().map(|s| s.as_str()).collect();
                assert_eq!(keys, vec!["x", "y"]);
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Task 8.9 – Unit tests for array of tables
// ---------------------------------------------------------------------------
#[cfg(test)]
mod aot_tests {
    use crate::{parse, Value};

    // Helper: navigate a dot-separated path through nested Tables.
    fn get_path<'a>(val: &'a Value, path: &[&str]) -> &'a Value {
        let mut cur = val;
        for seg in path {
            match cur {
                Value::Table(m) => {
                    cur = m.get(*seg).unwrap_or_else(|| panic!("key '{}' not found", seg))
                }
                other => panic!("expected Table at '{}', got {:?}", seg, other),
            }
        }
        cur
    }

    // -----------------------------------------------------------------------
    // 12.1 – Basic append: [[products]] twice creates an array of 2 tables
    // -----------------------------------------------------------------------

    #[test]
    fn basic_append_two_elements() {
        let input = "[[products]]\nname = \"hammer\"\n[[products]]\nname = \"nail\"\n";
        let doc = parse(input).unwrap();
        match get_path(&doc, &["products"]) {
            Value::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert!(matches!(&arr[0], Value::Table(_)));
                assert!(matches!(&arr[1], Value::Table(_)));
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 12.2 – Each [[key]] header associates key/value pairs with the most
    //         recently appended element
    // -----------------------------------------------------------------------

    #[test]
    fn keyvals_associated_with_last_appended_element() {
        let input = "[[products]]\nname = \"hammer\"\nprice = 9\n[[products]]\nname = \"nail\"\nprice = 1\n";
        let doc = parse(input).unwrap();
        match get_path(&doc, &["products"]) {
            Value::Array(arr) => {
                assert_eq!(arr.len(), 2);
                match &arr[0] {
                    Value::Table(m) => {
                        assert_eq!(m.get("name"), Some(&Value::String("hammer".to_string())));
                        assert_eq!(m.get("price"), Some(&Value::Integer(9)));
                    }
                    other => panic!("expected Table, got {:?}", other),
                }
                match &arr[1] {
                    Value::Table(m) => {
                        assert_eq!(m.get("name"), Some(&Value::String("nail".to_string())));
                        assert_eq!(m.get("price"), Some(&Value::Integer(1)));
                    }
                    other => panic!("expected Table, got {:?}", other),
                }
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 12.6 – Nested AOT: [[fruits.varieties]] nested under [[fruits]]
    // -----------------------------------------------------------------------

    #[test]
    fn nested_aot_under_parent_aot() {
        let input = concat!(
            "[[fruits]]\n",
            "name = \"apple\"\n",
            "[[fruits.varieties]]\n",
            "name = \"red delicious\"\n",
            "[[fruits.varieties]]\n",
            "name = \"granny smith\"\n",
            "[[fruits]]\n",
            "name = \"banana\"\n",
            "[[fruits.varieties]]\n",
            "name = \"plantain\"\n",
        );
        let doc = parse(input).unwrap();
        match get_path(&doc, &["fruits"]) {
            Value::Array(fruits) => {
                assert_eq!(fruits.len(), 2);
                // First fruit: apple with 2 varieties
                match &fruits[0] {
                    Value::Table(m) => {
                        assert_eq!(m.get("name"), Some(&Value::String("apple".to_string())));
                        match m.get("varieties") {
                            Some(Value::Array(vars)) => {
                                assert_eq!(vars.len(), 2);
                                match &vars[0] {
                                    Value::Table(v) => assert_eq!(
                                        v.get("name"),
                                        Some(&Value::String("red delicious".to_string()))
                                    ),
                                    other => panic!("expected Table, got {:?}", other),
                                }
                                match &vars[1] {
                                    Value::Table(v) => assert_eq!(
                                        v.get("name"),
                                        Some(&Value::String("granny smith".to_string()))
                                    ),
                                    other => panic!("expected Table, got {:?}", other),
                                }
                            }
                            other => panic!("expected Array for varieties, got {:?}", other),
                        }
                    }
                    other => panic!("expected Table for fruits[0], got {:?}", other),
                }
                // Second fruit: banana with 1 variety
                match &fruits[1] {
                    Value::Table(m) => {
                        assert_eq!(m.get("name"), Some(&Value::String("banana".to_string())));
                        match m.get("varieties") {
                            Some(Value::Array(vars)) => {
                                assert_eq!(vars.len(), 1);
                                match &vars[0] {
                                    Value::Table(v) => assert_eq!(
                                        v.get("name"),
                                        Some(&Value::String("plantain".to_string()))
                                    ),
                                    other => panic!("expected Table, got {:?}", other),
                                }
                            }
                            other => panic!("expected Array for varieties, got {:?}", other),
                        }
                    }
                    other => panic!("expected Table for fruits[1], got {:?}", other),
                }
            }
            other => panic!("expected Array for fruits, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 12.5 – Static-array conflict: `fruits = []` followed by `[[fruits]]`
    //         should return Err
    // -----------------------------------------------------------------------

    #[test]
    fn static_array_conflict_returns_err() {
        let input = "fruits = []\n[[fruits]]\nname = \"apple\"\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 12.3 – Standard-table conflict: `[fruits]` followed by `[[fruits]]`
    //         should return Err
    // -----------------------------------------------------------------------

    #[test]
    fn standard_table_conflict_returns_err() {
        let input = "[fruits]\nname = \"apple\"\n[[fruits]]\nname = \"banana\"\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 12.4 – [[aot]] after [aot] (standard table) should return Err
    // -----------------------------------------------------------------------

    #[test]
    fn aot_after_standard_table_returns_err() {
        let input = "[aot]\nkey = 1\n[[aot]]\nkey = 2\n";
        assert!(parse(input).is_err());
    }

    // -----------------------------------------------------------------------
    // 12.8 – Sub-table under AOT element: [products.details] after
    //         [[products]] adds a sub-table to the last element
    // -----------------------------------------------------------------------

    #[test]
    fn sub_table_under_aot_element() {
        let input = concat!(
            "[[products]]\n",
            "name = \"widget\"\n",
            "[products.details]\n",
            "weight = 5\n",
            "color = \"blue\"\n",
        );
        let doc = parse(input).unwrap();
        match get_path(&doc, &["products"]) {
            Value::Array(arr) => {
                assert_eq!(arr.len(), 1);
                match &arr[0] {
                    Value::Table(m) => {
                        assert_eq!(m.get("name"), Some(&Value::String("widget".to_string())));
                        match m.get("details") {
                            Some(Value::Table(d)) => {
                                assert_eq!(d.get("weight"), Some(&Value::Integer(5)));
                                assert_eq!(
                                    d.get("color"),
                                    Some(&Value::String("blue".to_string()))
                                );
                            }
                            other => panic!("expected Table for details, got {:?}", other),
                        }
                    }
                    other => panic!("expected Table for products[0], got {:?}", other),
                }
            }
            other => panic!("expected Array for products, got {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Task 8.10 – Unit tests for key rules
// ---------------------------------------------------------------------------
#[cfg(test)]
mod key_tests {
    use crate::{parse, Value};

    // Helper: extract a nested value by path from a Table.
    fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
        match v {
            Value::Table(m) => m.get(key).unwrap_or_else(|| panic!("key '{}' not found", key)),
            _ => panic!("expected Table"),
        }
    }

    // -----------------------------------------------------------------------
    // 3.1 – Bare keys: [A-Za-z0-9_-]+ characters are valid
    // -----------------------------------------------------------------------

    #[test]
    fn bare_key_letters() {
        let doc = parse("abcXYZ = 1\n").unwrap();
        assert_eq!(get(&doc, "abcXYZ"), &Value::Integer(1));
    }

    #[test]
    fn bare_key_digits() {
        let doc = parse("key123 = 2\n").unwrap();
        assert_eq!(get(&doc, "key123"), &Value::Integer(2));
    }

    #[test]
    fn bare_key_underscore_and_dash() {
        let doc = parse("my_key-name = 3\n").unwrap();
        assert_eq!(get(&doc, "my_key-name"), &Value::Integer(3));
    }

    #[test]
    fn bare_key_all_valid_chars() {
        let doc = parse("A-Z_a-z_0-9 = true\n").unwrap();
        assert_eq!(get(&doc, "A-Z_a-z_0-9"), &Value::Boolean(true));
    }

    // -----------------------------------------------------------------------
    // 3.2 – Quoted keys: basic string and literal string as keys
    // -----------------------------------------------------------------------

    #[test]
    fn quoted_key_basic_string() {
        let doc = parse("\"my key\" = \"hello\"\n").unwrap();
        assert_eq!(get(&doc, "my key"), &Value::String("hello".to_string()));
    }

    #[test]
    fn quoted_key_literal_string() {
        let doc = parse("'my-key' = 42\n").unwrap();
        assert_eq!(get(&doc, "my-key"), &Value::Integer(42));
    }

    #[test]
    fn quoted_key_with_special_chars() {
        let doc = parse("\"key with spaces\" = true\n").unwrap();
        assert_eq!(get(&doc, "key with spaces"), &Value::Boolean(true));
    }

    // -----------------------------------------------------------------------
    // 3.4 – Empty quoted key: "" = "value" is valid
    // -----------------------------------------------------------------------

    #[test]
    fn empty_quoted_key_basic() {
        let doc = parse("\"\" = \"value\"\n").unwrap();
        assert_eq!(get(&doc, ""), &Value::String("value".to_string()));
    }

    #[test]
    fn empty_quoted_key_literal() {
        let doc = parse("'' = 99\n").unwrap();
        assert_eq!(get(&doc, ""), &Value::Integer(99));
    }

    // -----------------------------------------------------------------------
    // 3.5 – Dotted keys: a.b.c = 1 creates nested tables
    // -----------------------------------------------------------------------

    #[test]
    fn dotted_key_two_levels() {
        let doc = parse("a.b = 1\n").unwrap();
        let a = get(&doc, "a");
        assert_eq!(get(a, "b"), &Value::Integer(1));
    }

    #[test]
    fn dotted_key_three_levels() {
        let doc = parse("a.b.c = 1\n").unwrap();
        let a = get(&doc, "a");
        let b = get(a, "b");
        assert_eq!(get(b, "c"), &Value::Integer(1));
    }

    #[test]
    fn dotted_key_creates_table_structure() {
        let doc = parse("a.b = 1\n").unwrap();
        match get(&doc, "a") {
            Value::Table(_) => {}
            other => panic!("expected Table for 'a', got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 3.6 – Whitespace around dot: a . b = 1 is equivalent to a.b = 1
    // -----------------------------------------------------------------------

    #[test]
    fn dotted_key_whitespace_around_dot() {
        let doc = parse("a . b = 1\n").unwrap();
        let a = get(&doc, "a");
        assert_eq!(get(a, "b"), &Value::Integer(1));
    }

    #[test]
    fn dotted_key_whitespace_around_dot_equivalent() {
        let doc1 = parse("a.b = 42\n").unwrap();
        let doc2 = parse("a . b = 42\n").unwrap();
        assert_eq!(doc1, doc2);
    }

    // -----------------------------------------------------------------------
    // 3.9 – Key/quoted-key equivalence: `key` and `"key"` refer to the same entry
    // -----------------------------------------------------------------------

    #[test]
    fn bare_and_quoted_key_same_entry() {
        // Defining `key` and then `"key"` should be a duplicate error
        let input = "key = 1\n\"key\" = 2\n";
        assert!(parse(input).is_err(), "duplicate key should be rejected");
    }

    #[test]
    fn quoted_key_accesses_same_as_bare() {
        // A value set with a bare key is accessible under the same string
        let doc = parse("name = \"Alice\"\n").unwrap();
        assert_eq!(get(&doc, "name"), &Value::String("Alice".to_string()));
        // And a value set with a quoted key is accessible under the same string
        let doc2 = parse("\"name\" = \"Bob\"\n").unwrap();
        assert_eq!(get(&doc2, "name"), &Value::String("Bob".to_string()));
    }

    // -----------------------------------------------------------------------
    // 3.10 – Digit-only dotted key: 3.14159 = "pi" is nested tables, not float
    // -----------------------------------------------------------------------

    #[test]
    fn digit_only_dotted_key_not_float() {
        let doc = parse("3.14159 = \"pi\"\n").unwrap();
        // Should produce a table with key "3" containing a table with key "14159"
        let three = get(&doc, "3");
        assert_eq!(get(three, "14159"), &Value::String("pi".to_string()));
    }

    #[test]
    fn digit_only_dotted_key_structure() {
        let doc = parse("3.14159 = \"pi\"\n").unwrap();
        match get(&doc, "3") {
            Value::Table(_) => {}
            other => panic!("expected Table for key '3', got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 3.3 – Empty bare key rejection: `= value` with no key should return Err
    // -----------------------------------------------------------------------

    #[test]
    fn empty_bare_key_rejected() {
        assert!(parse("= \"value\"\n").is_err());
    }

    #[test]
    fn empty_bare_key_no_space_rejected() {
        assert!(parse("=\"value\"\n").is_err());
    }
}

// ---------------------------------------------------------------------------
// Task 8.11 – Unit tests for error reporting
// ---------------------------------------------------------------------------
#[cfg(test)]
mod error_tests {
    use crate::parse;

    // -----------------------------------------------------------------------
    // Helper: assert parse returns Err with a non-empty message.
    // -----------------------------------------------------------------------
    fn assert_err_with_message(input: &str) -> crate::ParseError {
        let result = parse(input);
        assert!(result.is_err(), "expected Err for input: {:?}", input);
        let err = result.unwrap_err();
        assert!(!err.message.is_empty(), "error message should not be empty");
        err
    }

    // -----------------------------------------------------------------------
    // 15.1 – Parser returns Err (not panics) for invalid inputs
    // 15.2 – ParseError has non-empty message
    // -----------------------------------------------------------------------

    // Lexer-level error: invalid escape sequence in basic string.
    // The lexer tracks line/col, so we also verify line > 0 and col > 0.
    #[test]
    fn invalid_escape_sequence_returns_err() {
        let err = assert_err_with_message(r#"x = "\a""#);
        assert!(err.line > 0, "line should be > 0, got {}", err.line);
        assert!(err.col > 0, "col should be > 0, got {}", err.col);
    }

    // Lexer-level error: control character (U+0001) in basic string.
    #[test]
    fn control_char_in_basic_string_returns_err() {
        let input = "x = \"\u{0001}\"";
        let err = assert_err_with_message(input);
        assert!(err.line > 0, "line should be > 0, got {}", err.line);
        assert!(err.col > 0, "col should be > 0, got {}", err.col);
    }

    // Parser-level error: duplicate key.
    #[test]
    fn duplicate_key_returns_err() {
        let input = "name = \"Alice\"\nname = \"Bob\"\n";
        assert_err_with_message(input);
    }

    // Parser-level error: duplicate table header.
    #[test]
    fn duplicate_table_header_returns_err() {
        let input = "[server]\nhost = \"a\"\n[server]\nhost = \"b\"\n";
        assert_err_with_message(input);
    }

    // Parser-level error: trailing comma in inline table.
    #[test]
    fn trailing_comma_inline_table_returns_err() {
        let input = "x = {a = 1,}\n";
        assert_err_with_message(input);
    }

    // Lexer-level error: integer overflow (value outside i64 range).
    #[test]
    fn integer_overflow_returns_err() {
        // 2^63 = 9223372036854775808, which is one more than i64::MAX
        let input = "x = 9223372036854775808\n";
        let err = assert_err_with_message(input);
        assert!(err.line > 0, "line should be > 0, got {}", err.line);
        assert!(err.col > 0, "col should be > 0, got {}", err.col);
    }

    // Lexer-level error: invalid date field range (month 13 is invalid).
    #[test]
    fn invalid_date_field_range_returns_err() {
        let input = "x = 2024-13-01\n";
        let err = assert_err_with_message(input);
        assert!(err.line > 0, "line should be > 0, got {}", err.line);
        assert!(err.col > 0, "col should be > 0, got {}", err.col);
    }

    // -----------------------------------------------------------------------
    // 15.2 – ParseError line/col tracking: error on line 2 vs line 1
    // For lexer-level errors the lexer tracks the actual position.
    // -----------------------------------------------------------------------

    // Error on line 1: invalid escape on the first line.
    #[test]
    fn error_on_line_1_has_correct_line() {
        let input = "x = \"\\a\"\n";
        let err = assert_err_with_message(input);
        assert_eq!(err.line, 1, "error should be on line 1, got {}", err.line);
    }

    // Error on line 2: invalid escape on the second line.
    #[test]
    fn error_on_line_2_has_correct_line() {
        let input = "valid = 1\nx = \"\\a\"\n";
        let err = assert_err_with_message(input);
        assert_eq!(err.line, 2, "error should be on line 2, got {}", err.line);
    }

    // -----------------------------------------------------------------------
    // 15.2 – Non-empty message for all representative error cases
    // -----------------------------------------------------------------------

    #[test]
    fn all_error_cases_have_non_empty_message() {
        let cases = [
            // lexer errors
            ("invalid escape", "x = \"\\a\"\n"),
            ("control char", "x = \"\u{0001}\"\n"),
            ("integer overflow", "x = 9223372036854775808\n"),
            ("invalid date", "x = 2024-13-01\n"),
            // parser errors
            ("duplicate key", "a = 1\na = 2\n"),
            ("duplicate table", "[t]\n[t]\n"),
            ("trailing comma inline", "x = {a = 1,}\n"),
        ];
        for (label, input) in &cases {
            let result = parse(input);
            assert!(result.is_err(), "{}: expected Err", label);
            let err = result.unwrap_err();
            assert!(!err.message.is_empty(), "{}: message should not be empty", label);
        }
    }

    // -----------------------------------------------------------------------
    // 15.2 – Precise line/col tests for representative error categories
    // -----------------------------------------------------------------------

    // 1. Duplicate key: "a = 1\na = 2\n"
    //    Parser-level error; parser currently reports line=1, col=1.
    #[test]
    fn duplicate_key_line_col() {
        let input = "a = 1\na = 2\n";
        let e = parse(input).expect_err("expected duplicate key error");
        assert_eq!(e.line, 1, "duplicate key: wrong line (got {})", e.line);
        assert_eq!(e.col, 1, "duplicate key: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "duplicate key: message must not be empty");
    }

    // 2. Invalid escape sequence in a basic string: a = "\q"
    //    Lexer-level error; \q is at col 7 (1-based: a=1, space=2, ==3, space=4, "=5, \=6, q=7).
    #[test]
    fn invalid_escape_line_col() {
        let input = "a = \"\\q\"\n";
        let e = parse(input).expect_err("expected invalid escape error");
        assert_eq!(e.line, 1, "invalid escape: wrong line (got {})", e.line);
        assert_eq!(e.col, 7, "invalid escape: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "invalid escape: message must not be empty");
    }

    // 3. Integer overflow: value larger than i64::MAX
    //    Lexer-level error; number starts at col 5 (a=1, space=2, ==3, space=4, digit=5).
    #[test]
    fn integer_overflow_line_col() {
        // 9223372036854775808 = i64::MAX + 1
        let input = "a = 9223372036854775808\n";
        let e = parse(input).expect_err("expected integer overflow error");
        assert_eq!(e.line, 1, "integer overflow: wrong line (got {})", e.line);
        assert_eq!(e.col, 5, "integer overflow: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "integer overflow: message must not be empty");
    }

    // 4. Invalid date/time: month 13
    //    Lexer-level error; date token starts at col 5.
    #[test]
    fn invalid_date_month_13_line_col() {
        let input = "a = 2024-13-01\n";
        let e = parse(input).expect_err("expected invalid date error");
        assert_eq!(e.line, 1, "invalid date: wrong line (got {})", e.line);
        assert_eq!(e.col, 5, "invalid date: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "invalid date: message must not be empty");
    }

    // 5. Inline table extension via a subsequent [table] header
    //    Parser-level error; parser reports line=1, col=1.
    #[test]
    fn inline_table_extension_line_col() {
        let input = "a = {x = 1}\n[a]\nb = 2\n";
        let e = parse(input).expect_err("expected inline table extension error");
        assert_eq!(e.line, 1, "inline table extension: wrong line (got {})", e.line);
        assert_eq!(e.col, 1, "inline table extension: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "inline table extension: message must not be empty");
    }

    // 6. Trailing content after a value (no newline/EOF): "a = 1 b = 2\n"
    //    Parser-level error; parser reports line=1, col=1.
    #[test]
    fn trailing_content_after_value_line_col() {
        let input = "a = 1 b = 2\n";
        let e = parse(input).expect_err("expected trailing content error");
        assert_eq!(e.line, 1, "trailing content: wrong line (got {})", e.line);
        assert_eq!(e.col, 1, "trailing content: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "trailing content: message must not be empty");
    }

    // 7. Duplicate table header: "[a]\n[a]\n"
    //    Parser-level error; parser reports line=1, col=1.
    #[test]
    fn duplicate_table_header_line_col() {
        let input = "[a]\n[a]\n";
        let e = parse(input).expect_err("expected duplicate table header error");
        assert_eq!(e.line, 1, "duplicate table header: wrong line (got {})", e.line);
        assert_eq!(e.col, 1, "duplicate table header: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "duplicate table header: message must not be empty");
    }

    // 8. Appending to a static array via [[key]]
    //    Parser-level error; parser reports line=1, col=1.
    #[test]
    fn append_to_static_array_via_aot_line_col() {
        let input = "a = [1, 2]\n[[a]]\nb = 3\n";
        let e = parse(input).expect_err("expected static array append error");
        assert_eq!(e.line, 1, "static array append: wrong line (got {})", e.line);
        assert_eq!(e.col, 1, "static array append: wrong col (got {})", e.col);
        assert!(!e.message.is_empty(), "static array append: message must not be empty");
    }

    // -----------------------------------------------------------------------
    // 15.5 – First error in document order: when multiple errors could be
    //         reported, the parser reports the first one encountered.
    // -----------------------------------------------------------------------

    // Two errors: invalid escape on line 1 and duplicate key on line 3.
    // The first error (invalid escape, line 1) should be reported.
    #[test]
    fn first_error_in_document_order() {
        // Line 1 has an invalid escape; line 3 would be a duplicate key.
        let input = "a = \"\\q\"\nb = 1\nb = 2\n";
        let e = parse(input).expect_err("expected error");
        assert_eq!(e.line, 1, "first error should be on line 1, got {}", e.line);
    }

}
