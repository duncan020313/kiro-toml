# Requirements Document

## Introduction

This document specifies the requirements for a TOML v1.0.0 parser library implemented in Rust. The library parses UTF-8 encoded TOML documents into a Rust value representation, supports all TOML value types and structural constructs defined in the TOML v1.0.0 specification, and produces clear, actionable error messages for invalid input. The library is designed to be serde-compatible and map TOML documents unambiguously to Rust data structures.

## Glossary

- **Parser**: The component that accepts a UTF-8 string and produces a `Value` tree or a `ParseError`.
- **Value**: The Rust enum representing any TOML value (String, Integer, Float, Boolean, OffsetDateTime, LocalDateTime, LocalDate, LocalTime, Array, Table).
- **Table**: A TOML table represented as an ordered map from `String` keys to `Value`.
- **Array**: A TOML array represented as a `Vec<Value>`.
- **ArrayOfTables**: A sequence of tables introduced by `[[header]]` syntax.
- **InlineTable**: A table expressed on a single line within `{ }` braces.
- **Key**: A bare, quoted, or dotted key string used to identify a value within a table.
- **BareKey**: A key composed solely of ASCII letters, digits, underscores, and dashes.
- **QuotedKey**: A key expressed as a basic string (`"..."`) or literal string (`'...'`).
- **DottedKey**: A sequence of bare or quoted keys joined by `.` that implicitly creates nested tables.
- **BasicString**: A string delimited by `"` that supports escape sequences.
- **MultiLineBasicString**: A string delimited by `"""` that allows newlines and escape sequences.
- **LiteralString**: A string delimited by `'` with no escape processing.
- **MultiLineLiteralString**: A string delimited by `'''` that allows newlines with no escape processing.
- **OffsetDateTime**: An RFC 3339 date-time value with a UTC offset.
- **LocalDateTime**: An RFC 3339 date-time value without a UTC offset.
- **LocalDate**: An RFC 3339 date value (date portion only).
- **LocalTime**: An RFC 3339 time value (time portion only).
- **ParseError**: A structured error type returned when the Parser encounters invalid TOML input.
- **PrettyPrinter**: The component that serializes a `Value` tree back into a valid TOML document string.
- **Document**: A complete TOML file or string being parsed.

---

## Requirements

### Requirement 1: Document Encoding and Basic Structure

**User Story:** As a developer, I want the parser to accept only valid UTF-8 TOML documents, so that I can rely on well-defined input handling.

#### Acceptance Criteria

1. THE Parser SHALL accept a `&str` or `String` input representing a TOML Document.
2. WHEN the input contains bytes that are not valid UTF-8, THE Parser SHALL return a ParseError describing the encoding violation.
3. THE Parser SHALL treat the input as case-sensitive for all keys and values.
4. THE Parser SHALL treat tab (U+0009) and space (U+0020) as whitespace and ignore them around keys, values, and headers.
5. THE Parser SHALL treat LF (U+000A) and CRLF (U+000D U+000A) as valid newline sequences.
6. WHEN a key/value pair is not followed by a newline or EOF, THE Parser SHALL return a ParseError.

---

### Requirement 2: Comments

**User Story:** As a developer, I want comments to be ignored during parsing, so that I can annotate TOML files without affecting the parsed output.

#### Acceptance Criteria

1. WHEN a `#` character is encountered outside a string value, THE Parser SHALL treat all subsequent characters on that line as a comment and ignore them.
2. WHEN a comment contains a control character other than tab (U+0000–U+0008, U+000A–U+001F, U+007F), THE Parser SHALL return a ParseError.

---

### Requirement 3: Keys

**User Story:** As a developer, I want the parser to support bare, quoted, and dotted keys, so that I can use a wide range of key names in TOML documents.

#### Acceptance Criteria

