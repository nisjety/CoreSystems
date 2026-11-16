//! The room canvas agent tools (WS19-S9): `space.canvas.read` and `space.canvas.write`.
//!
//! A room agent reads the room's shared Markdown document and, when a tool policy allows it, saves a new version of it. This
//! module is the execution-core side. It follows the Space self-API adapter (`space_self_api`, WS18-S4) for the registry rows,
//! the payload digest, the idempotency key and the model-facing refusal codes. Its effect differs: the save is a Data Plane call
//! to wiki-store, not a Session Core call.
//!
//! The hop, each step failing closed:
//! 1. A zero-retention or waived run is refused before any Control call (`zdr_durable_mutation_forbidden`).
//! 2. The arguments are parsed against the closed schema. The room comes from the run, never from the model.
//! 3. The payload digest is computed over the flattened arguments, and the idempotency key is derived from the run and the step.
//! 4. Control issues a `run-action-v1` decision bound to that digest and key.
//! 5. wiki-store receives the call with the run's own data-plane bearer and the decision in `X-Space-Decision`. It recomputes
//!    the digest, takes the author from the decision and runs the compare-and-swap. A conflict returns the head metadata, never
//!    the text.
//!
//! The offer is narrow. `SPACE_CANVAS_AGENT_TOOLS` is a rollout switch, off by default, and never an authorization input. A row
//! is offered only through the signed model-action view (`space_self_api::admit_view`) and only to a run whose tool profile
//! carries the canvas marker (`space_self_api::offered_under_profile`, W3-DEC-CANVAS). The write row's level is `ask`, so it is
//! not offered until the approval resume of WS10-S9 exists. An incomplete configuration disables the adapter. No canvas row is
//! attested by an owner in this build, so Capability Core does not list either row for a run.

use std::sync::OnceLock;
use std::time::Duration;

use mp_contracts::space_self_api as sa;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::control_http_client::{
    bounded_secret, env_flag, env_value, service_base_url, service_endpoint as endpoint,
};
use crate::space_self_api::{
    bounded_identifier, golden, CODE_DECISION_UNAVAILABLE, CODE_INVALID_ARGUMENTS,
    CODE_NOT_AUTHORIZED, CODE_NOT_OFFERED, CODE_UNKNOWN_OUTCOME, CODE_ZDR,
};
use crate::tool_bridge::ToolExecution;

/// The read row of the golden registry.
pub(crate) const READ_ACTION: &str = "space.canvas.read";
/// The write row of the golden registry.
pub(crate) const WRITE_ACTION: &str = "space.canvas.write";
/// The rollout switch. Unset or `0` offers no canvas tool. It never authorizes anything.
pub(crate) const SWITCH_ENV: &str = "SPACE_CANVAS_AGENT_TOOLS";
/// The Data Plane base URL of wiki-store, the name the gateway reads too.
const WIKI_STORE_URL_ENV: &str = "WIKI_STORE_URL";
const CONTROL_DECISION_PATH: &str = "/api/v1/internal/spaces/run-action-decision";
const DECISION_HEADER: &str = "x-space-decision";
const MAX_CONTENT_BYTES: usize = 65_536;
const MAX_SUMMARY_CHARS: usize = 200;
const MAX_VERSION_ID_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 16_384;
const MAX_REPLY_BYTES: usize = 1 << 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const DATA_TIMEOUT: Duration = Duration::from_secs(10);
/// The model-facing code for a save the Data Plane refused because of its rate or run limit.
const CODE_LIMIT_REACHED: &str = "limit_reached";

/// The configured clients. Built once per process from the environment when the switch is on.
struct CanvasClient {
    control_base_url: Url,
    decision_token: String,
    wiki_store_url: Url,
    http: Client,
}

static SHARED: OnceLock<Result<Option<CanvasClient>, String>> = OnceLock::new();

/// The process-wide client: `Ok(None)` while the switch is off, `Err` when the switch is on and the configuration is incomplete.
fn shared() -> Result<Option<&'static CanvasClient>, String> {
    match SHARED.get_or_init(CanvasClient::from_env).as_ref() {
        Ok(client) => Ok(client.as_ref()),
        Err(detail) => Err(detail.clone()),
    }
}

/// Whether the rollout switch is on (`SPACE_CANVAS_AGENT_TOOLS`).
#[must_use]
pub(crate) fn enabled() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(std::env::var(SWITCH_ENV).ok().as_deref())
}

