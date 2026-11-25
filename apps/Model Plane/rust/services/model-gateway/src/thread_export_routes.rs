//! `GET /v1/threads/:thread_id/export`: the owner's whole-conversation export, as JSON
//! (`verevon.thread-export.v1`) or as Markdown (Wave 3 EP-12 S10, decisions D7 and D9).
//!
//! The gateway forwards the caller's session bearer to Session Core's `ExportThread` and renders
//! what comes back. Session Core is the authority: it decides whether the thread may leave at all
//! and reads the active branch in the same transaction as that decision (EP-12 S9). This module
//! owns the route, the two renderers and the one refusal the gateway makes before any call: a
//! zero-retention or waived caller, as the thread goal reads refuse. Exports are for people in v1,
//! so the route policy refuses a key principal before this handler runs.
//!
//! The route answers 404 `thread_export_disabled` while `MODEL_GATEWAY_THREAD_EXPORT` is not the
//! literal `true`, so the SPA hides the feature. Every answer is an attachment with
//! `Cache-Control: no-store`, because its body is the owner's conversation. Errors are the typed
//! envelope `{error:{code,message,details}}` with `details` an object, except a ZDR refusal, which
//! keeps the catalogue body (with its `feature`) as every ZDR refusal on this gateway does.
//!
//! The run summaries take their token totals and model from the cost-core usage ledger when
//! `MODEL_GATEWAY_THREAD_EXPORT_USAGE` is the literal `true` (F-f10-13). The gateway reads each run's
//! entries as the viewer, with the viewer's own `aud=cost-core` bearer, for the newest fifty runs, and
//! never for a zero-retention or waived viewer. Session Core holds no usage, so its zero and empty
//! values are unknown: with the switch off the copy is as it was, and with it on an unknown value is
//! `null`, or no `model` key, never zero.
//!
//! `POST /v1/threads/import` (EP-12 S15, decision D11) takes such a JSON file back as the request
//! body and asks Session Core's `ImportThread` to make a new thread of it, owned by the caller. The
//! file is untrusted input; Session Core parses it strictly and stores every turn as quoted text.
//! The body is held to [`IMPORT_BODY_LIMIT_BYTES`], Session Core's own file cap. The route shares
//! the export's switch and answers `{data:{thread_id, imported_messages, dropped}}` with 201.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, DefaultBodyLimit, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use futures::future::join_all;
use mp_contracts::model_plane::v1::{
    ExportProfile, ExportThreadRequest, ExportThreadResponse, ExportedMessage, ExportedRun,
    ImportThreadRequest,
};
use mp_contracts::zdr_refusal::Refusal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::auth::{Claims, VerifiedCostBearer, VerifiedSessionBearer};
use crate::budget::{cost_core_base_url, list_run_entries, RunLedgerEntry, RUN_ENTRIES_LIMIT};
use crate::http_routes::{authenticated_session_request, timestamp_to_rfc3339, HttpJsonError};
use crate::state::AppState;

/// The gateway's switch for the export route. Opt-in: only the literal `true` enables it. Read at
/// each call, as the other thread switches are.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_THREAD_EXPORT";

/// The ZDR feature key an export refusal names (`zdr-refusals.json`, EP-12 D7).
const ZDR_FEATURE: &str = "conversation_export";

/// What a JSON export carries as its `format`, and the version of the document it names.
const FORMAT_ID: &str = "verevon.thread-export.v1";
const FORMAT_VERSION: u32 = 1;

/// The typed code while the switch is off. Session Core answers `Unavailable` with the same word
/// while its own export switch is off, and the route gives the same 404 for both.
const DISABLED: &str = "thread_export_disabled";
const DISABLED_MESSAGE: &str = "conversation export is not enabled on this deployment";

/// The typed code of a thread that may not leave, and the trailers that carry it and its reason
/// (Session Core's `thread_export.rs`).
const NOT_EXPORTABLE: &str = "thread_not_exportable";
const ERROR_CODE_TRAILER: &str = "x-mp-error-code";
const ERROR_REASON_TRAILER: &str = "x-mp-error-reason";

/// The ZDR feature key an import refusal names (`zdr-refusals.json`, EP-12 D11).
const IMPORT_ZDR_FEATURE: &str = "conversation_import";

/// The largest import body: Session Core's file cap (`thread_import.rs`), so a file the gateway
/// lets through is never refused for its size behind it.
pub(crate) const IMPORT_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// The typed code of a file Session Core refuses, with its reason on [`ERROR_REASON_TRAILER`].
const IMPORT_INVALID: &str = "thread_import_invalid";

/// Session Core's answer while its import switch is off. The route gives the same 404 as its own
/// switch does.
const IMPORT_DISABLED: &str = "thread_import_disabled";

/// Whether the export route is on in this process.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The switch for the run usage join (F-f10-13). It sits on top of [`ENABLED_ENV`]: only the literal
/// `true` turns it on, read at each call. Off, the copy is as it was before the join existed.
pub const USAGE_ENV: &str = "MODEL_GATEWAY_THREAD_EXPORT_USAGE";

/// Whether the run usage join is on in this process.
#[must_use]
pub fn usage_enabled() -> bool {
    usage_enabled_with(&|key| std::env::var(key).ok())
}

/// The usage switch's answer, with the environment passed in so the rule is testable without it.
#[must_use]
pub fn usage_enabled_with(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    mp_contracts::loop_fold::switch_from(lookup(USAGE_ENV).as_deref())
}

/// The routes, merged into the authed router beside the thread goal.
pub(crate) fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/v1/threads/:thread_id/export",
            axum::routing::get(get_thread_export),
        )
        .route(
            "/v1/threads/import",
            axum::routing::post(post_thread_import)
                .layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)),
        )
}

/// The two renderings of an export. There is no default: a caller always chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Markdown,
}

impl Format {
    fn parse(raw: Option<&str>) -> Result<Self, HttpJsonError> {
        match raw {
            Some("json") => Ok(Self::Json),
            Some("markdown") => Ok(Self::Markdown),
            _ => Err(invalid_query("format", &["json", "markdown"])),
        }
    }

    const fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json; charset=utf-8",
            Self::Markdown => "text/markdown; charset=utf-8",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }
}

/// Which copy of the thread the caller asked for. A caller always chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Profile {
    Full,
    Redacted,
}

impl Profile {
    fn parse(raw: Option<&str>) -> Result<Self, HttpJsonError> {
        match raw {
            Some("full") => Ok(Self::Full),
            Some("redacted") => Ok(Self::Redacted),
            _ => Err(invalid_query("profile", &["full", "redacted"])),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Redacted => "redacted",
        }
    }

