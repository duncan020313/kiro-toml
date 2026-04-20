# Implementation Plan: toml-rust-parser

## Overview

Implement a pure-Rust TOML v1.0.0 parser library as a single crate with modules for lexing, parsing, pretty-printing, and error handling. Tasks are ordered so each step compiles and is wired into the public API before moving on. Property-based tests use `proptest` and correspond directly to the 12 correctness properties in the design document.

## Tasks

- [x] 1. Scaffold the crate
  - Create `Cargo.toml` with `[lib]` target, add `indexmap` and `proptest` (dev) dependencies
  - Create `src/lib.rs` with `pub mod` declarations for `value`, `error`, `lexer`, `parser`, `printer`
  - Stub each module file (`value.rs`, `error.rs`, `lexer.rs`, `parser.rs`, `printer.rs`) with empty `// TODO` placeholders so the crate compiles
  - Expose `pub use` re-exports for `parse`, `to_toml_string`, `Value`, `ParseError` in `lib.rs`
  - _Requirements: 13.1, 15.1_

- [x] 2. Implement `Value` enum and date/time types (`value.rs`)
  - [x] 2.1 Define `UtcOffset`, `LocalDate`, `LocalTime`, `OffsetDateTime`, `LocalDateTime` structs with `Debug`, `Clone`, `PartialEq`, `Eq` derives
  - [x] 2.2 Define the `Value` enum with all ten variants using `IndexMap<String, Value>` for `Table`
  - _Requirements: 13.1, 13.2, 8.1–8.4_

- [x] 3. Implement `ParseError` (`error.rs`)
  - [x] 3.1 Define `ParseError` struct with `message: String`, `line: u32`, `col: u32`; derive `Debug`, `Clone`, `PartialEq`
  - [x] 3.2 Implement `std::fmt::Display` and `std::error::Error` for `ParseError`
  - [x] 3.3 Add a `ParseError::new(message, line, col)` constructor helper
  - _Requirements: 15.1, 15.2, 15.3, 15.4_

- [-] 4. Implement the lexer (`lexer.rs`)$  - [-] 4.1 Define the `Token<'a>` enum with all variants listed in the design (structural tokens, `BareKey`, string variants, `Integer`, `Float`, `Boolean`, date/time tokens)
  - [x] 4.2 Implement `Lexer::new`, position/line/col tracking, and the `next_token` dispatch loop (whitespace skip, first-character dispatch)
  - [x] 4.3 Implement single-character structural tokens (`=`, `.`, `,`, `{`, `}`) and bracket disambiguation (`[` vs `[[`, `]` vs `]]`)
  - [x] 4.4 Implement newline handling (LF and CRLF) and comment skipping (`#` to end of line); validate no control characters in comments
    - _Requirements: 1.5, 2.1, 2.2_
  - [x] 4.5 Implement bare-key tokenization (`[A-Za-z0-9_-]+`)
    - _Requirements: 3.1_
  - [x] 4.6 Implement basic string lexing: escape sequence processing (`\b \t \n \f \r \" \\ \uXXXX \UXXXXXXXX`), control-character rejection, return `Token::BasicString(String)`
    - _Requirements: 4.1, 4.2, 4.3, 4.4_
  - [x] 4.7 Implement literal string lexing: return raw `&str` slice, reject control characters except tab
    - _Requirements: 4.8, 4.9_
  - [x] 4.8 Implement multi-line basic string lexing: trim leading newline, line-ending backslash trimming, allow up to two unescaped quotes before closing `"""`
    - _Requirements: 4.5, 4.6, 4.7_
  - [x] 4.9 Implement multi-line literal string lexing: trim leading newline, allow up to two unescaped single quotes before closing `'''`, reject invalid control characters
    - _Requirements: 4.10, 4.11, 4.12_
  - [x] 4.10 Implement integer tokenization: decimal (with optional sign, leading-zero rejection, underscore rules), hex (`0x`), octal (`0o`), binary (`0b`); reject `+` prefix on non-decimal; parse into `i64` with overflow check
    - _Requirements: 5.1–5.11_
  - [x] 4.11 Implement float tokenization: integer part + optional fractional + optional exponent, underscore rules, special values (`inf`, `+inf`, `-inf`, `nan`, `+nan`, `-nan`), parse into `f64`
    - _Requirements: 6.1–6.7_
  - [x] 4.12 Implement boolean tokenization (`true` / `false` exact match)
    - _Requirements: 7.1, 7.2, 7.3_
  - [x] 4.13 Implement date/time disambiguation: detect `DDDD-DD` pattern, read ahead to classify as `OffsetDateTime`, `LocalDateTime`, `LocalDate`, or `LocalTime`; validate field ranges; truncate (not round) fractional seconds beyond 9 digits
    - _Requirements: 8.1–8.6_
  - [x] 4.14 Implement `Lexer::peek_token` using a one-token lookahead buffer
  - [ ]* 4.15 Write unit tests for the lexer covering all token types, escape sequences, numeric edge cases, date/time disambiguation, and error paths
    - _Requirements: 4.1–4.12, 5.1–5.11, 6.1–6.7, 7.1–7.3, 8.1–8.6_

