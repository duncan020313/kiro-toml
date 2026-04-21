// Integration tests for toml-rust-parser
// Tests cover all major TOML constructs with round-trip verification.
// Requirements: 1.1, 13.1, 13.2, 13.3, 14.4

use indexmap::IndexMap;
use toml_rust_parser::{parse, to_toml_string, Value};
use toml_rust_parser::value::{LocalDate, UtcOffset};

// -----------------------------------------------------------------------
// Helper: round-trip a TOML string (parse → serialize → parse → compare)
// -----------------------------------------------------------------------
fn round_trip(toml: &str) -> (Value, Value) {
    let first = parse(toml).expect("first parse failed");
    let serialized = to_toml_string(&first);
    let second = parse(&serialized).expect("second parse failed");
    (first, second)
}

fn assert_round_trip(toml: &str) {
    let (first, second) = round_trip(toml);
    assert_values_eq(&first, &second, toml);
}

fn assert_values_eq(a: &Value, b: &Value, ctx: &str) {
    match (a, b) {
        (Value::Float(fa), Value::Float(fb)) => {
            assert!(
                (fa.is_nan() && fb.is_nan()) || fa.to_bits() == fb.to_bits(),
                "float mismatch: {} vs {} (ctx: {})", fa, fb, ctx
            );
        }
        (Value::Array(aa), Value::Array(ab)) => {
            assert_eq!(aa.len(), ab.len(), "array length mismatch (ctx: {})", ctx);
            for (x, y) in aa.iter().zip(ab.iter()) {
                assert_values_eq(x, y, ctx);
            }
        }
        (Value::Table(ta), Value::Table(tb)) => {
            assert_eq!(ta.len(), tb.len(), "table length mismatch (ctx: {})", ctx);
            for (k, va) in ta {
                let vb = tb.get(k.as_str())
                    .unwrap_or_else(|| panic!("key '{}' missing in round-trip (ctx: {})", k, ctx));
                assert_values_eq(va, vb, ctx);
            }
        }
        _ => assert_eq!(a, b, "value mismatch (ctx: {})", ctx),
    }
}

// -----------------------------------------------------------------------
// String types
// -----------------------------------------------------------------------

#[test]
fn test_basic_string() {
    let toml = r#"s = "hello world""#;
    let v = parse(toml).unwrap();
    assert_eq!(v, Value::Table({
        let mut m = IndexMap::new();
        m.insert("s".to_string(), Value::String("hello world".to_string()));
        m
    }));
    assert_round_trip(toml);
}

#[test]
fn test_basic_string_escapes() {
    let toml = "s = \"tab:\\there\\nnewline\"";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::String(s) = &t["s"] {
            assert!(s.contains('\t'));
            assert!(s.contains('\n'));
        } else { panic!("expected string"); }
    } else { panic!("expected table"); }
    assert_round_trip(toml);
}

#[test]
fn test_literal_string() {
    let toml = r#"s = 'C:\Users\no\escape'"#;
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["s"], Value::String(r"C:\Users\no\escape".to_string()));
    }
    assert_round_trip(toml);
}

#[test]
fn test_multiline_basic_string() {
    let toml = "s = \"\"\"\nline1\nline2\n\"\"\"";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::String(s) = &t["s"] {
            assert!(s.contains("line1"));
            assert!(s.contains("line2"));
        } else { panic!("expected string"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_multiline_literal_string() {
    let toml = "s = '''\nraw\\nno escape\n'''";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::String(s) = &t["s"] {
            assert!(s.contains(r"raw\nno escape"));
        } else { panic!("expected string"); }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Number types
// -----------------------------------------------------------------------

#[test]
fn test_integer_decimal() {
    let toml = "n = 42";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["n"], Value::Integer(42));
    }
    assert_round_trip(toml);
}

#[test]
fn test_integer_negative() {
    let toml = "n = -100";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["n"], Value::Integer(-100));
    }
    assert_round_trip(toml);
}

#[test]
fn test_integer_hex() {
    let toml = "n = 0xDEADBEEF";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["n"], Value::Integer(0xDEADBEEF));
    }
    assert_round_trip(toml);
}

#[test]
fn test_integer_octal() {
    let toml = "n = 0o755";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["n"], Value::Integer(0o755));
    }
    assert_round_trip(toml);
}

