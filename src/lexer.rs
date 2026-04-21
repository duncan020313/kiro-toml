use crate::error::ParseError;
use crate::value::{LocalDate, LocalDateTime, LocalTime, OffsetDateTime, UtcOffset};

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    // Structural
    Equals,
    Dot,
    Comma,
    Newline,
    Eof,
    LBracket,
    RBracket,
    DoubleLBracket,
    DoubleRBracket,
    LBrace,
    RBrace,
    // Literals
    BareKey(&'a str),
    BasicString(String),
    LiteralString(&'a str),
    MlBasicString(String),
    MlLiteralString(&'a str),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    OffsetDateTimeToken(OffsetDateTime),
    LocalDateTimeToken(LocalDateTime),
    LocalDateToken(LocalDate),
    LocalTimeToken(LocalTime),
}

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    peek_buf: Option<Token<'a>>,
    /// When true, digit sequences followed by '.' are NOT treated as floats.
    /// This allows digit-only dotted keys like `3.14159` to be parsed as
    /// nested bare-key segments rather than a float literal.
    pub key_mode: bool,
}
impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0, line: 1, col: 1, peek_buf: None, key_mode: false }
    }

    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError::new(msg, self.line, self.col)
    }

    fn err_at(&self, msg: impl Into<String>, line: u32, col: u32) -> ParseError {
        ParseError::new(msg, line, col)
    }

    fn current_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.input[self.pos + offset..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.current_char()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    pub fn next_token(&mut self) -> Result<Token<'a>, ParseError> {
        if let Some(tok) = self.peek_buf.take() {
            return Ok(tok);
        }
        self.lex_token()
    }

    pub fn peek_token(&mut self) -> Result<&Token<'a>, ParseError> {
        if self.peek_buf.is_none() {
            let tok = self.lex_token()?;
            self.peek_buf = Some(tok);
        }
        Ok(self.peek_buf.as_ref().unwrap())
    }

    /// Push a token back so it will be returned by the next `next_token` call.
    /// Panics if the peek buffer is already occupied.
    pub fn push_back(&mut self, tok: Token<'a>) {
        assert!(self.peek_buf.is_none(), "push_back: peek buffer already occupied");
        self.peek_buf = Some(tok);
    }

    fn lex_token(&mut self) -> Result<Token<'a>, ParseError> {
        loop {
            // Skip spaces and tabs
            while matches!(self.current_char(), Some(' ') | Some('\t')) {
                self.advance();
            }

            let line = self.line;
            let col = self.col;

            match self.current_char() {
                None => return Ok(Token::Eof),
                Some('\n') => {
                    self.advance();
                    return Ok(Token::Newline);
                }
                Some('\r') => {
                    self.advance();
                    if self.current_char() == Some('\n') {
                        self.advance();
                    }
                    return Ok(Token::Newline);
                }
                Some('#') => {
                    self.advance(); // consume '#'
                    // skip until newline or EOF, validate no control chars
                    loop {
                        match self.current_char() {
                            None | Some('\n') | Some('\r') => break,
                            Some(c) => {
                                let cp = c as u32;
                                if (cp <= 0x0008) || (0x000A..=0x001F).contains(&cp) || cp == 0x007F {
                                    return Err(self.err_at(
                                        format!("control character U+{:04X} in comment", cp),
                                        line, col,
                                    ));
                                }
                                self.advance();
                            }
                        }
                    }
                    // continue loop to get next real token
                    continue;
                }
                Some('=') => { self.advance(); return Ok(Token::Equals); }
                Some('.') => { self.advance(); return Ok(Token::Dot); }
                Some(',') => { self.advance(); return Ok(Token::Comma); }
                Some('{') => { self.advance(); return Ok(Token::LBrace); }
                Some('}') => { self.advance(); return Ok(Token::RBrace); }
                Some('[') => {
                    self.advance();
                    if self.current_char() == Some('[') {
                        self.advance();
                        return Ok(Token::DoubleLBracket);
                    }
                    return Ok(Token::LBracket);
                }
                Some(']') => {
                    self.advance();
                    if self.current_char() == Some(']') {
                        self.advance();
                        return Ok(Token::DoubleRBracket);
                    }
                    return Ok(Token::RBracket);
                }
                Some('"') => {
                    // Check for multi-line """
                    if self.remaining().starts_with("\"\"\"") {
                        return self.lex_ml_basic_string();
                    }
                    return self.lex_basic_string();
                }
                Some('\'') => {
                    if self.remaining().starts_with("'''") {
                        return self.lex_ml_literal_string();
                    }
                    return self.lex_literal_string();
                }
                Some('t') => {
                    if self.remaining().starts_with("true") {
                        let after = self.remaining().as_bytes().get(4).copied();
                        if !matches!(after, Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')) {
                            self.pos += 4;
                            self.col += 4;
                            return Ok(Token::Boolean(true));
                        }
                    }
                    return self.lex_bare_key();
                }
                Some('f') => {
                    if self.remaining().starts_with("false") {
                        let after = self.remaining().as_bytes().get(5).copied();
                        if !matches!(after, Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')) {
                            self.pos += 5;
                            self.col += 5;
                            return Ok(Token::Boolean(false));
                        }
                    }
                    return self.lex_bare_key();
                }
                Some('i') => {
                    if self.remaining().starts_with("inf") {
                        let after = self.remaining().as_bytes().get(3).copied();
                        if !matches!(after, Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')) {
                            self.pos += 3;
                            self.col += 3;
                            return Ok(Token::Float(f64::INFINITY));
                        }
                    }
                    return self.lex_bare_key();
                }
                Some('n') => {
                    if self.remaining().starts_with("nan") {
                        let after = self.remaining().as_bytes().get(3).copied();
                        if !matches!(after, Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')) {
                            self.pos += 3;
                            self.col += 3;
                            return Ok(Token::Float(f64::NAN));
                        }
                    }
                    return self.lex_bare_key();
                }
                Some('+') | Some('-') => {
                    return self.lex_number_or_special(line, col);
                }
                Some('0'..='9') => {
                    return self.lex_number_or_datetime(line, col);
                }
                Some(c) if is_bare_key_char(c) => {
                    return self.lex_bare_key();
                }
                Some(c) => {
                    return Err(self.err(format!("unexpected character {:?}", c)));
                }
            }
        }
    }

    // ---- Bare key ----
    fn lex_bare_key(&mut self) -> Result<Token<'a>, ParseError> {
        let start = self.pos;
        while let Some(c) = self.current_char() {
            if is_bare_key_char(c) {
                self.advance();
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(self.err("expected bare key character"));
        }
        Ok(Token::BareKey(&self.input[start..self.pos]))
    }

    // ---- Basic string ----
    fn lex_basic_string(&mut self) -> Result<Token<'a>, ParseError> {
        let line = self.line;
        let col = self.col;
        self.advance(); // consume opening "
        let mut s = String::new();
        loop {
            match self.current_char() {
                None | Some('\n') | Some('\r') => {
                    return Err(self.err_at("unterminated basic string", line, col));
                }
                Some('"') => {
                    self.advance();
                    return Ok(Token::BasicString(s));
                }
                Some('\\') => {
                    self.advance();
                    let esc = self.process_escape()?;
                    s.push(esc);
                }
                Some(c) => {
                    let cp = c as u32;
                    if cp <= 0x0008 || (0x000A..=0x001F).contains(&cp) || cp == 0x007F {
                        return Err(self.err(format!("control character U+{:04X} in basic string", cp)));
                    }
                    s.push(c);
                    self.advance();
                }
            }
        }
    }

    // ---- Literal string ----
    fn lex_literal_string(&mut self) -> Result<Token<'a>, ParseError> {
        let line = self.line;
        let col = self.col;
        self.advance(); // consume opening '
        let start = self.pos;
        loop {
            match self.current_char() {
                None | Some('\n') | Some('\r') => {
                    return Err(self.err_at("unterminated literal string", line, col));
                }
                Some('\'') => {
                    let end = self.pos;
                    self.advance();
                    return Ok(Token::LiteralString(&self.input[start..end]));
                }
                Some(c) => {
                    let cp = c as u32;
                    // Allow tab (0x09), reject other control chars
                    if cp != 0x09 && (cp <= 0x0008 || (0x000A..=0x001F).contains(&cp) || cp == 0x007F) {
                        return Err(self.err(format!("control character U+{:04X} in literal string", cp)));
                    }
                    self.advance();
                }
            }
        }
    }

    // ---- Multi-line basic string ----
    fn lex_ml_basic_string(&mut self) -> Result<Token<'a>, ParseError> {
        let line = self.line;
        let col = self.col;
        // consume opening """
        self.advance(); self.advance(); self.advance();
        // trim single leading newline
        if self.current_char() == Some('\n') {
            self.advance();
        } else if self.current_char() == Some('\r') && self.peek_char_at(1) == Some('\n') {
            self.advance(); self.advance();
        }
        let mut s = String::new();
        loop {
            match self.current_char() {
                None => return Err(self.err_at("unterminated multi-line basic string", line, col)),
                Some('"') => {
                    // Count consecutive quotes
                    let mut quote_count = 0;
                    while self.current_char() == Some('"') && quote_count < 5 {
                        self.advance();
                        quote_count += 1;
                    }
                    if quote_count >= 3 {
                        // closing delimiter found; extra quotes before closing are content
                        let extra = quote_count - 3;
                        for _ in 0..extra {
                            s.push('"');
                        }
                        return Ok(Token::MlBasicString(s));
                    } else {
                        // Not closing, push the quotes as content
                        for _ in 0..quote_count {
                            s.push('"');
                        }
                    }
                }
                Some('\\') => {
                    self.advance();
                    // Line-ending backslash: skip whitespace/newlines
                    if matches!(self.current_char(), Some(' ') | Some('\t') | Some('\n') | Some('\r')) {
                        // trim whitespace
                        loop {
                            match self.current_char() {
                                Some(' ') | Some('\t') | Some('\n') | Some('\r') => { self.advance(); }
                                _ => break,
                            }
                        }
                    } else {
                        let esc = self.process_escape()?;
                        s.push(esc);
                    }
                }
                Some(c) => {
                    let cp = c as u32;
                    // Allow tab (0x09), LF (0x0A), CR (0x0D)
                    if cp != 0x09 && cp != 0x0A && cp != 0x0D
                        && (cp <= 0x0008 || (0x000B..=0x001F).contains(&cp) || cp == 0x007F)
                    {
                        return Err(self.err(format!("control character U+{:04X} in multi-line basic string", cp)));
                    }
                    s.push(c);
                    self.advance();
                }
            }
        }
    }

    // ---- Multi-line literal string ----
    #[allow(unused_assignments)]
    fn lex_ml_literal_string(&mut self) -> Result<Token<'a>, ParseError> {
        let line = self.line;
        let col = self.col;
        // consume opening '''
        self.advance(); self.advance(); self.advance();
        // trim single leading newline
        if self.current_char() == Some('\n') {
            self.advance();
        } else if self.current_char() == Some('\r') && self.peek_char_at(1) == Some('\n') {
            self.advance(); self.advance();
        }
        let start = self.pos;
        // We need to find the closing ''' and return the raw slice
        // But we also need to handle up to 2 extra quotes before closing
        // We'll collect into a string to handle the extra-quote edge case properly
        // Actually for MlLiteralString we return &'a str, so we need to find the end position
        // The content between ''' and ''' (with possible extra quotes before closing)
        // Strategy: scan for ''' and handle extra quotes
        let mut content_end = start;
        loop {
            match self.current_char() {
                None => return Err(self.err_at("unterminated multi-line literal string", line, col)),
                Some('\'') => {
                    let quote_start = self.pos;
                    let mut quote_count = 0;
                    while self.current_char() == Some('\'') && quote_count < 5 {
                        self.advance();
                        quote_count += 1;
                    }
                    if quote_count >= 3 {
                        // closing found; content_end is quote_start + (quote_count - 3) extra quotes
                        let extra = quote_count - 3;
                        content_end = quote_start + extra;
                        return Ok(Token::MlLiteralString(&self.input[start..content_end]));
                    } else {
                        // not closing, continue
                        content_end = self.pos;
                    }
                }
                Some(c) => {
                    let cp = c as u32;
                    // Allow tab (0x09), LF (0x0A), CR (0x0D)
                    if cp != 0x09 && cp != 0x0A && cp != 0x0D
                        && (cp <= 0x0008 || (0x000B..=0x001F).contains(&cp) || cp == 0x007F)
                    {
                        return Err(self.err(format!("control character U+{:04X} in multi-line literal string", cp)));
                    }
                    self.advance();
                    content_end = self.pos;
                }
            }
        }
    }

    // ---- Escape sequence processing ----
    // Returns the unescaped char. Caller has already consumed the backslash.
    fn process_escape(&mut self) -> Result<char, ParseError> {
        match self.current_char() {
            Some('b') => { self.advance(); Ok('\u{0008}') }
            Some('t') => { self.advance(); Ok('\u{0009}') }
            Some('n') => { self.advance(); Ok('\u{000A}') }
            Some('f') => { self.advance(); Ok('\u{000C}') }
            Some('r') => { self.advance(); Ok('\u{000D}') }
            Some('"') => { self.advance(); Ok('"') }
            Some('\\') => { self.advance(); Ok('\\') }
            Some('u') => {
                self.advance();
                self.lex_unicode_escape(4)
            }
            Some('U') => {
                self.advance();
                self.lex_unicode_escape(8)
            }
            Some(c) => {
                Err(self.err(format!("invalid escape sequence '\\{}'", c)))
            }
            None => Err(self.err("unexpected EOF in escape sequence")),
        }
    }

    fn lex_unicode_escape(&mut self, digits: usize) -> Result<char, ParseError> {
        let mut value: u32 = 0;
        for _ in 0..digits {
            match self.current_char() {
                Some(c) if c.is_ascii_hexdigit() => {
                    value = value * 16 + c.to_digit(16).unwrap();
                    self.advance();
                }
                Some(c) => return Err(self.err(format!("invalid hex digit '{}' in unicode escape", c))),
                None => return Err(self.err("unexpected EOF in unicode escape")),
            }
        }
        char::from_u32(value).ok_or_else(|| self.err(format!("U+{:04X} is not a valid Unicode scalar value", value)))
    }

    // ---- Number / special float dispatch ----
    fn lex_number_or_special(&mut self, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        let sign_char = self.current_char().unwrap();
        // Check for +inf, -inf, +nan, -nan
        if sign_char == '+' || sign_char == '-' {
            if self.remaining().starts_with("+inf") || self.remaining().starts_with("-inf") {
                let val = if sign_char == '+' { f64::INFINITY } else { f64::NEG_INFINITY };
                self.pos += 4; self.col += 4;
                return Ok(Token::Float(val));
            }
            if self.remaining().starts_with("+nan") || self.remaining().starts_with("-nan") {
                self.pos += 4; self.col += 4;
                return Ok(Token::Float(f64::NAN));
            }
        }
        self.lex_number(line, col)
    }

    fn lex_number_or_datetime(&mut self, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        // Check if this looks like a date/time: 4 digits followed by '-'
        let rem = self.remaining();
        let bytes = rem.as_bytes();
        // Check for YYYY-MM pattern (date) or HH:MM pattern (time)
        let is_date_like = bytes.len() >= 7
            && bytes[0].is_ascii_digit()
            && bytes[1].is_ascii_digit()
            && bytes[2].is_ascii_digit()
            && bytes[3].is_ascii_digit()
            && bytes[4] == b'-'
            && bytes[5].is_ascii_digit()
            && bytes[6].is_ascii_digit();

        let is_time_like = bytes.len() >= 5
            && bytes[0].is_ascii_digit()
            && bytes[1].is_ascii_digit()
            && bytes[2] == b':'
            && bytes[3].is_ascii_digit()
            && bytes[4].is_ascii_digit();

        if is_date_like {
            return self.lex_datetime(line, col);
        }
        if is_time_like {
            return self.lex_time_token(line, col);
        }
        self.lex_number(line, col)
    }

    fn lex_number(&mut self, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        let start = self.pos;
        let start_line = line;
        let start_col = col;

        // Check for sign
        let has_sign = matches!(self.current_char(), Some('+') | Some('-'));
        let sign_neg = self.current_char() == Some('-');
        if has_sign {
            self.advance();
        }

        // Check for base prefix
        if self.current_char() == Some('0') {
            let next = self.peek_char_at(1);
            match next {
                Some('x') => {
                    if has_sign {
                        return Err(self.err_at("sign prefix not allowed on hex integer", start_line, start_col));
                    }
                    self.advance(); self.advance(); // consume '0x'
                    return self.lex_based_integer(16, start, sign_neg, start_line, start_col);
                }
                Some('o') => {
                    if has_sign {
                        return Err(self.err_at("sign prefix not allowed on octal integer", start_line, start_col));
                    }
                    self.advance(); self.advance(); // consume '0o'
                    return self.lex_based_integer(8, start, sign_neg, start_line, start_col);
                }
                Some('b') => {
                    if has_sign {
                        return Err(self.err_at("sign prefix not allowed on binary integer", start_line, start_col));
                    }
                    self.advance(); self.advance(); // consume '0b'
                    return self.lex_based_integer(2, start, sign_neg, start_line, start_col);
                }
                _ => {}
            }
        }

        // Decimal integer or float
        self.lex_decimal_number(start, sign_neg, start_line, start_col)
    }

    fn lex_based_integer(&mut self, base: u32, _start: usize, _sign_neg: bool, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        let _digit_start = self.pos;
        let valid_digit = |c: char| -> bool {
            match base {
                16 => c.is_ascii_hexdigit(),
                8 => matches!(c, '0'..='7'),
                2 => matches!(c, '0' | '1'),
                _ => false,
            }
        };

        if !matches!(self.current_char(), Some(c) if valid_digit(c)) {
            return Err(self.err_at(format!("expected digit after base prefix"), line, col));
        }

        let mut value: u64 = 0;
        let mut prev_was_underscore = false;
        let mut first = true;

        loop {
            match self.current_char() {
                Some('_') => {
                    if first || prev_was_underscore {
                        return Err(self.err("invalid underscore placement in integer"));
                    }
                    prev_was_underscore = true;
                    self.advance();
                }
                Some(c) if valid_digit(c) => {
                    prev_was_underscore = false;
                    first = false;
                    let d = c.to_digit(base).unwrap() as u64;
                    value = value.checked_mul(base as u64)
                        .and_then(|v| v.checked_add(d))
                        .ok_or_else(|| self.err_at("integer overflow", line, col))?;
                    self.advance();
                }
                _ => break,
            }
        }

        if prev_was_underscore {
            return Err(self.err("integer cannot end with underscore"));
        }

        // For hex/octal/binary, interpret as unsigned then cast to i64
        // TOML spec says these are stored as i64 but the bit pattern is unsigned
        Ok(Token::Integer(value as i64))
    }

    fn lex_decimal_number(&mut self, _start: usize, sign_neg: bool, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        // Collect integer digits
        let int_start = self.pos;
        let mut has_leading_zero = false;
        let mut digit_count = 0;
        let mut prev_underscore = false;
        let mut first = true;

        loop {
            match self.current_char() {
                Some('_') => {
                    if first || prev_underscore {
                        return Err(self.err("invalid underscore placement in number"));
                    }
                    prev_underscore = true;
                    self.advance();
                }
                Some(c @ '0'..='9') => {
                    if first && c == '0' {
                        has_leading_zero = true;
                    }
                    first = false;
                    prev_underscore = false;
                    digit_count += 1;
                    self.advance();
                }
                _ => break,
            }
        }

        if prev_underscore {
            return Err(self.err("number cannot end with underscore"));
        }
        if digit_count == 0 {
            return Err(self.err_at("expected digit", line, col));
        }

        // Check for float indicators.
        // In key_mode, a '.' after digits is a dotted-key separator, not a decimal point.
        let is_float = !self.key_mode
            && matches!(self.current_char(), Some('.') | Some('e') | Some('E'));

        if is_float {
            return self.lex_float_continuation(int_start, sign_neg, has_leading_zero, line, col);
        }

        // Integer
        if has_leading_zero && digit_count > 1 {
            return Err(self.err_at("leading zeros not allowed in integer", line, col));
        }

        // Parse the integer digits (strip underscores)
        let int_str = &self.input[int_start..self.pos];
        let clean: String = int_str.chars().filter(|&c| c != '_').collect();
        let abs_val: u64 = clean.parse::<u64>().map_err(|_| self.err_at("integer overflow", line, col))?;

        let result = if sign_neg {
            if abs_val > (i64::MAX as u64) + 1 {
                return Err(self.err_at("integer overflow", line, col));
            }
            if abs_val == (i64::MAX as u64) + 1 {
                i64::MIN
            } else {
                -(abs_val as i64)
            }
        } else {
            if abs_val > i64::MAX as u64 {
                return Err(self.err_at("integer overflow", line, col));
            }
            abs_val as i64
        };

        Ok(Token::Integer(result))
    }

    fn lex_float_continuation(&mut self, int_start: usize, sign_neg: bool, _has_leading_zero: bool, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        // We're positioned after the integer digits; collect fractional and/or exponent
        let mut _has_frac = false;
        let mut _has_exp = false;

        if self.current_char() == Some('.') {
            _has_frac = true;
            self.advance();
            // Must have at least one digit after '.'
            if !matches!(self.current_char(), Some('0'..='9')) {
                return Err(self.err("expected digit after decimal point in float"));
            }
            let mut prev_underscore = false;
            loop {
                match self.current_char() {
                    Some('_') => {
                        if prev_underscore {
                            return Err(self.err("adjacent underscores in float"));
                        }
                        prev_underscore = true;
                        self.advance();
                    }
                    Some('0'..='9') => {
                        prev_underscore = false;
                        self.advance();
                    }
                    _ => break,
                }
            }
            if prev_underscore {
                return Err(self.err("float cannot end with underscore"));
            }
        }

        if matches!(self.current_char(), Some('e') | Some('E')) {
            _has_exp = true;
            self.advance();
            if matches!(self.current_char(), Some('+') | Some('-')) {
                self.advance();
            }
            if !matches!(self.current_char(), Some('0'..='9')) {
                return Err(self.err("expected digit in float exponent"));
            }
            let mut prev_underscore = false;
            loop {
                match self.current_char() {
                    Some('_') => {
                        if prev_underscore {
                            return Err(self.err("adjacent underscores in float exponent"));
                        }
                        prev_underscore = true;
                        self.advance();
                    }
                    Some('0'..='9') => {
                        prev_underscore = false;
                        self.advance();
                    }
                    _ => break,
                }
            }
            if prev_underscore {
                return Err(self.err("float exponent cannot end with underscore"));
            }
        }

        let float_str = if sign_neg {
            format!("-{}", &self.input[int_start..self.pos])
        } else {
            self.input[int_start..self.pos].to_string()
        };
        let clean: String = float_str.chars().filter(|&c| c != '_').collect();
        let val: f64 = clean.parse().map_err(|_| self.err_at("invalid float", line, col))?;
        Ok(Token::Float(val))
    }

    // ---- Date/time lexing ----
    fn parse_digits(&mut self, count: usize, what: &str) -> Result<u32, ParseError> {
        let mut val = 0u32;
        for _ in 0..count {
            match self.current_char() {
                Some(c @ '0'..='9') => {
                    val = val * 10 + (c as u32 - '0' as u32);
                    self.advance();
                }
                _ => return Err(self.err(format!("expected {} digits for {}", count, what))),
            }
        }
        Ok(val)
    }

    fn expect_char(&mut self, expected: char) -> Result<(), ParseError> {
        match self.current_char() {
            Some(c) if c == expected => { self.advance(); Ok(()) }
            Some(c) => Err(self.err(format!("expected '{}', found '{}'", expected, c))),
            None => Err(self.err(format!("expected '{}', found EOF", expected))),
        }
    }

    fn lex_datetime(&mut self, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        // Parse YYYY-MM-DD
        let year = self.parse_digits(4, "year")? as u16;
        self.expect_char('-')?;
        let month = self.parse_digits(2, "month")? as u8;
        self.expect_char('-')?;
        let day = self.parse_digits(2, "day")? as u8;

        // Validate date ranges
        if month < 1 || month > 12 {
            return Err(self.err_at(format!("invalid month {}", month), line, col));
        }
        if day < 1 || day > 31 {
            return Err(self.err_at(format!("invalid day {}", day), line, col));
        }

        let date = LocalDate { year, month, day };

        // Check for time separator: T, t, or space (space only if followed by a digit)
        let is_space_separator = self.current_char() == Some(' ')
            && matches!(self.peek_char_at(1), Some('0'..='9'));
        match self.current_char() {
            Some('T') | Some('t') => {
                self.advance();
                let time = self.lex_time(line, col)?;
                // Check for offset
                match self.current_char() {
                    Some('Z') | Some('z') => {
                        self.advance();
                        return Ok(Token::OffsetDateTimeToken(OffsetDateTime {
                            date, time, offset: UtcOffset::Z,
                        }));
                    }
                    Some('+') | Some('-') => {
                        let offset = self.lex_offset(line, col)?;
                        return Ok(Token::OffsetDateTimeToken(OffsetDateTime {
                            date, time, offset,
                        }));
                    }
                    _ => {
                        return Ok(Token::LocalDateTimeToken(LocalDateTime { date, time }));
                    }
                }
            }
            Some(' ') if is_space_separator => {
                self.advance();
                let time = self.lex_time(line, col)?;
                // Check for offset
                match self.current_char() {
                    Some('Z') | Some('z') => {
                        self.advance();
                        return Ok(Token::OffsetDateTimeToken(OffsetDateTime {
                            date, time, offset: UtcOffset::Z,
                        }));
                    }
                    Some('+') | Some('-') => {
                        let offset = self.lex_offset(line, col)?;
                        return Ok(Token::OffsetDateTimeToken(OffsetDateTime {
                            date, time, offset,
                        }));
                    }
                    _ => {
                        return Ok(Token::LocalDateTimeToken(LocalDateTime { date, time }));
                    }
                }
            }
            _ => {
                return Ok(Token::LocalDateToken(date));
            }
        }
    }

    fn lex_time_token(&mut self, line: u32, col: u32) -> Result<Token<'a>, ParseError> {
        let time = self.lex_time(line, col)?;
        Ok(Token::LocalTimeToken(time))
    }

    fn lex_time(&mut self, line: u32, col: u32) -> Result<crate::value::LocalTime, ParseError> {
        let hour = self.parse_digits(2, "hour")? as u8;
        self.expect_char(':')?;
        let minute = self.parse_digits(2, "minute")? as u8;
        self.expect_char(':')?;
        let second = self.parse_digits(2, "second")? as u8;

        if hour > 23 {
            return Err(self.err_at(format!("invalid hour {}", hour), line, col));
        }
        if minute > 59 {
            return Err(self.err_at(format!("invalid minute {}", minute), line, col));
        }
        if second > 60 {
            return Err(self.err_at(format!("invalid second {}", second), line, col));
        }

        let nanosecond = if self.current_char() == Some('.') {
            self.advance();
            // Read fractional digits
            let frac_start = self.pos;
            let mut frac_digits = 0usize;
            while matches!(self.current_char(), Some('0'..='9')) {
                self.advance();
                frac_digits += 1;
            }
            if frac_digits == 0 {
                return Err(self.err("expected digits after decimal point in time"));
            }
            let frac_str = &self.input[frac_start..frac_start + frac_digits.min(9)];
            // Truncate to 9 digits, right-pad with zeros
            let mut ns_str = frac_str.to_string();
            while ns_str.len() < 9 {
                ns_str.push('0');
            }
            ns_str.parse::<u32>().unwrap_or(0)
        } else {
            0
        };

        Ok(crate::value::LocalTime { hour, minute, second, nanosecond })
    }

    fn lex_offset(&mut self, line: u32, col: u32) -> Result<UtcOffset, ParseError> {
        let sign = self.current_char().unwrap();
        self.advance();
        let h = self.parse_digits(2, "offset hours")? as i16;
        self.expect_char(':')?;
        let m = self.parse_digits(2, "offset minutes")? as i16;
        if h > 23 || m > 59 {
            return Err(self.err_at(format!("invalid UTC offset {}:{}", h, m), line, col));
        }
        let total = h * 60 + m;
        Ok(UtcOffset::Minutes(if sign == '-' { -total } else { total }))
    }
}

