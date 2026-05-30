//! PageRunner: fetch → transform → diff → artifacts → events → optional ingest.
//!
//! Single-page execution unit. Batch/crawl composition lives higher
//! (edge for fast path, orchestrator for durable workflows).

use chrono::Utc;
use std::sync::Arc;
use url::Url;

use quarry_core::contracts::{ChunkRef, DataPlaneIngestRequest, SourceTrace};
use quarry_core::event::EventType;
use quarry_core::ids::kinds::RunKind;
use quarry_core::output::{
    ChangeInfo, DriverInfo, FormatRef, NormalizedOutput, OutputFormats, UrlTriple,
};
use quarry_core::zdr::{self, WriteKind, ZdrMode};
use quarry_core::{error::ErrorCode, QuarryError, QuarryResult};
use quarry_security::SecurityEngine;
use quarry_transform::{
    chunk, diff, fingerprint::content_fingerprint, links,
    readability::html_to_readable_markdown, metadata,
};
use serde_json::json;

use crate::artifact_store::ArtifactStore;
use crate::driver::Driver;
use crate::events::EventSink;
use crate::host_scheduler::{BadKind, HostScheduler};
use crate::ingest_client::DataPlaneIngest;
use crate::local_index::{LocalDocument, TantivyLocalIndex};
use crate::policy::RunPolicy;

pub struct PageRunner {
    pub driver: Arc<dyn Driver>,
    pub security: Arc<dyn SecurityEngine>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub event_sink: EventSink,
    pub zdr: ZdrMode,
    /// Transport-agnostic Data Plane ingest. P2 / cluster #grpc. In
    /// production this points to either `IngestClient` (HTTP/JSON) or
    /// `GrpcIngestAdapter` (HTTP/2/protobuf) depending on
    /// `QUARRY_EDGE__DATA_PLANE_TRANSPORT`. `None` skips ingest
    /// entirely (used in test harnesses and dev mode).
    pub ingest: Option<Arc<dyn DataPlaneIngest>>,
    pub org_id: Option<String>,
    /// Optional cancellation token. When set, the spawned ingest task
    /// races against this token and aborts cleanly when the parent run
    /// is cancelled — without this, in-flight ingests continue running
    /// after their owning run is gone, wasting cycles and producing
    /// orphan documents in the Data Plane.
    pub cancel_token: Option<tokio_util::sync::CancellationToken>,
    /// Cycle 19 / cluster #16: when set, every successful scrape is
    /// indexed into the local Tantivy corpus so subsequent `/v1/search`
    /// hits return instantly without going to live SERPs.
    /// Indexing is best-effort + fire-and-forget — index failures never
    /// fail the scrape. Skipped entirely when `zdr=on` (the local index
    /// is a durable artifact; ZDR forbids durable writes).
    pub local_index: Option<TantivyLocalIndex>,
    /// Cycle 21 / cluster #2 — RunPolicy bag (Determinism + sub-policies).
    /// Defaults to `best_effort`. Stamped into output meta so consumers
    /// can audit which policy produced the artifact.
    pub policy: RunPolicy,
    /// Cycle 21 / cluster #3 — per-host adaptive scheduler. `None`
    /// disables throttling (used in test harnesses); production wires
    /// a single shared scheduler so all in-flight scrapes coordinate.
    pub scheduler: Option<Arc<HostScheduler>>,
}

impl PageRunner {
    pub async fn run(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
    ) -> QuarryResult<NormalizedOutput> {
        match self
            .run_inner(run_id, requested_url, prev_fingerprint)
            .await
        {
            Ok(out) => Ok(out),
            Err(e) => {
                self.event_sink
                    .emit(
                        run_id.clone(),
                        EventType::PageFailed,
                        json!({
                            "url": requested_url.to_string(),
                            "error": e.to_string(),
                        }),
                        format!("{}:{}:failed", run_id, requested_url),
                    )
                    .await;
                Err(e)
            }
        }
    }

