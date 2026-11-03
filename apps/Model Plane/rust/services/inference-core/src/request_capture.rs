//! Request snapshots: what one model call was sent and offered (EP-14 S6).
//!
//! The `Infer` and `InferStream` handlers hand each call to [`RequestCapture::offer`]
//! after routing. When the call qualifies, the offer builds a
//! `request-snapshot-v1` (`mp_contracts::request_snapshot`) and pushes it onto a
//! bounded queue with `try_send`; a background task writes it to session-core's
//! `RunLedgerService.RecordRequestSnapshot`. Nothing on the provider path awaits
//! the queue or the write, so a slow or absent session-core never slows or fails
//! a model call. A full queue drops the snapshot, counts it under
//! `inference_request_snapshot_dropped_total{reason="queue_full"}`, and the next
//! snapshot of the same run carries the number dropped so the record says it is
//! incomplete instead of looking whole.
//!
//! A call qualifies only when all of these hold: `INFERENCE_REQUEST_CAPTURE` is
//! on (only the literal `true`), the call names a run, its effective retention is
//! not zero (`zdr` from the token or the request), and the caller holds no
//! connection-bound waiver (DEC-32: a waived run is treated as ZDR for every new
//! store). session-core checks the run's own retention class again before it
//! stores anything.
//!
//! The snapshot never holds a message body or image bytes, and nothing here logs
//! any part of a snapshot: a log line names the run and the error only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::run_ledger_service_client::RunLedgerServiceClient;
use mp_contracts::model_plane::v1::RecordRequestSnapshotRequest;
use mp_contracts::request_snapshot::{
    self, cap_text, canonical_json, sha256_hex, CappedText, ImagePartManifestEntry,
    MessageManifestEntry, RequestSnapshotV1, SnapshotParams, SnapshotPromptTiers,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tonic::transport::{Channel, Endpoint};

use crate::metrics;
use crate::provider;

/// The switch. Default off; only the literal `true` turns capture on.
pub const CAPTURE_ENV: &str = "INFERENCE_REQUEST_CAPTURE";

/// How many snapshots may wait for session-core. Past this the newest is dropped.
pub const QUEUE_CAPACITY: usize = 1_024;

/// How many runs may hold an unreported drop count at once. Past this a drop is
/// still counted in the metric, but its run is not told.
const MAX_RUNS_WITH_DROPS: usize = 4_096;

/// The `reason` label values of the drop counter.
const DROP_QUEUE_FULL: &str = "queue_full";
const DROP_WRITE_FAILED: &str = "write_failed";

/// The service identity inference-core presents to auth-core, and the scope the
/// snapshot write needs. Both must match `inference-core` in
/// `PLANE_SERVICE_PRINCIPALS_JSON` (Control Plane `plane-service-principals.json`).
const SERVICE_ID: &str = "inference-core";
const WRITE_SCOPE: &str = "session:request-snapshots:write";

/// How many orgs keep a cached session-core token. Past this the cache starts over.
const MAX_CACHED_TOKENS: usize = 1_024;

/// The strict reading of [`CAPTURE_ENV`].
#[must_use]
pub fn capture_enabled_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// Whether a call may be snapshotted at all. A ZDR call, a waived caller and a
/// call that names no run never are, whatever else is true.
#[must_use]
pub fn qualifies(zdr: bool, waived: bool, run_id: &str) -> bool {
    !zdr && !waived && !run_id.trim().is_empty()
}

/// One model call, as the handler saw it after routing.
pub struct CapturedCall<'a> {
    /// The run the call names (`InferRequest.run_id`).
    pub run_id: &'a str,
    /// The wire `InferPurpose`.
    pub purpose: i32,
    /// The provider request as built from the verified principal: its `zdr` is
    /// the effective one, its org and user the token's.
    pub request: &'a provider::InferRequest,
    /// Whether the verified caller holds a connection-bound ZDR waiver.
    pub waived: bool,
    /// The model and provider that served the call, when known. A streamed call
    /// is captured as soon as its stream opens, before any chunk names them.
    pub served_model: &'a str,
    pub served_provider: &'a str,
}

