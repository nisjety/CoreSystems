//! Secret scrubbing for checkpoint/step payloads.
//!
//! The scrubbers themselves live in the shared `mp-scrub` crate (WS12.S4) and are
//! re-exported here, so every existing caller keeps its path:
//! - [`scrub_json_value`]: redacts values whose object key looks sensitive.
//! - [`scrub_string`]: redacts known secret token patterns in free-form strings.
//! - [`redact_command`]: the same plus the positional `--token <secret>` rule.
//! - [`scrub_live`], [`scrub_url`], [`scrub_live_json`]: the Live set, structured
//!   credential shapes only, for page text on its way to a model or an event.
//!
//! What stays here is [`scrub_fold`]: it walks `mp_contracts::loop_fold::LoopFoldV1`,
//! and `mp-scrub` deliberately depends on nothing of the Model Plane.
//!
//! All of them run before any persistence (session-core `SaveCheckpoint` /
//! `CompleteStep`) to satisfy Security gate #1 in `docs/VERIFICATION.md`.

pub use mp_scrub::{
    redact_command, scrub_json_value, scrub_live, scrub_live_json, scrub_string, scrub_url, MARKER,
};
use serde_json::Value;

#[cfg(test)]
const REDACTED: &str = MARKER;

/// What replaces a saved-state field that still looks like a credential after
/// [`scrub_string`]. Fixed words, so nothing of the original survives.
const FOLD_WITHHELD: &str = "[content withheld from the saved state: it looked like a credential]";

/// Scrub one text field of a fold: the known secret patterns first, then, if the
/// result still has a credential's shape, the whole field is replaced by a stub (the
/// caller counts it; the fold is then never saved as resumable). Returns whether the
/// field was replaced.
fn scrub_fold_text(text: &mut String) -> bool {
    let scrubbed = scrub_string(text);
    if mp_contracts::loop_fold::contains_credential_shape(&scrubbed) {
        FOLD_WITHHELD.clone_into(text);
        true
    } else {
        if scrubbed != *text {
            *text = scrubbed;
        }
        false
    }
}

/// What [`scrub_fold`] had to withhold, so the writer can tell a fold that merely lost a
/// call's arguments from one whose conversation was damaged.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct FoldScrub {
    /// Fields of the conversation replaced by a fixed stub: a message's content, name or
    /// summary, the open round's assistant text, a call's id or name, a completed
    /// outcome's output, error or call id.
    pub withheld_conversation: usize,
    /// Retrieval signatures replaced by a stub. They only feed the duplicate-search gate,
    /// so a stub there damages nothing a resume reads.
    pub withheld_retrievals: usize,
    /// Calls whose arguments were cleared (never rewritten). An empty-argument call is
    /// never redispatched, so the fold stays resumable.
    pub withheld_arguments: usize,
}

impl FoldScrub {
    /// Part of the conversation was replaced by a stub: the fold must not be resumable.
    #[must_use]
    pub const fn conversation_withheld(&self) -> bool {
        self.withheld_conversation > 0
    }
}

/// A call's arguments are never rewritten: a resume must see exactly what the
/// model asked for, or an empty string. They are cleared when any scrubbing pass
/// would have changed them (a sensitive key, a secret value, a secret in text that
/// is not JSON) or when they carry a credential's shape. A call saved with empty
/// arguments is not one a resume may dispatch.
fn withhold_arguments_if_sensitive(arguments_json: &mut String) -> bool {
    let changed = match serde_json::from_str::<Value>(arguments_json) {
        Ok(original) => {
            let mut scrubbed = original.clone();
            scrub_json_value(&mut scrubbed);
            scrubbed != original
        }
        Err(_) => scrub_string(arguments_json) != *arguments_json,
    };
    if changed || mp_contracts::loop_fold::contains_credential_shape(arguments_json) {
        arguments_json.clear();
        true
    } else {
        false
    }
}