    async fn run_inner(
        &self,
        run_id: &RunKind,
        requested_url: &Url,
        prev_fingerprint: Option<String>,
    ) -> QuarryResult<NormalizedOutput> {
        // Preflight
        let verdict = self.security.preflight(requested_url).await;
        if verdict.decision == quarry_security::Decision::Block {
            self.event_sink
                .emit(
                    run_id.clone(),
                    EventType::PageBlocked,
                    json!({
                        "url": requested_url.to_string(),
                        "reasons": verdict.reasons.clone(),
                    }),
                    format!("{}:{}:blocked", run_id, requested_url),
                )
                .await;
            return Err(quarry_core::error::QuarryError::new(
                quarry_core::error::ErrorCode::SecurityBlocked,
                verdict.reasons.join("; "),
            ));
        }

        // DNS-time SSRF guard: resolve host and block private/loopback/link-local
        // ranges before we hand the URL to the driver (TOCTOU-resistant).
        if !self.security.allow_private_hosts() {
            crate::dns_guard::guard_url(requested_url).await?;
        }

        // Fetch — gated by the per-host scheduler when configured.
        // The slot permit is held only for the duration of the fetch
        // and drops eagerly (`drop(_slot)` before any other work) so
        // we don't hold scarce per-host concurrency during DOM cleanup
        // / markdown conversion / artifact storage.
        let host_for_scheduler = requested_url
            .host_str()
            .unwrap_or("unknown")
            .to_string();
        let _slot = match self.scheduler.as_ref() {
            Some(s) => Some(s.acquire(&host_for_scheduler).await),
            None => None,
        };
        let fetch_result = self.driver.fetch(requested_url).await;

        // Inform the scheduler before we propagate the error or move
        // on. mark_bad shrinks the host's target on rate-limit / block /
        // server / timeout; mark_good (after success) feeds the EWMA
        // and may grow the target.
        if let (Some(s), Err(err)) = (self.scheduler.as_ref(), fetch_result.as_ref()) {
            let kind = match err.code {
                quarry_core::error::ErrorCode::RateLimited => BadKind::RateLimited,
                quarry_core::error::ErrorCode::UpstreamBlocked
                | quarry_core::error::ErrorCode::Forbidden => BadKind::Blocked,
                quarry_core::error::ErrorCode::Timeout => BadKind::Timeout,
                _ => BadKind::Server,
            };
            s.record_bad(&host_for_scheduler, kind).await;
        }
        let resp = fetch_result?;
        if let Some(s) = self.scheduler.as_ref() {
            // Use the driver-reported duration so the scheduler EWMA
            // reflects the upstream call latency, not Rust-side
            // post-processing time.
            s.record_good(
                &host_for_scheduler,
                std::time::Duration::from_millis(resp.duration_ms),
            )
            .await;
        }
        drop(_slot);
        self.event_sink
            .emit(
                run_id.clone(),
                EventType::PageFetched,
                json!({
                    "url": resp.final_url.to_string(),
                    "status": resp.status,
                    "duration_ms": resp.duration_ms,
                }),
                format!("{}:{}:fetched", run_id, resp.final_url),
            )
            .await;
        let ct = resp
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone());

        // Transform
        let body_text = String::from_utf8_lossy(&resp.body).to_string();
        let md = html_to_readable_markdown(&body_text);
        let link_list = links::extract(&body_text, &resp.final_url);
        let meta = metadata::extract(&body_text, ct);

        // Fingerprint + diff. `fp` is the byte fingerprint (canonical for
        // artifact addressing); `text_fp` is the whitespace/case-normalized
        // fingerprint included in change events so consumers can suppress
        // boilerplate-only churn at the subscription layer.
        let fp = content_fingerprint(&resp.body);
        let text_fp = quarry_transform::fingerprint::text_fingerprint(&md);
        let prev_fp = prev_fingerprint.map(quarry_transform::Fingerprint);
        let change = diff::compare(prev_fp.as_ref(), &fp);
        {
            use quarry_core::output::ChangeStatus;
            let (evt, key_suffix) = match change {
                ChangeStatus::New | ChangeStatus::Changed => {
                    (EventType::ChangeDetected, "change_detected")
                }
                ChangeStatus::Unchanged => (EventType::ChangeUnchanged, "change_unchanged"),
            };
            self.event_sink
                .emit(
                    run_id.clone(),
                    evt,
                    json!({
                        "url": resp.final_url.to_string(),
                        "fingerprint": fp.0,
                        "text_fingerprint": text_fp.0,
                        "prev": prev_fp.as_ref().map(|p| p.0.clone()),
                    }),
                    format!("{}:{}:{}", run_id, resp.final_url, key_suffix),
                )
                .await;
        }