#[test]
fn test_integer_binary() {
    let toml = "n = 0b11010110";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["n"], Value::Integer(0b11010110));
    }
    assert_round_trip(toml);
}

#[test]
fn test_integer_underscore() {
    let toml = "n = 1_000_000";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["n"], Value::Integer(1_000_000));
    }
    assert_round_trip(toml);
}

#[test]
fn test_float_basic() {
    let toml = "f = 3.14";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::Float(f) = t["f"] {
            assert!((f - 3.14).abs() < 1e-10);
        } else { panic!("expected float"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_float_exponent() {
    let toml = "f = 6.626e-34";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert!(matches!(t["f"], Value::Float(_)));
    }
    assert_round_trip(toml);
}

#[test]
fn test_float_special_inf() {
    let toml = "a = inf\nb = -inf\nc = +inf";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["a"], Value::Float(f64::INFINITY));
        assert_eq!(t["b"], Value::Float(f64::NEG_INFINITY));
        assert_eq!(t["c"], Value::Float(f64::INFINITY));
    }
    assert_round_trip(toml);
}

#[test]
fn test_float_special_nan() {
    let toml = "f = nan";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::Float(f) = t["f"] {
            assert!(f.is_nan());
        } else { panic!("expected float nan"); }
    }
    // NaN round-trip: just verify it parses both times
    let serialized = to_toml_string(&v);
    let reparsed = parse(&serialized).unwrap();
    if let Value::Table(t) = &reparsed {
        if let Value::Float(f) = t["f"] {
            assert!(f.is_nan());
        }
    }
}

// -----------------------------------------------------------------------
// Booleans
// -----------------------------------------------------------------------

#[test]
fn test_boolean_true() {
    let toml = "b = true";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["b"], Value::Boolean(true));
    }
    assert_round_trip(toml);
}

#[test]
fn test_boolean_false() {
    let toml = "b = false";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["b"], Value::Boolean(false));
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Date/time types
// -----------------------------------------------------------------------

#[test]
fn test_offset_datetime() {
    let toml = "dt = 1979-05-27T07:32:00Z";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert!(matches!(t["dt"], Value::OffsetDateTime(_)));
        if let Value::OffsetDateTime(odt) = &t["dt"] {
            assert_eq!(odt.date.year, 1979);
            assert_eq!(odt.date.month, 5);
            assert_eq!(odt.date.day, 27);
            assert_eq!(odt.time.hour, 7);
            assert_eq!(odt.time.minute, 32);
            assert_eq!(odt.offset, UtcOffset::Z);
        }
    }
    assert_round_trip(toml);
}

#[test]
fn test_offset_datetime_with_offset() {
    let toml = "dt = 1979-05-27T07:32:00+05:30";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::OffsetDateTime(odt) = &t["dt"] {
            assert_eq!(odt.offset, UtcOffset::Minutes(5 * 60 + 30));
        } else { panic!("expected OffsetDateTime"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_local_datetime() {
    let toml = "dt = 1979-05-27T07:32:00";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert!(matches!(t["dt"], Value::LocalDateTime(_)));
    }
    assert_round_trip(toml);
}

#[test]
fn test_local_date() {
    let toml = "d = 1979-05-27";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["d"], Value::LocalDate(LocalDate { year: 1979, month: 5, day: 27 }));
    }
    assert_round_trip(toml);
}

#[test]
fn test_local_time() {
    let toml = "t = 07:32:00";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert!(matches!(t["t"], Value::LocalTime(_)));
        if let Value::LocalTime(lt) = &t["t"] {
            assert_eq!(lt.hour, 7);
            assert_eq!(lt.minute, 32);
            assert_eq!(lt.second, 0);
        }
    }
    assert_round_trip(toml);
}

#[test]
fn test_local_time_fractional() {
    let toml = "t = 07:32:00.999";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::LocalTime(lt) = &t["t"] {
            assert_eq!(lt.nanosecond, 999_000_000);
        } else { panic!("expected LocalTime"); }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Arrays
// -----------------------------------------------------------------------

#[test]
fn test_array_empty() {
    let toml = "a = []";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["a"], Value::Array(vec![]));
    }
    assert_round_trip(toml);
}

