//! Integration tests for pre-persist secret scrubbing.
//!
//! Exercises `scrub::scrub_string` + `scrub::scrub_json_value` on payloads
//! shaped like the `execute_step` checkpoint envelope built in `grpc.rs`.

use execution_core::scrub::{scrub_json_value, scrub_string};
use serde_json::json;

const REDACTED: &str = "[REDACTED]";

#[test]
fn nested_env_api_key_is_redacted() {
    // Assemble the key-shaped value at runtime so no literal secret token sits
    // in committed source — a scrub test must not itself embed scanner-tripping
    // literals. The scrubber sees the same runtime string either way.
    let api_key = format!("sk-{}", "thisshouldnotleak1234567890");
    let mut checkpoint = json!({
        "step_id": "step-1",
        "status": "completed",
        "output": "ok",
        "error": "",
        "compaction_triggered": false,
        "env": {
            "API_KEY": api_key,
            "DATABASE_URL": "postgres://localhost/db",
            "PATH": "/usr/bin"
        }
    });

    scrub_json_value(&mut checkpoint);

    assert_eq!(checkpoint["env"]["API_KEY"], json!(REDACTED));
    // PATH is benign
    assert_eq!(checkpoint["env"]["PATH"], json!("/usr/bin"));
    // Non-sensitive scalars preserved
    assert_eq!(checkpoint["step_id"], json!("step-1"));
    assert_eq!(checkpoint["status"], json!("completed"));
    assert_eq!(checkpoint["compaction_triggered"], json!(false));
}

#[test]
fn bearer_jwt_in_output_string_is_redacted() {
    let output =
        "called api with Authorization: Bearer eyJhbGciOiJIUzI1NiJ.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c done";
    let scrubbed = scrub_string(output);
    assert!(scrubbed.contains(REDACTED), "expected redaction marker");
    assert!(!scrubbed.contains("eyJhbGciOi"), "jwt leaked: {scrubbed}");
    assert!(
        !scrubbed.contains("Bearer eyJ"),
        "bearer leaked: {scrubbed}"
    );
}

#[test]
fn benign_tool_name_untouched() {
    let mut v = json!({
        "tool_name": "bash",
        "args": ["echo", "hello"],
        "exit_code": 0
    });
    scrub_json_value(&mut v);
    assert_eq!(v["tool_name"], json!("bash"));
    assert_eq!(v["args"], json!(["echo", "hello"]));
    assert_eq!(v["exit_code"], json!(0));
}

#[test]
fn serialized_checkpoint_bytes_contain_no_secrets() {
    let secret_key = format!("sk-{}", "verysecret1234567890abcdef");
    let secret_jwt =
        "eyJhbGciOiJIUzI1NiJ.eyJzdWIiOiJ1c2VyLTEifQ.abcdefghijklmnopqrstuvwxyz0123456789";
    let secret_pw = "hunter2-do-not-log";

    let mut checkpoint = json!({
        "step_id": "step-42",
        "status": "completed",
        "output": format!("authed with Bearer {secret_jwt}"),
        "error": "",
        "compaction_triggered": false,
        "env": {
            "OPENAI_API_KEY": secret_key,
            "PATH": "/usr/bin"
        },
        "password": secret_pw,
    });

    scrub_json_value(&mut checkpoint);

    let bytes = serde_json::to_vec(&checkpoint).expect("serialize checkpoint");
    let as_str = std::str::from_utf8(&bytes).expect("utf8");

    assert!(
        !as_str.contains(secret_key.as_str()),
        "api key leaked in serialized bytes: {as_str}"
    );
    assert!(
        !as_str.contains(secret_jwt),
        "jwt leaked in serialized bytes: {as_str}"
    );
    assert!(
        !as_str.contains(secret_pw),
        "password leaked in serialized bytes: {as_str}"
    );
    assert!(as_str.contains(REDACTED), "no redaction marker present");
}