1. THE Parser SHALL accept BareKeys composed of characters in `[A-Za-z0-9_-]`.
2. THE Parser SHALL accept QuotedKeys delimited by `"` (basic string rules) or `'` (literal string rules).
3. WHEN a bare key is empty (i.e., `= value` with no key), THE Parser SHALL return a ParseError.
4. THE Parser SHALL accept an empty QuotedKey (`""` or `''`) as a valid key.
5. THE Parser SHALL accept DottedKeys and create nested Table entries for each dot-separated segment.
6. THE Parser SHALL ignore whitespace around the `.` separator in DottedKeys.
7. WHEN a key is defined more than once in the same table scope, THE Parser SHALL return a ParseError.
8. WHEN a DottedKey attempts to redefine a key that was previously assigned a non-table value, THE Parser SHALL return a ParseError.
9. THE Parser SHALL treat a BareKey and its quoted equivalent as identical (e.g., `key` and `"key"` refer to the same entry).
10. WHEN a DottedKey consists entirely of ASCII digit segments (e.g., `3.14159`), THE Parser SHALL parse it as nested string-keyed tables rather than a float literal.

---

### Requirement 4: String Values

**User Story:** As a developer, I want the parser to support all four TOML string types, so that I can represent any text content in a TOML document.

#### Acceptance Criteria

1. THE Parser SHALL parse BasicStrings delimited by `"` and process all defined escape sequences (`\b`, `\t`, `\n`, `\f`, `\r`, `\"`, `\\`, `\uXXXX`, `\UXXXXXXXX`).
2. WHEN a BasicString contains an unrecognized escape sequence, THE Parser SHALL return a ParseError.
3. WHEN a `\uXXXX` or `\UXXXXXXXX` escape encodes a value that is not a valid Unicode scalar value, THE Parser SHALL return a ParseError.
4. WHEN a BasicString contains a control character other than tab (U+0000–U+0008, U+000A–U+001F, U+007F) that is not escaped, THE Parser SHALL return a ParseError.
5. THE Parser SHALL parse MultiLineBasicStrings delimited by `"""`, trim a single leading newline immediately after the opening delimiter, and process all escape sequences valid for BasicStrings.
6. WHEN a MultiLineBasicString uses a line-ending backslash, THE Parser SHALL trim the backslash and all subsequent whitespace and newlines up to the next non-whitespace character.
7. THE Parser SHALL allow one or two unescaped quotation marks to appear anywhere inside a MultiLineBasicString, including immediately before the closing `"""` delimiter.
8. THE Parser SHALL parse LiteralStrings delimited by `'` with no escape processing.
9. WHEN a LiteralString contains a control character other than tab, THE Parser SHALL return a ParseError.
10. THE Parser SHALL parse MultiLineLiteralStrings delimited by `'''`, trim a single leading newline immediately after the opening delimiter, and apply no escape processing.
11. THE Parser SHALL allow one or two unescaped single quotes to appear anywhere inside a MultiLineLiteralString, including immediately before the closing `'''` delimiter.
12. WHEN a MultiLineLiteralString contains a control character other than tab, LF, or CR, THE Parser SHALL return a ParseError.

---

### Requirement 5: Integer Values

**User Story:** As a developer, I want the parser to support decimal, hexadecimal, octal, and binary integer literals, so that I can express numeric values in the most readable form.

#### Acceptance Criteria

1. THE Parser SHALL parse decimal integers with an optional leading `+` or `-` sign.
2. THE Parser SHALL reject decimal integers with leading zeros (except the literal `0`, `+0`, and `-0`).
3. THE Parser SHALL parse hexadecimal integers prefixed with `0x`, treating hex digits as case-insensitive.
4. THE Parser SHALL parse octal integers prefixed with `0o`.
5. THE Parser SHALL parse binary integers prefixed with `0b`.
6. WHEN a non-decimal integer literal (hex, octal, binary) is prefixed with `+`, THE Parser SHALL return a ParseError.
7. THE Parser SHALL allow leading zeros in the digit portion of non-decimal integer literals (after the prefix).
8. THE Parser SHALL accept underscores between digits as visual separators and ignore them in the numeric value.
9. WHEN an underscore is adjacent to the prefix, at the start, or at the end of the digit sequence, THE Parser SHALL return a ParseError.
10. THE Parser SHALL represent all integer values as Rust `i64`.
11. WHEN an integer value is outside the range −2^63 to 2^63−1, THE Parser SHALL return a ParseError.

---

### Requirement 6: Float Values

**User Story:** As a developer, I want the parser to support IEEE 754 binary64 float literals including special values, so that I can represent floating-point numbers accurately.

#### Acceptance Criteria

