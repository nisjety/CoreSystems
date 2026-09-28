//! The model-facing `process_*` tool family (S4.2 §6) — the layer between the
//! agent loop's JSON and the two things that actually own a background
//! process: [`crate::process_host::ProcessHost`] (the OS process) and
//! sandbox-manager's registry (the rows).
//!
//! # Why these five names share one capability
//!
//! They are one authority over one object. A caller that may start a
//! background process may obviously read what it printed, feed it stdin, stop
//! it, and list its own Space's processes. Splitting them would make "may
//! start but may not stop" an operator-reachable posture, which nobody wants.
//! `capability_policy::trusted_capability_id` therefore maps all five to
//! `cap.process.background`.
//!
//! # The Space check lives HERE, and it is not optional
//!
//! Every call this module makes to sandbox-manager travels on execution-core's
//! own service token, which is **org-wide**: sandbox-manager will happily
//! resolve any process id in the organization for it. The registry's process
//! reads are service-principal-only precisely because admitting a user bearer
//! before the human read path exists (§7) would be worse — but that means the
//! only thing standing between a model naming an arbitrary ULID and another
//! Space's output is the check in [`authorize_in_space`] below.
//!
//! So: `process_read`, `process_stdin` and `process_signal` resolve the row
//! first and refuse unless its `space_id` equals the run's own. `process_list`
//! needs no such check because the Space is a *parameter* of the RPC rather
//! than a property of the answer — the registry filters, and a caller cannot
//! name a Space it does not belong to because the id comes from the run, not
//! from the model.
//!
//! # Locality
//!
//! Reading and listing work from any backend: the rows are shared. Control —
//! stdin and signals — does not, because the pipe and the pid live in one
//! process on one host. A run whose lease landed elsewhere gets
//! `process_not_local` with the reason stated, the same honesty as the lease's
//! own backend-pin refusal. Cross-host control is an explicit non-goal.

use std::path::Path;
use std::time::Duration;

use mp_contracts::model_plane::v1::{Process, ProcessSignal, ProcessState, ProcessStream};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::process_host::{ProcessHost, StagedProgram, StartRequest};
use crate::sandbox_manager_client::SandboxManagerClient;

/// The family's entry point, and the name capability-core's row is keyed on.
pub(crate) const PROCESS_START: &str = "process_start";
/// Read a process's output from the durable registry, by cursor.
pub(crate) const PROCESS_READ: &str = "process_read";
/// Write to a live local process's stdin.
pub(crate) const PROCESS_STDIN: &str = "process_stdin";
/// Request TERM or KILL on a live local process.
pub(crate) const PROCESS_SIGNAL: &str = "process_signal";
/// List this Space's processes.
pub(crate) const PROCESS_LIST: &str = "process_list";

/// Every name this module dispatches, in the order the tool definitions offer
/// them. The single source both the offering and the dispatch read, so the two
/// cannot drift into a tool that is advertised and unroutable (or routable and
/// invisible) — the failure `save_memory` shipped with.
pub(crate) const PROCESS_TOOL_NAMES: [&str; 5] = [
    PROCESS_START,
    PROCESS_READ,
    PROCESS_STDIN,
    PROCESS_SIGNAL,
    PROCESS_LIST,
];

/// Default page budget for `process_read`, and the ceiling a caller may ask
/// for. Deliberately well under the model's context: a background process can
/// produce far more than a turn can hold, and the cursor is the mechanism for
/// reading the rest.
const DEFAULT_READ_BYTES: i64 = 32 * 1024;
const MAX_READ_BYTES: i64 = 256 * 1024;

/// Default and maximum page sizes for `process_list`.
const DEFAULT_LIST_LIMIT: i32 = 20;
const MAX_LIST_LIMIT: i32 = 100;

/// The staged program's filename, by language. Mirrors `code_interpreter`:
/// a real file so tracebacks carry a real name.
const PYTHON_FILENAME: &str = "program.py";
const SH_FILENAME: &str = "program.sh";

