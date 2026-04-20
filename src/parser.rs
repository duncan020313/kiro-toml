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
