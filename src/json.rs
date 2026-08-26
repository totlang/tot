//! JSON output.
//!
//! There is deliberately no JSON *input* here, and none is needed: `,` and `:` are whitespace
//! in tot, so every JSON document already parses with [`parse`](crate::parse). Reading JSON
//! is reading tot.

use crate::value::{Value, write_escaped};

/// Render as JSON on a single line.
pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, false, 0);
    out
}

/// Render as JSON with two-space indentation.
pub fn to_string_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, true, 0);
    out
}

fn write_value(out: &mut String, value: &Value, pretty: bool, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Integer(i) => out.push_str(i.as_str()),
        // `Float`'s Display normalizes the two tot-only spellings, `1.` and `.1`.
        Value::Float(f) => out.push_str(&f.to_string()),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                break_line(out, pretty, level + 1);
                write_value(out, item, pretty, level + 1);
            }
            break_line(out, pretty, level);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, member)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                break_line(out, pretty, level + 1);
                write_string(out, key);
                out.push(':');
                if pretty {
                    out.push(' ');
                }
                write_value(out, member, pretty, level + 1);
            }
            break_line(out, pretty, level);
            out.push('}');
        }
    }
}

fn break_line(out: &mut String, pretty: bool, level: usize) {
    if pretty {
        out.push('\n');
        for _ in 0..level * 2 {
            out.push(' ');
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    write_escaped(out, s);
    out.push('"');
}