impl CanvasClient {
    fn from_env() -> Result<Option<Self>, String> {
        if !enabled() {
            return Ok(None);
        }
        let control = env_value("CONTROL_PLANE_USER_CORE_URL");
        let decision_token = env_value("EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN");
        let wiki_store = env_value(WIKI_STORE_URL_ENV);
        let (Some(control), Some(decision_token), Some(wiki_store)) =
            (control, decision_token, wiki_store)
        else {
            return Err(format!(
                "{SWITCH_ENV} requires CONTROL_PLANE_USER_CORE_URL, EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN and {WIKI_STORE_URL_ENV} together"
            ));
        };
        let allow_insecure = env_flag("EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK");
        let control_base_url = service_base_url(&control, "Control Plane", allow_insecure)?;
        let wiki_store_url = service_base_url(&wiki_store, "wiki-store", allow_insecure)?;
        let decision_token = bounded_secret(&decision_token, "Control run-action service token")?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| "Space canvas HTTP client is unavailable".to_owned())?;
        Ok(Some(Self {
            control_base_url,
            decision_token,
            wiki_store_url,
            http,
        }))
    }
}

/// Whether a name is one of the two canvas rows.
#[must_use]
pub(crate) fn is_canvas_action(name: &str) -> bool {
    name == READ_ACTION || name == WRITE_ACTION
}

/// Whether the view may admit a canvas row: the name is a canvas row and the switch and the configuration are complete.
#[must_use]
pub(crate) fn is_built_action(name: &str) -> bool {
    is_canvas_action(name) && matches!(shared(), Ok(Some(_)))
}

/// The arguments of one call, typed per action. Each struct refuses unknown fields, so the model cannot name a room, a canvas
/// or any field the registry does not declare.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Arguments {
    Read,
    Write {
        /// The version the model read. Empty means the model expects no canvas yet (the flattened form of a null).
        expected_version_id: String,
        content: String,
        /// Empty when the model sent no summary.
        summary: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteInput {
    expected_version_id: String,
    content: String,
    #[serde(default)]
    summary: Option<String>,
}

fn parse_arguments(action_id: &str, input: &str) -> Result<Arguments, String> {
    match action_id {
        READ_ACTION => {
            let text = if input.trim().is_empty() { "{}" } else { input };
            serde_json::from_str::<EmptyInput>(text)
                .map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())?;
            Ok(Arguments::Read)
        }
        WRITE_ACTION => {
            let parsed: WriteInput =
                serde_json::from_str(input).map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())?;
            // A version id is opaque text of at most 128 bytes with no surrounding space. The content is 1 to 65,536
            // bytes with no NUL, and the summary is at most 200 characters with no NUL, as the Data Plane checks them.
            let expected = parsed.expected_version_id;
            if expected.len() > MAX_VERSION_ID_BYTES || expected.trim() != expected.as_str() {
                return Err(CODE_INVALID_ARGUMENTS.to_owned());
            }
            if !(1..=MAX_CONTENT_BYTES).contains(&parsed.content.len())
                || parsed.content.contains('\0')
            {
                return Err(CODE_INVALID_ARGUMENTS.to_owned());
            }
            let summary = parsed.summary.unwrap_or_default();
            if summary.contains('\0') || summary.chars().count() > MAX_SUMMARY_CHARS {
                return Err(CODE_INVALID_ARGUMENTS.to_owned());
            }
            Ok(Arguments::Write {
                expected_version_id: expected,
                content: parsed.content,
                summary,
            })
        }
        _ => Err(CODE_NOT_OFFERED.to_owned()),
    }
}

/// The argument pairs in the golden `argument_order`, flattened as the registry's payload digest frames them.
fn argument_pairs(arguments: &Arguments) -> Vec<(String, String)> {
    match arguments {
        Arguments::Read => Vec::new(),
        Arguments::Write {
            expected_version_id,
            content,
            summary,
        } => vec![
            ("expected_version_id".to_owned(), expected_version_id.clone()),
            ("content".to_owned(), content.clone()),
            ("summary".to_owned(), summary.clone()),
        ],
    }
}

fn payload_digest(
    action_id: &str,
    run_id: &str,
    org_id: &str,
    idempotency_key: &str,
    arguments: &Arguments,
) -> Result<String, String> {
    let pairs = argument_pairs(arguments);
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    sa::payload_digest(action_id, run_id, org_id, idempotency_key, &borrowed)
        .map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())
}