        // Canonical page hash combines normalized URL with content fingerprint,
        // giving a stable artifact key that is independent of run_id.
        let page_hash = quarry_core::artifact::page_hash(resp.final_url.as_ref(), &fp.0);

        // Artifacts — skip durable writes when ZDR is active.
        let (html_fmt, md_fmt) = if zdr::guard(self.zdr, WriteKind::Artifact).is_ok() {
            let html_art = self
                .artifacts
                .put(run_id, &page_hash, "html", resp.body.clone())
                .await?;
            let md_art = self
                .artifacts
                .put(run_id, &page_hash, "markdown", md.as_bytes().to_vec())
                .await?;
            self.event_sink
                .emit(
                    run_id.clone(),
                    EventType::ArtifactWritten,
                    json!({
                        "url": resp.final_url.to_string(),
                        "format": "html",
                        "artifact_id": html_art.artifact_id.to_string(),
                        "bytes": html_art.bytes,
                    }),
                    format!("{}:{}:artifact:html", run_id, resp.final_url),
                )
                .await;
            self.event_sink
                .emit(
                    run_id.clone(),
                    EventType::ArtifactWritten,
                    json!({
                        "url": resp.final_url.to_string(),
                        "format": "markdown",
                        "artifact_id": md_art.artifact_id.to_string(),
                        "bytes": md_art.bytes,
                    }),
                    format!("{}:{}:artifact:markdown", run_id, resp.final_url),
                )
                .await;

            use quarry_core::output::DriverKind;
            let plan = match self.driver.kind() {
                DriverKind::Static => crate::driver_plan::DriverPlan::static_fetch("default"),
                DriverKind::Tls => crate::driver_plan::DriverPlan::tls_fetch(self.driver.tls_profile().unwrap_or_default(), "driver-selected"),
                DriverKind::Browser => crate::driver_plan::DriverPlan::browser_fetch("default"),
            };
            let meta_bytes = serde_json::to_vec(&plan)
                .map_err(|e| QuarryError::new(ErrorCode::Internal, e.to_string()))?;
            let meta_art = self.artifacts.put(run_id, &page_hash, "meta.json", meta_bytes).await?;
            self.event_sink
                .emit(
                    run_id.clone(),
                    EventType::ArtifactWritten,
                    json!({ "url": resp.final_url.to_string(), "format": "meta", "artifact_id": meta_art.artifact_id.to_string(), "bytes": meta_art.bytes }),
                    format!("{}:{}:artifact:meta", run_id, resp.final_url),
                )
                .await;

            (
                Some(FormatRef { artifact_id: html_art.artifact_id, bytes: html_art.bytes }),
                Some(FormatRef { artifact_id: md_art.artifact_id, bytes: md_art.bytes }),
            )
        } else {
            (None, None)
        };

        let fetched_at = Utc::now();

        let output = NormalizedOutput {
            run_id: run_id.clone(),
            url: UrlTriple {
                requested: requested_url.to_string(),
                final_url: resp.final_url.to_string(),
                canonical: None,
            },
            status: resp.status,
            fetched_at,
            fingerprint: fp.0.clone(),
            formats: OutputFormats {
                html: html_fmt.clone(),
                markdown: md_fmt.clone(),
                raw: None,
                links: link_list,
                screenshot: None,
                pdf: None,
                extract: None,
            },
            change: ChangeInfo {
                status: change,
                prev_fingerprint: prev_fp.map(|p| p.0),
            },
            metadata: meta,
            driver: {
                let meta = self.driver.browser_meta();
                DriverInfo {
                    kind: self.driver.kind(),
                    duration_ms: resp.duration_ms,
                    profile: self.driver.tls_profile().map(|p| match p {
                        quarry_tls::TlsProfile::Chrome => "chrome".to_string(),
                        quarry_tls::TlsProfile::Firefox => "firefox".to_string(),
                        quarry_tls::TlsProfile::Safari => "safari".to_string(),
                    }),
                    version: None,
                    session_id: meta.as_ref().and_then(|m| m.session_id.clone()),
                    live_view_url: meta.as_ref().and_then(|m| m.live_view_url.clone()),
                    recording_id: meta.as_ref().and_then(|m| m.recording_id.clone()),
                }
            },
            // Cycle 21 / cluster #2 — stamp policy + identity so the
            // artifact is self-describing for audit. Two equal stamps
            // on two outputs proves they ran under the same policy;
            // a Strict-mode stamp with mismatched fingerprints is a
            // reproducibility-claim violation.
            determinism: Some({
                let identity = crate::policy::record_determinism_inputs(
                    requested_url.as_str(),
                    &self.policy,
                );
                quarry_core::output::DeterminismStamp {
                    mode: match self.policy.determinism {
                        crate::policy::Determinism::Strict => "strict".into(),
                        crate::policy::Determinism::BestEffort => "best_effort".into(),
                        crate::policy::Determinism::Off => "off".into(),
                    },
                    policy_fp: identity.policy_fp,
                    identity_id: identity.id,
                }
            }),
        };