/// What an offer did with a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// The call does not qualify; nothing was built.
    Skipped,
    /// The snapshot is queued for session-core.
    Queued,
    /// The queue was full; the snapshot was dropped and counted.
    Dropped,
}

/// One snapshot waiting for session-core.
#[derive(Debug, Clone)]
pub struct CaptureJob {
    pub org_id: String,
    pub user_id: String,
    pub run_id: String,
    pub purpose: i32,
    pub request_id: String,
    pub snapshot_json: String,
    pub system: CappedText,
    pub tools: CappedText,
    /// Snapshots of this run dropped since its last queued one.
    pub dropped_before: u32,
}

/// Per-run counts of snapshots that never reached session-core.
#[derive(Clone, Default)]
struct DropLedger(Arc<Mutex<HashMap<String, u32>>>);

impl DropLedger {
    fn add(&self, run_id: &str) {
        let mut runs = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = runs.get_mut(run_id) {
            *count = count.saturating_add(1);
        } else if runs.len() < MAX_RUNS_WITH_DROPS {
            runs.insert(run_id.to_owned(), 1);
        }
    }

    fn take(&self, run_id: &str) -> u32 {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(run_id)
            .unwrap_or(0)
    }
}

/// The capture handle the gRPC service holds. Cloning shares the queue.
#[derive(Clone)]
pub struct RequestCapture {
    tx: mpsc::Sender<CaptureJob>,
    drops: DropLedger,
}

impl RequestCapture {
    /// The capture handle and its writer task, or `None` when the switch is off
    /// or no session-core address is configured. With `None` the handlers do
    /// exactly what they did before capture existed.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        if !capture_enabled_from(std::env::var(CAPTURE_ENV).ok().as_deref()) {
            return None;
        }
        // The same names and precedence as `InferenceConfig::session_core_url`.
        let Some(url) = std::env::var("SESSION_CORE_URL")
            .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            tracing::warn!("INFERENCE_REQUEST_CAPTURE is on but no session-core address is set; capture is off");
            return None;
        };
        let Some(writer) = SnapshotWriter::from_url(&url) else {
            tracing::warn!("INFERENCE_REQUEST_CAPTURE is on but the session-core address is invalid; capture is off");
            return None;
        };
        let (capture, rx) = Self::with_queue(QUEUE_CAPACITY);
        tokio::spawn(writer.run(rx, capture.drops.clone()));
        Some(capture)
    }

    /// A handle over a fresh queue of `capacity`, and the receiving end.
    #[must_use]
    pub fn with_queue(capacity: usize) -> (Self, mpsc::Receiver<CaptureJob>) {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Self {
                tx,
                drops: DropLedger::default(),
            },
            rx,
        )
    }

    /// Offer one call. Never awaits: a full queue drops the snapshot.
    pub fn offer(&self, call: &CapturedCall<'_>) -> Offer {
        if !qualifies(call.request.zdr, call.waived, call.run_id) {
            return Offer::Skipped;
        }
        let mut job = build_job(call);
        job.dropped_before = self.drops.take(call.run_id);
        match self.tx.try_send(job) {
            Ok(()) => Offer::Queued,
            Err(mpsc::error::TrySendError::Full(job) | mpsc::error::TrySendError::Closed(job)) => {
                // The count taken for this job is not lost with it.
                for _ in 0..=job.dropped_before {
                    self.drops.add(call.run_id);
                }
                metrics::record_request_snapshot_dropped(DROP_QUEUE_FULL);
                Offer::Dropped
            }
        }
    }
}

/// The system prompt of a call: every system message, in order.
fn system_text(request: &provider::InferRequest) -> String {
    request
        .messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The tool catalogue of a call as canonical JSON; empty when no tool was offered.
fn tools_json(request: &provider::InferRequest) -> String {
    if request.tools.is_empty() {
        return String::new();
    }
    let tools: Vec<serde_json::Value> = request
        .tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "parameters_json": tool.parameters_json,
            })
        })
        .collect();
    canonical_json(&serde_json::Value::Array(tools))
}

fn digest_or_empty(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        sha256_hex(text.as_bytes())
    }
}