/// The golden schema hash of one row, which the decision must name.
fn schema_hash(action_id: &str) -> Result<String, String> {
    golden()?
        .row(action_id)
        .map(|row| row.schema_sha256.clone())
        .ok_or_else(|| CODE_NOT_OFFERED.to_owned())
}

#[derive(Debug, Serialize)]
struct ControlDecisionRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    action_id: &'a str,
    action_schema_hash: &'a str,
    payload_digest: &'a str,
    idempotency_key: &'a str,
}

#[derive(Debug, Deserialize)]
struct ControlEnvelope {
    data: ControlToken,
}

#[derive(Debug, Deserialize)]
struct ControlToken {
    token: String,
}

/// The save body. Unknown fields are refused by wiki-store, so the body carries only the values the digest binds.
#[derive(Debug, Serialize)]
struct WriteBody<'a> {
    expected_version_id: Option<&'a str>,
    content: &'a str,
    summary: Option<&'a str>,
    idempotency_key: &'a str,
}

#[derive(Deserialize)]
struct DataEnvelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct WriteData {
    #[serde(default)]
    version: Option<VersionRef>,
}

#[derive(Deserialize)]
struct VersionRef {
    version_id: String,
    version_no: i64,
}

#[derive(Deserialize)]
struct ReadData {
    canvas: Option<CanvasInfo>,
    head: Option<HeadFull>,
}

#[derive(Deserialize)]
struct CanvasInfo {
    canvas_id: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct HeadFull {
    version_id: String,
    version_no: i64,
    author_kind: String,
    content: String,
}

/// The head metadata a conflict returns. It carries no content.
#[derive(Deserialize, Serialize)]
struct HeadRef {
    version_id: String,
    version_no: i64,
    author_kind: String,
    created_at: String,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    details: Option<ErrorDetails>,
}

#[derive(Deserialize)]
struct ErrorDetails {
    #[serde(default)]
    head: Option<HeadRef>,
}

/// One answer from wiki-store, with its body held for mapping.
struct WikiReply {
    status: StatusCode,
    body: Vec<u8>,
}

/// Sends one request to wiki-store. `Err(())` means no usable answer came back: the request did not complete, or the reply was
/// larger than any canvas reply can be.
async fn send(request: reqwest::RequestBuilder) -> Result<WikiReply, ()> {
    let response = request.timeout(DATA_TIMEOUT).send().await.map_err(|_| ())?;
    let status = response.status();
    let body = response.bytes().await.map_err(|_| ())?;
    if body.len() > MAX_REPLY_BYTES {
        return Err(());
    }
    Ok(WikiReply {
        status,
        body: body.to_vec(),
    })
}

/// The canvas URL of one Space: `{base}/v1/space-canvas/spaces/{space_ref}` plus the tail. The Space reference is one
/// percent-encoded path segment.
fn canvas_url(base: &Url, space_ref: &str, tail: &[&str]) -> Result<Url, String> {
    let mut url = base.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| "wiki-store URL cannot carry a path".to_owned())?;
        segments
            .pop_if_empty()
            .extend(["v1", "space-canvas", "spaces"])
            .push(space_ref)
            .extend(tail);
    }
    Ok(url)
}

/// Control's `run-action-v1` decision for one call, bound to the digest and the key. A client refusal is `not_authorized`. An
/// unreachable or failing Control is `decision_unavailable`. No effect has happened when this fails.
async fn request_decision(
    client: &CanvasClient,
    run_id: &str,
    org_id: &str,
    action_id: &str,
    schema_hash: &str,
    digest: &str,
    idempotency_key: &str,
) -> Result<String, String> {
    let url = endpoint(&client.control_base_url, CONTROL_DECISION_PATH)?;
    let response = client
        .http
        .post(url)
        .timeout(CONTROL_TIMEOUT)
        .header("x-service-id", "execution-core")
        .header("x-service-token", &client.decision_token)
        .json(&ControlDecisionRequest {
            run_id,
            org_id,
            action_id,
            action_schema_hash: schema_hash,
            payload_digest: digest,
            idempotency_key,
        })
        .send()
        .await
        .map_err(|_| CODE_DECISION_UNAVAILABLE.to_owned())?;
    if response.status().is_client_error() {
        return Err(CODE_NOT_AUTHORIZED.to_owned());
    }
    if !response.status().is_success() {
        return Err(CODE_DECISION_UNAVAILABLE.to_owned());
    }
    let envelope = response
        .json::<ControlEnvelope>()
        .await
        .map_err(|_| CODE_DECISION_UNAVAILABLE.to_owned())?;
    let token = envelope.data.token.trim();
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
        return Err(CODE_DECISION_UNAVAILABLE.to_owned());
    }
    Ok(token.to_owned())
}

