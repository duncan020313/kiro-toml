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
            // Skip newlines.
            while matches!(self.lexer.peek_token()?, Token::Newline) {
                self.lexer.next_token()?;
            }
            match self.lexer.peek_token()? {
                Token::Eof => break,
                Token::LBracket => {
                    self.lexer.next_token()?; // consume LBracket
                    self.parse_table_header()?;
                }
                Token::DoubleLBracket => {
                    self.lexer.next_token()?; // consume DoubleLBracket
                    self.parse_aot_header()?;
                }
                _ => {
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
        let mut segments = vec![self.parse_simple_key()?];
        while matches!(self.lexer.peek_token()?, Token::Dot) {
            self.lexer.next_token()?; // consume Dot
            segments.push(self.parse_simple_key()?);
        }
        Ok(segments)
    }

    fn parse_simple_key(&mut self) -> Result<String, ParseError> {
        let tok = self.lexer.next_token()?;
        match tok {
            Token::BareKey(s) => Ok(s.to_string()),
            Token::BasicString(s) => Ok(s),
            Token::LiteralString(s) => Ok(s.to_string()),
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
            if is_comma {
                self.lexer.next_token()?; // consume comma
                // trailing comma is fine – loop will hit RBracket next
            } else if is_rbracket {
                self.lexer.next_token()?; // consume ]
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