- [x] 5. Checkpoint — ensure the crate compiles and lexer unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Implement the parser — core infrastructure (`parser.rs`)
  - [x] 6.1 Define `TrackedValue`, `CurrentTableKind`, and `TableTracker` structs/enums as described in the design
  - [x] 6.2 Implement `TableTracker` key-insertion logic: single-segment insert with duplicate detection, multi-segment dotted-key walk with implicit-table creation
    - _Requirements: 3.5, 3.7, 3.8_
  - [x] 6.3 Implement `TableTracker` standard-table header processing: path walk, implicit-table promotion, duplicate-header rejection, inline-table extension rejection
    - _Requirements: 10.1, 10.4, 10.5, 10.6, 10.7, 11.5_
  - [x] 6.4 Implement `TableTracker` array-of-tables header processing: path walk, append semantics, static-array conflict rejection, standard-table conflict rejection
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5_
  - [x] 6.5 Implement `Parser::new` and the top-level `parse_document` loop (skip newlines, dispatch on `Eof` / `LBracket` / `DoubleLBracket` / keyval)
    - _Requirements: 10.8_
  - [x] 6.6 Implement `parse_key` (bare and quoted simple keys, dotted key accumulation, whitespace around `.`)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.6, 3.9, 3.10_
  - [x] 6.7 Implement `parse_value` dispatch table (all token → `Value` variant mappings)
    - _Requirements: 13.1_
  - [x] 6.8 Implement `parse_array`: collect elements, allow trailing comma, allow newlines/comments between elements
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_
  - [x] 6.9 Implement `parse_inline_table`: collect key/value pairs, reject trailing comma, reject newlines, mark result as closed/immutable
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5, 11.6_
  - [x] 6.10 Implement `parse_table_header` and `parse_aot_header` (consume brackets, parse key, delegate to `TableTracker`)
    - _Requirements: 10.1, 10.2, 10.3, 12.1_
  - [x] 6.11 Implement `parse_keyval` (parse key, consume `=`, parse value, require newline or EOF after value)
    - _Requirements: 1.6_
  - [x] 6.12 Wire `Parser::parse` to call `parse_document` and convert the `TableTracker` root into a `Value::Table`
    - _Requirements: 13.2_
  - [x] 6.13 Expose `pub fn parse(input: &str) -> Result<Value, ParseError>` in `lib.rs`
    - _Requirements: 1.1_

- [x] 7. Checkpoint — ensure the crate compiles and basic parse smoke tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [~] 8. Write parser unit tests
  - [x] 8.1 Write unit tests for all string types: basic, literal, multi-line basic, multi-line literal; include escape sequences, control-character rejection, line-ending backslash, quote-within-multiline edge cases
    - _Requirements: 4.1–4.12_
  - [x] 8.2 Write unit tests for integer parsing: decimal, hex, octal, binary, sign, leading zeros, underscores, overflow, invalid underscore placement
    - _Requirements: 5.1–5.11_
  - [x] 8.3 Write unit tests for float parsing: fractional, exponent, combined, underscores, special values, `-0.0`, `+0.0`
    - _Requirements: 6.1–6.7_
  - [-] 8.4 Write unit tests for boolean parsing: `true`, `false`, case-sensitivity rejection
    - _Requirements: 7.1–7.3_
  - [~] 8.5 Write unit tests for all four date/time types: valid formats, space-separator, fractional seconds, truncation, invalid field ranges
    - _Requirements: 8.1–8.6_
  - [~] 8.6 Write unit tests for arrays: empty, mixed types, trailing comma, nested arrays, multi-line with comments
    - _Requirements: 9.1–9.5_
  - [~] 8.7 Write unit tests for standard tables: basic header, dotted header, whitespace in header, super-table after sub-table, duplicate header rejection, implicit-table promotion
    - _Requirements: 10.1–10.9_
  - [~] 8.8 Write unit tests for inline tables: valid, trailing-comma rejection, newline rejection, duplicate key rejection, extension rejection
    - _Requirements: 11.1–11.6_
  - [~] 8.9 Write unit tests for array of tables: basic append, nested AOT, static-array conflict, standard-table conflict, sub-table under AOT element
    - _Requirements: 12.1–12.8_
  - [~] 8.10 Write unit tests for key rules: bare keys, quoted keys, empty quoted key, dotted keys, whitespace around dot, key/quoted-key equivalence, digit-only dotted key
    - _Requirements: 3.1–3.10_
  - [~] 8.11 Write unit tests for error reporting: verify `ParseError` carries correct `line` and `col` for representative error cases
    - _Requirements: 15.1–15.5_