/// Build the snapshot of a qualifying call. Digests are over the full texts; the
/// blobs are cut to their caps.
fn build_job(call: &CapturedCall<'_>) -> CaptureJob {
    let request = call.request;
    let system = system_text(request);
    let tools = tools_json(request);
    let served_model = if call.served_model.is_empty() {
        request.model.clone()
    } else {
        call.served_model.to_owned()
    };
    let routed_from = if served_model == request.model {
        String::new()
    } else {
        request.model.clone()
    };
    let snapshot = RequestSnapshotV1 {
        schema: request_snapshot::SCHEMA.to_owned(),
        model: served_model,
        provider: call.served_provider.to_owned(),
        routed_from,
        params: SnapshotParams {
            temperature: request.temperature,
            max_tokens: u32::try_from(request.max_tokens).unwrap_or(0),
            tool_choice: request.tool_choice.clone(),
            reasoning_effort: request
                .reasoning_effort
                .map(|level| level.as_str().to_owned())
                .unwrap_or_default(),
            thinking_budget_tokens: u32::try_from(request.thinking_budget_tokens).unwrap_or(0),
            structured_output: request.structured_output_schema.is_some(),
        },
        system_digest: digest_or_empty(&system),
        tools_digest: digest_or_empty(&tools),
        prompt_tiers: SnapshotPromptTiers {
            stable: u32::try_from(request.prompt_tiers.stable).unwrap_or(u32::MAX),
            context: u32::try_from(request.prompt_tiers.context).unwrap_or(u32::MAX),
        },
        messages: request
            .messages
            .iter()
            .map(|message| {
                MessageManifestEntry::of(
                    &message.role,
                    &message.name,
                    !message.compaction_summary.is_empty(),
                    &message.content,
                )
            })
            .collect(),
        image_parts: request
            .image_parts
            .iter()
            .map(|part| ImagePartManifestEntry {
                index: u32::try_from(part.message_index).unwrap_or(u32::MAX),
                mime: part.mime_type.clone(),
                bytes: u32::try_from(part.data.len()).unwrap_or(u32::MAX),
            })
            .collect(),
    };
    CaptureJob {
        org_id: request.org_id.clone(),
        user_id: request.user_id.clone(),
        run_id: call.run_id.trim().to_owned(),
        purpose: call.purpose,
        request_id: request.request_id.clone(),
        snapshot_json: snapshot.to_canonical_json(),
        system: cap_text(&system, request_snapshot::SYSTEM_CAP_BYTES),
        tools: cap_text(&tools, request_snapshot::TOOLS_CAP_BYTES),
        dropped_before: 0,
    }
}

#[derive(Deserialize)]
struct InternalTokenResponse {
    token: String,
    #[serde(default, rename = "expiresInSeconds")]
    expires_in_seconds: Option<u64>,
}

/// Mints and caches one `aud=session-core` service token per org. The token's
/// org is the call's org, so session-core can hold the write to that org's runs.
struct SnapshotAuth {
    http: reqwest::Client,
    token_url: String,
    service_key: String,
    cache: tokio::sync::Mutex<HashMap<String, (String, Instant)>>,
}

impl SnapshotAuth {
    fn from_env() -> Option<Self> {
        let auth_core_url = std::env::var("AUTH_CORE_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())?;
        let service_key = std::env::var("INFERENCE_ROUTING_SERVICE_API_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            http,
            token_url: format!("{auth_core_url}/api/session-core/internal-token"),
            service_key,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    async fn bearer(&self, org_id: &str) -> Option<String> {
        if let Some((token, expires_at)) = self.cache.lock().await.get(org_id) {
            if *expires_at > Instant::now() {
                return Some(token.clone());
            }
        }
        let response = self
            .http
            .post(&self.token_url)
            .header("x-service-id", SERVICE_ID)
            .header("x-service-api-key", &self.service_key)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [WRITE_SCOPE],
                "reason": "request snapshot write",
            }))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "request snapshot: session-core token mint failed");
            return None;
        }
        let body: InternalTokenResponse = response.json().await.ok()?;
        let ttl = Duration::from_secs(
            body.expires_in_seconds
                .unwrap_or(300)
                .saturating_sub(60)
                .max(30),
        );
        let mut cache = self.cache.lock().await;
        if cache.len() >= MAX_CACHED_TOKENS {
            cache.clear();
        }
        cache.insert(org_id.to_owned(), (body.token.clone(), Instant::now() + ttl));
        Some(body.token)
    }
}

