//! The Space workspace's read-only tools (WS24-S3): `workspace_read`,
//! `workspace_grep`, `workspace_glob` and `workspace_list`.
//!
//! A Space-scoped Do run reads the Space's files from its hydrated workspace, the tree
//! `code_interpreter` runs against. Each read is a pure file operation under the canonical root
//! ([`crate::workspace_edit`]): no network, no code and no write. The tools are offered only while
//! [`MODE_ENV`] switches them on, to a run that is Space-scoped and not retention-bound, and they
//! dispatch through the signed capability gate as `cap.workspace.read` (migration 0018).
//!
//! A read sees the Space tree as this run hydrated it. Edits made by other runs are not visible
//! until they are promoted, and the tool descriptions say so, so a model does not take a read for
//! the Space's latest state.

use mp_contracts::model_plane::v1 as pb;
use serde_json::{json, Value};

use crate::workspace_edit::{self, EditError, WorkspaceLock, WorkspaceRoot};

/// The environment switch. Read on every offer and every dispatch. Only the literal `read` or
/// `all` enables the tools; any other value, including unset, leaves them off.
pub const MODE_ENV: &str = "WORKSPACE_TOOLS";

pub const GLOB_TOOL: &str = "workspace_glob";
pub const GREP_TOOL: &str = "workspace_grep";
pub const LIST_TOOL: &str = "workspace_list";
pub const READ_TOOL: &str = "workspace_read";

/// The most matches a grep returns when the call names no `max_results`.
const GREP_DEFAULT_RESULTS: usize = 100;

/// How far the switch reaches. `all` will also offer the write tools once WS24-S4 adds them;
/// until then it offers exactly what `read` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Read,
    All,
}

#[must_use]
pub fn mode() -> Mode {
    parse_mode(std::env::var(MODE_ENV).ok().as_deref())
}

#[must_use]
pub fn parse_mode(raw: Option<&str>) -> Mode {
    match raw {
        Some("read") => Mode::Read,
        Some("all") => Mode::All,
        _ => Mode::Off,
    }
}

/// Whether a run is offered the tools: the switch is on, the run is Space-scoped and it is not
/// retention-bound. A zero-retention or waived run never hydrates the Space to host disk and never
/// takes a durable lease, so it has no workspace to read.
#[must_use]
pub fn offered(mode: Mode, zdr: bool, space_scoped: bool) -> bool {
    mode != Mode::Off && space_scoped && !zdr
}

#[must_use]
pub fn is_workspace_tool(name: &str) -> bool {
    matches!(name, GLOB_TOOL | GREP_TOOL | LIST_TOOL | READ_TOOL)
}

/// The read tools in a fixed order, so the offered set, and the prompt cache built on it, is the
/// same on every run.
#[must_use]
pub fn tool_defs() -> Vec<pb::ToolDefinition> {
    vec![
        def(
            GLOB_TOOL,
            "List the paths in this Space's workspace that match a glob, such as src/**/*.rs. \
             Start the pattern from a literal directory: a pattern that would scan more than 5000 \
             entries is refused. The workspace is the Space's files as this run began plus this \
             run's own edits; other runs' edits are not visible until they are promoted. Results \
             are capped at 500 paths, and truncated says when there are more.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "A glob over root-relative segments: * and ? within a segment, ** across segments"
                    }
                },
                "required": ["pattern"]
            }),
        ),
        def(
            GREP_TOOL,
            "Search the text of this Space's workspace files for a regular expression and return \
             each matching line with its path and line number. It searches the same view as \
             workspace_read: the files as this run began plus this run's own edits, and not other \
             runs' edits until they are promoted. Binary files, symbolic links and files over 1 MiB \
             are skipped. Results are capped, and truncated says when there are more.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "A regular expression in Rust regex syntax"
                    },
                    "path": {
                        "type": "string",
                        "description": "A directory or file to search, relative to the workspace root; default the whole workspace"
                    },
                    "context": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 3,
                        "description": "Lines of context before and after each match; default 0"
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "description": "Most matches to return; default 100"
                    }
                },
                "required": ["pattern"]
            }),
        ),
        def(
            LIST_TOOL,
            "List the entries of a directory in this Space's workspace, with the size of each file. \
             It shows the same view as workspace_read: the files as this run began plus this run's \
             own edits, and not other runs' edits until they are promoted. Results are capped at \
             500 entries, and truncated says when there are more.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "A directory relative to the workspace root; default the root"
                    },
                    "depth": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 3,
                        "description": "How many levels to descend; default 1"
                    }
                }
            }),
        ),
        def(
            READ_TOOL,
            "Read a window of lines from one file in this Space's workspace, with the file's version \
             token. The workspace is the Space's files as this run began plus this run's own edits; \
             other runs' edits are not visible until they are promoted. Lines are 1-based. A window \
             that stops before the end of the file reports next_line, so read on from there. Pass \
             the version token from this read when you later change the file.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "A file path relative to the workspace root"
                    },
                    "start_line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "The first line to return, 1-based; default 1"
                    },
                    "max_lines": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 800,
                        "description": "The most lines to return; default 200"
                    }
                },
                "required": ["path"]
            }),
        ),
    ]
}

