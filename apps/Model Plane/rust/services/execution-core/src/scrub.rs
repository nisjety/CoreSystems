//! Secret scrubbing for checkpoint/step payloads.
//!
//! Two entry points:
//! - [`scrub_json_value`]: redacts values in a [`serde_json::Value`] whose
//!   object key matches a sensitive-name pattern.
//! - [`scrub_string`]: redacts known secret token patterns inside free-form
//!   strings (Bearer tokens, JWTs, `sk-*`, AWS access keys, PEM blocks,
//!   connection-string passwords, inline `key=secret` assignments, Slack /
//!   Google API keys).
//!
//! Both run before any persistence (session-core `SaveCheckpoint` /
//! `CompleteStep`) to satisfy Security gate #1 in `docs/VERIFICATION.md`.
//!
//! Pattern set extended 2026-05-30 per `docs/capability-ownership-matrix.md`
//! §G5, adapting the regex coverage of OpenAI Codex's `secrets` crate
//! (`redact_secrets`, Apache-2.0) — independently reimplemented here. The
//! additions close the gaps our existing patterns missed: credentials
//! embedded in DSN/URI userinfo, `KEY=value` assignments inside shell command
//! strings, and Slack/Google key formats.

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
            // Slack tokens (bot/user/app/refresh/legacy)
            Regex::new(r"xox[baprs]-[A-Za-z0-9-]{10,}").expect("slack token regex"),
            // Google API keys
            Regex::new(r"AIza[0-9A-Za-z_\-]{35}").expect("google api key regex"),
            // HTTP Basic auth header value
            Regex::new(r"(?i)basic\s+[A-Za-z0-9+/=]{8,}").expect("basic auth regex"),
            // PEM private key block
            Regex::new(
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            )
            .expect("pem regex"),
        ]
    })
}

/// Capture-preserving redaction patterns: `(regex, replacement)`.
///
/// Unlike [`string_patterns`], these keep the surrounding context (the key
/// name, the URI scheme/host) and redact only the secret portion via capture
/// groups, so scrubbed output stays diagnosable — e.g.
/// `postgres://svc:[REDACTED]@db/app` and `DB_PASSWORD=[REDACTED]`.
fn capture_patterns() -> &'static [(Regex, &'static str)] {
    static RE: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            // Credentials in connection-string / URI userinfo:
            // scheme://user:SECRET@host  →  scheme://user:[REDACTED]@host
            (
                Regex::new(r"(?i)\b([a-z][a-z0-9+.\-]*://[^:/?#\s]+:)([^@/?#\s]+)(@)")
                    .expect("uri userinfo regex"),
                "${1}[REDACTED]${3}",
            ),
            // Inline assignment in free text / shell commands. The leading
            // `[a-z0-9_]*` lets ENV-style prefixes match (DB_PASSWORD,
            // OPENAI_API_KEY) — `\bpassword` alone misses `_PASSWORD` since
            // `_P` is not a word boundary.
            // DB_PASSWORD=secret | api_key: "xyz"  →  DB_PASSWORD=[REDACTED]
            (
                Regex::new(
                    r#"(?i)\b([a-z0-9_]*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|auth[_-]?token|client[_-]?secret|private[_-]?key|token))(\s*[:=]\s*)("?)([^\s"'&,;]{3,})"#,
                )
                .expect("inline assignment regex"),
                "${1}${2}${3}[REDACTED]",
            ),
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
/// Matched patterns: `Bearer <token>`, `Basic <b64>`, JWTs, `sk-...`, AWS
/// access key ids, GitHub / Slack tokens, Google API keys, PEM private key
/// blocks, connection-string passwords, and inline `key=value` secret
/// assignments. The capture-preserving patterns keep surrounding context and
/// redact only the secret portion.
#[must_use]
pub fn scrub_string(s: &str) -> String {
    let mut out = s.to_owned();
    for re in string_patterns() {
        out = re.replace_all(&out, REDACTED).into_owned();
    }
    for (re, replacement) in capture_patterns() {
        out = re.replace_all(&out, *replacement).into_owned();
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

    #[test]
    fn redacts_connection_string_password_keeps_host() {
        // Valid DSNs URL-encode reserved chars, so the password has no raw '@'.
        let out = scrub_string("postgres://svc_user:s3cr3tPass@db.internal:5432/app");
        assert!(out.contains("[REDACTED]"), "got: {out}");
        assert!(!out.contains("s3cr3tPass"), "password leaked: {out}");
        // host + user + scheme preserved for diagnosability
        assert!(out.contains("postgres://svc_user:"));
        assert!(out.contains("@db.internal:5432/app"));
    }

    #[test]
    fn redacts_inline_assignments_keeps_key() {
        let out = scrub_string("export DB_PASSWORD=hunter2 && api_key: \"abc123def456\"");
        assert!(!out.contains("hunter2"), "value leaked: {out}");
        assert!(!out.contains("abc123def456"), "value leaked: {out}");
        // key names preserved
        assert!(out.contains("DB_PASSWORD="));
        assert!(out.contains("api_key"));
    }

    #[test]
    fn redacts_slack_and_google_keys() {
        let out = scrub_string(
            "tok=xoxb-1234567890-abcdefghijklmnop key=AIzaSyA1234567890abcdefghijklmnopqrstuvw",
        );
        assert!(!out.contains("xoxb-1234567890"), "slack leaked: {out}");
        assert!(!out.contains("AIzaSyA1234567890"), "google leaked: {out}");
    }

    #[test]
    fn preserves_ordinary_assignments() {
        // Non-secret key=value must survive untouched.
        let out = scrub_string("status=completed path=/usr/bin count=42");
        assert_eq!(out, "status=completed path=/usr/bin count=42");
    }
}