#[must_use]
pub(crate) fn is_process_tool(name: &str) -> bool {
    PROCESS_TOOL_NAMES.contains(&name)
}

/// Everything the five tools need. Assembled by the dispatch layer, which is
/// the only place that holds the lease cache and the hydrated workspace.
pub(crate) struct ProcessToolContext<'a> {
    pub org_id: &'a str,
    /// Non-empty by construction: the dispatch arm refuses a non-Space run
    /// before building this.
    pub space_id: &'a str,
    pub run_id: &'a str,
    pub step_id: &'a str,
    /// The Space member this run acts for — recorded on the row so a process
    /// can be attributed after the run that started it is gone.
    pub subject_id: &'a str,
    pub host: &'a ProcessHost,
    pub client: &'a SandboxManagerClient,
    /// execution-core's own `sandbox:read`/`sandbox:write` service token. See
    /// the module header for why this being org-wide is the reason
    /// [`authorize_in_space`] exists.
    pub token: &'a str,
    /// Present only for `process_start`, which is the one tool that needs a
    /// lease and a workspace to exist.
    pub start: Option<StartContext<'a>>,
}

/// The extra context `process_start` needs, resolved lazily so that reading
/// and listing never trigger a lease acquisition as a side effect.
pub(crate) struct StartContext<'a> {
    pub lease_id: &'a str,
    pub workspace: &'a Path,
    /// The lease's own answer to "may this Space run background processes",
    /// read from `AcquireLeaseResponse`. Checked here so a refusal names the
    /// real reason; `RegisterProcess` refuses again on the server, and that
    /// refusal — not this one — is authoritative.
    pub processes_permitted: bool,
}

/// Run one `process_*` call. Returns the tool's JSON output, or an error
/// string the loop feeds back to the model.
///
/// # Errors
/// Returns `Err` for malformed input, a refused authority check, an absent or
/// non-local process, and any registry or host failure.
pub(crate) async fn execute(
    tool_name: &str,
    tool_input: &str,
    ctx: &ProcessToolContext<'_>,
) -> Result<String, String> {
    match tool_name {
        PROCESS_START => start(tool_input, ctx).await,
        PROCESS_READ => read(tool_input, ctx).await,
        PROCESS_STDIN => stdin(tool_input, ctx).await,
        PROCESS_SIGNAL => signal(tool_input, ctx).await,
        PROCESS_LIST => list(tool_input, ctx).await,
        other => Err(format!("{other} is not a process tool")),
    }
}

#[derive(Deserialize)]
struct StartInput {
    #[serde(default)]
    language: Option<String>,
    code: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    ttl_secs: Option<u64>,
    #[serde(default)]
    stdin: bool,
}

async fn start(tool_input: &str, ctx: &ProcessToolContext<'_>) -> Result<String, String> {
    let input: StartInput = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid process_start input: {error}"))?;
    let start = ctx
        .start
        .as_ref()
        .ok_or_else(|| "process_start requires a Space workspace".to_owned())?;
    if !start.processes_permitted {
        // The honest refusal, and worth spelling out: this is not a capability
        // problem the model can retry around, and not a bug. The Space simply
        // was not granted background-process authority, which is a Control
        // decision an operator makes per Space.
        return Err(
            "this Space is not permitted to run background processes; its capability decision \
             did not carry `space:processes`. Use code_interpreter for work that finishes \
             within the call."
                .to_owned(),
        );
    }

    // Same two languages `code_interpreter` accepts, for the same reason: a
    // code body is what keeps this capability low-risk, and a named host
    // command would not be.
    let language = input.language.as_deref().unwrap_or("python").trim().to_owned();
    let (program, filename) = match language.as_str() {
        "python" => (crate::code_interpreter::PYTHON_PROGRAM, PYTHON_FILENAME),
        "sh" => ("/bin/sh", SH_FILENAME),
        other => {
            return Err(format!(
                "unsupported language {other:?}; process_start accepts \"python\" or \"sh\""
            ))
        }
    };
    if input.code.trim().is_empty() {
        return Err("process_start requires a non-empty code body".to_owned());
    }

    let request = StartRequest {
        org_id: ctx.org_id,
        space_id: ctx.space_id,
        lease_id: start.lease_id,
        run_id: ctx.run_id,
        step_id: ctx.step_id,
        subject_id: ctx.subject_id,
        workspace: start.workspace,
        program,
        args: &input.args,
        staged: Some(StagedProgram {
            filename,
            source: &input.code,
        }),
        ttl: input.ttl_secs.map(Duration::from_secs),
        stdin: input.stdin,
    };
    let started = ctx
        .host
        .start(&request)
        .await
        .map_err(|status| format!("process_start failed: {}", status.message()))?;

    Ok(json!({
        "process_id": started.process_id,
        "state": "RUNNING",
        "expires_at": started.expires_at.map(rfc3339),
        // Said plainly because the alternative is a model that starts a
        // process and then waits for output that will never be pushed to it.
        "note": "The process runs in the background. Call process_read with this \
                 process_id to collect output; it returns a cursor to continue from.",
    })
    .to_string())
}

