//! `visma_submit_feedback`: a local, append-only log of problems with this server (a wrong skill, a
//! missing endpoint, an unclear result). Nothing is sent anywhere. The operator reads the file.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};

pub const NAME: &str = "visma_submit_feedback";
pub const KINDS: &[&str] = &[
    "wrong_skill",
    "missing_skill",
    "wrong_result",
    "bug",
    "suggestion",
];
const MAX_MESSAGE_CHARS: usize = 1_000;
const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    kind: String,
    message: String,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    skill: Option<String>,
}

/// Where the log lives and who is recorded as the author (the configured caller label).
pub struct SubmitFeedback {
    pub path: PathBuf,
    pub caller: String,
}

#[async_trait]
impl Tool for SubmitFeedback {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: NAME,
            description: "Report a problem with this Visma server to its operator: a skill that is wrong or \
                missing, a result that looked wrong, a bug, or a suggestion. The report is stored in a local \
                file only; nothing is sent to Visma or to anyone else. Write the message in plain words, \
                and never put credentials, tokens or personal data in it. Use it when the user asks you to \
                report something, or when a skill has clearly misled you."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": KINDS},
                    "message": {"type": "string", "maxLength": MAX_MESSAGE_CHARS, "description": "what happened and what was expected"},
                    "tool": {"type": "string", "maxLength": MAX_NAME_CHARS, "description": "the tool involved, if any"},
                    "skill": {"type": "string", "maxLength": MAX_NAME_CHARS, "description": "the skill involved, if any"}
                },
                "required": ["kind", "message"],
                "additionalProperties": false
            }),
            kind: ToolKind::Plan,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: Args = parse_args(args)?;
        if !KINDS.contains(&a.kind.as_str()) {
            return Err(ToolError::InvalidArgs(format!(
                "kind must be one of {}",
                KINDS.join(", ")
            )));
        }
        let message = clean(&a.message, MAX_MESSAGE_CHARS, "message")?;
        if message.is_empty() {
            return Err(ToolError::InvalidArgs("message must not be empty".into()));
        }
        let tool = optional(a.tool, "tool")?;
        let skill = optional(a.skill, "skill")?;
        let id = format!("fb-{:x}", Utc::now().timestamp_micros());
        let record = json!({
            "id": id,
            "at": Utc::now().to_rfc3339(),
            "caller": self.caller,
            "kind": a.kind,
            "tool": tool,
            "skill": skill,
            "message": message,
        });
        append(&self.path, &record)
            .map_err(|e| ToolError::Failed(format!("could not store the feedback: {e}")))?;
        Ok(json!({
            "stored": true,
            "id": id,
            "file": self.path.display().to_string(),
            "note": "Stored locally for the operator. Nothing was sent to Visma.",
        }))
    }
}

/// Printable text only (control characters removed), never truncated silently: too long is refused.
fn clean(text: &str, max_chars: usize, field: &str) -> Result<String, ToolError> {
    if text.chars().count() > max_chars {
        return Err(ToolError::InvalidArgs(format!(
            "{field} is longer than {max_chars} characters"
        )));
    }
    Ok(vsv2_common::text::printable(text.trim(), max_chars)
        .trim()
        .to_string())
}

fn optional(value: Option<String>, field: &str) -> Result<Option<String>, ToolError> {
    match value {
        None => Ok(None),
        Some(text) => {
            let cleaned = clean(&text, MAX_NAME_CHARS, field)?;
            Ok((!cleaned.is_empty()).then_some(cleaned))
        }
    }
}

/// Append one JSON line. The file is created owner-readable only, because a report may describe
/// internal details of the setup.
fn append(path: &PathBuf, record: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existed = path.exists();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if !existed {
        restrict(path)?;
    }
    let mut line = record.to_string();
    line.push('\n');
    file.write_all(line.as_bytes())
}

#[cfg(unix)]
fn restrict(path: &PathBuf) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &PathBuf) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(dir: &tempfile::TempDir) -> SubmitFeedback {
        SubmitFeedback {
            path: dir.path().join("nested").join("feedback.jsonl"),
            caller: "mcp:tester".into(),
        }
    }

    fn lines(path: &PathBuf) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn appends_one_record_per_report_with_the_caller_and_time() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let first = t
            .call(json!({"kind": "wrong_skill", "message": "the filter example\nused a name Visma ignores", "skill": "live-erp-queries"}))
            .await
            .unwrap();
        assert_eq!(first["stored"], true);
        t.call(
            json!({"kind": "suggestion", "message": "add an endpoint", "tool": "visma_erp_query"}),
        )
        .await
        .unwrap();
        let records = lines(&t.path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["kind"], "wrong_skill");
        assert_eq!(records[0]["caller"], "mcp:tester");
        assert_eq!(
            records[0]["message"],
            "the filter example\nused a name Visma ignores"
        );
        assert_eq!(records[1]["tool"], "visma_erp_query");
        assert!(records[1]["skill"].is_null());
        assert_eq!(records[0]["id"], first["id"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_log_is_created_owner_readable_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        t.call(json!({"kind": "bug", "message": "x"}))
            .await
            .unwrap();
        let mode = std::fs::metadata(&t.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn invalid_reports_are_refused_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        for bad in [
            json!({"kind": "praise", "message": "x"}),
            json!({"kind": "bug", "message": "   "}),
            json!({"kind": "bug", "message": "x".repeat(MAX_MESSAGE_CHARS + 1)}),
            json!({"kind": "bug", "message": "x", "tool": "t".repeat(MAX_NAME_CHARS + 1)}),
            json!({"kind": "bug", "message": "x", "extra": true}),
        ] {
            assert!(matches!(t.call(bad).await, Err(ToolError::InvalidArgs(_))));
        }
        assert!(!t.path.exists());
    }

    #[tokio::test]
    async fn control_characters_are_stripped_from_stored_text() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        t.call(json!({"kind": "bug", "message": "line one\u{1b}[31m red\u{0}"}))
            .await
            .unwrap();
        let message = lines(&t.path)[0]["message"].as_str().unwrap().to_string();
        assert!(
            !message.contains('\u{1b}') && !message.contains('\u{0}'),
            "{message:?}"
        );
    }

    #[test]
    fn it_is_a_plan_tool_that_changes_only_a_local_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(tool(&dir).def().kind, ToolKind::Plan);
        assert_eq!(tool(&dir).def().name, NAME);
    }
}