1. THE Parser SHALL parse floats consisting of an integer part optionally followed by a fractional part (`.` followed by one or more digits) and/or an exponent part (`e` or `E` followed by an optional sign and digits).
2. WHEN a float has a decimal point, THE Parser SHALL require at least one digit on each side of the decimal point.
3. WHEN a fractional part and an exponent part are both present, THE Parser SHALL require the fractional part to precede the exponent part.
4. THE Parser SHALL accept underscores between digits in floats under the same rules as integers.
5. THE Parser SHALL parse the special values `inf`, `+inf`, `-inf`, `nan`, `+nan`, and `-nan` as IEEE 754 infinity and NaN respectively.
6. THE Parser SHALL accept `-0.0` and `+0.0` as valid float values mapping to their IEEE 754 representations.
7. THE Parser SHALL represent all float values as Rust `f64`.

---

### Requirement 7: Boolean Values

**User Story:** As a developer, I want the parser to support boolean literals, so that I can represent true/false flags in TOML documents.

#### Acceptance Criteria

1. THE Parser SHALL parse the lowercase token `true` as a boolean `true` value.
2. THE Parser SHALL parse the lowercase token `false` as a boolean `false` value.
3. WHEN a boolean token is not exactly `true` or `false` (e.g., `True`, `TRUE`), THE Parser SHALL return a ParseError.

---

### Requirement 8: Date and Time Values

**User Story:** As a developer, I want the parser to support all four RFC 3339 date/time types, so that I can represent temporal values unambiguously in TOML documents.

#### Acceptance Criteria

1. THE Parser SHALL parse OffsetDateTime values in RFC 3339 format (`YYYY-MM-DDTHH:MM:SS[.frac]Z` or `±HH:MM` offset), accepting a space character in place of the `T` delimiter.
2. THE Parser SHALL parse LocalDateTime values in RFC 3339 format without an offset (`YYYY-MM-DDTHH:MM:SS[.frac]`), accepting a space character in place of the `T` delimiter.
3. THE Parser SHALL parse LocalDate values as `YYYY-MM-DD`.
4. THE Parser SHALL parse LocalTime values as `HH:MM:SS[.frac]`.
5. THE Parser SHALL support at least millisecond precision (3 fractional second digits) for all date-time and time types.
6. WHEN a date-time value contains fractional seconds with more precision than the implementation supports, THE Parser SHALL truncate (not round) the excess digits.
7. WHEN a date or time value does not conform to RFC 3339 format, THE Parser SHALL return a ParseError.

---

### Requirement 9: Array Values

**User Story:** As a developer, I want the parser to support arrays of mixed types with optional trailing commas and multi-line formatting, so that I can express lists of values flexibly.

#### Acceptance Criteria

1. THE Parser SHALL parse arrays delimited by `[` and `]` containing zero or more comma-separated values of any TOML type.
2. THE Parser SHALL allow values of different types within the same array.
3. THE Parser SHALL allow a trailing comma after the last element of an array.
4. THE Parser SHALL allow newlines and comments between array elements, commas, and the closing bracket.
5. THE Parser SHALL allow arrays to be nested (arrays of arrays).

---

### Requirement 10: Standard Tables

**User Story:** As a developer, I want the parser to support standard table headers, so that I can organize key/value pairs into named sections.

#### Acceptance Criteria

1. THE Parser SHALL parse a table header of the form `[key]` and associate all subsequent key/value pairs (until the next header or EOF) with that table.
2. THE Parser SHALL support dotted keys and quoted keys in table headers to define nested tables.
3. THE Parser SHALL ignore whitespace around the key inside table header brackets.
4. WHEN a table header is defined more than once, THE Parser SHALL return a ParseError.
5. WHEN a `[table]` header attempts to redefine a table that was already explicitly defined via a prior `[table]` header or direct key assignment, THE Parser SHALL return a ParseError.
6. THE Parser SHALL allow a `[table]` header to define or extend a table that was previously created implicitly via DottedKeys, provided no key within that table has been directly assigned.
7. THE Parser SHALL allow a `[table]` header to add sub-tables to a table previously created implicitly via DottedKeys.
8. THE Parser SHALL parse the root (top-level) table as the implicit table containing all key/value pairs before the first header.
9. THE Parser SHALL allow super-tables to be defined after their sub-tables (e.g., `[x]` appearing after `[x.y.z.w]`).

---

### Requirement 11: Inline Tables

**User Story:** As a developer, I want the parser to support inline tables, so that I can express compact grouped data on a single line.

