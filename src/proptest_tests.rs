// Property-based tests for toml-rust-parser using the `proptest` crate.
// Feature: toml-rust-parser

#[cfg(test)]
mod proptest_tests {
    use crate::value::{LocalDate, LocalDateTime, LocalTime, OffsetDateTime, UtcOffset, Value};
    use crate::{parse, to_toml_string};
    use indexmap::IndexMap;
    use proptest::prelude::*;

    // -----------------------------------------------------------------------
    // Task 12.1 – Strategies
    // -----------------------------------------------------------------------

    /// Generate valid TOML bare-key characters: [A-Za-z0-9_-]
    fn arb_bare_key() -> impl Strategy<Value = String> {
        "[A-Za-z][A-Za-z0-9_-]{0,15}".prop_map(|s| s)
    }

    /// Generate valid TOML string content: printable Unicode, no control chars
    /// (except tab which is allowed), no characters that would break basic strings.
    pub fn arb_string() -> impl Strategy<Value = String> {
        prop::string::string_regex(r"[ -~\t\u{0080}-\u{D7FF}\u{E000}-\u{FFFD}]{0,30}")
            .unwrap()
            .prop_map(|s| s)
    }

    /// Generate i64 values including boundary cases.
    pub fn arb_integer() -> impl Strategy<Value = i64> {
        prop_oneof![
            Just(i64::MIN),
            Just(i64::MAX),
            Just(0i64),
            Just(1i64),
            Just(-1i64),
            any::<i64>(),
        ]
    }

    /// Generate f64 values including special IEEE 754 values.
    pub fn arb_float() -> impl Strategy<Value = f64> {
        prop_oneof![
            Just(f64::INFINITY),
            Just(f64::NEG_INFINITY),
            Just(f64::NAN),
            Just(0.0f64),
            Just(-0.0f64),
            any::<f64>().prop_filter("finite", |f| f.is_finite()),
        ]
    }

    /// Generate valid LocalDate values.
    fn arb_local_date() -> impl Strategy<Value = LocalDate> {
        (1u16..=9999u16, 1u8..=12u8).prop_flat_map(|(year, month)| {
            let max_day = days_in_month(year, month);
            (Just(year), Just(month), 1u8..=max_day)
                .prop_map(|(y, m, d)| LocalDate { year: y, month: m, day: d })
        })
    }

    /// Generate valid LocalTime values.
    fn arb_local_time() -> impl Strategy<Value = LocalTime> {
        (0u8..=23u8, 0u8..=59u8, 0u8..=59u8, 0u32..=999_999_999u32).prop_map(
            |(h, m, s, ns)| LocalTime {
                hour: h,
                minute: m,
                second: s,
                nanosecond: ns,
            },
        )
    }

    /// Generate valid OffsetDateTime, LocalDateTime, LocalDate, or LocalTime values.
    pub fn arb_datetime() -> impl Strategy<Value = Value> {
        prop_oneof![
            // OffsetDateTime
            (arb_local_date(), arb_local_time(), arb_utc_offset()).prop_map(|(date, time, offset)| {
                Value::OffsetDateTime(OffsetDateTime { date, time, offset })
            }),
            // LocalDateTime
            (arb_local_date(), arb_local_time()).prop_map(|(date, time)| {
                Value::LocalDateTime(LocalDateTime { date, time })
            }),
            // LocalDate
            arb_local_date().prop_map(Value::LocalDate),
            // LocalTime
            arb_local_time().prop_map(Value::LocalTime),
        ]
    }

    fn arb_utc_offset() -> impl Strategy<Value = UtcOffset> {
        prop_oneof![
            Just(UtcOffset::Z),
            (-1439i16..=1439i16).prop_map(UtcOffset::Minutes),
        ]
    }

    /// Generate a table with valid bare keys and bounded-depth values.
    pub fn arb_table(depth: u32) -> impl Strategy<Value = IndexMap<String, Value>> {
        prop::collection::hash_map(arb_bare_key(), arb_value_inner(depth), 0..=5).prop_map(
            |hm| {
                let mut map = IndexMap::new();
                for (k, v) in hm {
                    map.insert(k, v);
                }
                map
            },
        )
    }