    const fn proto(self) -> ExportProfile {
        match self {
            Self::Full => ExportProfile::Full,
            Self::Redacted => ExportProfile::Redacted,
        }
    }
}

/// The query string of the export. Both names are required.
#[derive(Debug, Deserialize)]
pub(crate) struct ExportQuery {
    format: Option<String>,
    profile: Option<String>,
}

/// `GET /v1/threads/:thread_id/export`. The switch and the ZDR gate come first, then the query,
/// then Session Core's read; the render comes after the read, so a refused export never renders. With
/// the usage switch on, the ledger's usage is read between the read and the render, as the viewer.
pub(crate) async fn get_thread_export(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    cost_bearer: Option<VerifiedCostBearer>,
    Path(thread_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, HttpJsonError> {
    switch_gate()?;
    export_gate(&claims)?;
    let format = Format::parse(query.format.as_deref())?;
    let profile = Profile::parse(query.profile.as_deref())?;
    let export = state
        .thread_export_client
        .clone()
        .export_thread(authenticated_session_request(
            ExportThreadRequest {
                thread_id: thread_id.trim().to_owned(),
                profile: profile.proto() as i32,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| export_error(&error))?
        .into_inner();
    let ledger = if usage_enabled() {
        let run_ids: Vec<String> = export.runs.iter().map(|run| run.run_id.clone()).collect();
        Some(
            ledger_usages(
                &state.http_client,
                &cost_core_base_url(),
                cost_bearer.as_ref().map(VerifiedCostBearer::as_str),
                &claims,
                &run_ids,
            )
            .await,
        )
    } else {
        None
    };
    let source = ledger
        .as_ref()
        .map_or(UsageSource::Session, UsageSource::Ledger);
    let exported_at = Utc::now().to_rfc3339();
    let body = match format {
        Format::Json => render_json(&export, profile, &exported_at, source)?,
        Format::Markdown => render_markdown(&export, profile, &exported_at, source)?,
    };
    Ok(attachment(format, &export.thread_id, body))
}

/// The switch first: a deployment without the feature answers the same for every caller.
fn switch_gate() -> Result<(), HttpJsonError> {
    gate_for(enabled())
}

/// The switch's answer, with the switch passed in so the rule is testable without the environment.
fn gate_for(enabled: bool) -> Result<(), HttpJsonError> {
    if enabled {
        Ok(())
    } else {
        Err(envelope(
            StatusCode::NOT_FOUND,
            DISABLED,
            DISABLED_MESSAGE,
            json!({}),
        ))
    }
}

/// A zero-retention caller may not take a copy off the platform (DEC-32). A waived caller is
/// refused here too, as a route mismatch, because its waiver names one route and this is not it.
/// Session Core refuses both again behind this gate.
fn export_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    if crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false))? {
        return Err(crate::zdr_posture::http(&Refusal::feature_not_supported(
            ZDR_FEATURE,
        )));
    }
    Ok(())
}

/// Session Core's refusals as the export answers them. A refusal the ZDR catalogue names keeps its
/// catalogue body; a thread that may not leave names its reason; a thread someone else owns reads
/// as one that does not exist.
fn export_error(error: &tonic::Status) -> HttpJsonError {
    if let Some(refusal) = Refusal::from_status(error) {
        return crate::zdr_posture::http(&refusal);
    }
    match error.code() {
        tonic::Code::Unavailable if error.message() == DISABLED => envelope(
            StatusCode::NOT_FOUND,
            DISABLED,
            DISABLED_MESSAGE,
            json!({}),
        ),
        tonic::Code::FailedPrecondition
            if trailer_text(error, ERROR_CODE_TRAILER) == Some(NOT_EXPORTABLE) =>
        {
            let reason = trailer_text(error, ERROR_REASON_TRAILER);
            envelope(
                StatusCode::PRECONDITION_FAILED,
                NOT_EXPORTABLE,
                not_exportable_message(reason),
                reason.map_or_else(|| json!({}), |reason| json!({ "reason": reason })),
            )
        }
        tonic::Code::NotFound | tonic::Code::PermissionDenied => envelope(
            StatusCode::NOT_FOUND,
            "thread_not_found",
            "thread not found",
            json!({}),
        ),
        tonic::Code::InvalidArgument => envelope(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the export request is not valid",
            json!({}),
        ),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "thread_export_unavailable",
            "the conversation cannot be read right now; try again",
            json!({}),
        ),
        _ => render_failed(),
    }
}

/// The query string of an import. The title is optional: without it the file's title is used.
#[derive(Debug, Deserialize)]
pub(crate) struct ImportQuery {
    title: Option<String>,
}

/// `POST /v1/threads/import`. The switch and the ZDR gate come first, so a refused caller's file
/// is never read; then the body, which must be the export file's UTF-8 text; then Session Core,
/// which parses the file and writes the thread.
pub(crate) async fn post_thread_import(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Query(query): Query<ImportQuery>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, HttpJsonError> {
    switch_gate()?;
    import_gate(&claims)?;
    let export_json = import_body(body)?;
    let imported = state
        .thread_export_client
        .clone()
        .import_thread(authenticated_session_request(
            ImportThreadRequest {
                export_json,
                title: query.title.unwrap_or_default().trim().to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| import_error(&error))?
        .into_inner();
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "data": {
                "thread_id": imported.thread_id,
                "imported_messages": imported.imported_messages,
                "dropped": imported.dropped,
            }
        })),
    )
        .into_response())
}

/// An import writes new durable conversation rows, which a zero-retention caller may never do. A
/// waived caller is refused as a route mismatch, as on the export. Session Core refuses both again.
fn import_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    if crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false))? {
        return Err(crate::zdr_posture::http(&Refusal::feature_not_supported(
            IMPORT_ZDR_FEATURE,
        )));
    }
    Ok(())
}

/// The file's text. A body past the limit is named as such; one that is not UTF-8 cannot be an
/// export file.
fn import_body(body: Result<Bytes, BytesRejection>) -> Result<String, HttpJsonError> {
    let bytes = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            import_too_large()
        } else {
            import_invalid(Some("unreadable"))
        }
    })?;
    String::from_utf8(bytes.to_vec()).map_err(|_| import_invalid(Some("not_utf8")))
}

fn import_too_large() -> HttpJsonError {
    envelope(
        StatusCode::PAYLOAD_TOO_LARGE,
        IMPORT_INVALID,
        "the file is larger than an import takes",
        json!({ "reason": "too_large", "limit_bytes": IMPORT_BODY_LIMIT_BYTES }),
    )
}