/// The background writer: drains the queue into session-core one snapshot at a time.
struct SnapshotWriter {
    channel: Channel,
    auth: Option<SnapshotAuth>,
}

impl SnapshotWriter {
    fn from_url(url: &str) -> Option<Self> {
        let channel = Endpoint::from_shared(url.trim().to_owned())
            .ok()?
            .connect_lazy();
        Some(Self {
            channel,
            auth: SnapshotAuth::from_env(),
        })
    }

    async fn run(self, mut rx: mpsc::Receiver<CaptureJob>, drops: DropLedger) {
        while let Some(job) = rx.recv().await {
            let run_id = job.run_id.clone();
            let dropped_before = job.dropped_before;
            if let Err(status) = self.write(job).await {
                // The run id and the gRPC code only: never any part of the snapshot.
                tracing::warn!(run_id = %run_id, code = ?status.code(), "request snapshot write failed");
                for _ in 0..=dropped_before {
                    drops.add(&run_id);
                }
                metrics::record_request_snapshot_dropped(DROP_WRITE_FAILED);
            }
        }
    }

    async fn write(&self, job: CaptureJob) -> Result<(), tonic::Status> {
        let mut request = tonic::Request::new(RecordRequestSnapshotRequest {
            run_id: job.run_id,
            // inference-core runs as several replicas, so it cannot number a
            // run's calls; 0 asks session-core to number this one.
            call_seq: 0,
            purpose: job.purpose,
            request_id: job.request_id,
            snapshot_json: job.snapshot_json,
            system_text: job.system.text,
            tools_json: job.tools.text,
            dropped_before: job.dropped_before,
            org_id: job.org_id.clone(),
            user_id: job.user_id,
            system_truncated: job.system.truncated,
            tools_truncated: job.tools.truncated,
        });
        let Some(auth) = &self.auth else {
            return Err(tonic::Status::unauthenticated(
                "no session-core credential is configured for snapshot writes",
            ));
        };
        let Some(token) = auth.bearer(&job.org_id).await else {
            return Err(tonic::Status::unauthenticated(
                "session-core token mint failed",
            ));
        };
        let value = format!("Bearer {token}")
            .parse()
            .map_err(|_| tonic::Status::internal("session-core token is not valid metadata"))?;
        request.metadata_mut().insert("authorization", value);
        RunLedgerServiceClient::new(self.channel.clone())
            .record_request_snapshot(request)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::InferPurpose;

    fn request(zdr: bool) -> provider::InferRequest {
        let mut request = provider::InferRequest {
            request_id: "req-1".to_owned(),
            model: "verevon-balance".to_owned(),
            temperature: 0.2,
            max_tokens: 512,
            zdr,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            ..provider::InferRequest::default()
        };
        request.messages = vec![
            provider::ChatMessage {
                compaction_summary: String::new(),
                role: "system".to_owned(),
                content: "You are Verevon.".to_owned(),
                name: String::new(),
            },
            provider::ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "secret question body".to_owned(),
                name: String::new(),
            },
        ];
        request.tools = vec![provider::ToolDefinition {
            name: "web_search".to_owned(),
            description: "Search the web.".to_owned(),
            parameters_json: "{}".to_owned(),
        }];
        request
    }

    fn call<'a>(request: &'a provider::InferRequest, run_id: &'a str, waived: bool) -> CapturedCall<'a> {
        CapturedCall {
            run_id,
            purpose: InferPurpose::AgentRound as i32,
            request,
            waived,
            served_model: "gpt-5.1",
            served_provider: "azure-openai",
        }
    }

    #[test]
    fn capture_off_sends_nothing() {
        // Only the literal `true` turns capture on; anything else leaves the
        // handlers without a capture handle at all.
        for raw in [None, Some(""), Some("1"), Some("on"), Some("TRUE "), Some("yes"), Some("false")] {
            let expected = raw.is_some_and(|value| value.trim() == "true");
            assert_eq!(capture_enabled_from(raw), expected, "{raw:?}");
        }
        assert!(capture_enabled_from(Some("true")));
        assert!(!capture_enabled_from(Some("TRUE")));
    }