#[test]
fn test_array_integers() {
    let toml = "a = [1, 2, 3]";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["a"], Value::Array(vec![
            Value::Integer(1), Value::Integer(2), Value::Integer(3)
        ]));
    }
    assert_round_trip(toml);
}

#[test]
fn test_array_mixed_types() {
    let toml = r#"a = [1, "two", true]"#;
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::Array(arr) = &t["a"] {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Value::Integer(1));
            assert_eq!(arr[1], Value::String("two".to_string()));
            assert_eq!(arr[2], Value::Boolean(true));
        } else { panic!("expected array"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_array_trailing_comma() {
    let toml = "a = [1, 2, 3,]";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::Array(arr) = &t["a"] {
            assert_eq!(arr.len(), 3);
        } else { panic!("expected array"); }
    }
    // Round-trip (printer may omit trailing comma)
    let serialized = to_toml_string(&v);
    let reparsed = parse(&serialized).unwrap();
    assert_values_eq(&v, &reparsed, toml);
}

#[test]
fn test_array_nested() {
    let toml = "a = [[1, 2], [3, 4]]";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::Array(outer) = &t["a"] {
            assert_eq!(outer.len(), 2);
            assert!(matches!(&outer[0], Value::Array(_)));
        } else { panic!("expected array"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_array_multiline() {
    let toml = "a = [\n  1,\n  2,\n  # comment\n  3\n]";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        if let Value::Array(arr) = &t["a"] {
            assert_eq!(arr.len(), 3);
        } else { panic!("expected array"); }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Standard tables
// -----------------------------------------------------------------------

#[test]
fn test_standard_table_basic() {
    let toml = "[server]\nhost = \"localhost\"\nport = 8080";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(server) = &root["server"] {
            assert_eq!(server["host"], Value::String("localhost".to_string()));
            assert_eq!(server["port"], Value::Integer(8080));
        } else { panic!("expected table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_standard_table_nested() {
    let toml = "[a.b.c]\nval = 1";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(a) = &root["a"] {
            if let Value::Table(b) = &a["b"] {
                if let Value::Table(c) = &b["c"] {
                    assert_eq!(c["val"], Value::Integer(1));
                } else { panic!("expected c table"); }
            } else { panic!("expected b table"); }
        } else { panic!("expected a table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_standard_table_multiple() {
    let toml = "[a]\nx = 1\n\n[b]\ny = 2";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        assert!(root.contains_key("a"));
        assert!(root.contains_key("b"));
    }
    assert_round_trip(toml);
}

#[test]
fn test_super_table_after_sub_table() {
    // [x.y.z] defined before [x] — valid TOML
    let toml = "[x.y.z]\nval = 1\n\n[x]\nother = 2";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(x) = &root["x"] {
            assert_eq!(x["other"], Value::Integer(2));
        }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Array of tables (AOT)
// -----------------------------------------------------------------------

#[test]
fn test_aot_basic() {
    let toml = "[[products]]\nname = \"Hammer\"\n\n[[products]]\nname = \"Nail\"";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Array(products) = &root["products"] {
            assert_eq!(products.len(), 2);
            if let Value::Table(p0) = &products[0] {
                assert_eq!(p0["name"], Value::String("Hammer".to_string()));
            }
            if let Value::Table(p1) = &products[1] {
                assert_eq!(p1["name"], Value::String("Nail".to_string()));
            }
        } else { panic!("expected array"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_aot_nested() {
    let toml = "[[fruits]]\nname = \"apple\"\n\n[[fruits.varieties]]\nname = \"red\"\n\n[[fruits]]\nname = \"banana\"";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Array(fruits) = &root["fruits"] {
            assert_eq!(fruits.len(), 2);
            if let Value::Table(apple) = &fruits[0] {
                assert_eq!(apple["name"], Value::String("apple".to_string()));
                if let Value::Array(varieties) = &apple["varieties"] {
                    assert_eq!(varieties.len(), 1);
                }
            }
        }
    }
    assert_round_trip(toml);
}

#[test]
fn test_aot_with_subtable() {
    let toml = "[[items]]\nid = 1\n\n[items.meta]\ncolor = \"red\"";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Array(items) = &root["items"] {
            if let Value::Table(item) = &items[0] {
                assert!(item.contains_key("meta"));
            }
        }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Inline tables
// -----------------------------------------------------------------------

#[test]
fn test_inline_table_basic() {
    let toml = r#"point = {x = 1, y = 2}"#;
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(point) = &root["point"] {
            assert_eq!(point["x"], Value::Integer(1));
            assert_eq!(point["y"], Value::Integer(2));
        } else { panic!("expected table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_inline_table_empty() {
    let toml = "t = {}";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(t) = &root["t"] {
            assert!(t.is_empty());
        } else { panic!("expected table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_inline_table_nested() {
    let toml = r#"a = {b = {c = 42}}"#;
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(a) = &root["a"] {
            if let Value::Table(b) = &a["b"] {
                assert_eq!(b["c"], Value::Integer(42));
            } else { panic!("expected b table"); }
        } else { panic!("expected a table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_inline_table_in_array() {
    let toml = r#"points = [{x = 1, y = 2}, {x = 3, y = 4}]"#;
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Array(pts) = &root["points"] {
            assert_eq!(pts.len(), 2);
            assert!(matches!(&pts[0], Value::Table(_)));
        }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Dotted keys
// -----------------------------------------------------------------------

#[test]
fn test_dotted_key_basic() {
    let toml = "a.b = 1";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(a) = &root["a"] {
            assert_eq!(a["b"], Value::Integer(1));
        } else { panic!("expected nested table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_dotted_key_multiple_segments() {
    let toml = "a.b.c = \"deep\"";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(a) = &root["a"] {
            if let Value::Table(b) = &a["b"] {
                assert_eq!(b["c"], Value::String("deep".to_string()));
            } else { panic!("expected b table"); }
        } else { panic!("expected a table"); }
    }
    assert_round_trip(toml);
}

#[test]
fn test_dotted_key_whitespace_around_dot() {
    let toml = "a . b = 42";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(a) = &root["a"] {
            assert_eq!(a["b"], Value::Integer(42));
        } else { panic!("expected nested table"); }
    }
}

#[test]
fn test_dotted_key_multiple_in_table() {
    let toml = "a.x = 1\na.y = 2";
    let v = parse(toml).unwrap();
    if let Value::Table(root) = &v {
        if let Value::Table(a) = &root["a"] {
            assert_eq!(a["x"], Value::Integer(1));
            assert_eq!(a["y"], Value::Integer(2));
        } else { panic!("expected a table"); }
    }
    assert_round_trip(toml);
}

// -----------------------------------------------------------------------
// Comprehensive round-trip: a document using all major constructs
// -----------------------------------------------------------------------

#[test]
fn test_comprehensive_round_trip() {
    let toml = r#"
# Root-level scalars
title = "TOML Example"
enabled = true
count = 42
ratio = 3.14
created = 1979-05-27

[owner]
name = "Tom Preston-Werner"
dob = 1979-05-27T07:32:00Z

[database]
server = "192.168.1.1"
ports = [8001, 8001, 8002]
enabled = true

[servers.alpha]
ip = "10.0.0.1"
role = "frontend"

[servers.beta]
ip = "10.0.0.2"
role = "backend"

[[products]]
name = "Hammer"
sku = 738594937

[[products]]
name = "Nail"
sku = 284758393
color = "gray"
"#;
    assert_round_trip(toml);
}

#[test]
fn test_root_level_values_preserved() {
    let toml = "x = 1\ny = \"hello\"\nz = true";
    let v = parse(toml).unwrap();
    if let Value::Table(t) = &v {
        assert_eq!(t["x"], Value::Integer(1));
        assert_eq!(t["y"], Value::String("hello".to_string()));
        assert_eq!(t["z"], Value::Boolean(true));
    }
    assert_round_trip(toml);
}

#[test]
fn test_empty_document() {
    let toml = "";
    let v = parse(toml).unwrap();
    assert_eq!(v, Value::Table(IndexMap::new()));
    assert_round_trip(toml);
}

#[test]
fn test_comment_only_document() {
    let toml = "# just a comment\n";
    let v = parse(toml).unwrap();
    assert_eq!(v, Value::Table(IndexMap::new()));
}