fn import_invalid(reason: Option<&str>) -> HttpJsonError {
    envelope(
        StatusCode::BAD_REQUEST,
        IMPORT_INVALID,
        "the file is not a Verevon conversation export this deployment can import",
        reason.map_or_else(|| json!({}), |reason| json!({ "reason": reason })),
    )
}

/// Session Core's refusals as the import answers them. A ZDR refusal keeps its catalogue body; a
/// refused file names its reason; a service credential reads as forbidden.
fn import_error(error: &tonic::Status) -> HttpJsonError {
    if let Some(refusal) = Refusal::from_status(error) {
        return crate::zdr_posture::http(&refusal);
    }
    match error.code() {
        tonic::Code::Unavailable if error.message() == IMPORT_DISABLED => envelope(
            StatusCode::NOT_FOUND,
            IMPORT_DISABLED,
            "conversation import is not enabled on this deployment",
            json!({}),
        ),
        tonic::Code::InvalidArgument
            if trailer_text(error, ERROR_CODE_TRAILER) == Some(IMPORT_INVALID) =>
        {
            let reason = trailer_text(error, ERROR_REASON_TRAILER);
            if reason == Some("too_large") {
                return import_too_large();
            }
            import_invalid(reason)
        }
        tonic::Code::InvalidArgument => import_invalid(None),
        tonic::Code::PermissionDenied => envelope(
            StatusCode::FORBIDDEN,
            "forbidden",
            "an import is made by a person for their own conversations",
            json!({}),
        ),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "thread_import_unavailable",
            "the conversation cannot be imported right now; try again",
            json!({}),
        ),
        _ => envelope(
            StatusCode::BAD_GATEWAY,
            "thread_import_failed",
            "the conversation could not be imported",
            json!({}),
        ),
    }
}

/// A trailer's text, when Session Core set it.
fn trailer_text<'a>(error: &'a tonic::Status, name: &str) -> Option<&'a str> {
    error.metadata().get(name)?.to_str().ok()
}

/// The sentence a refused thread shows, by the closed reason word Session Core names.
fn not_exportable_message(reason: Option<&str>) -> &'static str {
    match reason {
        Some("space_thread") => "a Space conversation cannot be exported",
        Some("non_persistent_run") => {
            "a run here was kept outside retention, so the conversation cannot be exported in full"
        }
        _ => "this conversation cannot be exported in full",
    }
}

/// A typed error body: `{error:{code,message,details}}`, with `details` always an object.
fn envelope(status: StatusCode, code: &str, message: &str, details: Value) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
}

/// A query parameter that is missing or is not one of its choices.
fn invalid_query(parameter: &str, allowed: &[&str]) -> HttpJsonError {
    envelope(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "the export request is not valid",
        json!({ "parameter": parameter, "allowed": allowed }),
    )
}

/// The export read is broken: a field the copy carries as JSON is not JSON of the shape the copy
/// needs. The export is refused rather than sent with that field missing.
fn render_failed() -> HttpJsonError {
    envelope(
        StatusCode::BAD_GATEWAY,
        "thread_export_failed",
        "the conversation could not be exported",
        json!({}),
    )
}