fn def(name: &str, description: &str, schema: Value) -> pb::ToolDefinition {
    pb::ToolDefinition {
        name: name.to_owned(),
        description: description.to_owned(),
        parameters_json: schema.to_string(),
    }
}

/// Runs one read tool against the Space's workspace root and returns its result as JSON text. The
/// shared lock is held for the whole call, so a read never interleaves with an edit made by the
/// same run. The file work runs on a blocking thread, because a grep can read many files.
pub async fn execute(
    tool_name: &str,
    tool_input: &str,
    root: &WorkspaceRoot,
    lock: &WorkspaceLock,
) -> Result<String, String> {
    let _shared = lock.read().await;
    let args: Value = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid_argument: the tool input is not JSON: {error}"))?;
    let (tool, root) = (tool_name.to_owned(), root.clone());
    match tokio::task::spawn_blocking(move || run(&tool, &args, &root)).await {
        Ok(result) => result,
        Err(error) => Err(format!(
            "io_error: the workspace read did not finish: {error}"
        )),
    }
}

fn run(tool: &str, args: &Value, root: &WorkspaceRoot) -> Result<String, String> {
    let value = match tool {
        GLOB_TOOL => glob(args, root),
        GREP_TOOL => grep(args, root),
        LIST_TOOL => list(args, root),
        READ_TOOL => read(args, root),
        other => Err(format!(
            "invalid_argument: {other} is not a workspace read tool"
        )),
    }?;
    Ok(value.to_string())
}

fn read(args: &Value, root: &WorkspaceRoot) -> Result<Value, String> {
    let path = text_arg(args, "path")?;
    let outcome = workspace_edit::read(
        root,
        path,
        optional_whole(args, "start_line")?.unwrap_or(1),
        optional_whole(args, "max_lines")?,
    )
    .map_err(refused)?;
    Ok(json!({
        "path": path,
        "version": outcome.version.as_str(),
        "total_lines": outcome.total_lines,
        "start_line": outcome.start_line,
        "lines": outcome.lines,
        "next_line": outcome.next_line,
    }))
}

fn grep(args: &Value, root: &WorkspaceRoot) -> Result<Value, String> {
    let outcome = workspace_edit::grep(
        root,
        text_arg(args, "pattern")?,
        optional_text(args, "path")?,
        optional_whole(args, "context")?.unwrap_or(0),
        optional_whole(args, "max_results")?.unwrap_or(GREP_DEFAULT_RESULTS),
    )
    .map_err(refused)?;
    let matches: Vec<Value> = outcome
        .matches
        .iter()
        .map(|found| {
            json!({
                "path": found.path,
                "line": found.line,
                "text": found.text,
                "before": found.before,
                "after": found.after,
            })
        })
        .collect();
    Ok(json!({"matches": matches, "truncated": outcome.truncated}))
}

fn glob(args: &Value, root: &WorkspaceRoot) -> Result<Value, String> {
    let outcome = workspace_edit::glob(root, text_arg(args, "pattern")?).map_err(refused)?;
    Ok(json!({"paths": outcome.paths, "truncated": outcome.truncated}))
}

fn list(args: &Value, root: &WorkspaceRoot) -> Result<Value, String> {
    let outcome = workspace_edit::list(
        root,
        optional_text(args, "path")?,
        optional_whole(args, "depth")?.unwrap_or(1),
    )
    .map_err(refused)?;
    let entries: Vec<Value> = outcome
        .entries
        .iter()
        .map(|entry| json!({"path": entry.path, "is_dir": entry.is_dir, "size": entry.size}))
        .collect();
    Ok(json!({"entries": entries, "truncated": outcome.truncated}))
}

