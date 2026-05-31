//! TOON — Token-Oriented Object Notation.
//!
//! Compact, indentation-driven encoding for `serde_json::Value` payloads
//! intended for LLM prompts and tool I/O. Objectives:
//!
//! * Drop the structural noise of JSON (quotes, braces, commas).
//! * Be deterministic so caches and idempotency hashes are stable.
//!
//! **Lossy / display-oriented — NOT reversible.** Unquoted scalars conflate
//! types: the string `"30"`, the number `30`, and the bools/null literals all
//! emit bare tokens, so `encode` is not injective and there is deliberately no
//! `decode`. Use TOON only for prompt rendering / display where the original
//! `Value` is still available; never on a path that must round-trip data or
//! could replace a source record (see `docs/capability-ownership-matrix.md`
//! §5 P8 and GOAL.md: correctness/traceability win). Making it reversible
//! would require type-tagging scalars — a format change.
//!
//! Format rules:
//!
//! * Scalars on a single line. Strings are unquoted unless they contain
//!   a colon, `#`, leading/trailing whitespace, or start with `-`/`[`/`{`
//!   — in which case they are JSON-quoted.
//! * Objects render keys at the current indent; values follow `: ` for
//!   scalars or a newline + deeper indent for nested containers.
//! * Arrays of scalars render inline as `[a, b, c]` when total length
//!   fits within `INLINE_ARRAY_BUDGET` chars; otherwise each element on
//!   its own `- ` line.
//! * Arrays of objects always expand: each element is a `- ` block.
//! * Empty containers render as `[]` / `{}`.
//!
//! No external dependency on serde derives — input is `serde_json::Value`
//! so callers can adapt arbitrary types via `serde_json::to_value`.

use serde_json::Value;

const INDENT: &str = "  ";
const INLINE_ARRAY_BUDGET: usize = 80;

/// Maximum nesting depth the encoder will traverse before emitting a
/// truncation marker. Protects against pathological input (e.g. a JSON
/// payload built to overflow the encoder stack via deeply nested arrays
/// or objects). Values beyond this depth are rendered as `"..."`.
pub const MAX_DEPTH: usize = 64;

/// Encode a JSON value as TOON. Returns a fresh `String`. Depth is
/// bounded by [`MAX_DEPTH`]; values nested beyond that limit render as
/// `"..."` rather than blow the stack.
pub fn encode(value: &Value) -> String {
    let mut out = String::new();
    encode_value(value, 0, &mut out, true);
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn encode_value(value: &Value, depth: usize, out: &mut String, top_level: bool) {
    if depth >= MAX_DEPTH {
        out.push_str("\"...\"");
        return;
    }
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&encode_scalar_string(s)),
        Value::Array(arr) => encode_array(arr, depth, out, top_level),
        Value::Object(map) => encode_object(map, depth, out, top_level),
    }
}

fn encode_object(
    map: &serde_json::Map<String, Value>,
    depth: usize,
    out: &mut String,
    top_level: bool,
) {
    if depth >= MAX_DEPTH {
        out.push_str("\"...\"");
        return;
    }
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    if !top_level {
        out.push('\n');
    }
    let mut first = true;
    for (k, v) in map {
        if !first {
            out.push('\n');
        }
        first = false;
        push_indent(out, depth);
        out.push_str(&encode_key(k));
        out.push(':');
        match v {
            Value::Object(inner) if !inner.is_empty() => {
                encode_object(inner, depth + 1, out, false);
            }
            Value::Array(inner) if !inner.is_empty() => {
                if let Some(inline) = try_inline_array(inner) {
                    out.push(' ');
                    out.push_str(&inline);
                } else {
                    encode_array(inner, depth + 1, out, false);
                }
            }
            _ => {
                out.push(' ');
                encode_value(v, depth + 1, out, true);
            }
        }
    }
}