#### Acceptance Criteria

1. THE Parser SHALL parse inline tables delimited by `{` and `}` containing zero or more comma-separated key/value pairs.
2. THE Parser SHALL reject a trailing comma after the last key/value pair in an inline table.
3. THE Parser SHALL reject newlines within an inline table (outside of string values).
4. WHEN a key is defined more than once within an inline table, THE Parser SHALL return a ParseError.
5. WHEN an attempt is made to add keys or sub-tables to an inline table outside its braces (e.g., via a subsequent `[table]` header or dotted key), THE Parser SHALL return a ParseError.
6. WHEN an attempt is made to redefine an already-defined table as an inline table, THE Parser SHALL return a ParseError.

---

### Requirement 12: Array of Tables

**User Story:** As a developer, I want the parser to support array-of-tables headers, so that I can express sequences of structured records in TOML documents.

#### Acceptance Criteria

1. THE Parser SHALL parse array-of-tables headers of the form `[[key]]` and append a new table element to the named array on each occurrence.
2. THE Parser SHALL associate all key/value pairs following a `[[key]]` header (until the next header or EOF) with the most recently appended table element.
3. WHEN a `[table]` header uses the same name as an already-established array of tables, THE Parser SHALL return a ParseError.
4. WHEN a `[[array]]` header uses the same name as an already-defined standard table, THE Parser SHALL return a ParseError.
5. WHEN an attempt is made to append to an array that was defined as a static array value (e.g., `fruits = []`), THE Parser SHALL return a ParseError.
6. THE Parser SHALL support nested arrays of tables (e.g., `[[fruits.varieties]]` nested under `[[fruits]]`).
7. WHEN a sub-table or sub-array-of-tables header references a parent array element that has not yet been defined, THE Parser SHALL return a ParseError.
8. WHEN a `[table]` header or `[[array]]` header conflicts with a previously defined `[[array]]` element's sub-table or sub-array, THE Parser SHALL return a ParseError.

---

### Requirement 13: Value Representation (Rust Data Model)

**User Story:** As a developer, I want the parsed TOML document to be represented as a well-typed Rust enum, so that I can work with TOML values idiomatically in Rust code.

#### Acceptance Criteria

1. THE Parser SHALL produce a `Value` enum with variants: `String(String)`, `Integer(i64)`, `Float(f64)`, `Boolean(bool)`, `OffsetDateTime(...)`, `LocalDateTime(...)`, `LocalDate(...)`, `LocalTime(...)`, `Array(Vec<Value>)`, `Table(IndexMap<String, Value>)`.
2. THE Parser SHALL represent TOML Tables (including the root table) as `Table(IndexMap<String, Value>)` preserving insertion order.
3. THE Parser SHALL represent TOML Arrays of Tables as `Array(Vec<Value>)` where each element is a `Table` variant.

---

### Requirement 14: Pretty Printer (Round-Trip)

**User Story:** As a developer, I want to serialize a `Value` tree back into a valid TOML string, so that I can programmatically generate or transform TOML documents.

#### Acceptance Criteria

1. THE PrettyPrinter SHALL format any `Value` tree into a valid TOML Document string.
2. THE PrettyPrinter SHALL emit standard table headers (`[key]`) for nested `Table` values and array-of-tables headers (`[[key]]`) for arrays of tables at the top level.
3. THE PrettyPrinter SHALL emit inline tables for `Table` values that appear as array elements or as values nested within another inline context where a header is not applicable.
4. FOR ALL valid `Value` trees, parsing the output of the PrettyPrinter SHALL produce a `Value` tree equivalent to the original (round-trip property).

---

### Requirement 15: Error Reporting

**User Story:** As a developer, I want the parser to produce descriptive error messages with source location information, so that I can quickly diagnose and fix invalid TOML documents.

#### Acceptance Criteria

1. THE Parser SHALL return a `ParseError` value (not panic) for all invalid TOML inputs.
2. THE ParseError SHALL include the line number and column number of the first invalid character or token.
3. THE ParseError SHALL include a human-readable description of the error (e.g., "duplicate key 'name' at line 3, column 1").
4. THE ParseError SHALL implement the standard Rust `std::error::Error` trait.
5. WHEN multiple errors could be reported, THE Parser SHALL report the first error encountered in document order.
