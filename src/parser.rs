use indexmap::IndexMap;
use crate::error::ParseError;
use crate::lexer::{Lexer, Token};
use crate::value::{LocalDate, LocalDateTime, LocalTime, OffsetDateTime, Value};

// ---------------------------------------------------------------------------
// Task 6.1 – Internal data structures
// ---------------------------------------------------------------------------

/// Tracks whether a table entry was explicitly defined or implicitly created.
enum TrackedValue {
    /// Fully defined, immutable.
    Defined(Value),
    /// Created by a dotted key – extensible until promoted.
    ImplicitTable(IndexMap<String, TrackedValue>),
    /// Inline table – closed/immutable.
    InlineTable(IndexMap<String, TrackedValue>),
    /// [[header]] array of tables.
    ArrayOfTables(Vec<IndexMap<String, TrackedValue>>),
}

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
// Returns a mutable reference to the map.
// ---------------------------------------------------------------------------
fn navigate_to<'m>(
    root: &'m mut IndexMap<String, TrackedValue>,
    path: &[String],
    line: u32,
    col: u32,
) -> Result<&'m mut IndexMap<String, TrackedValue>, ParseError> {
    let mut current = root;
    for seg in path {
        let entry = current.get_mut(seg).ok_or_else(|| {
            ParseError::new(format!("internal: path segment '{}' not found", seg), line, col)
        })?;
        current = match entry {
            TrackedValue::ImplicitTable(m) => m,
            TrackedValue::Defined(Value::Table(m)) => {
                // Wrap in a temporary – we need to re-borrow via the enum arm.
                // We can't return a &mut to the inner IndexMap directly through
                // Value::Table because Value is not TrackedValue. We handle this
                // by converting on the fly: this path is only reached when an
                // AOT element's sub-table was promoted to Defined(Value::Table).
                // We need a different approach: keep everything as TrackedValue.
                // This branch should not normally be hit during navigation because
                // we store sub-tables as ImplicitTable/ArrayOfTables in TrackedValue.
                // If it is hit, return an error.
                return Err(ParseError::new(
                    format!("cannot extend already-defined table at '{}'", seg),
                    line, col,
                ));
            }
            TrackedValue::ArrayOfTables(arr) => {
                arr.last_mut().ok_or_else(|| {
                    ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                })?
            }
            TrackedValue::InlineTable(_) => {
                return Err(ParseError::new(
                    format!("cannot extend inline table '{}'", seg),
                    line, col,
                ));
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
        // Navigate to the current table.
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
                // Create ImplicitTable if absent; error if already Defined non-table or InlineTable.
                if !map.contains_key(seg.as_str()) {
                    map.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new()));
                }
                let entry = map.get_mut(seg.as_str()).unwrap();
                map = match entry {
                    TrackedValue::ImplicitTable(m) => m,
                    TrackedValue::Defined(_) => {
                        return Err(ParseError::new(
                            format!("'{}' is already defined as a non-table value", seg),
                            line, col,
                        ));
                    }
                    TrackedValue::InlineTable(_) => {
                        return Err(ParseError::new(
                            format!("cannot extend inline table '{}'", seg),
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
            // Insert at the final segment.
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
                current.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new()));
            }
            let entry = current.get_mut(seg.as_str()).unwrap();
            current = match entry {
                TrackedValue::ImplicitTable(m) => m,
                TrackedValue::Defined(Value::Table(_)) => {
                    // A table defined via AOT element – not reachable here since
                    // we store AOT elements as ImplicitTable inside ArrayOfTables.
                    return Err(ParseError::new(
                        format!("cannot use '{}' as intermediate: already defined", seg),
                        line, col,
                    ));
                }
                TrackedValue::ArrayOfTables(arr) => {
                    arr.last_mut().ok_or_else(|| {
                        ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                    })?
                }
                TrackedValue::InlineTable(_) => {
                    return Err(ParseError::new(
                        format!("cannot extend inline table '{}'", seg),
                        line, col,
                    ));
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
                // Create a new empty standard table.
                current.insert(last.clone(), TrackedValue::ImplicitTable(IndexMap::new()));
            }
            Some(TrackedValue::ImplicitTable(_)) => {
                // Promote implicit table to standard table (mark as defined).
                // We leave it as ImplicitTable but track that it has been "opened"
                // by recording the path. The promotion is conceptual: we just
                // allow it once. We need to mark it so it can't be re-opened.
                // We'll use a sentinel: replace ImplicitTable with Defined(Value::Table)
                // but we need to keep the contents. We'll do the promotion here.
                let existing = current.remove(last.as_str()).unwrap();
                if let TrackedValue::ImplicitTable(m) = existing {
                    // Re-insert as a "standard table" marker. We use a special
                    // variant: we'll repurpose ImplicitTable but mark it as
                    // "already opened as standard table" by converting to
                    // a new variant. Since we don't have one, we'll use a
                    // naming convention: store as ImplicitTable but track
                    // the opened path in current_path so duplicate detection works.
                    // Actually the simplest approach: keep as ImplicitTable but
                    // add a sentinel key that can't appear in real TOML.
                    // Better: introduce a new variant. But the spec says to use
                    // the given enum. Let's use Defined(Value::Table) to mark
                    // "opened as standard table" – but then we can't extend it.
                    // 
                    // The correct approach per the spec:
                    // - ImplicitTable can be promoted to standard table ONCE.
                    // - After promotion it becomes a "standard table" that can
                    //   still receive key/value pairs (via the current_path mechanism)
                    //   but cannot be re-opened with another [header].
                    //
                    // We'll use a dedicated wrapper: store as ImplicitTable with
                    // a special marker. Since we can't add variants, we'll track
                    // "opened standard tables" in a separate set on TableTracker.
                    current.insert(last.clone(), TrackedValue::ImplicitTable(m));
                }
            }
            Some(TrackedValue::Defined(_)) => {
                return Err(ParseError::new(
                    format!("table '{}' already defined", last),
                    line, col,
                ));
            }
            Some(TrackedValue::InlineTable(_)) => {
                return Err(ParseError::new(
                    format!("cannot redefine inline table '{}'", last),
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
                current.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new()));
            }
            let entry = current.get_mut(seg.as_str()).unwrap();
            current = match entry {
                TrackedValue::ImplicitTable(m) => m,
                TrackedValue::ArrayOfTables(arr) => {
                    arr.last_mut().ok_or_else(|| {
                        ParseError::new(format!("empty array of tables at '{}'", seg), line, col)
                    })?
                }
                TrackedValue::InlineTable(_) => {
                    return Err(ParseError::new(
                        format!("cannot extend inline table '{}'", seg),
                        line, col,
                    ));
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
            Some(TrackedValue::ImplicitTable(_)) => {
                return Err(ParseError::new(
                    format!("'{}' is already defined as a table; cannot use as array of tables", last),
                    line, col,
                ));
            }
            Some(TrackedValue::InlineTable(_)) => {
                return Err(ParseError::new(
                    format!("cannot redefine inline table '{}' as array of tables", last),
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
        TrackedValue::ImplicitTable(m) => Value::Table(convert_map(m)),
        TrackedValue::InlineTable(m) => Value::Table(convert_map(m)),
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
    /// Tracks which standard-table paths have already been opened (to detect duplicates).
    opened_standard_tables: std::collections::HashSet<Vec<String>>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Parser {
            lexer: Lexer::new(input),
            tracker: TableTracker::new(),
            opened_standard_tables: std::collections::HashSet::new(),
        }
    }

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
            other => {
                let (line, col) = self.lexer_position();
                Err(ParseError::new(
                    format!("expected key, found {:?}", other),
                    line, col,
                ))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.7 – parse_value
    // -----------------------------------------------------------------------
    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let (line, col) = self.lexer_position();
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
                line, col,
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.8 – parse_array
    // -----------------------------------------------------------------------
    fn parse_array(&mut self) -> Result<Value, ParseError> {
        // LBracket already consumed by caller (parse_value).
        let mut elements = Vec::new();
        loop {
            // Skip newlines and comments.
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
            match self.lexer.peek_token()? {
                Token::Comma => {
                    self.lexer.next_token()?; // consume comma
                }
                Token::RBracket => {
                    self.lexer.next_token()?; // consume ]
                    return Ok(Value::Array(elements));
                }
                other => {
                    let (line, col) = self.lexer_position();
                    return Err(ParseError::new(
                        format!("expected ',' or ']' in array, found {:?}", other),
                        line, col,
                    ));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.9 – parse_inline_table
    // -----------------------------------------------------------------------
    fn parse_inline_table(&mut self) -> Result<Value, ParseError> {
        // LBrace already consumed by caller (parse_value).
        let mut pairs: IndexMap<String, TrackedValue> = IndexMap::new();

        if matches!(self.lexer.peek_token()?, Token::RBrace) {
            self.lexer.next_token()?; // consume }
            return Ok(Value::Table(IndexMap::new()));
        }

        loop {
            let (line, col) = self.lexer_position();
            let key_segs = self.parse_key()?;
            self.expect_token(Token::Equals, "=")?;
            let value = self.parse_value()?;

            // Insert into pairs, handling dotted keys.
            insert_into_inline(&mut pairs, key_segs, value, line, col)?;

            match self.lexer.peek_token()? {
                Token::Comma => {
                    self.lexer.next_token()?; // consume comma
                    // Check for trailing comma (next must not be })
                    if matches!(self.lexer.peek_token()?, Token::RBrace) {
                        let (line, col) = self.lexer_position();
                        return Err(ParseError::new(
                            "trailing comma not allowed in inline table",
                            line, col,
                        ));
                    }
                }
                Token::RBrace => {
                    self.lexer.next_token()?; // consume }
                    return Ok(Value::Table(convert_map(pairs)));
                }
                Token::Newline => {
                    let (line, col) = self.lexer_position();
                    return Err(ParseError::new(
                        "newlines not allowed inside inline table",
                        line, col,
                    ));
                }
                other => {
                    let (line, col) = self.lexer_position();
                    return Err(ParseError::new(
                        format!("expected ',' or '}}' in inline table, found {:?}", other),
                        line, col,
                    ));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 6.10 – parse_table_header / parse_aot_header
    // -----------------------------------------------------------------------
    fn parse_table_header(&mut self) -> Result<(), ParseError> {
        // LBracket already consumed by caller.
        let (line, col) = self.lexer_position();
        let key = self.parse_key()?;
        self.expect_token(Token::RBracket, "]")?;
        self.require_newline_or_eof()?;

        // Duplicate standard table detection.
        if self.opened_standard_tables.contains(&key) {
            return Err(ParseError::new(
                format!("table '{}' defined more than once", key.join(".")),
                line, col,
            ));
        }
        self.opened_standard_tables.insert(key.clone());

        self.tracker.set_standard_table(key, line, col)
    }

    fn parse_aot_header(&mut self) -> Result<(), ParseError> {
        // DoubleLBracket already consumed by caller.
        let (line, col) = self.lexer_position();
        let key = self.parse_key()?;
        self.expect_token(Token::DoubleRBracket, "]]")?;
        self.require_newline_or_eof()?;
        self.tracker.append_aot(key, line, col)
    }

    // -----------------------------------------------------------------------
    // Task 6.11 – parse_keyval
    // -----------------------------------------------------------------------
    fn parse_keyval(&mut self) -> Result<(), ParseError> {
        let (line, col) = self.lexer_position();
        let key = self.parse_key()?;
        self.expect_token(Token::Equals, "=")?;
        let value = self.parse_value()?;
        self.require_newline_or_eof()?;
        self.tracker.insert_keyval(key, value, line, col)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Returns the current lexer position (line, col) by peeking.
    /// Falls back to (0, 0) on error.
    fn lexer_position(&mut self) -> (u32, u32) {
        // We can't easily get position without peeking; use a best-effort approach.
        // The lexer tracks position internally; we expose it via a peek.
        // For now return (1,1) as a fallback – the lexer embeds position in errors.
        (1, 1)
    }

    fn expect_token(&mut self, expected: Token<'a>, name: &str) -> Result<(), ParseError> {
        let tok = self.lexer.next_token()?;
        // Compare by discriminant for tokens without data, or exact match.
        let matches = match (&tok, &expected) {
            (Token::Equals, Token::Equals) => true,
            (Token::RBracket, Token::RBracket) => true,
            (Token::DoubleRBracket, Token::DoubleRBracket) => true,
            (Token::RBrace, Token::RBrace) => true,
            (Token::Comma, Token::Comma) => true,
            (Token::Dot, Token::Dot) => true,
            _ => false,
        };
        if !matches {
            Err(ParseError::new(
                format!("expected '{}', found {:?}", name, tok),
                1, 1,
            ))
        } else {
            Ok(())
        }
    }

    fn require_newline_or_eof(&mut self) -> Result<(), ParseError> {
        match self.lexer.peek_token()? {
            Token::Newline | Token::Eof => {
                if matches!(self.lexer.peek_token()?, Token::Newline) {
                    self.lexer.next_token()?;
                }
                Ok(())
            }
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
                current.insert(seg.clone(), TrackedValue::ImplicitTable(IndexMap::new()));
            }
            let entry = current.get_mut(seg.as_str()).unwrap();
            current = match entry {
                TrackedValue::ImplicitTable(m) => m,
                TrackedValue::Defined(_) => {
                    return Err(ParseError::new(
                        format!("'{}' is already defined", seg),
                        line, col,
                    ));
                }
                TrackedValue::InlineTable(_) => {
                    return Err(ParseError::new(
                        format!("cannot extend inline table '{}'", seg),
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
