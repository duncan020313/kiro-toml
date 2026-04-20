#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl ParseError {
    pub fn new(message: impl Into<String>, line: u32, col: u32) -> Self {
        ParseError { message: message.into(), line, col }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at line {}, col {}", self.message, self.line, self.col)
    }
}

impl std::error::Error for ParseError {}