- [~] 9. Checkpoint — ensure all parser unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [~] 10. Implement the pretty printer (`printer.rs`)
  - [~] 10.1 Implement `PrettyPrinter::new` and the `print_value_inline` method for all scalar types and inline arrays/tables
    - _Requirements: 14.1_
  - [~] 10.2 Implement key formatting: bare key when `[A-Za-z0-9_-]` only, otherwise basic-string quoted
    - _Requirements: 14.1_
  - [~] 10.3 Implement `print_table` two-pass strategy: Phase 1 emits scalars and inline-eligible values; Phase 2 emits sub-table headers recursively; Phase 3 emits array-of-tables headers recursively
    - _Requirements: 14.2, 14.3_
  - [~] 10.4 Implement `PrettyPrinter::print` (entry point) and expose `pub fn to_toml_string(value: &Value) -> String` in `lib.rs`
    - _Requirements: 14.1, 14.4_
  - [ ]* 10.5 Write unit tests for the pretty printer: scalars, nested tables, arrays of tables, inline tables inside arrays, special float values, date/time formatting
    - _Requirements: 14.1–14.4_

- [~] 11. Checkpoint — ensure pretty printer unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [~] 12. Write property-based tests (`proptest`)
  - [~] 12.1 Implement `proptest` strategies: `arb_string`, `arb_integer`, `arb_float`, `arb_datetime`, `arb_table`, and `arb_value` (bounded depth)
  - [~] 12.2 Write property test for Property 1: parse-print round trip
    - **Property 1: Parse-Print Round Trip**
    - **Validates: Requirements 4.1, 4.5, 13.1, 13.2, 13.3, 14.4**
  - [~] 12.3 Write property test for Property 2: whitespace invariance
    - **Property 2: Whitespace Invariance**
    - **Validates: Requirements 1.4, 3.6, 10.3**
  - [~] 12.4 Write property test for Property 3: LF/CRLF equivalence
    - **Property 3: LF/CRLF Equivalence**
    - **Validates: Requirements 1.5**
  - [~] 12.5 Write property test for Property 4: comments are transparent
    - **Property 4: Comments Are Transparent**
    - **Validates: Requirements 2.1**
  - [~] 12.6 Write property test for Property 5: duplicate definition rejection
    - **Property 5: Duplicate Definition Rejection**
    - **Validates: Requirements 3.7, 3.9, 10.4, 10.5**
  - [ ]* 12.7 Write property test for Property 6: invalid escape rejection
    - **Property 6: Invalid Escape Rejection**
    - **Validates: Requirements 4.2, 4.3**
  - [ ]* 12.8 Write property test for Property 7: integer overflow rejection
    - **Property 7: Integer Overflow Rejection**
    - **Validates: Requirements 5.11**
  - [ ]* 12.9 Write property test for Property 8: underscore separator invariance
    - **Property 8: Underscore Separator Invariance**
    - **Validates: Requirements 5.8, 6.4**
  - [ ]* 12.10 Write property test for Property 9: inline table immutability
    - **Property 9: Inline Table Immutability**
    - **Validates: Requirements 11.5, 11.6**
  - [ ]* 12.11 Write property test for Property 10: array of tables append semantics
    - **Property 10: Array of Tables Append Semantics**
    - **Validates: Requirements 12.1, 12.2, 13.3**
  - [ ]* 12.12 Write property test for Property 11: static array append rejection
    - **Property 11: Static Array Append Rejection**
    - **Validates: Requirements 12.5**
  - [ ]* 12.13 Write property test for Property 12: fractional second truncation
    - **Property 12: Fractional Second Truncation**
    - **Validates: Requirements 8.6**

- [~] 13. Write integration tests (`tests/`)
  - [~] 13.1 Create `tests/integration.rs`; add valid TOML round-trip integration tests covering all major constructs (strings, numbers, booleans, dates, arrays, tables, AOT, inline tables, dotted keys)
    - _Requirements: 1.1, 13.1, 13.2, 13.3, 14.4_
  - [ ]* 13.2 Add integration tests for invalid TOML inputs asserting `parse` returns `Err` with a non-empty message for each error category in the design (encoding, duplicate key, invalid escape, overflow, invalid date, structural violation, trailing content)
    - _Requirements: 15.1, 15.2, 15.3_

- [~] 14. Final checkpoint — ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for a faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation at each major phase boundary
- Property tests (12.2–12.13) require the `arb_*` strategies from task 12.1 to be implemented first
- The `proptest` crate must be listed under `[dev-dependencies]` in `Cargo.toml`
- Each property test must be annotated with `// Feature: toml-rust-parser, Property N: <title>` and configured with `ProptestConfig::with_cases(256)`