/// Render an array on a single line if every element is a scalar and
/// the resulting `[a, b, c]` fits the inline budget. Returns None if the
/// array should be expanded to a multi-line `- ` block.
fn try_inline_array(arr: &[Value]) -> Option<String> {
    if !is_array_inlineable(arr) {
        return None;
    }
    let mut buf = String::from("[");
    for (i, v) in arr.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        let mut tmp = String::new();
        encode_value(v, 0, &mut tmp, true);
        buf.push_str(&tmp);
    }
    buf.push(']');
    if buf.len() <= INLINE_ARRAY_BUDGET {
        Some(buf)
    } else {
        None
    }
}

fn encode_array(arr: &[Value], depth: usize, out: &mut String, top_level: bool) {
    if depth >= MAX_DEPTH {
        out.push_str("\"...\"");
        return;
    }
    if arr.is_empty() {
        out.push_str("[]");
        return;
    }
    if let Some(inline) = try_inline_array(arr) {
        out.push_str(&inline);
        return;
    }
    if !top_level {
        out.push('\n');
    }
    let mut first = true;
    for v in arr {
        if !first {
            out.push('\n');
        }
        first = false;
        push_indent(out, depth);
        out.push_str("- ");
        match v {
            Value::Object(inner) if !inner.is_empty() => {
                // First key inline with the dash, remaining keys at depth+1.
                let mut iter = inner.iter();
                if let Some((k, first_val)) = iter.next() {
                    out.push_str(&encode_key(k));
                    out.push(':');
                    match first_val {
                        Value::Object(o) if !o.is_empty() => {
                            encode_object(o, depth + 2, out, false);
                        }
                        Value::Array(a) if !a.is_empty() && !is_array_inlineable(a) => {
                            encode_array(a, depth + 2, out, false);
                        }
                        _ => {
                            out.push(' ');
                            encode_value(first_val, depth + 2, out, true);
                        }
                    }
                    for (k, v) in iter {
                        out.push('\n');
                        push_indent(out, depth + 1);
                        out.push_str(&encode_key(k));
                        out.push(':');
                        match v {
                            Value::Object(o) if !o.is_empty() => {
                                encode_object(o, depth + 2, out, false);
                            }
                            Value::Array(a) if !a.is_empty() && !is_array_inlineable(a) => {
                                encode_array(a, depth + 2, out, false);
                            }
                            _ => {
                                out.push(' ');
                                encode_value(v, depth + 2, out, true);
                            }
                        }
                    }
                }
            }
            _ => encode_value(v, depth + 1, out, true),
        }
    }
}

fn is_array_inlineable(arr: &[Value]) -> bool {
    arr.iter().all(|v| {
        matches!(
            v,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
        )
    })
}

fn encode_key(k: &str) -> String {
    if needs_quoting(k) {
        serde_json::to_string(k).unwrap_or_else(|_| k.to_owned())
    } else {
        k.to_owned()
    }
}

fn encode_scalar_string(s: &str) -> String {
    if needs_quoting(s) {
        serde_json::to_string(s).unwrap_or_else(|_| s.to_owned())
    } else {
        s.to_owned()
    }
}

fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s != s.trim() {
        return true;
    }
    let first = s.chars().next().unwrap();
    if matches!(first, '-' | '[' | '{' | '"' | '\'' | '#' | '&' | '*' | '@') {
        return true;
    }
    if matches!(s, "true" | "false" | "null") {
        return true;
    }
    s.chars()
        .any(|c| matches!(c, ':' | '\n' | '\r' | '\t' | '#'))
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str(INDENT);
    }
}