/// Scrub a durable-loop fold in place, before it is serialized and saved.
///
/// **Per field, never over the serialized JSON.** [`scrub_json_value`] redacts a
/// whole object whose key matches `auth`, and the fold's `authority` is exactly
/// such a key: scrubbing the serialized fold would replace the authority a resume
/// must read intact with `"[REDACTED]"` (pinned by a test).
///
/// Every free-text field (message content, name and compaction summary, the
/// assistant text of the open round, completed outputs and errors, the retrievals
/// already attempted, and a call's id and name) goes through [`scrub_string`]. A
/// field that still looks like a credential is then replaced whole by a fixed stub
/// and counted: the fold's final gate, `LoopFoldV1::to_bytes`, refuses the
/// **entire** fold for one benign "Authorization:" or certificate header. A stubbed
/// piece of the conversation makes the fold unfit to resume from (see below): the
/// writer drops it and saves a tombstone. Only call arguments are withheld in place,
/// by [`withhold_arguments_if_sensitive`]: byte-exact or empty.
///
/// Structural fields (ids, counters, the authority, the pause descriptor) are the
/// code's own and are not touched. `to_bytes` stays the last gate.
///
/// The result says what was withheld. A stub in a piece of the conversation (the goal, a
/// tool result, a system message, a call's identity) is invisible to a resume, which
/// would continue on a damaged history, so the writer saves a tombstone instead of that
/// fold ([`FoldScrub::conversation_withheld`]); the fold that was scrubbed here must then
/// be dropped, not saved. Cleared call arguments are by design and keep the fold
/// resumable.
pub fn scrub_fold(fold: &mut mp_contracts::loop_fold::LoopFoldV1) -> FoldScrub {
    let mut report = FoldScrub::default();
    let mut text = |field: &mut String| {
        if scrub_fold_text(field) {
            report.withheld_conversation += 1;
        }
    };
    for message in &mut fold.messages {
        text(&mut message.content);
        text(&mut message.name);
        text(&mut message.compaction_summary);
    }
    let mut withheld_retrievals = 0;
    for retrieval in &mut fold.attempted_retrievals {
        if scrub_fold_text(retrieval) {
            withheld_retrievals += 1;
        }
    }
    let mut withheld_arguments = 0;
    if let Some(round) = fold.round_in_flight.as_mut() {
        text(&mut round.assistant_content);
        for call in &mut round.calls {
            text(&mut call.id);
            text(&mut call.name);
            if withhold_arguments_if_sensitive(&mut call.arguments_json) {
                withheld_arguments += 1;
            }
        }
        for done in &mut round.completed {
            text(&mut done.output);
            text(&mut done.error);
            text(&mut done.call_id);
        }
    }
    report.withheld_retrievals = withheld_retrievals;
    report.withheld_arguments = withheld_arguments;
    report
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

    #[test]
    fn redact_command_covers_a_separated_flag_value() {
        // The rule that exists only here: every pattern above keys on a ':'
        // or '=', so `--token abc123` as two argv elements reaches none of
        // them.
        let secret = "s".repeat(20);
        let args = vec![
            "--token".to_owned(),
            secret.clone(),
            "https://api.example.com".to_owned(),
        ];
        let (program, redacted) = redact_command("curl", &args);
        assert_eq!(program, "curl");
        assert_eq!(redacted[1], REDACTED);
        assert_eq!(redacted[2], "https://api.example.com");
        assert_eq!(args[1], secret, "the caller's argv must not be mutated");
    }

    #[test]
    fn redact_command_recognizes_each_flag_spelling() {
        let secret = "s".repeat(20);
        for flag in [
            "--token",
            "--password",
            "-p",
            "--api-key",
            "--API_KEY",
            "--access-key",
            "--auth",
            "--client-secret",
        ] {
            let args = vec![flag.to_owned(), secret.clone()];
            let (_, redacted) = redact_command("tool", &args);
            assert_eq!(redacted[1], REDACTED, "flag {flag} did not redact");
        }
    }

    #[test]
    fn redact_command_leaves_the_joined_form_to_the_patterns() {
        // `--token=x` never reaches the positional rule (an '=' disqualifies
        // it); the inline capture pattern handles it instead, so the two
        // rules compose rather than overlap.
        let secret = "s".repeat(20);
        let args = vec![format!("--token={secret}")];
        let (_, redacted) = redact_command("tool", &args);
        assert!(!redacted[0].contains(&secret));
        assert!(redacted[0].starts_with("--token="));
    }

    #[test]
    fn redact_command_does_not_swallow_the_next_flag_or_an_ordinary_path() {
        // `--token --verbose` is a missing value, not a secret; and `--key`
        // is deliberately not a secret flag because it is a file path far
        // more often than a credential.
        let args = vec!["--token".to_owned(), "--verbose".to_owned()];
        let (_, redacted) = redact_command("tool", &args);
        assert_eq!(redacted, vec!["--token", "--verbose"]);

        let args = vec!["--key".to_owned(), "/etc/ssl/app.pem".to_owned()];
        let (_, redacted) = redact_command("tool", &args);
        assert_eq!(redacted, vec!["--key", "/etc/ssl/app.pem"]);
    }

    #[test]
    fn redact_command_handles_a_trailing_secret_flag() {
        let args = vec!["run".to_owned(), "--token".to_owned()];
        let (_, redacted) = redact_command("tool", &args);
        assert_eq!(redacted, vec!["run", "--token"]);
    }
}
