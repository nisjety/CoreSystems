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

// ---- The durable loop fold (build plan WS03 S6) ----
//
// A fold is conversation content saved so a run can continue after a pause or a
// crash. It must never be a credential store: session-core refuses a fold that parses
// badly, and `LoopFoldV1::to_bytes` refuses the WHOLE fold for one credential-shaped
// string. `scrub_fold` is the writer's pass between the two, and it works per field:
// a credential-shaped piece of the conversation is stubbed and REPORTED (the writer
// then saves a tombstone, never a resumable fold with a stub in it), and only a call's
// arguments are withheld in place.

use execution_core::scrub::{scrub_fold, FoldScrub};
use mp_contracts::loop_fold::{
    Authority, FoldCall, FoldCompleted, FoldMessage, LoopFoldV1, PauseKind, PendingPause,
    RoundInFlight, FOLD_VERSION,
};

const WITHHELD: &str = "[content withheld from the saved state: it looked like a credential]";

fn authority() -> Authority {
    Authority {
        mode: "ask".to_owned(),
        plan_mode: false,
        autonomy_rung: "workspace_write".to_owned(),
        space_id: String::new(),
        min_privacy_tier: 1,
        provider_hint: String::new(),
        tool_names: vec!["delete_records".to_owned(), "knowledge_search".to_owned()],
        max_rounds: 8,
    }
}

fn message(role: &str, content: &str) -> FoldMessage {
    FoldMessage {
        role: role.to_owned(),
        content: content.to_owned(),
        name: String::new(),
        compaction_summary: String::new(),
    }
}

fn call(id: &str, name: &str, arguments_json: &str, step_id: &str) -> FoldCall {
    FoldCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments_json: arguments_json.to_owned(),
        step_id: step_id.to_owned(),
    }
}

