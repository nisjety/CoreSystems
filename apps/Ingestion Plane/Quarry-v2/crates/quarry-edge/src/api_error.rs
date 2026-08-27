//! Structured API error envelope (OSS-parity P2 3D) — Tavily-style.
//!
//! Extends the existing `{error, code, hint}` shape with machine-actionable
//! rate-limit fields (`window`, `retry_after_seconds`, `next_actions`) so
//! Verevon can show precise back-off UX and clients can auto-retry correctly.
//! Optional fields serialize only when present, so existing consumers are
//! unaffected.

use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
pub struct ApiError {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Human-readable rate-limit window, e.g. "60s", "1h".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// Seconds to wait before retrying.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    /// Suggested client next steps.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub next_actions: Vec<String>,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            code: code.into(),
            hint: None,
            window: None,
            retry_after_seconds: None,
            next_actions: Vec::new(),
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Build a `RATE_LIMITED` error carrying retry guidance + next actions.
    pub fn rate_limited(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        Self {
            error: message.into(),
            code: "RATE_LIMITED".into(),
            hint: Some("reduce request rate or retry after the indicated window".into()),
            window: Some(humanize_secs(retry_after_seconds)),
            retry_after_seconds: Some(retry_after_seconds),
            next_actions: vec![
                format!("retry after {} seconds", retry_after_seconds),
                "reduce concurrent request rate".into(),
            ],
        }
    }
}

/// Render seconds as a compact window label (e.g. 90 → "1m30s", 3600 → "1h").
pub(crate) fn humanize_secs(secs: u64) -> String {
    if secs == 0 {
        return "0s".into();
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{h}h"));
    }
    if m > 0 {
        out.push_str(&format!("{m}m"));
    }
    if s > 0 {
        out.push_str(&format!("{s}s"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_windows() {
        assert_eq!(humanize_secs(60), "1m");
        assert_eq!(humanize_secs(90), "1m30s");
        assert_eq!(humanize_secs(3600), "1h");
        assert_eq!(humanize_secs(45), "45s");
    }

    #[test]
    fn rate_limited_carries_retry_fields() {
        let e = ApiError::rate_limited("throttled", 60);
        assert_eq!(e.code, "RATE_LIMITED");
        assert_eq!(e.retry_after_seconds, Some(60));
        assert_eq!(e.window.as_deref(), Some("1m"));
        assert_eq!(e.next_actions.len(), 2);
    }

    #[test]
    fn plain_error_omits_optional_fields() {
        let e = ApiError::new("BAD_REQUEST", "nope");
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("retry_after_seconds"));
        assert!(!json.contains("next_actions"));
        assert!(!json.contains("window"));
    }
}
