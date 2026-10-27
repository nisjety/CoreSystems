//! The closed set of metric label keys a Model Plane service may attach to a `counter!`,
//! `gauge!` or `histogram!` (WS06-S3).
//!
//! A Prometheus label is a permanent, unauthenticated, cross-tenant surface: a label whose value
//! comes from request data (a thread id, a run id, a space id, a raw URI path) puts that
//! identifier into the metrics store of every scraper. The guard is by KEY, not by value: a key
//! that is on this list is reviewed to carry a closed set of values (an enum, a status code, a
//! route template); a key that is not on it fails the per-service `metric_label_guard` test.
//! Adding a label is a reviewed edit to [`METRIC_LABELS`].
//!
//! Each service's `tests/metric_label_guard.rs` reads its own `src` tree and calls
//! [`disallowed_metric_labels`] on every file.

/// Label keys a metric may carry. Every key here carries a closed or bucketed value set.
pub const METRIC_LABELS: &[&str] = &[
    // The WS06 design list.
    "method",
    "status",
    "result",
    "outcome",
    "reason",
    "type",
    "subject_class",
    "service",
    "model",
    "provider",
    "operation",
    "version",
    "rpc",
    "direction",
    // The matched route TEMPLATE (`/v1/threads/:thread_id`), never the raw URI path.
    "route",
    // Present in the tree when the guard was written; each is an enum or a bucketed name.
    "check",
    "code",
    "decision",
    "kind",
    "mode",
    "op",
    "phase",
    "point",
    "source",
    "surface",
    "to",
    // Built-in tool name, with every `mcp__*` name collapsed to the `mcp__` bucket.
    "tool",
];

/// One label key a metric macro used that is not on [`METRIC_LABELS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisallowedMetricLabel {
    /// The metric name literal (first macro argument), or empty when it is not a literal.
    pub metric: String,
    /// The offending label key.
    pub label: String,
}

/// True when `label` is on the closed list.
#[must_use]
pub fn is_allowed_metric_label(label: &str) -> bool {
    METRIC_LABELS.contains(&label)
}

/// Every `"key" =>` label of every `counter!`, `gauge!` and `histogram!` invocation in `source`
/// whose key is not on [`METRIC_LABELS`]. Comments are skipped; string literals are respected
/// when matching the macro's parentheses. An empty result means the file is clean.
#[must_use]
pub fn disallowed_metric_labels(source: &str) -> Vec<DisallowedMetricLabel> {
    let code = strip_line_comments(source);
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let Some(open) = find_metric_macro(&code, i) else {
            break;
        };
        let end = matching_paren(bytes, open);
        let body = &code[open + 1..end];
        let metric = first_string_literal(body).unwrap_or_default();
        for label in label_keys(body) {
            if !is_allowed_metric_label(&label) {
                out.push(DisallowedMetricLabel {
                    metric: metric.clone(),
                    label,
                });
            }
        }
        i = end.max(open + 1);
    }
    out
}

/// Replace `// ...` comments (outside string literals) with spaces, keeping offsets stable.
fn strip_line_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_str = false;
    let mut chars = source.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some((_, n)) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && matches!(chars.peek(), Some((_, '/'))) {
            out.push(' ');
            for (_, n) in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Byte offset of the `(` that opens the next `counter!`/`gauge!`/`histogram!` at or after `from`.
fn find_metric_macro(code: &str, from: usize) -> Option<usize> {
    let mut best: Option<usize> = None;
    for name in ["counter!", "gauge!", "histogram!"] {
        let mut start = from;
        while let Some(rel) = code[start..].find(name) {
            let at = start + rel;
            let before_ok = code[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            let rest = code[at + name.len()..].trim_start();
            if before_ok && rest.starts_with('(') {
                let open = code.len() - rest.len();
                best = Some(best.map_or(open, |b| b.min(open)));
                break;
            }
            start = at + name.len();
        }
    }
    best
}

/// Index of the `)` that closes the `(` at `open`, skipping string literals. Clamped to the end.
fn matching_paren(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut i = open;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' {
                i += 1;
            } else if b == b'"' {
                in_str = false;
            }
        } else if b == b'"' {
            in_str = true;
        } else if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    bytes.len()
}

fn first_string_literal(body: &str) -> Option<String> {
    let start = body.find('"')? + 1;
    let len = body[start..].find('"')?;
    Some(body[start..start + len].to_owned())
}

/// The `"key"` of every `"key" =>` in a macro body, in order.
fn label_keys(body: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = body;
    while let Some(q) = rest.find('"') {
        let after = &rest[q + 1..];
        let Some(close) = after.find('"') else { break };
        let lit = &after[..close];
        let tail = after[close + 1..].trim_start();
        if tail.starts_with("=>") {
            keys.push(lit.to_owned());
        }
        rest = &after[close + 1..];
    }
    keys
}