    /// Inner recursive value generator with depth limit.
    fn arb_value_inner(depth: u32) -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            arb_string().prop_map(Value::String),
            arb_integer().prop_map(Value::Integer),
            arb_float().prop_map(Value::Float),
            any::<bool>().prop_map(Value::Boolean),
            arb_datetime(),
        ];

        if depth == 0 {
            leaf.boxed()
        } else {
            let recurse = prop_oneof![
                // Array of scalars/mixed
                prop::collection::vec(arb_value_inner(depth - 1), 0..=4)
                    .prop_map(Value::Array),
                // Table
                arb_table(depth - 1).prop_map(Value::Table),
            ];
            prop_oneof![leaf, recurse].boxed()
        }
    }

    /// Generate arbitrary bounded-depth Value trees (root is always a Table).
    pub fn arb_value() -> impl Strategy<Value = Value> {
        arb_table(2).prop_map(Value::Table)
    }

    // -----------------------------------------------------------------------
    // Helper: days in month (simplified, ignores leap seconds)
    // -----------------------------------------------------------------------
    fn days_in_month(year: u16, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => 28,
        }
    }

    fn is_leap_year(year: u16) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    // -----------------------------------------------------------------------
    // Helper: normalize NaN for comparison (NaN != NaN in IEEE 754)
    // Also: table comparison is order-insensitive (printer may reorder keys)
    // -----------------------------------------------------------------------
    fn values_equivalent(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Float(fa), Value::Float(fb)) => {
                (fa.is_nan() && fb.is_nan())
                    || (fa == fb)
                    // -0.0 and +0.0 are equal in TOML round-trip
                    || (fa.to_bits() == fb.to_bits())
            }
            (Value::Array(aa), Value::Array(ab)) => {
                aa.len() == ab.len()
                    && aa.iter().zip(ab.iter()).all(|(x, y)| values_equivalent(x, y))
            }
            (Value::Table(ta), Value::Table(tb)) => {
                // Order-insensitive: the printer may reorder keys (AOT entries go last)
                if ta.len() != tb.len() {
                    return false;
                }
                ta.iter().all(|(ka, va)| {
                    tb.get(ka.as_str())
                        .map(|vb| values_equivalent(va, vb))
                        .unwrap_or(false)
                })
            }
            _ => a == b,
        }
    }

    // -----------------------------------------------------------------------
    // Task 12.2 – Property 1: Parse-Print Round Trip
    // Validates: Requirements 4.1, 4.5, 13.1, 13.2, 13.3, 14.4
    // -----------------------------------------------------------------------
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 1: parse-print round trip
        fn prop_round_trip(value in arb_value()) {
            let printed = to_toml_string(&value);
            let reparsed = parse(&printed)
                .expect(&format!("parse failed on printed output:\n{}", printed));
            prop_assert!(
                values_equivalent(&value, &reparsed),
                "round-trip mismatch:\noriginal: {:?}\nprinted:\n{}\nreparsed: {:?}",
                value, printed, reparsed
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 12.3 – Property 2: Whitespace Invariance
    // Validates: Requirements 1.4, 3.6, 10.3
    // -----------------------------------------------------------------------
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 2: whitespace invariance
        fn prop_whitespace_invariance(
            key in arb_bare_key(),
            value in arb_string(),
            spaces_before_eq in 0usize..=4,
            spaces_after_eq in 0usize..=4,
        ) {
            let before = " ".repeat(spaces_before_eq);
            let after = " ".repeat(spaces_after_eq);
            // Canonical form
            let canonical = format!("{} = \"{}\"", key, escape_for_toml(&value));
            // Variant with extra whitespace around =
            let variant = format!("{}{}={}{}", key, before, after, format!("\"{}\"", escape_for_toml(&value)));

            let canonical_result = parse(&canonical);
            let variant_result = parse(&variant);

            match (canonical_result, variant_result) {
                (Ok(cv), Ok(vv)) => {
                    prop_assert!(
                        values_equivalent(&cv, &vv),
                        "whitespace variant produced different value:\ncanonical: {:?}\nvariant: {:?}",
                        cv, vv
                    );
                }
                (Err(_), Err(_)) => {
                    // Both failed – acceptable (e.g. key collision edge case)
                }
                (Ok(_), Err(e)) => {
                    prop_assert!(false, "canonical parsed but variant failed: {}", e);
                }
                (Err(e), Ok(_)) => {
                    prop_assert!(false, "canonical failed but variant parsed: {}", e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 12.3 (continued) – Whitespace around dot in dotted keys
    // -----------------------------------------------------------------------
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 2: whitespace invariance (dotted keys)
        fn prop_whitespace_dotted_key(
            key1 in arb_bare_key(),
            key2 in arb_bare_key(),
            value in arb_string(),
            spaces_around_dot in 0usize..=3,
        ) {
            let sp = " ".repeat(spaces_around_dot);
            let canonical = format!("{}.{} = \"{}\"", key1, key2, escape_for_toml(&value));
            let variant = format!("{}{}{}{}{}= \"{}\"", key1, sp, ".", sp, key2, escape_for_toml(&value));

            let canonical_result = parse(&canonical);
            let variant_result = parse(&variant);

            match (canonical_result, variant_result) {
                (Ok(cv), Ok(vv)) => {
                    prop_assert!(
                        values_equivalent(&cv, &vv),
                        "dotted-key whitespace variant produced different value:\ncanonical: {:?}\nvariant: {:?}",
                        cv, vv
                    );
                }
                (Err(_), Err(_)) => {}
                (Ok(_), Err(e)) => {
                    prop_assert!(false, "canonical parsed but dotted variant failed: {}", e);
                }
                (Err(e), Ok(_)) => {
                    prop_assert!(false, "canonical failed but dotted variant parsed: {}", e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 12.4 – Property 3: LF/CRLF Equivalence
    // Validates: Requirements 1.5
    // -----------------------------------------------------------------------
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 3: LF/CRLF equivalence
        fn prop_lf_crlf_equivalence(value in arb_value()) {
            let lf_doc = to_toml_string(&value);
            // Replace every bare LF (not already preceded by CR) with CRLF
            let crlf_doc = lf_doc.replace('\n', "\r\n");

            let lf_result = parse(&lf_doc);
            let crlf_result = parse(&crlf_doc);

            match (lf_result, crlf_result) {
                (Ok(lv), Ok(cv)) => {
                    prop_assert!(
                        values_equivalent(&lv, &cv),
                        "LF and CRLF produced different values:\nLF doc:\n{}\nCRLF doc:\n{}",
                        lf_doc, crlf_doc
                    );
                }
                (Err(e), _) => {
                    prop_assert!(false, "LF document failed to parse: {}", e);
                }
                (Ok(_), Err(e)) => {
                    prop_assert!(false, "CRLF document failed to parse: {}", e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 12.5 – Property 4: Comments Are Transparent
    // Validates: Requirements 2.1
    // -----------------------------------------------------------------------
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 4: comments are transparent
        fn prop_comments_transparent(
            value in arb_value(),
            // Comment text: printable ASCII only, no control chars
            comment_text in "[!-~][!-~ ]{0,20}",
        ) {
            let base_doc = to_toml_string(&value);

            // Append a comment to the end of each non-empty line
            let commented_doc: String = base_doc
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        line.to_string()
                    } else {
                        format!("{} # {}", line, comment_text)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            // Preserve trailing newline if original had one
            let commented_doc = if base_doc.ends_with('\n') {
                format!("{}\n", commented_doc)
            } else {
                commented_doc
            };

            let base_result = parse(&base_doc);
            let commented_result = parse(&commented_doc);

            match (base_result, commented_result) {
                (Ok(bv), Ok(cv)) => {
                    prop_assert!(
                        values_equivalent(&bv, &cv),
                        "comment changed parse result:\nbase: {:?}\ncommented: {:?}\ndoc:\n{}",
                        bv, cv, commented_doc
                    );
                }
                (Err(e), _) => {
                    prop_assert!(false, "base document failed to parse: {}", e);
                }
                (Ok(_), Err(e)) => {
                    prop_assert!(false, "commented document failed to parse: {}", e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 12.6 – Property 5: Duplicate Definition Rejection
    // Validates: Requirements 3.7, 3.9, 10.4, 10.5
    // -----------------------------------------------------------------------
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 5: duplicate definition rejection (key)
        fn prop_duplicate_key_rejected(
            key in arb_bare_key(),
            val1 in arb_string(),
            val2 in arb_string(),
        ) {
            // Define the same key twice in the root table
            let doc = format!(
                "{} = \"{}\"\n{} = \"{}\"\n",
                key, escape_for_toml(&val1),
                key, escape_for_toml(&val2)
            );
            let result = parse(&doc);
            prop_assert!(
                result.is_err(),
                "expected parse error for duplicate key '{}', but got Ok:\n{}",
                key, doc
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        // Feature: toml-rust-parser, Property 5: duplicate definition rejection (table header)
        fn prop_duplicate_table_header_rejected(
            key in arb_bare_key(),
            val1 in arb_string(),
            val2 in arb_string(),
        ) {
            // Define the same table header twice
            let doc = format!(
                "[{}]\nx = \"{}\"\n\n[{}]\ny = \"{}\"\n",
                key, escape_for_toml(&val1),
                key, escape_for_toml(&val2)
            );
            let result = parse(&doc);
            prop_assert!(
                result.is_err(),
                "expected parse error for duplicate table header '[{}]', but got Ok:\n{}",
                key, doc
            );
        }
    }

    // -----------------------------------------------------------------------
    // Helper: escape a string for embedding in a TOML basic string literal
    // -----------------------------------------------------------------------
    fn escape_for_toml(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '\x08' => out.push_str("\\b"),
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                '\x0C' => out.push_str("\\f"),
                '\r' => out.push_str("\\r"),
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                    out.push_str(&format!("\\u{:04X}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }
}