fn is_bare_key_char(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token().expect("lex error");
            let done = tok == Token::Eof;
            tokens.push(tok);
            if done { break; }
        }
        tokens
    }

    #[test]
    fn test_structural_tokens() {
        let tokens = lex_all("= . , { } [ ] [[ ]]");
        assert_eq!(tokens[0], Token::Equals);
        assert_eq!(tokens[1], Token::Dot);
        assert_eq!(tokens[2], Token::Comma);
        assert_eq!(tokens[3], Token::LBrace);
        assert_eq!(tokens[4], Token::RBrace);
        assert_eq!(tokens[5], Token::LBracket);
        assert_eq!(tokens[6], Token::RBracket);
        assert_eq!(tokens[7], Token::DoubleLBracket);
        assert_eq!(tokens[8], Token::DoubleRBracket);
    }

    #[test]
    fn test_bare_key() {
        let tokens = lex_all("hello_world-123");
        assert_eq!(tokens[0], Token::BareKey("hello_world-123"));
    }

    #[test]
    fn test_basic_string() {
        let tokens = lex_all(r#""hello\nworld""#);
        assert_eq!(tokens[0], Token::BasicString("hello\nworld".to_string()));
    }

    #[test]
    fn test_literal_string() {
        let tokens = lex_all("'hello\\nworld'");
        assert_eq!(tokens[0], Token::LiteralString("hello\\nworld"));
    }

    #[test]
    fn test_integer_decimal() {
        let tokens = lex_all("42 -17 +100 1_000_000");
        assert_eq!(tokens[0], Token::Integer(42));
        assert_eq!(tokens[1], Token::Integer(-17));
        assert_eq!(tokens[2], Token::Integer(100));
        assert_eq!(tokens[3], Token::Integer(1_000_000));
    }

    #[test]
    fn test_integer_hex() {
        let tokens = lex_all("0xDEADBEEF 0xff");
        assert_eq!(tokens[0], Token::Integer(0xDEADBEEF_i64));
        assert_eq!(tokens[1], Token::Integer(0xff));
    }

    #[test]
    fn test_integer_octal() {
        let tokens = lex_all("0o755");
        assert_eq!(tokens[0], Token::Integer(0o755));
    }

    #[test]
    fn test_integer_binary() {
        let tokens = lex_all("0b11010110");
        assert_eq!(tokens[0], Token::Integer(0b11010110));
    }

    #[test]
    fn test_float() {
        let tokens = lex_all("3.14 1e10 -2.5e-3");
        assert_eq!(tokens[0], Token::Float(3.14));
        assert_eq!(tokens[1], Token::Float(1e10));
        assert_eq!(tokens[2], Token::Float(-2.5e-3));
    }

    #[test]
    fn test_float_special() {
        let tokens = lex_all("inf -inf +inf nan");
        assert_eq!(tokens[0], Token::Float(f64::INFINITY));
        assert_eq!(tokens[1], Token::Float(f64::NEG_INFINITY));
        assert_eq!(tokens[2], Token::Float(f64::INFINITY));
        assert!(matches!(tokens[3], Token::Float(f) if f.is_nan()));
    }

    #[test]
    fn test_boolean() {
        let tokens = lex_all("true false");
        assert_eq!(tokens[0], Token::Boolean(true));
        assert_eq!(tokens[1], Token::Boolean(false));
    }

    #[test]
    fn test_newline_and_comment() {
        let tokens = lex_all("a # comment\nb");
        assert_eq!(tokens[0], Token::BareKey("a"));
        assert_eq!(tokens[1], Token::Newline);
        assert_eq!(tokens[2], Token::BareKey("b"));
    }

    #[test]
    fn test_local_date() {
        let tokens = lex_all("1979-05-27");
        assert_eq!(tokens[0], Token::LocalDateToken(crate::value::LocalDate { year: 1979, month: 5, day: 27 }));
    }

    #[test]
    fn test_local_datetime() {
        let tokens = lex_all("1979-05-27T07:32:00");
        assert_eq!(tokens[0], Token::LocalDateTimeToken(crate::value::LocalDateTime {
            date: crate::value::LocalDate { year: 1979, month: 5, day: 27 },
            time: crate::value::LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 },
        }));
    }

    #[test]
    fn test_offset_datetime() {
        let tokens = lex_all("1979-05-27T07:32:00Z");
        assert_eq!(tokens[0], Token::OffsetDateTimeToken(crate::value::OffsetDateTime {
            date: crate::value::LocalDate { year: 1979, month: 5, day: 27 },
            time: crate::value::LocalTime { hour: 7, minute: 32, second: 0, nanosecond: 0 },
            offset: crate::value::UtcOffset::Z,
        }));
    }

    #[test]
    fn test_local_time() {
        let tokens = lex_all("07:32:00.999999999");
        assert_eq!(tokens[0], Token::LocalTimeToken(crate::value::LocalTime {
            hour: 7, minute: 32, second: 0, nanosecond: 999_999_999,
        }));
    }

    #[test]
    fn test_fractional_truncation() {
        // More than 9 fractional digits should be truncated, not rounded
        let tokens = lex_all("07:32:00.1234567899999");
        assert_eq!(tokens[0], Token::LocalTimeToken(crate::value::LocalTime {
            hour: 7, minute: 32, second: 0, nanosecond: 123_456_789,
        }));
    }

    #[test]
    fn test_peek_token() {
        let mut lexer = Lexer::new("a = 1");
        assert_eq!(*lexer.peek_token().unwrap(), Token::BareKey("a"));
        assert_eq!(*lexer.peek_token().unwrap(), Token::BareKey("a")); // peek again
        assert_eq!(lexer.next_token().unwrap(), Token::BareKey("a")); // consume
        assert_eq!(lexer.next_token().unwrap(), Token::Equals);
    }

    #[test]
    fn test_invalid_escape() {
        let mut lexer = Lexer::new(r#""\q""#);
        assert!(lexer.next_token().is_err());
    }

    #[test]
    fn test_leading_zero_rejected() {
        let mut lexer = Lexer::new("01");
        assert!(lexer.next_token().is_err());
    }

    #[test]
    fn test_ml_basic_string() {
        let tokens = lex_all("\"\"\"hello\nworld\"\"\"");
        assert_eq!(tokens[0], Token::MlBasicString("hello\nworld".to_string()));
    }

    #[test]
    fn test_ml_literal_string() {
        let tokens = lex_all("'''hello\nworld'''");
        assert_eq!(tokens[0], Token::MlLiteralString("hello\nworld"));
    }
}