/// The rendering as an attachment, never cached.
fn attachment(format: Format, thread_id: &str, body: String) -> Response {
    let name = attachment_name(thread_id, format);
    let disposition = format!("attachment; filename=\"{name}\"");
    (
        [
            (header::CONTENT_TYPE, format.content_type()),
            (header::CONTENT_DISPOSITION, disposition.as_str()),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

/// `verevon-thread-<id>.<ext>`. The id keeps only ASCII letters, digits, `-` and `_`, so the header
/// value is plain and cannot carry a quote or a line break.
fn attachment_name(thread_id: &str, format: Format) -> String {
    let safe: String = thread_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    format!("verevon-thread-{safe}.{}", format.extension())
}

/// The JSON copy. Its fields are declared in the order they are written, so every export has the
/// same key order, and a turn carries every key whether or not it has anything to put in it.
#[derive(Serialize)]
struct ExportDocument {
    format: &'static str,
    version: u32,
    exported_at: String,
    profile: &'static str,
    thread: ThreadJson,
    messages: Vec<MessageJson>,
    runs: Vec<RunJson>,
    truncated: bool,
}

#[derive(Serialize)]
struct ThreadJson {
    id: String,
    title: String,
    created_at: String,
}

#[derive(Serialize)]
struct MessageJson {
    id: String,
    role: String,
    content: String,
    created_at: String,
    artifacts: Vec<Value>,
    citations: Vec<Value>,
    metadata: Map<String, Value>,
}

#[derive(Serialize)]
struct RunJson {
    id: String,
    status: String,
    origin: String,
    /// Left out while the model is unknown, so that no empty name stands in for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// `null` while unknown: a zero is a count, so it never stands in for an unknown total.
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    created_at: String,
}

/// The JSON copy, pretty-printed, as `verevon.thread-export.v1`.
fn render_json(
    export: &ExportThreadResponse,
    profile: Profile,
    exported_at: &str,
    source: UsageSource<'_>,
) -> Result<String, HttpJsonError> {
    let document = ExportDocument {
        format: FORMAT_ID,
        version: FORMAT_VERSION,
        exported_at: exported_at.to_owned(),
        profile: profile.as_str(),
        thread: ThreadJson {
            id: export.thread_id.clone(),
            title: export.title.clone(),
            created_at: timestamp_to_rfc3339(export.created_at.clone()),
        },
        messages: export
            .messages
            .iter()
            .map(message_json)
            .collect::<Result<Vec<_>, _>>()?,
        runs: export
            .runs
            .iter()
            .map(|run| run_json(run, source))
            .collect(),
        truncated: export.truncated,
    };
    serde_json::to_string_pretty(&document).map_err(|_| render_failed())
}

fn message_json(message: &ExportedMessage) -> Result<MessageJson, HttpJsonError> {
    Ok(MessageJson {
        id: message.message_id.clone(),
        role: message.role.clone(),
        content: message.content.clone(),
        created_at: timestamp_to_rfc3339(message.created_at.clone()),
        artifacts: array_field(&message.artifacts_json)?,
        citations: array_field(&message.citations_json)?,
        metadata: object_field(&message.metadata_json)?,
    })
}

fn run_json(run: &ExportedRun, source: UsageSource<'_>) -> RunJson {
    let (model, input_tokens, output_tokens) = run_usage(run, source);
    RunJson {
        id: run.run_id.clone(),
        status: run.status.clone(),
        origin: run.origin.clone(),
        model,
        input_tokens,
        output_tokens,
        created_at: timestamp_to_rfc3339(run.created_at.clone()),
    }
}

/// Where a copy's run summaries take their usage from. `Session` is the switch off: the session's own
/// summary, which holds zero and an empty model when it does not know them, exactly as the copy has
/// always written them. `Ledger` is the switch on: the usage the ledger answered for, by run id. A run
/// the ledger did not answer for is unknown, and an unknown value is written as `null` or left out.
#[derive(Clone, Copy)]
enum UsageSource<'a> {
    Session,
    Ledger(&'a HashMap<String, LedgerUsage>),
}

/// The model, input tokens and output tokens a run carries under `source`. `None` is unknown.
fn run_usage(
    run: &ExportedRun,
    source: UsageSource<'_>,
) -> (Option<String>, Option<u64>, Option<u64>) {
    match source {
        UsageSource::Session => (
            Some(run.model.clone()),
            Some(run.input_tokens),
            Some(run.output_tokens),
        ),
        UsageSource::Ledger(usages) => {
            usages.get(&run.run_id).map_or((None, None, None), |usage| {
                (
                    usage.model.clone(),
                    Some(usage.input_tokens),
                    Some(usage.output_tokens),
                )
            })
        }
    }
}

/// What the ledger holds for one run, summed over its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LedgerUsage {
    input_tokens: u64,
    output_tokens: u64,
    /// The model most of the run's entries name. Absent when no entry names one.
    model: Option<String>,
}

/// The most runs the usage join looks up: the newest of the export's runs, which come oldest first.
/// Each lookup is one cost-core call, so the bound keeps the export's wait bounded. A run past it is
/// unknown.
const USAGE_LOOKUP_RUNS: usize = 50;

/// The lookups in flight at once.
const USAGE_LOOKUP_CONCURRENCY: usize = 8;

/// The usage the ledger holds for the export's runs, read as the viewer. The newest
/// [`USAGE_LOOKUP_RUNS`] runs are looked up, [`USAGE_LOOKUP_CONCURRENCY`] at a time. A run the ledger
/// has no entries for, that cost-core does not answer for, or that it answers for with a list too long
/// to be whole, is absent from the result. A viewer without a verified cost-core bearer, and a viewer
/// who may not take a copy off the platform, is not looked up at all: the result is empty.
async fn ledger_usages(
    http_client: &reqwest::Client,
    base_url: &str,
    viewer_bearer: Option<&str>,
    claims: &Claims,
    run_ids: &[String],
) -> HashMap<String, LedgerUsage> {
    let mut usages = HashMap::new();
    let Some(bearer) = viewer_bearer else {
        return usages;
    };
    if !usage_lookup_allowed(claims) {
        return usages;
    }
    let newest = &run_ids[run_ids.len().saturating_sub(USAGE_LOOKUP_RUNS)..];
    for wave in newest.chunks(USAGE_LOOKUP_CONCURRENCY) {
        let calls = wave
            .iter()
            .map(|run_id| lookup_run_usage(http_client, base_url, bearer, run_id));
        let answers = join_all(calls).await;
        for (run_id, usage) in wave.iter().zip(answers) {
            if let Some(usage) = usage {
                usages.insert(run_id.clone(), usage);
            }
        }
    }
    usages
}

/// One run's usage as the ledger holds it, or `None` when cost-core gives no list for it.
async fn lookup_run_usage(
    http_client: &reqwest::Client,
    base_url: &str,
    bearer: &str,
    run_id: &str,
) -> Option<LedgerUsage> {
    let entries = list_run_entries(http_client, base_url, bearer, run_id).await?;
    ledger_usage(&entries)
}

/// Whether the viewer's runs may be read from the ledger at all. A zero-retention or waived viewer's
/// runs are never looked up: the export refuses such a viewer, and the lookup keeps to the same rule.
fn usage_lookup_allowed(claims: &Claims) -> bool {
    matches!(
        crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false)),
        Ok(false)
    )
}

/// One run's usage from its ledger entries: the token totals, and the model most of its entries name.
/// `None` when the ledger has no entry for the run, and when the list is as long as cost-core answers
/// with: its older entries may have been cut off, and a short total is not reported as the whole.
fn ledger_usage(entries: &[RunLedgerEntry]) -> Option<LedgerUsage> {
    if entries.is_empty() || entries.len() >= RUN_ENTRIES_LIMIT {
        return None;
    }
    let mut oldest_first: Vec<&RunLedgerEntry> = entries.iter().collect();
    oldest_first.sort_by_key(|entry| entry.created_at);
    let input_tokens = oldest_first
        .iter()
        .fold(0_u64, |sum, entry| sum.saturating_add(entry.input_tokens));
    let output_tokens = oldest_first
        .iter()
        .fold(0_u64, |sum, entry| sum.saturating_add(entry.output_tokens));
    Some(LedgerUsage {
        input_tokens,
        output_tokens,
        model: modal_model(&oldest_first),
    })
}

/// The model the most entries name, blank names left out. A tie goes to the model whose first entry
/// was recorded first. `oldest_first` is in the order the entries were recorded.
fn modal_model(oldest_first: &[&RunLedgerEntry]) -> Option<String> {
    // Each model with its count and the position of its first entry.
    let mut tally: Vec<(&str, usize, usize)> = Vec::new();
    for (position, entry) in oldest_first.iter().enumerate() {
        let model = entry.model.trim();
        if model.is_empty() {
            continue;
        }
        match tally.iter_mut().find(|(name, _, _)| *name == model) {
            Some((_, count, _)) => *count += 1,
            None => tally.push((model, 1, position)),
        }
    }
    tally
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|(name, _, _)| name.to_owned())
}

/// A field the copy carries as JSON text. Empty means the turn has none.
fn json_field(raw: &str) -> Result<Option<Value>, HttpJsonError> {
    if raw.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(raw).map(Some).map_err(|_| render_failed())
}

fn array_field(raw: &str) -> Result<Vec<Value>, HttpJsonError> {
    match json_field(raw)? {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(render_failed()),
    }
}

fn object_field(raw: &str) -> Result<Map<String, Value>, HttpJsonError> {
    match json_field(raw)? {
        None => Ok(Map::new()),
        Some(Value::Object(fields)) => Ok(fields),
        Some(_) => Err(render_failed()),
    }
}

