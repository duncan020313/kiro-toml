pub mod error;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod value;

#[cfg(test)]
mod proptest_tests;

pub use error::ParseError;
pub use value::Value;

/// Parse a TOML document string into a Value tree.
pub fn parse(input: &str) -> Result<Value, ParseError> {
    parser::Parser::new(input).parse()
}

/// Serialize a Value tree into a TOML document string.
pub fn to_toml_string(value: &Value) -> String {
    printer::PrettyPrinter::print(value)
}