        // Cycle 19 / cluster #16: index successful scrape into the local
        // Tantivy corpus so subsequent `/v1/search` queries hit warm.
        // Best-effort + fire-and-forget — index failures never fail the
        // scrape. Skipped under ZDR (the index is a durable artifact;
        // ZDR forbids durable writes).
        if let Some(idx) = self.local_index.clone() {
            if zdr::guard(self.zdr, WriteKind::Artifact).is_ok() {
                let host = url::Url::parse(&resp.final_url.to_string())
                    .ok()
                    .and_then(|u| u.host_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let doc = LocalDocument {
                    url: resp.final_url.to_string(),
                    title: output.metadata.title.clone().unwrap_or_default(),
                    body: md.clone(),
                    host,
                    org_id: self.org_id.clone().unwrap_or_default(),
                    fingerprint: fp.0.clone(),
                    fetched_at,
                };
                tokio::spawn(async move {
                    if let Err(e) = idx.add_document(doc).await {
                        tracing::warn!(error = %e, "local index add_document failed (non-fatal)");
                    }
                });
            }
        }

        if let (Some(ingest), Some(org_id)) = (&self.ingest, &self.org_id) {
            if zdr::guard(self.zdr, WriteKind::Artifact).is_ok() {
                let chunks: Vec<ChunkRef> = chunk::paragraph_chunks(&md, 1000)
                    .into_iter()
                    .map(|c| ChunkRef {
                        start: c.start,
                        end: c.end,
                        text: c.text,
                    })
                    .collect();

                let source_trace = SourceTrace {
                    source_url: resp.final_url.to_string(),
                    fetched_at,
                    fingerprint: fp.0.clone(),
                    field_traces: vec![],
                };

                let ingest_req = DataPlaneIngestRequest {
                    run_id: run_id.clone(),
                    org_id: org_id.clone(),
                    source_url: resp.final_url.to_string(),
                    title: output.metadata.title.clone(),
                    markdown: Some(md),
                    html_ref: html_fmt.as_ref().map(|f| f.artifact_id.clone()),
                    raw_ref: None,
                    chunks,
                    metadata: json!({}),
                    fingerprint: fp.0.clone(),
                    zdr: self.zdr,
                    retention_policy: None,
                    source_trace: Some(source_trace),
                };

                let ingest = ingest.clone();
                let event_sink = self.event_sink.clone();
                let ingest_run_id = run_id.clone();
                let ingest_url = resp.final_url.to_string();
                let cancel_token = self.cancel_token.clone();
                tokio::spawn(async move {
                    let ingest_fut = async {
                        match ingest.ingest(&ingest_req).await {
                            Ok(resp) => {
                                tracing::info!(
                                    document_id = %resp.document_id,
                                    index_status = ?resp.index_status,
                                    "data plane ingest succeeded"
                                );
                                event_sink
                                    .emit(
                                        ingest_run_id,
                                        EventType::StoreRecordWritten,
                                        json!({
                                            "url": ingest_url,
                                            "document_id": resp.document_id,
                                            "index_status": resp.index_status,
                                        }),
                                        format!("{}:ingest:written", ingest_req.run_id),
                                    )
                                    .await;
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "data plane ingest failed (non-fatal)");
                            }
                        }
                    };

                    // Race ingest against the cancellation token. If the
                    // parent run is cancelled (e.g., via Temporal cancel
                    // signal), abort the ingest cleanly so we don't leave
                    // orphan documents in the Data Plane.
                    match cancel_token {
                        Some(tok) => {
                            tokio::select! {
                                _ = ingest_fut => {}
                                _ = tok.cancelled() => {
                                    tracing::info!(
                                        "ingest aborted by cancellation token"
                                    );
                                }
                            }
                        }
                        None => ingest_fut.await,
                    }
                });
            }
        }

        Ok(output)
    }
}
