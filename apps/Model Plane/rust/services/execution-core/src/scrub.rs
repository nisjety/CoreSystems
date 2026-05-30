//! Secret scrubbing for checkpoint/step payloads.
//!
//! Two entry points:
//! - [`scrub_json_value`]: redacts values in a [`serde_json::Value`] whose
//!   object key matches a sensitive-name pattern.
//! - [`scrub_string`]: redacts known secret token patterns inside free-form
//!   strings (Bearer tokens, JWTs, `sk-*`, AWS access keys, PEM blocks).
//!
//! Both run before any persistence (session-core `SaveCheckpoint` /
//! `CompleteStep`) to satisfy Security gate #1 in `docs/VERIFICATION.md`.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

const REDACTED: &str = "[REDACTED]";

fn sensitive_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(password|secret|token|api[_-]?key|auth|credential|bearer|private[_-]?key|session[_-]?id|cookie)",
        )
        .expect("sensitive_key_re compiles")
    })
}

fn env_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Treat any ALL_CAPS env-style key ending in _KEY/_TOKEN/_SECRET/_PASSWORD as sensitive.
    RE.get_or_init(|| {
        Regex::new(r"^[A-Z][A-Z0-9_]*(_KEY|_TOKEN|_SECRET|_PASSWORD|_PWD)$")
            .expect("env_key_re compiles")
    })
}

fn string_patterns() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            // Bearer <token>
            Regex::new(r"(?i)bearer\s+[A-Za-z0-9._\-+/=]+").expect("bearer regex"),
            // JWT: three dot-separated base64url segments
            Regex::new(r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+").expect("jwt regex"),
            // OpenAI / Anthropic style: sk-...
            Regex::new(r"sk-[A-Za-z0-9_\-]{16,}").expect("sk regex"),
            // AWS access key id
            Regex::new(r"AKIA[0-9A-Z]{16}").expect("aws akid regex"),
            // GitHub tokens
            Regex::new(r"gh[pousr]_[A-Za-z0-9]{20,}").expect("github token regex"),
            // PEM private key block
            Regex::new(
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            )
            .expect("pem regex"),
        ]
    })
}

/// Recursively redact sensitive-keyed values in a JSON tree.
///
/// Any object entry whose key matches the sensitive-name pattern has its
/// value replaced with `"[REDACTED]"`. String values elsewhere are passed
/// through [`scrub_string`] to catch inline secrets.
pub fn scrub_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if sensitive_key_re().is_match(key) || env_key_re().is_match(key) {
                    *v = Value::String(REDACTED.to_owned());
                } else {
                    scrub_json_value(v);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                scrub_json_value(item);
            }
        }
        Value::String(s) => {
            let scrubbed = scrub_string(s);
            if scrubbed != *s {
                *s = scrubbed;
            }
        }
        _ => {}
    }
}

/// Redact known secret patterns inside a free-form string.
///
/// Matched patterns: `Bearer <token>`, JWTs, `sk-...`, AWS access key ids,
/// GitHub tokens, PEM private key blocks.
#[must_use]
pub fn scrub_string(s: &str) -> String {
    let mut out = s.to_owned();
    for re in string_patterns() {
        out = re.replace_all(&out, REDACTED).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_sensitive_keys() {
        let mut v = json!({
            "tool_name": "bash",
            "env": { "API_KEY": "super-secret", "PATH": "/usr/bin" },
            "password": "hunter2",
            "note": "all good"
        });
        scrub_json_value(&mut v);
        assert_eq!(v["env"]["API_KEY"], json!(REDACTED));
        assert_eq!(v["env"]["PATH"], json!("/usr/bin"));
        assert_eq!(v["password"], json!(REDACTED));
        assert_eq!(v["tool_name"], json!("bash"));
        assert_eq!(v["note"], json!("all good"));
    }

    #[test]
    fn redacts_bearer_and_jwt_in_strings() {
        let out = scrub_string(
            "Authorization: Bearer eyJhbGciOi.eyJzdWIi.sig and sk-abcdef1234567890XYZ",
        );
        assert!(out.contains(REDACTED));
        assert!(!out.contains("eyJhbGciOi"));
        assert!(!out.contains("sk-abcdef"));
    }

    #[test]
    fn preserves_non_sensitive_content() {
        let mut v = json!({"step": {"id": "s1", "status": "completed", "output": "hello"}});
        scrub_json_value(&mut v);
        assert_eq!(v["step"]["output"], json!("hello"));
        assert_eq!(v["step"]["status"], json!("completed"));
    }
}