/// The readable copy: a heading, one section per turn with its text artifacts, a table of the runs
/// and a note of what the copy leaves out. The turn text is escaped by [`escape_text`]; an artifact
/// sits in a code fence, where nothing is rendered.
fn render_markdown(
    export: &ExportThreadResponse,
    profile: Profile,
    exported_at: &str,
    source: UsageSource<'_>,
) -> Result<String, HttpJsonError> {
    let heading = or_fallback(one_line(&export.title), "Untitled conversation");
    let thread = one_line(&export.thread_id);
    let label = profile.as_str();
    let mut out = format!(
        "# {heading}\n\nThread {thread}, exported {exported_at} from Verevon. \
         Profile: {label}.\n\n"
    );
    if export.truncated {
        out.push_str(
            "**Truncated.** This export stops at its size limit, so the turns after the last one \
             shown are missing.\n\n",
        );
    }
    for message in &export.messages {
        out.push_str(&format!("## {}\n\n", role_heading(&message.role)));
        let created = timestamp_to_rfc3339(message.created_at.clone());
        if !created.is_empty() {
            out.push_str(&format!("*{created}*\n\n"));
        }
        if !message.content.trim().is_empty() {
            out.push_str(&escape_text(&message.content));
            out.push_str("\n\n");
        }
        for artifact in array_field(&message.artifacts_json)? {
            out.push_str(&artifact_section(&artifact));
        }
    }
    out.push_str("## Runs\n\n");
    if export.runs.is_empty() {
        out.push_str("No runs on this thread.\n\n");
    } else {
        out.push_str("| Run | Status | Origin | Model | Started |\n");
        out.push_str("| --- | --- | --- | --- | --- |\n");
        for run in &export.runs {
            let started = timestamp_to_rfc3339(run.created_at.clone());
            let (model, _, _) = run_usage(run, source);
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                cell(&run.run_id),
                cell(&run.status),
                cell(&run.origin),
                cell(model.as_deref().unwrap_or_default()),
                cell(&started),
            ));
        }
        out.push('\n');
    }
    let note = match profile {
        Profile::Full => {
            "- Citations, tool steps and attachment details: in the JSON export only.\n"
        }
        Profile::Redacted => {
            "- Tool arguments and results, connector payloads, citation snippets, URL query \
             strings and attachment names: left out by the redacted profile.\n"
        }
    };
    out.push_str("## Not included\n\n");
    out.push_str(
        "- Binaries: attachment bodies and generated files are never part of an export.\n",
    );
    out.push_str("- Other branches: only the active branch of the conversation is exported.\n");
    out.push_str(note);
    Ok(out)
}

/// The heading a turn's role is written under. The copy names the speaker, not the storage role.
fn role_heading(role: &str) -> &'static str {
    match role {
        "user" => "You",
        "assistant" => "Verevon",
        "system" => "System",
        _ => "Message",
    }
}

/// A text artifact of a turn: its kind and title as a heading, then its text in a fence.
fn artifact_section(artifact: &Value) -> String {
    let kind = or_fallback(one_line(text_field(artifact, "kind")), "artifact");
    let title = or_fallback(one_line(text_field(artifact, "title")), "Untitled");
    let body = fenced(text_field(artifact, "content"));
    format!("### {kind}: {title}\n\n{body}")
}

fn text_field<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// `text` in a code fence longer than any run of backticks in it, so that the text cannot close
/// the fence early.
fn fenced(text: &str) -> String {
    let longest = text
        .split(|c: char| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest.max(2) + 1);
    let body = text.trim_end_matches('\n');
    format!("{fence}\n{body}\n{fence}\n\n")
}

/// Turn text as the copy writes it. Only `<` becomes `&lt;`: it is the one character that opens
/// markup (a tag, a comment or a declaration) in a viewer that renders HTML. `&` and `>` stay as
/// written, so code and quotations keep their meaning.
fn escape_text(text: &str) -> String {
    text.replace('<', "&lt;")
}

/// Text for one line of the copy (a heading, a title or a table cell): escaped, with line breaks
/// folded to spaces.
fn one_line(text: &str) -> String {
    escape_text(text).replace(|c: char| c == '\r' || c == '\n', " ")
}