/// A fold shaped like a pause: an open round with one finished call and one pending.
fn open_round_fold() -> LoopFoldV1 {
    LoopFoldV1 {
        v: FOLD_VERSION,
        run_id: "run-1".to_owned(),
        segment_no: 1,
        rounds_executed: 2,
        delegated_rounds: 0,
        step_seq: 2,
        children_started: 0,
        grounded: false,
        compaction_triggered: false,
        attempted_retrievals: vec!["hybrid|5|find the contract".to_owned()],
        messages: vec![
            message("system", "You are a careful agent."),
            message("user", "Delete the stale records"),
        ],
        round_in_flight: Some(RoundInFlight {
            assistant_content: "I will look, then delete.".to_owned(),
            calls: vec![
                call("call-1", "knowledge_search", r#"{"query":"stale"}"#, "tool_1_call-1"),
                call("call-2", "delete_records", r#"{"table":"stale"}"#, "tool_2_call-2"),
            ],
            completed: vec![FoldCompleted {
                step_id: "tool_1_call-1".to_owned(),
                output: "found 3 stale records".to_owned(),
                error: String::new(),
                call_id: "call-1".to_owned(),
            }],
        }),
        pending: Some(PendingPause {
            kind: PauseKind::Approval,
            pause_ref: "run-1:tool_2_call-2".to_owned(),
            step_id: "tool_2_call-2".to_owned(),
            call: "delete_records".to_owned(),
            action_fingerprint: "abc".to_owned(),
        }),
        authority: authority(),
        prompt_digest: "digest".to_owned(),
        extensions: serde_json::Map::new(),
    }
}

#[test]
fn scrub_fold_keeps_the_fold_parseable_and_removes_every_seeded_secret() {
    // Assembled at runtime so no literal secret sits in committed source.
    let api_key = format!("sk-{}", "seededkey1234567890abcdef");
    let jwt = "eyJhbGciOiJIUzI1NiJ.eyJzdWIiOiJ1c2VyLTEifQ.abcdefghijklmnopqrstuvwxyz0123456789";
    let password = "hunter2-do-not-log";
    let bearer = format!("Bearer {jwt}");

    let mut fold = open_round_fold();
    fold.messages[1].content = format!("use {api_key} to call it");
    fold.messages.push(message("user", &format!("curl with {bearer}")));
    fold.messages.push(message("assistant", &format!("the db_password={password} is set")));
    let round = fold.round_in_flight.as_mut().unwrap();
    round.assistant_content = format!("I found {api_key} and {jwt} in the file");
    round.completed[0].output = format!("response carried {bearer}");
    round.completed.push(FoldCompleted {
        step_id: String::new(),
        output: String::new(),
        error: format!("failed: password={password}"),
        call_id: "call-3".to_owned(),
    });

    let report = scrub_fold(&mut fold);
    assert!(
        !report.conversation_withheld(),
        "every seeded secret was redacted in place, nothing had to be stubbed: {report:?}"
    );

    let bytes = fold.to_bytes().expect("a scrubbed fold passes the credential gate");
    let text = std::str::from_utf8(&bytes).expect("utf8");
    for secret in [api_key.as_str(), jwt, password] {
        assert!(!text.contains(secret), "{secret} leaked into the fold: {text}");
    }
    assert!(text.contains(REDACTED), "the redaction is visible");
    // It still parses, and the authority (which a key-based scrub would destroy) and
    // every structural field survived.
    let back = LoopFoldV1::from_bytes(&bytes).expect("a scrubbed fold still parses");
    assert_eq!(back.authority, authority());
    assert_eq!(back.run_id, "run-1");
    assert_eq!(back.rounds_executed, 2);
    assert_eq!(back.pending, open_round_fold().pending);
    assert_eq!(back.round_in_flight.as_ref().unwrap().calls.len(), 2);
    assert_eq!(back.messages[0].content, "You are a careful agent.", "clean text is untouched");
}

/// Pins WHY `scrub_fold` is per field: the generic JSON scrub redacts any object key
/// matching `auth`, and `authority` matches, so scrubbing the serialized fold would
/// turn the one thing a resume must read intact into "[REDACTED]".
#[test]
fn key_based_scrubbing_over_a_whole_fold_would_destroy_the_authority() {
    let mut value = serde_json::to_value(open_round_fold()).expect("serialize");
    scrub_json_value(&mut value);
    assert_eq!(value["authority"], json!(REDACTED));
    assert!(
        serde_json::from_value::<LoopFoldV1>(value).is_err(),
        "a key-scrubbed fold is no longer a fold"
    );
    // The per-field pass leaves it alone.
    let mut fold = open_round_fold();
    assert_eq!(scrub_fold(&mut fold), FoldScrub::default(), "a clean fold withholds nothing");
    assert_eq!(fold.authority, authority());
}

#[test]
fn arguments_that_scrubbing_would_change_are_withheld_not_rewritten() {
    let secret = format!("sk-{}", "argumentkey1234567890abcd");
    let mut fold = open_round_fold();
    let clean = r#"{"table":"stale","limit":5}"#;
    let round = fold.round_in_flight.as_mut().unwrap();
    round.calls = vec![
        call("call-1", "knowledge_search", clean, "tool_1_call-1"),
        // A sensitive KEY: the JSON scrub would rewrite its value.
        call("call-2", "http_post", r#"{"url":"https://x","password":"hunter2"}"#, "tool_2_call-2"),
        // A secret VALUE under an innocent key.
        call("call-3", "http_post", &format!(r#"{{"body":"{secret}"}}"#), "tool_3_call-3"),
        // Not JSON at all, but carrying a secret.
        call("call-4", "shell", &format!("echo {secret}"), "tool_4_call-4"),
        // Not JSON and clean: kept byte for byte.
        call("call-5", "shell", "echo hello", "tool_5_call-5"),
    ];
    let report = scrub_fold(&mut fold);
    assert_eq!(report.withheld_arguments, 3, "the three calls whose arguments were cleared");
    assert!(
        !report.conversation_withheld(),
        "cleared arguments are by design: the fold stays resumable"
    );
    let calls = &fold.round_in_flight.as_ref().unwrap().calls;
    assert_eq!(calls[0].arguments_json, clean, "a clean call keeps its arguments byte for byte");
    assert_eq!(calls[1].arguments_json, "", "a sensitive key withholds the arguments");
    assert_eq!(calls[2].arguments_json, "", "a secret value withholds the arguments");
    assert_eq!(calls[3].arguments_json, "", "a secret in a non-JSON argument is withheld");
    assert_eq!(calls[4].arguments_json, "echo hello");
    // Withheld, never rewritten: the stored text is empty, not a half-redacted call a
    // resume could mistake for what the model asked.
    let text = serde_json::to_string(&fold).unwrap();
    assert!(!text.contains(&secret) && !text.contains("hunter2"), "{text}");
    assert!(!text.contains(REDACTED), "no rewritten argument: {text}");
    // Identity fields are untouched, so the call can still be matched.
    assert_eq!(calls[1].id, "call-2");
    assert_eq!(calls[1].step_id, "tool_2_call-2");
    assert!(fold.to_bytes().is_ok());
}

#[test]
fn a_credential_shaped_word_is_stubbed_and_reported_so_the_writer_can_tombstone_the_fold() {
    let mut fold = open_round_fold();
    fold.messages.push(message("assistant", "Set the Authorization: header before calling."));
    fold.messages.push(message("user", "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----"));
    fold.round_in_flight.as_mut().unwrap().assistant_content =
        "the x-api-key header is required".to_owned();
    // Without the pass the WHOLE fold is refused for these benign words.
    assert!(fold.to_bytes().is_err(), "the control: the gate refuses the fold as it stands");

    let report = scrub_fold(&mut fold);
    // Three conversation fields were stubbed: the writer must not save THIS fold as a
    // resumable one (it saves a tombstone), because a resume cannot see a stub.
    assert_eq!(report.withheld_conversation, 3);
    assert!(report.conversation_withheld());

    assert_eq!(fold.messages[2].content, WITHHELD);
    assert_eq!(fold.messages[3].content, WITHHELD);
    assert_eq!(fold.round_in_flight.as_ref().unwrap().assistant_content, WITHHELD);
    // The scrubbed copy serializes, but it must never be saved as a resumable fold: the
    // report above is what makes the writer drop it for a tombstone.
    let bytes = fold.to_bytes().expect("the scrubbed copy serializes with the offending fields stubbed");
    assert!(LoopFoldV1::from_bytes(&bytes).is_ok());
    // The clean fields around them are untouched.
    assert_eq!(fold.messages[1].content, "Delete the stale records");
    assert_eq!(fold.messages[0].content, "You are a careful agent.");
}

// ---- Attachments of a Do run (build plan WS08-S6) ----
//
// A run's attachments are request-scope memory: files in the run-inputs registry, images
// as inference parts. The saved fold holds the conversation only, so the one trace an
// attachment leaves in it is the seed's note, which names a file and its size and never
// carries its bytes. The fold has no field an attachment could be written to.

#[test]
fn a_fold_of_a_run_with_attachments_carries_the_note_but_never_the_bytes() {
    use mp_contracts::attachments::{sandbox_note, sha256_hex};
    use mp_contracts::model_plane::v1::{RunAttachment, RunAttachmentKind};

    let secret_cells = b"month,revenue\nsecret-cell-4711,10\n";
    let image_bytes = b"\x89PNG-bytes-that-must-not-leak";
    let attachment = |name: &str, content: &[u8], kind: RunAttachmentKind| RunAttachment {
        id: name.to_owned(),
        name: name.to_owned(),
        mime: "application/octet-stream".to_owned(),
        size_bytes: content.len() as u64,
        sha256: sha256_hex(content),
        content: content.to_vec(),
        kind: kind as i32,
    };
    let files = [attachment("sales.csv", secret_cells, RunAttachmentKind::File)];
    let _image = attachment("shot.png", image_bytes, RunAttachmentKind::Image);

    let mut fold = open_round_fold();
    fold.messages.insert(1, message("system", &sandbox_note(&files)));
    let report = scrub_fold(&mut fold);
    assert!(!report.conversation_withheld(), "the note is not credential-shaped");

    let bytes = fold.to_bytes().expect("the fold serializes");
    let text = String::from_utf8(bytes).expect("a fold is JSON text");
    assert!(text.contains("attachments/sales.csv"), "the note names the file: {text}");
    assert!(!text.contains("secret-cell-4711"), "file bytes in the fold");
    assert!(!text.contains("PNG-bytes-that-must-not-leak"), "image bytes in the fold");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    for key in value.as_object().expect("a fold is an object").keys() {
        assert!(
            !key.contains("attach") && !key.contains("image"),
            "the fold grew a field an attachment could ride in: {key}"
        );
    }
}