/// The closed code first, so the model can branch on it, then the sentence it can show.
fn refused(error: EditError) -> String {
    format!("{}: {error}", error.code())
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("invalid_argument: {key} must be a string"))
}

fn optional_text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => text_arg(args, key).map(Some),
    }
}

fn optional_whole(args: &Value, key: &str) -> Result<Option<usize>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|whole| usize::try_from(whole).ok())
            .map(Some)
            .ok_or_else(|| format!("invalid_argument: {key} must be a whole number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "verevon-workspace-tools-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn the_switch_is_strict_and_defaults_off() {
        assert_eq!(parse_mode(None), Mode::Off);
        assert_eq!(parse_mode(Some("")), Mode::Off);
        assert_eq!(parse_mode(Some("true")), Mode::Off);
        assert_eq!(parse_mode(Some("READ")), Mode::Off);
        assert_eq!(parse_mode(Some(" read")), Mode::Off);
        assert_eq!(parse_mode(Some("read")), Mode::Read);
        assert_eq!(parse_mode(Some("all")), Mode::All);
    }

    #[test]
    fn workspace_tools_absent_without_space() {
        assert!(!offered(Mode::Read, false, false));
        assert!(!offered(Mode::All, false, false));
    }

    #[test]
    fn workspace_tools_absent_under_zdr() {
        assert!(!offered(Mode::Read, true, true));
        assert!(!offered(Mode::All, true, true));
    }

    #[test]
    fn workspace_tools_absent_when_flag_off() {
        assert!(!offered(Mode::Off, false, true));
        assert!(offered(Mode::Read, false, true));
        assert!(offered(Mode::All, false, true));
    }

    #[test]
    fn every_read_tool_is_bound_to_the_read_capability() {
        let names: Vec<String> = tool_defs().into_iter().map(|def| def.name).collect();
        assert_eq!(
            names,
            [
                "workspace_glob",
                "workspace_grep",
                "workspace_list",
                "workspace_read"
            ]
        );
        for name in &names {
            assert!(
                is_workspace_tool(name),
                "{name} is not recognised as a workspace tool"
            );
            assert_eq!(
                crate::capability_policy::trusted_capability_id(name).as_deref(),
                Some("cap.workspace.read"),
                "{name} must bind to cap.workspace.read"
            );
        }
    }

    #[test]
    fn no_workspace_tool_name_trips_the_risky_substring_gate() {
        for def in tool_defs() {
            assert!(
                !crate::permission::is_risky_tool(&def.name),
                "{} trips the risky-name gate",
                def.name
            );
        }
    }

    #[test]
    fn every_schema_is_a_json_object_schema() {
        for def in tool_defs() {
            let schema: Value = serde_json::from_str(&def.parameters_json)
                .unwrap_or_else(|error| panic!("{} schema is not JSON: {error}", def.name));
            assert_eq!(schema["type"], "object", "{}", def.name);
            assert_eq!(schema["additionalProperties"], false, "{}", def.name);
        }
    }

    #[tokio::test]
    async fn a_read_returns_the_version_token_the_edit_engine_checks() {
        let dir = scratch("read");
        std::fs::write(dir.join("notes.txt"), b"alpha\nbeta\n").expect("write");
        let root = WorkspaceRoot::new(&dir).expect("root");
        let lock: WorkspaceLock = std::sync::Arc::new(tokio::sync::RwLock::new(()));
        let output = execute(READ_TOOL, r#"{"path":"notes.txt"}"#, &root, &lock)
            .await
            .expect("read");
        let value: Value = serde_json::from_str(&output).expect("json");
        assert_eq!(
            value["version"],
            workspace_edit::Version::of(b"alpha\nbeta\n").as_str()
        );
        assert_eq!(value["lines"], json!(["alpha", "beta"]));
        assert!(value["next_line"].is_null());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_path_that_leaves_the_workspace_is_refused_with_its_code() {
        let dir = scratch("escape");
        let root = WorkspaceRoot::new(&dir).expect("root");
        let lock: WorkspaceLock = std::sync::Arc::new(tokio::sync::RwLock::new(()));
        let error = execute(READ_TOOL, r#"{"path":"../outside.txt"}"#, &root, &lock)
            .await
            .expect_err("refused");
        assert!(error.starts_with("outside_root:"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