/// Text for a table cell: one line, with the pipe escaped so it cannot end the cell.
fn cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// `text`, or `fallback` when `text` is empty.
fn or_fallback(text: String, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware};
    use mp_contracts::zdr_refusal::code;
    use tonic::metadata::MetadataValue;
    use tower::ServiceExt;

    /// A fixed export time, so the copies compare exactly.
    const AT: &str = "2026-11-04T10:00:00Z";

    fn claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "user_id": "user-a",
            "zdr": zdr,
            "scopes": ["session:read"],
        }))
        .expect("a user token decodes")
    }

    fn key_claims() -> Claims {
        serde_json::from_value(json!({
            "sub": "apikey:key-1",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "zdr": false,
            "principal_type": "api_key",
            "key_id": "key-1",
            "acting_user_id": "user-a",
            "scopes": ["agent:runs"],
        }))
        .expect("an api key token decodes")
    }

    fn turn(role: &str, content: &str) -> ExportedMessage {
        ExportedMessage {
            message_id: "message-1".to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            created_at: None,
            artifacts_json: String::new(),
            citations_json: String::new(),
            metadata_json: String::new(),
        }
    }

    fn thread_copy(messages: Vec<ExportedMessage>) -> ExportThreadResponse {
        ExportThreadResponse {
            thread_id: "thread-1".to_owned(),
            title: "Quarterly notes".to_owned(),
            created_at: None,
            messages,
            runs: Vec::new(),
            truncated: false,
        }
    }

    #[test]
    fn json_export_has_stable_key_order() {
        let mut answer = turn("assistant", "Answer");
        answer.artifacts_json =
            r#"[{"id":"a1","kind":"document","title":"Plan","content":"Text","version":1}]"#
                .to_owned();
        answer.metadata_json = r#"{"tool_steps":[]}"#.to_owned();
        let body = render_json(
            &thread_copy(vec![answer]),
            Profile::Full,
            AT,
            UsageSource::Session,
        )
        .expect("the copy renders");
        let positions: Vec<usize> = [
            "\"format\"",
            "\"version\"",
            "\"exported_at\"",
            "\"profile\"",
            "\"thread\"",
            "\"messages\"",
            "\"runs\"",
            "\"truncated\"",
        ]
        .into_iter()
        .map(|key| body.find(key).expect("every top-level key is written"))
        .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "top-level keys are out of order:\n{body}"
        );
        let document: Value = serde_json::from_str(&body).expect("the copy is JSON");
        assert_eq!(document["format"], FORMAT_ID);
        assert_eq!(document["version"], 1);
        assert_eq!(document["profile"], "full");
        assert_eq!(document["messages"][0]["artifacts"][0]["title"], "Plan");
        assert_eq!(document["messages"][0]["citations"], json!([]));
        assert_eq!(document["messages"][0]["metadata"]["tool_steps"], json!([]));
    }

    #[test]
    fn markdown_escapes_user_text() {
        let mut copy = thread_copy(vec![turn("user", "<script>alert(1)</script>")]);
        copy.title = "<img src=x onerror=alert(1)>".to_owned();
        let text = render_markdown(&copy, Profile::Full, AT, UsageSource::Session)
            .expect("the copy renders");
        assert!(!text.contains("<script"), "{text}");
        assert!(!text.contains("<img"), "{text}");
        assert!(text.contains("&lt;script>alert(1)&lt;/script>"), "{text}");
        assert!(text.contains("## You"), "{text}");
    }

    #[test]
    fn refusal_is_typed_envelope_with_object_details() {
        let mut status = tonic::Status::failed_precondition(NOT_EXPORTABLE);
        status
            .metadata_mut()
            .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(NOT_EXPORTABLE));
        status.metadata_mut().insert(
            ERROR_REASON_TRAILER,
            MetadataValue::from_static("non_persistent_run"),
        );
        let (status_code, Json(body)) = export_error(&status);
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], NOT_EXPORTABLE);
        assert_eq!(body["error"]["details"]["reason"], "non_persistent_run");
        assert!(body["error"]["details"].is_object());
        assert!(body["error"]["message"].is_string());
    }

    #[test]
    fn zdr_refusal_keeps_its_catalogue_body() {
        let refusal = Refusal::feature_not_supported(ZDR_FEATURE);
        let status = refusal.grpc_status(tonic::Code::FailedPrecondition);
        let (status_code, Json(body)) = export_error(&status);
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], code::FEATURE_NOT_SUPPORTED);
        assert_eq!(body["error"]["feature"], ZDR_FEATURE);
    }

    #[test]
    fn switch_off_route_absent() {
        let (status_code, Json(body)) = gate_for(false).expect_err("a switched-off route refuses");
        assert_eq!(status_code, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], DISABLED);
        assert!(gate_for(true).is_ok());
    }

    #[tokio::test]
    async fn api_key_principal_is_refused() {
        let app = router()
            .layer(middleware::from_fn(crate::auth::authorize_principal_route))
            .layer(Extension(key_claims()))
            .with_state(AppState::new());
        let request = Request::builder()
            .uri("/v1/threads/thread-1/export?format=json&profile=full")
            .body(Body::empty())
            .expect("a request");
        let response = app.oneshot(request).await.expect("the router answers");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn api_key_principal_cannot_import() {
        let app = router()
            .layer(middleware::from_fn(crate::auth::authorize_principal_route))
            .layer(Extension(key_claims()))
            .with_state(AppState::new());
        let request = Request::builder()
            .method("POST")
            .uri("/v1/threads/import")
            .body(Body::from("{}"))
            .expect("a request");
        let response = app.oneshot(request).await.expect("the router answers");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn zdr_caller_cannot_import() {
        let (status_code, Json(body)) =
            import_gate(&claims(true)).expect_err("a ZDR caller is refused");
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["feature"], IMPORT_ZDR_FEATURE);
        assert!(import_gate(&claims(false)).is_ok());

        let refusal = Refusal::feature_not_supported(IMPORT_ZDR_FEATURE);
        let (status_code, Json(body)) =
            import_error(&refusal.grpc_status(tonic::Code::FailedPrecondition));
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], code::FEATURE_NOT_SUPPORTED);
    }

    #[test]
    fn import_refusal_is_typed_envelope_with_object_details() {
        let mut status = tonic::Status::invalid_argument(IMPORT_INVALID);
        status
            .metadata_mut()
            .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(IMPORT_INVALID));
        status
            .metadata_mut()
            .insert(ERROR_REASON_TRAILER, MetadataValue::from_static("version"));
        let (status_code, Json(body)) = import_error(&status);
        assert_eq!(status_code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], IMPORT_INVALID);
        assert_eq!(body["error"]["details"], json!({ "reason": "version" }));
        assert!(body["error"]["message"].is_string());

        let (status_code, Json(body)) =
            import_error(&tonic::Status::unavailable(IMPORT_DISABLED));
        assert_eq!(status_code, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], IMPORT_DISABLED);
        assert!(body["error"]["details"].is_object());

        let (status_code, _) = import_error(&tonic::Status::permission_denied("service"));
        assert_eq!(status_code, StatusCode::FORBIDDEN);
    }

    #[test]
    fn import_body_must_be_utf8() {
        let text = import_body(Ok(Bytes::from_static(b"{\"format\":1}"))).expect("UTF-8 is read");
        assert_eq!(text, "{\"format\":1}");
        let (status_code, Json(body)) = import_body(Ok(Bytes::from_static(&[0xff, 0xfe])))
            .expect_err("bytes that are not UTF-8 are refused");
        assert_eq!(status_code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["reason"], "not_utf8");
    }

    #[tokio::test]
    async fn import_body_past_the_limit_is_a_typed_413() {
        let app = axum::Router::new().route(
            "/import",
            axum::routing::post(|body: Result<Bytes, BytesRejection>| async move {
                import_body(body).map(|_| StatusCode::OK)
            })
            .layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)),
        );
        let request = Request::builder()
            .method("POST")
            .uri("/import")
            .body(Body::from(vec![b' '; IMPORT_BODY_LIMIT_BYTES + 1]))
            .expect("a request");
        let response = app.oneshot(request).await.expect("the router answers");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let body: Value = serde_json::from_slice(&body).expect("a typed envelope");
        assert_eq!(body["error"]["code"], IMPORT_INVALID);
        assert_eq!(body["error"]["details"]["reason"], "too_large");
    }

    #[test]
    fn zdr_caller_is_refused_before_any_read() {
        let (status_code, Json(body)) =
            export_gate(&claims(true)).expect_err("a ZDR caller is refused");
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["feature"], ZDR_FEATURE);
        assert!(export_gate(&claims(false)).is_ok());
    }

    #[test]
    fn query_choices_are_closed() {
        let (status_code, Json(body)) =
            Format::parse(Some("pdf")).expect_err("pdf is not a format");
        assert_eq!(status_code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["parameter"], "format");
        assert_eq!(body["error"]["details"]["allowed"], json!(["json", "markdown"]));
        assert!(Profile::parse(None).is_err());
        assert_eq!(Format::parse(Some("markdown")).expect("a choice"), Format::Markdown);
    }

    #[test]
    fn redacted_footer_names_what_is_dropped() {
        let copy = thread_copy(vec![turn("user", "hi")]);
        let text = render_markdown(&copy, Profile::Redacted, AT, UsageSource::Session)
            .expect("the copy renders");
        assert!(text.contains("## Not included"), "{text}");
        assert!(text.contains("Binaries"), "{text}");
        assert!(text.contains("Other branches"), "{text}");
        assert!(text.contains("Tool arguments and results"), "{text}");
        assert!(text.contains("Profile: redacted."), "{text}");
    }

    #[test]
    fn truncated_export_says_so() {
        let mut copy = thread_copy(Vec::new());
        copy.truncated = true;
        let text = render_markdown(&copy, Profile::Full, AT, UsageSource::Session)
            .expect("the copy renders");
        assert!(text.contains("**Truncated.**"), "{text}");
        let body =
            render_json(&copy, Profile::Full, AT, UsageSource::Session).expect("the copy renders");
        let document: Value = serde_json::from_str(&body).expect("the copy is JSON");
        assert_eq!(document["truncated"], true);
    }

    #[test]
    fn run_table_cells_cannot_end_a_cell() {
        let mut copy = thread_copy(Vec::new());
        copy.runs = vec![ExportedRun {
            run_id: "run-1".to_owned(),
            status: "completed".to_owned(),
            origin: "chat".to_owned(),
            model: "a|b".to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            created_at: None,
        }];
        let text = render_markdown(&copy, Profile::Full, AT, UsageSource::Session)
            .expect("the copy renders");
        assert!(text.contains("| run-1 | completed | chat | a\\|b |"), "{text}");
    }

    #[test]
    fn artifact_fence_outlasts_its_text() {
        let block = fenced("x ``` y");
        assert!(block.starts_with("````\n"), "{block}");
        assert!(block.contains("x ``` y\n````"), "{block}");
    }

    #[test]
    fn attachment_name_is_plain() {
        assert_eq!(
            attachment_name("thread-1_x\"; evil\r\n", Format::Json),
            "verevon-thread-thread-1_xevil.json"
        );
        assert_eq!(
            attachment_name("t1", Format::Markdown),
            "verevon-thread-t1.md"
        );
    }

    #[test]
    fn a_field_that_is_not_json_refuses_the_export() {
        assert!(array_field("").expect("an empty field is absent").is_empty());
        let (status_code, Json(body)) =
            array_field("{not json").expect_err("a broken field refuses");
        assert_eq!(status_code, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "thread_export_failed");
        assert!(array_field("{}").is_err(), "an object is not an array");
    }

    /// A persistent user whose one connection is waived for a turn: a route mismatch on the plain route.
    fn waived_claims() -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "user_id": "user-a",
            "zdr": false,
            "org_zdr": true,
            "zdr_waiver": {
                "version": 1,
                "provider": mp_contracts::agentic_subscription::PROVIDER_ID,
                "connection": "conn-1",
            },
            "scopes": ["session:read"],
        }))
        .expect("a waived user token decodes")
    }

    fn run(run_id: &str, model: &str) -> ExportedRun {
        ExportedRun {
            run_id: run_id.to_owned(),
            status: "completed".to_owned(),
            origin: "chat".to_owned(),
            model: model.to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            created_at: None,
        }
    }

    fn entry(model: &str, input: u64, output: u64, at: &str) -> RunLedgerEntry {
        RunLedgerEntry {
            model: model.to_owned(),
            input_tokens: input,
            output_tokens: output,
            created_at: chrono::DateTime::parse_from_rfc3339(at)
                .expect("a timestamp")
                .with_timezone(&Utc),
        }
    }

    #[test]
    fn ledger_entries_sum_their_tokens_and_name_the_model_most_of_them_use() {
        let usage = ledger_usage(&[
            entry("model-a", 10, 4, "2026-11-20T10:00:00Z"),
            entry("model-b", 1, 1, "2026-11-20T10:01:00Z"),
            entry("model-a", 7, 3, "2026-11-20T10:02:00Z"),
            entry("", 2, 2, "2026-11-20T10:03:00Z"),
        ])
        .expect("the ledger knows the run");
        assert_eq!(
            usage,
            LedgerUsage {
                input_tokens: 20,
                output_tokens: 10,
                model: Some("model-a".to_owned()),
            }
        );
    }

    #[test]
    fn a_model_tie_goes_to_the_model_recorded_first_and_blank_names_do_not_count() {
        // Listed out of order: the order comes from created_at, not from the list.
        let usage = ledger_usage(&[
            entry("model-a", 1, 1, "2026-11-20T10:02:00Z"),
            entry("model-b", 1, 1, "2026-11-20T10:01:00Z"),
            entry("model-a", 1, 1, "2026-11-20T10:03:00Z"),
            entry("model-b", 1, 1, "2026-11-20T10:04:00Z"),
            entry("  ", 1, 1, "2026-11-20T09:00:00Z"),
        ])
        .expect("the ledger knows the run");
        assert_eq!(usage.model.as_deref(), Some("model-b"));
        assert_eq!((usage.input_tokens, usage.output_tokens), (5, 5));
    }

    #[test]
    fn a_run_with_only_blank_models_has_its_totals_and_no_model() {
        let usage = ledger_usage(&[entry("", 3, 2, "2026-11-20T10:00:00Z")]).expect("known");
        assert_eq!(
            usage,
            LedgerUsage {
                input_tokens: 3,
                output_tokens: 2,
                model: None,
            }
        );
    }

    #[test]
    fn no_entries_and_a_list_as_long_as_cost_core_answers_are_unknown() {
        assert!(ledger_usage(&[]).is_none());
        let full: Vec<RunLedgerEntry> = (0..RUN_ENTRIES_LIMIT)
            .map(|_| entry("model-a", 1, 1, "2026-11-20T10:00:00Z"))
            .collect();
        assert!(
            ledger_usage(&full).is_none(),
            "a list that may be cut short is not totalled"
        );
        assert!(ledger_usage(&full[..RUN_ENTRIES_LIMIT - 1]).is_some());
    }

    #[test]
    fn with_the_switch_off_a_run_is_written_as_the_session_wrote_it() {
        let mut copy = thread_copy(Vec::new());
        copy.runs = vec![run("run-1", "")];
        let body =
            render_json(&copy, Profile::Full, AT, UsageSource::Session).expect("the copy renders");
        assert!(
            body.contains(
                "      \"model\": \"\",\n      \"input_tokens\": 0,\n      \"output_tokens\": 0,\n"
            ),
            "{body}"
        );
    }

    #[test]
    fn with_the_switch_on_an_unknown_run_is_null_or_absent_and_never_zero() {
        let mut copy = thread_copy(Vec::new());
        copy.runs = vec![run("run-1", "session-model")];
        let usages = HashMap::new();
        let body = render_json(&copy, Profile::Full, AT, UsageSource::Ledger(&usages))
            .expect("the copy renders");
        let document: Value = serde_json::from_str(&body).expect("the copy is JSON");
        assert!(document["runs"][0].get("model").is_none(), "{body}");
        assert!(document["runs"][0]["input_tokens"].is_null(), "{body}");
        assert!(document["runs"][0]["output_tokens"].is_null(), "{body}");
    }

    #[test]
    fn with_the_switch_on_a_known_run_carries_the_ledgers_totals_and_model() {
        let mut copy = thread_copy(Vec::new());
        copy.runs = vec![run("run-1", ""), run("run-2", "")];
        let usages = HashMap::from([(
            "run-1".to_owned(),
            LedgerUsage {
                input_tokens: 18,
                output_tokens: 8,
                model: Some("model-a".to_owned()),
            },
        )]);
        let body = render_json(&copy, Profile::Full, AT, UsageSource::Ledger(&usages))
            .expect("the copy renders");
        let document: Value = serde_json::from_str(&body).expect("the copy is JSON");
        assert_eq!(document["runs"][0]["model"], "model-a");
        assert_eq!(document["runs"][0]["input_tokens"], 18);
        assert_eq!(document["runs"][0]["output_tokens"], 8);
        assert!(document["runs"][1].get("model").is_none(), "{body}");
        assert!(document["runs"][1]["input_tokens"].is_null(), "{body}");
    }

    #[test]
    fn the_markdown_model_cell_is_the_ledgers_model_or_empty_when_unknown() {
        let mut copy = thread_copy(Vec::new());
        copy.runs = vec![run("run-1", ""), run("run-2", "")];
        let usages = HashMap::from([(
            "run-1".to_owned(),
            LedgerUsage {
                input_tokens: 1,
                output_tokens: 1,
                model: Some("model-a".to_owned()),
            },
        )]);
        let text = render_markdown(&copy, Profile::Full, AT, UsageSource::Ledger(&usages))
            .expect("the copy renders");
        assert!(
            text.contains("| run-1 | completed | chat | model-a |"),
            "{text}"
        );
        assert!(text.contains("| run-2 | completed | chat |  |"), "{text}");
    }

    #[test]
    fn the_usage_switch_reads_its_own_name_as_the_strict_literal() {
        let set =
            |value: &'static str| move |key: &str| (key == USAGE_ENV).then(|| value.to_owned());
        assert!(usage_enabled_with(&set("true")));
        for value in ["TRUE", " true", "1", "on", ""] {
            assert!(!usage_enabled_with(&set(value)), "{value:?}");
        }
        assert!(!usage_enabled_with(&|_| None));
    }

    #[tokio::test]
    async fn the_export_reads_each_run_from_the_ledger_as_the_viewer() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/cost/entries"))
            .and(wiremock::matchers::query_param("run_id", "run-sum"))
            .and(wiremock::matchers::header(
                "authorization",
                "Bearer fixture-cost-bearer",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "entries": [
                    { "run_id": "run-sum", "model": "model-a", "input_tokens": 10, "output_tokens": 4, "created_at": "2026-11-20T10:00:00Z" },
                    { "run_id": "run-sum", "model": "model-a", "input_tokens": 7, "output_tokens": 3, "created_at": "2026-11-20T10:01:00Z" },
                    { "run_id": "run-sum", "model": "model-b", "input_tokens": 1, "output_tokens": 1, "created_at": "2026-11-20T10:02:00Z" }
                ],
                "count": 3
            })))
            .expect(1)
            .mount(&server)
            .await;
        for (run_id, response) in [
            (
                "run-empty",
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(json!({ "entries": [], "count": 0 })),
            ),
            ("run-refused", wiremock::ResponseTemplate::new(404)),
        ] {
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path("/api/v1/cost/entries"))
                .and(wiremock::matchers::query_param("run_id", run_id))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
        }

        let run_ids = vec![
            "run-sum".to_owned(),
            "run-empty".to_owned(),
            "run-refused".to_owned(),
        ];
        let usages = ledger_usages(
            &reqwest::Client::new(),
            &server.uri(),
            Some("fixture-cost-bearer"),
            &claims(false),
            &run_ids,
        )
        .await;
        assert_eq!(usages.len(), 1, "{usages:?}");
        assert_eq!(
            usages["run-sum"],
            LedgerUsage {
                input_tokens: 18,
                output_tokens: 8,
                model: Some("model-a".to_owned()),
            }
        );
    }

    #[tokio::test]
    async fn a_zero_retention_waived_or_unbound_viewer_makes_no_lookup() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/cost/entries"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(json!({ "entries": [], "count": 0 })),
            )
            .expect(0)
            .mount(&server)
            .await;
        let run_ids = vec!["run-1".to_owned()];
        let client = reqwest::Client::new();
        for viewer in [claims(true), waived_claims()] {
            assert!(!usage_lookup_allowed(&viewer));
            let usages = ledger_usages(
                &client,
                &server.uri(),
                Some("fixture-cost-bearer"),
                &viewer,
                &run_ids,
            )
            .await;
            assert!(usages.is_empty());
        }
        assert!(usage_lookup_allowed(&claims(false)));
        let usages = ledger_usages(&client, &server.uri(), None, &claims(false), &run_ids).await;
        assert!(usages.is_empty());
    }

    #[tokio::test]
    async fn only_the_newest_runs_are_looked_up() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/cost/entries"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .expect(USAGE_LOOKUP_RUNS as u64)
            .mount(&server)
            .await;
        let run_ids: Vec<String> = (0..USAGE_LOOKUP_RUNS + 10)
            .map(|n| format!("run-{n}"))
            .collect();
        let usages = ledger_usages(
            &reqwest::Client::new(),
            &server.uri(),
            Some("fixture-cost-bearer"),
            &claims(false),
            &run_ids,
        )
        .await;
        assert!(usages.is_empty());
        let mut asked: Vec<usize> = Vec::new();
        for request in server
            .received_requests()
            .await
            .expect("requests are recorded")
        {
            for (key, value) in request.url.query_pairs() {
                if key == "run_id" {
                    asked.push(
                        value
                            .trim_start_matches("run-")
                            .parse()
                            .expect("a run number"),
                    );
                }
            }
        }
        asked.sort_unstable();
        assert_eq!(asked, (10..USAGE_LOOKUP_RUNS + 10).collect::<Vec<_>>());
    }
}