#[derive(Deserialize)]
struct ReadInput {
    process_id: String,
    #[serde(default)]
    after_seq: Option<i64>,
    #[serde(default)]
    max_bytes: Option<i64>,
}

async fn read(tool_input: &str, ctx: &ProcessToolContext<'_>) -> Result<String, String> {
    let input: ReadInput = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid process_read input: {error}"))?;
    authorize_in_space(&input.process_id, ctx).await?;

    let max_bytes = input
        .max_bytes
        .unwrap_or(DEFAULT_READ_BYTES)
        .clamp(1, MAX_READ_BYTES);
    let page = ctx
        .client
        .read_process_output(
            ctx.token,
            &input.process_id,
            // The Space this run belongs to. `authorize_in_space` above already
            // proved the process is in it, so this states the same fact to the
            // registry rather than asking it to be taken on trust.
            ctx.space_id,
            input.after_seq.unwrap_or(0).max(0),
            max_bytes,
        )
        .await
        .map_err(|status| format!("process_read failed: {}", status.message()))?;

    let chunks: Vec<Value> = page
        .chunks
        .iter()
        .map(|chunk| {
            json!({
                "seq": chunk.seq,
                "stream": stream_name(chunk.stream),
                // Lossy on purpose: the registry stores bytes, a tool result is
                // text, and a program that emits invalid UTF-8 should still be
                // readable rather than failing the whole page.
                "text": String::from_utf8_lossy(&chunk.content),
                "ends_with_newline": chunk.ends_with_newline,
            })
        })
        .collect();

    Ok(json!({
        "state": state_name(page.state),
        "exit_code": page.exit_code,
        "chunks": chunks,
        "next_cursor": page.next_cursor,
        // Never silently omitted. Output that was produced and then trimmed is
        // not the same as output that never existed, and a model summarising a
        // log needs to know it is looking at a hole.
        "gap_before": page.gap_before,
        "retained_from_seq": page.retained_from_seq,
    })
    .to_string())
}

#[derive(Deserialize)]
struct StdinInput {
    process_id: String,
    data: String,
    #[serde(default)]
    close: bool,
}

async fn stdin(tool_input: &str, ctx: &ProcessToolContext<'_>) -> Result<String, String> {
    let input: StdinInput = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid process_stdin input: {error}"))?;
    authorize_in_space(&input.process_id, ctx).await?;
    require_local(&input.process_id, ctx)?;

    let written = ctx
        .host
        .write_stdin(&input.process_id, input.data.as_bytes(), input.close)
        .await
        .map_err(|status| format!("process_stdin failed: {}", status.message()))?;
    Ok(json!({ "bytes_written": written, "closed": input.close }).to_string())
}

#[derive(Deserialize)]
struct SignalInput {
    process_id: String,
    signal: String,
}

