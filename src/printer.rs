use crate::value::{LocalTime, OffsetDateTime, UtcOffset, Value};
use indexmap::IndexMap;

pub struct PrettyPrinter {
    output: String,
}

impl PrettyPrinter {
    pub fn new() -> Self {
        PrettyPrinter {
            output: String::new(),
        }
    }

    /// Entry point: serialize a Value (must be a Table at root) into a TOML string.
    pub fn print(value: &Value) -> String {
        let mut printer = PrettyPrinter::new();
        if let Value::Table(table) = value {
            printer.print_table(table, &[]);
        }
        printer.output
    }

    // -------------------------------------------------------------------------
    // Key formatting
    // -------------------------------------------------------------------------

    /// Format a single key segment: bare if all chars are [A-Za-z0-9_-], else quoted.
    fn format_key(key: &str) -> String {
        if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !key.is_empty() {
            key.to_string()
        } else {
            format!("\"{}\"", escape_string(key))
        }
    }

    /// Format a dotted path as a TOML key path (e.g. `a.b."c d"`).
    fn format_path(path: &[String]) -> String {
        path.iter()
            .map(|s| Self::format_key(s))
            .collect::<Vec<_>>()
            .join(".")
    }

    // -------------------------------------------------------------------------
    // Inline value formatting
    // -------------------------------------------------------------------------

    /// Emit a value inline (used for scalars, inline arrays, inline tables).
    fn print_value_inline(&mut self, value: &Value) {
        match value {
            Value::String(s) => {
                self.output.push('"');
                self.output.push_str(&escape_string(s));
                self.output.push('"');
            }
            Value::Integer(i) => {
                self.output.push_str(&i.to_string());
            }
            Value::Float(f) => {
                self.output.push_str(&format_float(*f));
            }
            Value::Boolean(b) => {
                self.output.push_str(if *b { "true" } else { "false" });
            }
            Value::OffsetDateTime(dt) => {
                self.output.push_str(&format_offset_datetime(dt));
            }
            Value::LocalDateTime(dt) => {
                let date = &dt.date;
                let time = &dt.time;
                self.output.push_str(&format!(
                    "{:04}-{:02}-{:02}T{}",
                    date.year, date.month, date.day,
                    format_time(time)
                ));
            }
            Value::LocalDate(d) => {
                self.output.push_str(&format!("{:04}-{:02}-{:02}", d.year, d.month, d.day));
            }
            Value::LocalTime(t) => {
                self.output.push_str(&format_time(t));
            }
            Value::Array(arr) => {
                self.output.push_str("[ ");
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.print_value_inline(v);
                }
                self.output.push_str(" ]");
            }
            Value::Table(table) => {
                self.output.push_str("{ ");
                let mut first = true;
                for (k, v) in table {
                    if !first {
                        self.output.push_str(", ");
                    }
                    first = false;
                    self.output.push_str(&Self::format_key(k));
                    self.output.push_str(" = ");
                    self.print_value_inline(v);
                }
                self.output.push_str(" }");
            }
        }
    }

    // -------------------------------------------------------------------------
    // Table printing (three-phase strategy)
    // -------------------------------------------------------------------------

    fn print_table(&mut self, table: &IndexMap<String, Value>, path: &[String]) {
        // Phase 1: emit scalars, inline arrays, and inline-eligible values
        for (key, value) in table {
            if is_inline_value(value) {
                self.output.push_str(&Self::format_key(key));
                self.output.push_str(" = ");
                self.print_value_inline(value);
                self.output.push('\n');
            }
        }

        // Phase 2: emit sub-tables
        for (key, value) in table {
            if let Value::Table(sub_table) = value {
                let mut new_path = path.to_vec();
                new_path.push(key.clone());
                // Add blank line before table header for readability (except at very start)
                if !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                self.output.push('[');
                self.output.push_str(&Self::format_path(&new_path));
                self.output.push_str("]\n");
                self.print_table(sub_table, &new_path);
            }
        }

        // Phase 3: emit arrays of tables
        for (key, value) in table {
            if let Value::Array(arr) = value {
                if is_array_of_tables(arr) {
                    let mut new_path = path.to_vec();
                    new_path.push(key.clone());
                    for element in arr {
                        if let Value::Table(elem_table) = element {
                            if !self.output.is_empty() && !self.output.ends_with("\n\n") {
                                self.output.push('\n');
                            }
                            self.output.push_str("[[");
                            self.output.push_str(&Self::format_path(&new_path));
                            self.output.push_str("]]\n");
                            self.print_table(elem_table, &new_path);
                        }
                    }
                }
            }
        }
    }
}

// -------------------------------------------------------------------------
// Helper predicates
// -------------------------------------------------------------------------

/// Returns true if the value should be emitted inline (Phase 1).
fn is_inline_value(value: &Value) -> bool {
    match value {
        Value::Table(_) => false,
        Value::Array(arr) => !is_array_of_tables(arr),
        _ => true,
    }
}

/// Returns true if the array is non-empty and all elements are Tables.
fn is_array_of_tables(arr: &[Value]) -> bool {
    !arr.is_empty() && arr.iter().all(|v| matches!(v, Value::Table(_)))
}

// -------------------------------------------------------------------------
// Formatting helpers
// -------------------------------------------------------------------------

/// Escape a string for use inside double-quoted TOML basic strings.
fn escape_string(s: &str) -> String {
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

/// Format a float value per TOML spec.
fn format_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    // Must have a fractional part to be a valid TOML float
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// Format a LocalTime, trimming trailing nanosecond zeros.
fn format_time(t: &LocalTime) -> String {
    if t.nanosecond == 0 {
        format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
    } else {
        // Format nanoseconds as 9 digits, then trim trailing zeros
        let ns = format!("{:09}", t.nanosecond);
        let trimmed = ns.trim_end_matches('0');
        format!("{:02}:{:02}:{:02}.{}", t.hour, t.minute, t.second, trimmed)
    }
}

/// Format an OffsetDateTime as RFC 3339 with T separator.
fn format_offset_datetime(dt: &OffsetDateTime) -> String {
    let date = &dt.date;
    let time = &dt.time;
    let time_str = format_time(time);
    let offset_str = match &dt.offset {
        UtcOffset::Z => "Z".to_string(),
        UtcOffset::Minutes(mins) => {
            let sign = if *mins >= 0 { '+' } else { '-' };
            let abs = mins.unsigned_abs();
            format!("{}{:02}:{:02}", sign, abs / 60, abs % 60)
        }
    };
    format!(
        "{:04}-{:02}-{:02}T{}{}",
        date.year, date.month, date.day, time_str, offset_str
    )
}