/// The model-facing code for a wiki-store refusal. Every code is one the Space self-API set already names, or `limit_reached`.
fn refusal_code(status: StatusCode, code: &str) -> String {
    let mapped = match (status.as_u16(), code) {
        (_, "zdr_persistence_forbidden") | (412, _) => CODE_ZDR,
        (_, "space_canvas_disabled") | (404, _) => CODE_NOT_OFFERED,
        (_, "idempotency_key_conflict") => CODE_UNKNOWN_OUTCOME,
        (429, _) => CODE_LIMIT_REACHED,
        (400, _) | (413, _) => CODE_INVALID_ARGUMENTS,
        (403, _) => CODE_NOT_AUTHORIZED,
        _ => CODE_UNKNOWN_OUTCOME,
    };
    mapped.to_owned()
}

/// The error code of a wiki-store error envelope, or the empty string when the body is not one.
fn error_code(body: &[u8]) -> String {
    serde_json::from_slice::<ErrorEnvelope>(body)
        .map(|envelope| envelope.error.code)
        .unwrap_or_default()
}

/// Maps a save reply. 201 is a saved version, 200 is unchanged, and a head conflict returns the head metadata and no content.
fn map_write(reply: &WikiReply) -> Result<String, String> {
    if reply.status == StatusCode::CREATED {
        let envelope: DataEnvelope<WriteData> =
            serde_json::from_slice(&reply.body).map_err(|_| CODE_UNKNOWN_OUTCOME.to_owned())?;
        return match envelope.data.version {
            Some(version) => Ok(json!({
                "status": "saved",
                "version_id": version.version_id,
                "version_no": version.version_no,
            })
            .to_string()),
            None => Err(CODE_UNKNOWN_OUTCOME.to_owned()),
        };
    }
    if reply.status == StatusCode::OK {
        return Ok(json!({ "status": "unchanged" }).to_string());
    }
    let error: Option<ErrorBody> = serde_json::from_slice::<ErrorEnvelope>(&reply.body)
        .ok()
        .map(|envelope| envelope.error);
    let code = error
        .as_ref()
        .map(|body| body.code.clone())
        .unwrap_or_default();
    if reply.status == StatusCode::CONFLICT && code == "canvas_head_moved" {
        let head = error
            .and_then(|body| body.details)
            .and_then(|details| details.head);
        return Ok(json!({ "status": "conflict", "head": head }).to_string());
    }
    Err(refusal_code(reply.status, &code))
}

/// Maps a read reply. A Space without a canvas is `{"canvas":null}`, which is a state and not a failure.
fn map_read(reply: &WikiReply) -> Result<String, String> {
    if reply.status == StatusCode::NOT_FOUND {
        let code = error_code(&reply.body);
        if code == "canvas_not_found" {
            return Ok(json!({ "canvas": null }).to_string());
        }
        return Err(refusal_code(reply.status, &code));
    }
    if reply.status != StatusCode::OK {
        return Err(refusal_code(reply.status, &error_code(&reply.body)));
    }
    let envelope: DataEnvelope<ReadData> = serde_json::from_slice(&reply.body)
        .map_err(|_| CODE_DECISION_UNAVAILABLE.to_owned())?;
    match (envelope.data.canvas, envelope.data.head) {
        (Some(canvas), Some(head)) => Ok(json!({
            "canvas": {
                "canvas_id": canvas.canvas_id,
                "head_version_id": head.version_id,
                "head_version_no": head.version_no,
                "updated_at": canvas.updated_at,
                "last_author_kind": head.author_kind,
            },
            "content": head.content,
            "truncated": false,
        })
        .to_string()),
        _ => Ok(json!({ "canvas": null }).to_string()),
    }
}

/// Reads the head of the run's room. A read has no arguments, so its digest binds only the run and the key.
async fn read_canvas(
    client: &CanvasClient,
    bearer: &str,
    space_ref: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
) -> Result<String, String> {
    let idempotency_key = sa::idempotency_key(READ_ACTION, run_id, step_id);
    let arguments = Arguments::Read;
    let digest = payload_digest(READ_ACTION, run_id, org_id, &idempotency_key, &arguments)?;
    let schema = schema_hash(READ_ACTION)?;
    let token = request_decision(
        client,
        run_id,
        org_id,
        READ_ACTION,
        &schema,
        &digest,
        &idempotency_key,
    )
    .await?;
    let url = canvas_url(&client.wiki_store_url, space_ref, &[])?;
    let request = client
        .http
        .get(url)
        .bearer_auth(bearer)
        .header(DECISION_HEADER, &token);
    let reply = send(request)
        .await
        .map_err(|()| CODE_DECISION_UNAVAILABLE.to_owned())?;
    map_read(&reply)
}