/// Estimate token count for a TOON string. Heuristic: ~4 chars per token,
/// matching the convention used elsewhere in the Model Plane (see
/// session-core `assemble_segments` budget math).
#[must_use]
pub fn estimate_tokens(toon: &str) -> u32 {
    let len = u32::try_from(toon.len()).unwrap_or(u32::MAX);
    len.saturating_div(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_scalars() {
        assert_eq!(encode(&Value::Null), "null");
        assert_eq!(encode(&json!(true)), "true");
        assert_eq!(encode(&json!(42)), "42");
        assert_eq!(encode(&json!("hello")), "hello");
    }

    #[test]
    fn quotes_strings_with_colons() {
        assert_eq!(encode(&json!("a: b")), "\"a: b\"");
    }

    #[test]
    fn quotes_reserved_words() {
        assert_eq!(encode(&json!("true")), "\"true\"");
        assert_eq!(encode(&json!("null")), "\"null\"");
    }

    #[test]
    fn encodes_simple_object() {
        // serde_json::Map without `preserve_order` sorts keys; this is
        // intentional — TOON output is deterministic for cache stability.
        let v = json!({ "name": "alice", "age": 30 });
        assert_eq!(encode(&v), "age: 30\nname: alice");
    }

    #[test]
    fn encodes_nested_object() {
        let v = json!({ "user": { "name": "alice", "active": true } });
        assert_eq!(encode(&v), "user:\n  active: true\n  name: alice");
    }

    #[test]
    fn encodes_inline_scalar_array() {
        let v = json!({ "tags": ["a", "b", "c"] });
        assert_eq!(encode(&v), "tags: [a, b, c]");
    }

    #[test]
    fn expands_long_scalar_array() {
        let many: Vec<String> = (0..20).map(|i| format!("entry-{i:02}")).collect();
        let v = json!({ "items": many });
        let out = encode(&v);
        assert!(out.starts_with("items:\n  - entry-00"));
        assert!(out.contains("\n  - entry-19"));
    }

    #[test]
    fn expands_array_of_objects() {
        let v = json!({
            "rows": [
                { "id": 1, "name": "a" },
                { "id": 2, "name": "b" }
            ]
        });
        let out = encode(&v);
        // Keys sort alphabetically (id, name) — first key inlines with `- `.
        assert_eq!(out, "rows:\n  - id: 1\n    name: a\n  - id: 2\n    name: b");
    }

    #[test]
    fn empty_object_and_array_render_inline() {
        assert_eq!(encode(&json!({})), "{}");
        assert_eq!(encode(&json!([])), "[]");
    }

    #[test]
    fn deeply_nested_payload_round_trip_shape() {
        let v = json!({
            "request": {
                "id": "r1",
                "filters": { "doc_types": ["pdf", "html"], "lang": "en" },
                "candidates": [
                    { "score": 0.91, "title": "Doc A" },
                    { "score": 0.84, "title": "Doc B" }
                ]
            }
        });
        let out = encode(&v);
        assert!(out.contains("request:"));
        assert!(out.contains("filters:"));
        assert!(out.contains("doc_types: [pdf, html]"));
        assert!(out.contains("- score: 0.91"));
        assert!(out.contains("title: Doc A"));
    }

    #[test]
    fn token_savings_vs_json_for_typical_payload() {
        let v = json!({
            "policy": "default",
            "workspace": "ws-1",
            "messages": [
                { "role": "user", "content": "hello" },
                { "role": "assistant", "content": "hi" }
            ]
        });
        let json_len = serde_json::to_string(&v).unwrap().len();
        let toon_len = encode(&v).len();
        assert!(
            toon_len < json_len,
            "TOON ({toon_len}) should beat JSON ({json_len})"
        );
    }

    #[test]
    fn estimate_tokens_uses_4_chars_per_token() {
        assert_eq!(estimate_tokens("12345678"), 2);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn deeply_nested_input_does_not_overflow_stack() {
        // Build a left-spined nested object 200 levels deep — well past
        // MAX_DEPTH. Without the depth guard this would either blow the
        // stack or produce a multi-thousand-line output. With the guard
        // we just stop descending and emit a "..." marker.
        let mut v = Value::String("leaf".to_owned());
        for _ in 0..200 {
            let mut obj = serde_json::Map::new();
            obj.insert("inner".to_owned(), v);
            v = Value::Object(obj);
        }
        let out = encode(&v);
        // Output should be bounded and contain the truncation marker.
        assert!(out.contains("\"...\""));
        // Loose bound: depth-MAX_DEPTH levels of "inner:\n" plus indent
        // is well under 10KB even with the deepest indent.
        assert!(
            out.len() < 10_000,
            "output unexpectedly large: {}",
            out.len()
        );
    }

    #[test]
    fn deeply_nested_array_does_not_overflow_stack() {
        let mut v = Value::Number(1.into());
        for _ in 0..200 {
            v = Value::Array(vec![v]);
        }
        let out = encode(&v);
        assert!(out.contains("\"...\""));
    }
}