    #[test]
    fn capture_skips_zdr_request() {
        let (capture, mut rx) = RequestCapture::with_queue(4);
        let zdr = request(true);
        assert_eq!(capture.offer(&call(&zdr, "run-1", false)), Offer::Skipped);
        assert!(rx.try_recv().is_err(), "a ZDR call queues nothing");
    }

    #[test]
    fn capture_skips_waived_bearer() {
        let (capture, mut rx) = RequestCapture::with_queue(4);
        let plain = request(false);
        assert_eq!(capture.offer(&call(&plain, "run-1", true)), Offer::Skipped);
        assert!(rx.try_recv().is_err(), "a waived caller queues nothing");
    }

    #[test]
    fn capture_skips_empty_run_id() {
        let (capture, mut rx) = RequestCapture::with_queue(4);
        let plain = request(false);
        assert_eq!(capture.offer(&call(&plain, "", false)), Offer::Skipped);
        assert_eq!(capture.offer(&call(&plain, "   ", false)), Offer::Skipped);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn capture_queues_a_snapshot_without_message_bodies() {
        let (capture, mut rx) = RequestCapture::with_queue(4);
        let plain = request(false);
        assert_eq!(capture.offer(&call(&plain, "run-1", false)), Offer::Queued);
        let job = rx.try_recv().expect("queued");
        assert_eq!((job.org_id.as_str(), job.user_id.as_str()), ("org-a", "user-a"));
        assert_eq!(job.purpose, InferPurpose::AgentRound as i32);
        assert_eq!(job.system.text, "You are Verevon.");
        assert!(job.tools.text.contains("web_search"));
        assert!(!job.snapshot_json.contains("secret question body"));
        let snapshot: RequestSnapshotV1 =
            serde_json::from_str(&job.snapshot_json).expect("snapshot json");
        assert_eq!(snapshot.model, "gpt-5.1");
        assert_eq!(snapshot.routed_from, "verevon-balance");
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.system_digest, sha256_hex(b"You are Verevon."));
        assert_eq!(snapshot.tools_digest, sha256_hex(job.tools.text.as_bytes()));
    }

    /// A full queue never waits: the offer returns at once, the snapshot is
    /// dropped, and the next snapshot of the run says how many were lost.
    #[test]
    fn capture_never_blocks_provider_call() {
        let (capture, mut rx) = RequestCapture::with_queue(1);
        let plain = request(false);
        assert_eq!(capture.offer(&call(&plain, "run-1", false)), Offer::Queued);
        // Nobody drains the queue: `offer` is synchronous, so these return
        // instead of waiting for room.
        assert_eq!(capture.offer(&call(&plain, "run-1", false)), Offer::Dropped);
        assert_eq!(capture.offer(&call(&plain, "run-1", false)), Offer::Dropped);

        let first = rx.try_recv().expect("the first is queued");
        assert_eq!(first.dropped_before, 0);
        assert_eq!(capture.offer(&call(&plain, "run-1", false)), Offer::Queued);
        let next = rx.try_recv().expect("room again");
        assert_eq!(next.dropped_before, 2, "both drops are reported once");
        assert_eq!(capture.offer(&call(&plain, "run-1", false)), Offer::Queued);
        assert_eq!(rx.try_recv().expect("queued").dropped_before, 0, "and only once");
    }

    #[test]
    fn capture_caps_the_blobs_but_digests_the_full_text() {
        let (capture, mut rx) = RequestCapture::with_queue(1);
        let mut big = request(false);
        let long = "x".repeat(request_snapshot::SYSTEM_CAP_BYTES + 1);
        big.messages[0].content.clone_from(&long);
        assert_eq!(capture.offer(&call(&big, "run-1", false)), Offer::Queued);
        let job = rx.try_recv().expect("queued");
        assert!(job.system.truncated);
        assert_eq!(job.system.text.len(), request_snapshot::SYSTEM_CAP_BYTES);
        let snapshot: RequestSnapshotV1 =
            serde_json::from_str(&job.snapshot_json).expect("snapshot json");
        assert_eq!(snapshot.system_digest, sha256_hex(long.as_bytes()));
    }
}