/// Saves a new version. A decision lives two minutes, so one is requested for each attempt. A transport loss is retried once with
/// the same key and digest, so a save that landed is replayed rather than applied twice. A second loss is `unknown_outcome`.
async fn write_canvas(
    client: &CanvasClient,
    bearer: &str,
    space_ref: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
    arguments: &Arguments,
) -> Result<String, String> {
    let Arguments::Write {
        expected_version_id,
        content,
        summary,
    } = arguments
    else {
        return Err(CODE_NOT_OFFERED.to_owned());
    };
    let idempotency_key = sa::idempotency_key(WRITE_ACTION, run_id, step_id);
    let digest = payload_digest(WRITE_ACTION, run_id, org_id, &idempotency_key, arguments)?;
    let schema = schema_hash(WRITE_ACTION)?;
    let body = WriteBody {
        expected_version_id: (!expected_version_id.is_empty()).then_some(expected_version_id.as_str()),
        content: content.as_str(),
        summary: (!summary.is_empty()).then_some(summary.as_str()),
        idempotency_key: &idempotency_key,
    };
    let url = canvas_url(&client.wiki_store_url, space_ref, &["versions"])?;
    let mut attempt = 0_u8;
    loop {
        let token = request_decision(
            client,
            run_id,
            org_id,
            WRITE_ACTION,
            &schema,
            &digest,
            &idempotency_key,
        )
        .await?;
        let request = client
            .http
            .post(url.clone())
            .bearer_auth(bearer)
            .header(DECISION_HEADER, &token)
            .json(&body);
        match send(request).await {
            Ok(reply) => return map_write(&reply),
            Err(()) if attempt == 0 => attempt += 1,
            Err(()) => return Err(CODE_UNKNOWN_OUTCOME.to_owned()),
        }
    }
}

/// Executes one canvas call. The room is the run's Space. The data-plane bearer is the run's own delegated user credential, and
/// wiki-store checks it against the subject of the decision.
async fn execute(
    tool_name: &str,
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
    space_ref: &str,
    retention_bound: bool,
    data_plane_bearer: Option<&str>,
) -> Result<String, String> {
    if !is_canvas_action(tool_name) {
        return Err(CODE_NOT_OFFERED.to_owned());
    }
    // A zero-retention run is refused before any Control call (design 3.11).
    if retention_bound {
        return Err(CODE_ZDR.to_owned());
    }
    let client = match shared() {
        Ok(Some(client)) => client,
        Ok(None) => return Err(CODE_NOT_OFFERED.to_owned()),
        Err(_) => return Err(CODE_DECISION_UNAVAILABLE.to_owned()),
    };
    let arguments = parse_arguments(tool_name, tool_input)?;
    let run_id = bounded_identifier(run_id)?;
    let org_id = bounded_identifier(org_id)?;
    let step_id = bounded_identifier(step_id)?;
    let space_ref = bounded_identifier(space_ref)?;
    let bearer = data_plane_bearer
        .filter(|value| !value.trim().is_empty() && value.trim() == *value)
        .ok_or_else(|| CODE_NOT_AUTHORIZED.to_owned())?;
    match &arguments {
        Arguments::Read => {
            read_canvas(client, bearer, &space_ref, &run_id, &org_id, &step_id).await
        }
        Arguments::Write { .. } => {
            write_canvas(
                client, bearer, &space_ref, &run_id, &org_id, &step_id, &arguments,
            )
            .await
        }
    }
}

/// Executes one canvas call. Model-facing availability is decided by the signed view. This function refuses any name it does not
/// own, and returns no content on a refusal.
pub(crate) async fn execute_if_configured(
    tool_name: &str,
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
    space_ref: &str,
    retention_bound: bool,
    data_plane_bearer: Option<&str>,
) -> ToolExecution {
    match execute(
        tool_name,
        tool_input,
        run_id,
        org_id,
        step_id,
        space_ref,
        retention_bound,
        data_plane_bearer,
    )
    .await
    {
        Ok(output) => ToolExecution {
            output,
            error: None,
        },
        Err(code) => ToolExecution {
            output: String::new(),
            error: Some(code),
        },
    }
}