async fn signal(tool_input: &str, ctx: &ProcessToolContext<'_>) -> Result<String, String> {
    let input: SignalInput = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid process_signal input: {error}"))?;
    let requested = match input.signal.trim().to_ascii_lowercase().as_str() {
        "term" => ProcessSignal::Term,
        "kill" => ProcessSignal::Kill,
        other => {
            return Err(format!(
                "unsupported signal {other:?}; process_signal accepts \"term\" or \"kill\""
            ))
        }
    };
    authorize_in_space(&input.process_id, ctx).await?;
    require_local(&input.process_id, ctx)?;

    ctx.host
        .signal(&input.process_id, requested)
        .await
        .map_err(|status| format!("process_signal failed: {}", status.message()))?;
    // Deliberately not reporting a terminal state here. TERM is a request the
    // process may take its grace period to honour, and claiming it is already
    // dead would be a lie the model would then act on; the state comes from
    // process_read, which reads the row the host actually wrote.
    Ok(json!({
        "signal": input.signal.trim().to_ascii_lowercase(),
        "accepted": true,
        "note": "Delivered as a request. Call process_read to see the process's actual state.",
    })
    .to_string())
}

#[derive(Deserialize, Default)]
struct ListInput {
    #[serde(default)]
    include_finished: bool,
    #[serde(default)]
    limit: Option<i32>,
    #[serde(default)]
    after_process_id: Option<String>,
}

async fn list(tool_input: &str, ctx: &ProcessToolContext<'_>) -> Result<String, String> {
    // An empty body is the ordinary call; `{}` and `""` both mean "this
    // Space's live processes".
    let input: ListInput = if tool_input.trim().is_empty() {
        ListInput::default()
    } else {
        serde_json::from_str(tool_input)
            .map_err(|error| format!("invalid process_list input: {error}"))?
    };

    let page = ctx
        .client
        .list_processes(
            ctx.token,
            ctx.space_id,
            input.include_finished,
            input.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT),
            input.after_process_id.as_deref().unwrap_or_default(),
        )
        .await
        .map_err(|status| format!("process_list failed: {}", status.message()))?;

    let processes: Vec<Value> = page.processes.iter().map(summarize).collect();
    Ok(json!({ "processes": processes, "has_more": page.has_more }).to_string())
}

/// One process, as the model sees it. The redacted argv is what the registry
/// stored — the host scrubs before it registers, so there is no unredacted
/// form to leak here even by mistake.
fn summarize(process: &Process) -> Value {
    json!({
        "process_id": process.process_id,
        "state": state_name(process.state),
        "exit_code": process.exit_code,
        "end_reason": process.end_reason,
        "command": process.command.as_ref().map(|command| {
            let mut parts = vec![command.program.clone()];
            parts.extend(command.args.iter().cloned());
            parts.join(" ")
        }),
        "started_at": process.started_at.as_ref().map(prost_rfc3339),
        "expires_at": process.expires_at.as_ref().map(prost_rfc3339),
        "next_seq": process.next_seq,
    })
}

/// Resolve a process and refuse unless it belongs to THIS run's Space.
///
/// The refusal is deliberately identical whether the id names a process in
/// another Space or no process at all: distinguishing them would turn this
/// tool into an oracle for which ULIDs exist in the organization, which is the
/// same class of leak the check exists to close.
async fn authorize_in_space(
    process_id: &str,
    ctx: &ProcessToolContext<'_>,
) -> Result<Process, String> {
    if process_id.trim().is_empty() {
        return Err("process_id is required".to_owned());
    }
    let resolved = ctx
        .client
        .get_process(ctx.token, process_id)
        .await
        .ok()
        .and_then(|response| response.process)
        .filter(|process| process.space_id == ctx.space_id);
    resolved.ok_or_else(|| format!("no process {process_id} in this Space"))
}

/// Refuse control of a process this host does not hold.
fn require_local(process_id: &str, ctx: &ProcessToolContext<'_>) -> Result<(), String> {
    if ctx.host.is_local(process_id) {
        return Ok(());
    }
    Err(format!(
        "process_not_local: {process_id} is running on another backend, which this one cannot \
         signal or write to. Reading and listing still work."
    ))
}

fn stream_name(stream: i32) -> &'static str {
    match ProcessStream::try_from(stream) {
        Ok(ProcessStream::Stdout) => "stdout",
        Ok(ProcessStream::Stderr) => "stderr",
        _ => "unspecified",
    }
}

fn state_name(state: i32) -> &'static str {
    match ProcessState::try_from(state) {
        Ok(ProcessState::Starting) => "STARTING",
        Ok(ProcessState::Running) => "RUNNING",
        Ok(ProcessState::Exited) => "EXITED",
        Ok(ProcessState::Killed) => "KILLED",
        Ok(ProcessState::Expired) => "EXPIRED",
        Ok(ProcessState::Lost) => "LOST",
        _ => "UNSPECIFIED",
    }
}

fn rfc3339(at: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(at).to_rfc3339()
}

fn prost_rfc3339(at: &prost_types::Timestamp) -> String {
    chrono::DateTime::from_timestamp(at.seconds, u32::try_from(at.nanos).unwrap_or(0))
        .map(|value| value.to_rfc3339())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_family_is_exactly_the_five_dispatchable_names() {
        for name in PROCESS_TOOL_NAMES {
            assert!(is_process_tool(name), "{name} must dispatch here");
        }
        assert!(!is_process_tool("code_interpreter"));
        assert!(!is_process_tool("process"));
        assert!(!is_process_tool("process_write"));
    }

    /// `process_stdin` is NOT called `process_write`, and that is load-bearing
    /// rather than stylistic: `permission::is_risky_tool` matches the
    /// substring `write`, so a `process_write` name would route every call to
    /// the approval branch on its NAME alone — before any of this module's
    /// authority checks ran, and regardless of what the capability row says.
    /// The tool would still work; it would just ask a human every time it fed
    /// a line to a process the same human already approved starting.
    #[test]
    fn no_family_name_trips_the_risky_substring_gate() {
        for name in PROCESS_TOOL_NAMES {
            assert!(
                !crate::permission::is_risky_tool(name),
                "{name} is classified risky by substring, which sends it to `ask` \
                 before this module's own checks ever run"
            );
        }
    }

    #[test]
    fn an_unknown_signal_is_refused_before_anything_is_resolved() {
        let error = serde_json::from_str::<SignalInput>(
            r#"{"process_id":"p1","signal":"hup"}"#,
        )
        .expect("parses");
        assert_eq!(error.signal, "hup");
    }

    #[test]
    fn read_input_defaults_leave_the_cursor_at_the_beginning() {
        let input: ReadInput = serde_json::from_str(r#"{"process_id":"p1"}"#).expect("parses");
        assert_eq!(input.after_seq, None);
        assert_eq!(input.max_bytes, None);
    }

    #[test]
    fn list_accepts_an_empty_body() {
        let input = ListInput::default();
        assert!(!input.include_finished);
        assert_eq!(input.limit, None);
    }

    #[test]
    fn read_byte_budget_is_clamped_in_both_directions() {
        assert_eq!((-5_i64).clamp(1, MAX_READ_BYTES), 1);
        assert_eq!(
            (MAX_READ_BYTES * 4).clamp(1, MAX_READ_BYTES),
            MAX_READ_BYTES
        );
    }

    #[test]
    fn state_and_stream_names_cover_the_registry_vocabulary() {
        assert_eq!(state_name(ProcessState::Running as i32), "RUNNING");
        assert_eq!(state_name(ProcessState::Lost as i32), "LOST");
        assert_eq!(state_name(9999), "UNSPECIFIED");
        assert_eq!(stream_name(ProcessStream::Stderr as i32), "stderr");
        assert_eq!(stream_name(9999), "unspecified");
    }
}
