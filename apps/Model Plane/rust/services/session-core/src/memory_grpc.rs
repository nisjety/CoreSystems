//! Durable `MemoryService` implementation backed by session-core `agent_memory`.

use crate::{
    auth::{identity, VerifiedIdentity},
    grpc::{MemoryRetention, ZDR_MEMORY_READ_SUPPRESSED, ZDR_MEMORY_WRITE_SUPPRESSED},
    letta_adapter::LettaMemoryAdapter,
};
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    memory_service_server::{MemoryService, MemoryServiceServer},
    DeleteMemoryRequest, DeleteMemoryResponse, IndexMemoryRequest, IndexMemoryResponse,
    ListMemoryRequest, ListMemoryResponse, MemoryEntry, MemoryHealthRequest, MemoryHealthResponse,
    SearchMemoryRequest, SearchMemoryResponse,
};
use sqlx::PgPool;
use std::sync::Arc;
use tonic::{Request, Response, Status};

const MEMORY_READ_SCOPE: &str = "memory:read";
const MEMORY_WRITE_SCOPE: &str = "memory:write";
/// Reported when this process has no letta endpoint at all. Shared with
/// [`crate::memory_erasure`] so the two surfaces cannot drift into two
/// different spellings of the same posture.
pub(crate) const LETTA_NOT_CONFIGURED: &str = "DEGRADED_LETTA_NOT_CONFIGURED";

#[tonic::async_trait]
trait ThreadOwnership: Send + Sync {
    async fn user_id(&self, org_id: &str, thread_id: &str) -> Result<Option<String>, Status>;
}

struct PgThreadOwnership {
    pool: PgPool,
}

#[tonic::async_trait]
impl ThreadOwnership for PgThreadOwnership {
    async fn user_id(&self, org_id: &str, thread_id: &str) -> Result<Option<String>, Status> {
        sqlx::query_scalar("SELECT user_id FROM threads WHERE id = $1 AND org_id = $2")
            .bind(thread_id)
            .bind(org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_thread_ownership_error(&error))
    }
}

fn map_thread_ownership_error(error: &sqlx::Error) -> Status {
    tracing::warn!(%error, "thread ownership lookup failed");
    Status::internal("thread ownership lookup failed")
}

pub(crate) struct MemoryGrpc {
    pool: PgPool,
    letta: Option<LettaMemoryAdapter>,
    ownership: Arc<dyn ThreadOwnership>,
}

impl MemoryGrpc {
    pub(crate) fn new(pool: PgPool, letta: Option<LettaMemoryAdapter>) -> Self {
        Self {
            ownership: Arc::new(PgThreadOwnership { pool: pool.clone() }),
            pool,
            letta,
        }
    }

    #[cfg(test)]
    fn new_with_ownership_for_test(
        pool: PgPool,
        letta: Option<LettaMemoryAdapter>,
        ownership: Arc<dyn ThreadOwnership>,
    ) -> Self {
        Self {
            pool,
            letta,
            ownership,
        }
    }

    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub(crate) fn into_server(self) -> MemoryServiceServer<Self> {
        MemoryServiceServer::new(self)
    }

    async fn authorize_thread(
        &self,
        caller: &VerifiedIdentity,
        org_id: &str,
        thread_id: &str,
    ) -> Result<String, Status> {
        let owner_user_id = self
            .ownership
            .user_id(org_id, thread_id)
            .await?
            .ok_or_else(|| Status::not_found("thread not found"))?;
        if !caller.is_service() {
            caller.authorize_user(&owner_user_id)?;
        }
        Ok(owner_user_id)
    }
}

#[allow(clippy::result_large_err)]
fn authorize_memory_preflight(
    caller: &VerifiedIdentity,
    requested_org: &str,
    service_scope: &str,
) -> Result<(), Status> {
    caller.authorize_org(requested_org)?;
    if caller.zdr() {
        return Err(Status::failed_precondition(
            "ZDR credentials cannot access durable memory",
        ));
    }
    if caller.is_service() {
        caller.require_service_scope(service_scope)?;
    }
    Ok(())
}

/// Authorization for the user-scoped `ListMemory`/`DeleteMemory` RPCs.
///
/// Unlike `authorize_memory_preflight` (the thread-scoped `SearchMemory`/
/// `IndexMemory` gate, which hard-errors a ZDR caller), a ZDR caller here is
/// not an authorization failure -- it is a caller who is, by policy, entitled
/// to call this RPC and always get an empty/no-op answer. Returning `Ok(false)`
/// tells the handler to skip durable memory entirely and answer with an empty
/// list or a no-op delete, never a 500, matching the same
/// `MemoryRetention`-derived posture the context-assembly read path uses.
#[allow(clippy::result_large_err)]
fn authorize_user_memory_preflight(
    caller: &VerifiedIdentity,
    requested_org: &str,
    requested_user: &str,
    service_scope: &str,
) -> Result<bool, Status> {
    caller.authorize_org(requested_org)?;
    if caller.is_service() {
        caller.require_service_scope(service_scope)?;
    } else {
        // A user caller may only ever list/delete their own memories -- this
        // is the same tenant-isolation class of check as `authorize_thread`,
        // just without a thread to look an owner up from.
        caller.authorize_user(requested_user)?;
    }
    Ok(MemoryRetention::of(caller).permits_durable_memory())
}

fn list_response(
    entries: Vec<MemoryEntry>,
    degradation_reason: Option<&str>,
) -> ListMemoryResponse {
    ListMemoryResponse {
        entries,
        degraded: degradation_reason.is_some(),
        degradation_reason: degradation_reason.unwrap_or_default().to_owned(),
    }
}

fn delete_response(deleted: bool, degradation_reason: Option<&str>) -> DeleteMemoryResponse {
    DeleteMemoryResponse {
        deleted,
        degraded: degradation_reason.is_some(),
        degradation_reason: degradation_reason.unwrap_or_default().to_owned(),
    }
}

fn search_response(
    entries: Vec<MemoryEntry>,
    degradation_reason: Option<&str>,
) -> SearchMemoryResponse {
    SearchMemoryResponse {
        entries,
        degraded: degradation_reason.is_some(),
        degradation_reason: degradation_reason.unwrap_or_default().to_owned(),
    }
}

fn pg_row_to_entry(
    row: crate::dreaming::MemorySearchRow,
    owner_user_id: &str,
) -> MemoryEntry {
    MemoryEntry {
        memory_id: row.id,
        thread_id: row.thread_id,
        topic: row.topic,
        content: row.content,
        score: row.score,
        updated_at: Some(prost_types::Timestamp {
            seconds: row.updated_at.timestamp(),
            nanos: i32::try_from(row.updated_at.timestamp_subsec_nanos()).unwrap_or(i32::MAX),
        }),
        user_id: owner_user_id.to_owned(),
        provenance: memory_provenance(row.provenance) as i32,
    }
}

/// Standard Reciprocal Rank Fusion constant. Not tuned for this deployment —
/// k=60 is the figure the three external systems consulted while designing
/// this fix (supermemory, hindsight, RetainDB) all independently converged
/// on, and using an untuned, well-established default is the honest choice
/// until this system has enough query volume to justify tuning it against
/// real outcomes instead of by feel.
const RRF_K: f64 = 60.0;

/// Merge pgstore's relevance-scored rows with a semantic backend's results
/// via Reciprocal Rank Fusion (RRF), with a confidence-ordered fallback tier
/// for whatever neither source found relevant.
///
/// RRF only makes sense over lists that are each independently a relevance
/// ranking FOR THIS QUERY — it fuses rank positions, not raw scores, on the
/// assumption that "ranks well" already means "relevant" in each input. Only
/// two of pgstore's rows genuinely satisfy that: the ones flagged
/// `exact_match` (a literal restatement of the query — clearly relevant, and
/// ranked among themselves by confidence). The REST of pgstore's candidate
/// pool is ordered by `confidence` alone, a self-reported certainty about the
/// fact with no relationship to this query — feeding that ordering into RRF
/// as if it were a relevance ranking would let an unrelated but
/// high-confidence row RRF-outrank a real semantic match, exactly
/// reproducing the "recency/confidence masks relevance" bug this fix exists
/// to remove, just relocated into the fusion step instead of the SQL query.
///
/// So: `exact_match` rows and the semantic backend's results are the two
/// genuine relevance signals and are RRF-fused together (a fact confirmed by
/// BOTH signals earns a real, principled boost over one confirmed by only
/// one). Everything else in pgstore's pool is confidence/recency-ordered
/// filler, appended only to fill remaining slots below `limit` — "recency is
/// the last resort, not the first," restored without needing recency to
/// compete numerically against relevance at all.
///
/// A separate recency-weighted ranked list as a third RRF input (mirroring a
/// temporal-strategy pattern seen in external prior art) is deliberately NOT
/// attempted here — no reviewed source documented a concrete decay formula,
/// so recency stays confined to the fallback tier's own ordering, which is a
/// real gap noted for follow-up rather than papered over with an invented
/// formula.
///
/// Each input is expected pre-sorted by its own relevance (pgstore rows by
/// `score` via `search_agent_memory`; `letta_entries` by the caller); this
/// function re-sorts defensively rather than trust that invariant silently,
/// since a caller-ordering bug here would silently degrade back to the exact
/// failure mode this function exists to fix.
fn merge_memory_search_results(
    mut pg_rows: Vec<crate::dreaming::MemorySearchRow>,
    owner_user_id: &str,
    mut letta_entries: Vec<MemoryEntry>,
    limit: usize,
) -> Vec<MemoryEntry> {
    pg_rows.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    letta_entries
        .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let (relevant, fallback): (Vec<_>, Vec<_>) = pg_rows.into_iter().partition(|row| row.exact_match);

    // RRF-fuse the two genuine relevance signals. `order` records each
    // content's first-seen position (pgstore's relevant rows first, in their
    // own rank order, then letta's) so that exact score TIES break on that
    // deterministic sequence via `sort_by`'s stability — not on a HashMap's
    // iteration order, which is randomized per process and would otherwise
    // make tied results silently reorder between deploys.
    let mut rrf_score: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let mut representative: std::collections::HashMap<String, MemoryEntry> =
        std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (rank, row) in relevant.into_iter().enumerate() {
        let key = row.content.trim().to_owned();
        if key.is_empty() {
            continue;
        }
        if !rrf_score.contains_key(&key) {
            order.push(key.clone());
        }
        *rrf_score.entry(key.clone()).or_insert(0.0) += 1.0 / (RRF_K + (rank + 1) as f64);
        representative
            .entry(key)
            .or_insert_with(|| pg_row_to_entry(row, owner_user_id));
    }
    for (rank, entry) in letta_entries.into_iter().enumerate() {
        let key = entry.content.trim().to_owned();
        if key.is_empty() {
            continue;
        }
        if !rrf_score.contains_key(&key) {
            order.push(key.clone());
        }
        *rrf_score.entry(key.clone()).or_insert(0.0) += 1.0 / (RRF_K + (rank + 1) as f64);
        representative.entry(key).or_insert(entry);
    }

    let mut fused: Vec<MemoryEntry> = order
        .into_iter()
        .map(|key| {
            representative
                .remove(&key)
                .expect("every key in `order` was inserted into `representative` at the same time")
        })
        .collect();
    fused.sort_by(|a, b| {
        rrf_score[a.content.trim()]
            .partial_cmp(&rrf_score[b.content.trim()])
            .unwrap_or(std::cmp::Ordering::Equal)
            .reverse()
    });

    let mut seen: std::collections::HashSet<String> =
        fused.iter().map(|entry| entry.content.trim().to_owned()).collect();
    for row in fallback {
        if fused.len() >= limit {
            break;
        }
        let key = row.content.trim().to_owned();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        fused.push(pg_row_to_entry(row, owner_user_id));
    }
    fused.truncate(limit);
    fused
}

fn index_response(memory_id: String, degradation_reason: Option<&str>) -> IndexMemoryResponse {
    IndexMemoryResponse {
        memory_id,
        degraded: degradation_reason.is_some(),
        degradation_reason: degradation_reason.unwrap_or_default().to_owned(),
    }
}

fn health_response(letta: Option<&LettaMemoryAdapter>) -> MemoryHealthResponse {
    let snapshot = letta.map(LettaMemoryAdapter::health_snapshot);
    let ready = snapshot.is_some_and(|status| status.ready);
    MemoryHealthResponse {
        status: if ready { "ok" } else { "degraded" }.to_owned(),
        ready,
        memory_status: snapshot
            .map_or(LETTA_NOT_CONFIGURED, |status| status.status)
            .to_owned(),
    }
}

#[tonic::async_trait]
impl MemoryService for MemoryGrpc {
    async fn search_memory(
        &self,
        request: Request<SearchMemoryRequest>,
    ) -> Result<Response<SearchMemoryResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        if req.org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if req.thread_id.trim().is_empty() {
            return Err(Status::invalid_argument("thread_id is required"));
        }
        authorize_memory_preflight(&caller, req.org_id.trim(), MEMORY_READ_SCOPE)?;
        let owner_user_id = self
            .authorize_thread(&caller, req.org_id.trim(), req.thread_id.trim())
            .await?;

        let updated_after = req.updated_after.and_then(|ts| {
            u32::try_from(ts.nanos)
                .ok()
                .and_then(|nanos| DateTime::<Utc>::from_timestamp(ts.seconds, nanos))
        });

        let limit = if req.limit == 0 { 10 } else { req.limit };
        let topic_filter = req.topic_filter.clone();
        let query_trimmed = req.query.trim();

        // pgstore and the semantic backend are independent reads — run them
        // concurrently rather than sequentially. The semantic backend used to
        // be queried ONLY when pgstore returned fewer than `limit` rows, which
        // in practice was almost never, because pgstore's WHERE clause has no
        // relevance filter and always fills up to `limit` once a user has that
        // many memories at all — so the real semantic layer was shadowed by a
        // ranking bug, not genuinely redundant. It now runs on every non-
        // trivial query, bounded by the same timeout budget it already had.
        let letta_search = async {
            if query_trimmed.is_empty() {
                return None;
            }
            let letta = self.letta.as_ref()?;
            Some(
                letta
                    .search_detailed(
                        req.org_id.trim(),
                        req.thread_id.trim(),
                        &owner_user_id,
                        &req.query,
                        &topic_filter,
                        limit,
                    )
                    .await,
            )
        };
        let pg_search_request = crate::dreaming::AgentMemorySearch {
            org_id: req.org_id.trim(),
            thread_id: req.thread_id.trim(),
            owner_user_id: &owner_user_id,
            query: &req.query,
            topic_filter: &topic_filter,
            limit,
            updated_after,
        };
        let (pg_result, letta_outcome) = tokio::join!(
            crate::dreaming::search_agent_memory(&self.pool, &pg_search_request),
            letta_search,
        );
        let pg_rows = pg_result.map_err(|e| Status::internal(e.to_string()))?;

        let health_reason = self
            .letta
            .as_ref()
            .and_then(|letta| {
                let snapshot = letta.health_snapshot();
                (!snapshot.ready).then_some(snapshot.status)
            })
            .or_else(|| self.letta.is_none().then_some(LETTA_NOT_CONFIGURED));
        let degradation_reason = letta_outcome
            .as_ref()
            .and_then(|outcome| outcome.degradation_reason)
            .or(health_reason);
        let mut letta_entries = letta_outcome.map(|outcome| outcome.entries).unwrap_or_default();
        crate::memory_control::filter_forgotten(&self.pool, req.org_id.trim(), &owner_user_id, &mut letta_entries)
            .await.map_err(|_| Status::unavailable("memory deletion state unavailable"))?;

        let entries = merge_memory_search_results(
            pg_rows,
            &owner_user_id,
            letta_entries,
            usize::try_from(limit).unwrap_or(usize::MAX),
        );

        Ok(Response::new(search_response(entries, degradation_reason)))
    }

    async fn index_memory(
        &self,
        request: Request<IndexMemoryRequest>,
    ) -> Result<Response<IndexMemoryResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        if req.org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if req.thread_id.trim().is_empty() {
            return Err(Status::invalid_argument("thread_id is required"));
        }
        if req.content.trim().is_empty() {
            return Err(Status::invalid_argument("content is required"));
        }
        authorize_memory_preflight(&caller, req.org_id.trim(), MEMORY_WRITE_SCOPE)?;
        let owner_user_id = self
            .authorize_thread(&caller, req.org_id.trim(), req.thread_id.trim())
            .await?;

        let (conversation_only,): (bool,) = sqlx::query_as(
            "SELECT source_scope = 'conversation' FROM threads WHERE id = $1",
        ).bind(req.thread_id.trim()).fetch_one(&self.pool).await
            .map_err(|e| Status::internal(e.to_string()))?;
        if conversation_only {
            return Err(Status::failed_precondition("Conversation-only tasks cannot create long-term memories."));
        }
        let memory_id = crate::dreaming::index_agent_memory(
            &self.pool,
            req.org_id.trim(),
            req.thread_id.trim(),
            &owner_user_id,
            &req.topic,
            &req.content,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        if memory_id.is_empty() {
            return Err(Status::not_found("thread not found for org"));
        }

        // Mirrors dreaming::index_agent_memory's own scope decision (topic
        // "USER" -> scope "user", owner = the thread's user). Tagging the
        // semantic copy with an owner only for genuinely user-scoped memories
        // keeps ListMemory's "what do you remember about me" answer limited
        // to facts actually about that user, not every org/workspace/policy
        // fact the user happened to write. memory_id is always threaded
        // through so a later DeleteMemory removes both copies regardless of
        // topic.
        let is_user_scoped = req.topic.trim().eq_ignore_ascii_case("USER");
        let letta_owner = is_user_scoped.then_some(owner_user_id.as_str());
        let degradation_reason = if let Some(letta) = self.letta.as_ref() {
            letta
                .index_detailed(
                    req.org_id.trim(),
                    req.thread_id.trim(),
                    &req.topic,
                    &req.content,
                    letta_owner,
                    Some(&memory_id),
                )
                .await
                .degradation_reason
        } else {
            Some(LETTA_NOT_CONFIGURED)
        };

        if let Some(letta) = self.letta.as_ref() {
            crate::memory_control::erase_late_mirror(&self.pool, letta, req.org_id.trim(), &owner_user_id, &memory_id).await;
        }
        Ok(Response::new(index_response(memory_id, degradation_reason)))
    }

    async fn list_memory(
        &self,
        request: Request<ListMemoryRequest>,
    ) -> Result<Response<ListMemoryResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        if req.org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if req.user_id.trim().is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }
        let org_id = req.org_id.trim();
        let user_id = req.user_id.trim();
        let permits_durable_memory =
            authorize_user_memory_preflight(&caller, org_id, user_id, MEMORY_READ_SCOPE)?;

        if !permits_durable_memory {
            return Ok(Response::new(list_response(
                Vec::new(),
                Some(ZDR_MEMORY_READ_SUPPRESSED),
            )));
        }

        let limit = if req.limit == 0 { 100 } else { req.limit };

        let rows = crate::dreaming::list_user_memory(&self.pool, org_id, user_id, i64::from(limit))
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut entries: Vec<MemoryEntry> = rows
            .into_iter()
            .map(|row| MemoryEntry {
                memory_id: row.id,
                thread_id: row.thread_id,
                topic: row.topic,
                content: row.content,
                score: row.score,
                updated_at: Some(prost_types::Timestamp {
                    seconds: row.updated_at.timestamp(),
                    nanos: i32::try_from(row.updated_at.timestamp_subsec_nanos())
                        .unwrap_or(i32::MAX),
                }),
                user_id: user_id.to_owned(),
                provenance: memory_provenance(row.provenance) as i32,
            })
            .collect();

        let mut degradation_reason = self
            .letta
            .as_ref()
            .and_then(|letta| {
                let snapshot = letta.health_snapshot();
                (!snapshot.ready).then_some(snapshot.status)
            })
            .or_else(|| self.letta.is_none().then_some(LETTA_NOT_CONFIGURED));
        if let Some(letta) = self.letta.as_ref() {
            let remaining = limit.saturating_sub(u32::try_from(entries.len()).unwrap_or(u32::MAX));
            if remaining > 0 {
                let outcome = letta.list_detailed(org_id, user_id, remaining).await;
                degradation_reason = outcome.degradation_reason;
                for entry in outcome.entries {
                    let content = entry.content.trim();
                    if content.is_empty()
                        || entries
                            .iter()
                            .any(|existing| existing.content.trim() == content)
                    {
                        continue;
                    }
                    entries.push(entry);
                    if entries.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                        break;
                    }
                }
            }
        }

        crate::memory_control::filter_forgotten(&self.pool, org_id, user_id, &mut entries)
            .await.map_err(|_| Status::unavailable("memory deletion state unavailable"))?;
        Ok(Response::new(list_response(entries, degradation_reason)))
    }

    async fn delete_memory(
        &self,
        request: Request<DeleteMemoryRequest>,
    ) -> Result<Response<DeleteMemoryResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        if req.org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if req.user_id.trim().is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }
        if req.memory_id.trim().is_empty() {
            return Err(Status::invalid_argument("memory_id is required"));
        }
        let org_id = req.org_id.trim();
        let user_id = req.user_id.trim();
        let memory_id = req.memory_id.trim();
        let permits_durable_memory =
            authorize_user_memory_preflight(&caller, org_id, user_id, MEMORY_WRITE_SCOPE)?;

        if !permits_durable_memory {
            return Ok(Response::new(delete_response(
                false,
                Some(ZDR_MEMORY_WRITE_SUPPRESSED),
            )));
        }

        // Scoped to (org_id, user_id) in the SQL WHERE clause itself (see
        // dreaming::delete_user_memory), so this can never delete another
        // user's or another org's memory even if a memory_id were guessed.
        let deleted = crate::dreaming::delete_user_memory(&self.pool, org_id, user_id, memory_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // The semantic tier is asked even when the durable row was already
        // gone, because `ListMemory` merges BOTH tiers: a row the user can see
        // in "what do you remember" may exist only in Letta (rows written
        // before the durable index, or by a path that only indexed
        // semantically). Returning `not_found` on the durable miss — as this
        // did — made those rows visible and permanently undeletable, which is
        // the one outcome a memory-erasure path must never produce. Measured
        // 2026-09-14: 43 listed, 36 deletable, 7 stuck.
        let semantic = if let Some(letta) = self.letta.as_ref() {
            Some(letta.delete_detailed(org_id, user_id, memory_id).await)
        } else {
            None
        };
        let erased_semantically = semantic.as_ref().is_some_and(|outcome| outcome.deleted);

        if !deleted && !erased_semantically {
            return Err(Status::not_found("memory not found"));
        }

        // Best-effort: the durable record is already gone and is the source
        // of truth for existence, so a degraded semantic-side delete does not
        // fail the RPC -- it is only reported back for observability.
        let degradation_reason = if let Some(outcome) = semantic {
            // `outcome.deleted` used to be discarded entirely here, which is
            // exactly how letta-bridge's own pgstore/memstore tiers being a
            // silent no-op (see letta-bridge's pkg for the fix) went
            // unnoticed for so long: nothing, not even a log line,
            // distinguished "actually erased" from "declined silently".
            // `false` with no degradation_reason is not itself an error --
            // the semantic tier may legitimately never have held this
            // memory_id -- but it must be observable for DSAR audit trails.
            if !outcome.deleted && outcome.degradation_reason.is_none() {
                tracing::info!(
                    org_id,
                    user_id,
                    memory_id,
                    "letta-bridge semantic delete reported no matching record"
                );
            }
            outcome.degradation_reason
        } else {
            None
        };

        Ok(Response::new(delete_response(true, degradation_reason)))
    }

    async fn health(
        &self,
        request: Request<MemoryHealthRequest>,
    ) -> Result<Response<MemoryHealthResponse>, Status> {
        let _caller = identity(&request)?;
        Ok(Response::new(health_response(self.letta.as_ref())))
    }
}

/// Map the store's provenance onto the wire enum.
///
/// Explicit rather than `#[repr]`-coupled so a future variant cannot silently
/// acquire a wrong tag.
fn memory_provenance(
    provenance: crate::dreaming::MemoryProvenance,
) -> mp_contracts::model_plane::v1::MemoryProvenance {
    match provenance {
        crate::dreaming::MemoryProvenance::Unknown => {
            mp_contracts::model_plane::v1::MemoryProvenance::Unspecified
        }
        crate::dreaming::MemoryProvenance::Stated => {
            mp_contracts::model_plane::v1::MemoryProvenance::Stated
        }
        crate::dreaming::MemoryProvenance::Inferred => {
            mp_contracts::model_plane::v1::MemoryProvenance::Inferred
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::VerifiedIdentity;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct FakeThreadOwnership {
        calls: AtomicUsize,
        owner_user_id: String,
    }

    #[tonic::async_trait]
    impl ThreadOwnership for FakeThreadOwnership {
        async fn user_id(&self, _org_id: &str, _thread_id: &str) -> Result<Option<String>, Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(self.owner_user_id.clone()))
        }
    }

    fn request<T>(value: T, identity: VerifiedIdentity) -> Request<T> {
        let mut request = Request::new(value);
        request.extensions_mut().insert(identity);
        request
    }

    fn search_request(org_id: &str) -> SearchMemoryRequest {
        SearchMemoryRequest {
            org_id: org_id.to_owned(),
            thread_id: "thread-a".to_owned(),
            query: "query".to_owned(),
            limit: 5,
            ..SearchMemoryRequest::default()
        }
    }

    fn index_request(org_id: &str) -> IndexMemoryRequest {
        IndexMemoryRequest {
            org_id: org_id.to_owned(),
            thread_id: "thread-a".to_owned(),
            topic: "MEMORY".to_owned(),
            content: "must not persist".to_owned(),
            user_id: String::new(),
            memory_id: String::new(),
        }
    }

    fn list_request(org_id: &str, user_id: &str) -> ListMemoryRequest {
        ListMemoryRequest {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            limit: 10,
        }
    }

    fn delete_request(org_id: &str, user_id: &str, memory_id: &str) -> DeleteMemoryRequest {
        DeleteMemoryRequest {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            memory_id: memory_id.to_owned(),
        }
    }

    fn guarded_service(
        auth_core_url: &str,
        owner_user_id: &str,
    ) -> (MemoryGrpc, Arc<FakeThreadOwnership>) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let ownership = Arc::new(FakeThreadOwnership {
            calls: AtomicUsize::new(0),
            owner_user_id: owner_user_id.to_owned(),
        });
        let letta = LettaMemoryAdapter::new_for_test(
            "http://127.0.0.1:9",
            auth_core_url,
            "session-core",
            "session-core-test-credential",
        );
        (
            MemoryGrpc::new_with_ownership_for_test(pool, Some(letta), ownership.clone()),
            ownership,
        )
    }

    #[tokio::test]
    async fn rejected_identity_never_reaches_thread_db_memory_db_or_auth_core() {
        let auth = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&auth)
            .await;

        let (service, ownership) = guarded_service(&auth.uri(), "user-a");

        let cases = [
            service
                .search_memory(request(
                    search_request("org-b"),
                    VerifiedIdentity::user_for_test("org-a", "user-a"),
                ))
                .await
                .unwrap_err(),
            service
                .search_memory(request(
                    search_request("org-a"),
                    VerifiedIdentity::service_for_test("org-a", &["memory:write"], false),
                ))
                .await
                .unwrap_err(),
            service
                .index_memory(request(
                    index_request("org-a"),
                    VerifiedIdentity::service_for_test("org-a", &["memory:read"], false),
                ))
                .await
                .unwrap_err(),
            service
                .search_memory(request(
                    search_request("org-a"),
                    VerifiedIdentity::user_for_test_with_zdr("org-a", "user-a", true),
                ))
                .await
                .unwrap_err(),
        ];

        assert_eq!(cases[0].code(), tonic::Code::PermissionDenied);
        assert_eq!(cases[1].code(), tonic::Code::PermissionDenied);
        assert_eq!(cases[2].code(), tonic::Code::PermissionDenied);
        assert_eq!(cases[3].code(), tonic::Code::FailedPrecondition);
        assert_eq!(ownership.calls.load(Ordering::SeqCst), 0);
        assert!(auth.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn wrong_user_stops_after_scoped_ownership_lookup_before_memory_or_auth_core() {
        let auth = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&auth)
            .await;
        let (service, ownership) = guarded_service(&auth.uri(), "user-b");

        let error = service
            .search_memory(request(
                search_request("org-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .unwrap_err();

        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(ownership.calls.load(Ordering::SeqCst), 1);
        assert!(auth.received_requests().await.unwrap().is_empty());
    }

    /// `ListMemory`/`DeleteMemory` are user-scoped, not thread-scoped, so they
    /// have no thread ownership lookup to fall back on -- the caller's own
    /// identity is the only tenant-isolation boundary. This asserts a caller
    /// can never list or delete another user's or another org's memories, and
    /// that the rejection happens before any database or Letta call (the
    /// lazy-connected pool would hang/error on a real query, and the mock
    /// Auth Core asserts zero requests).
    #[tokio::test]
    async fn list_and_delete_memory_reject_another_users_or_orgs_request_before_touching_db_or_letta(
    ) {
        let auth = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&auth)
            .await;
        let (service, ownership) = guarded_service(&auth.uri(), "user-a");

        let cases = [
            // A user cannot list a different user's memories, even within
            // their own org.
            service
                .list_memory(request(
                    list_request("org-a", "user-b"),
                    VerifiedIdentity::user_for_test("org-a", "user-a"),
                ))
                .await
                .unwrap_err(),
            // Nor a different org's.
            service
                .list_memory(request(
                    list_request("org-b", "user-a"),
                    VerifiedIdentity::user_for_test("org-a", "user-a"),
                ))
                .await
                .unwrap_err(),
            // Same for delete: another user's id in the request is rejected
            // even though the org matches.
            service
                .delete_memory(request(
                    delete_request("org-a", "user-b", "memory-1"),
                    VerifiedIdentity::user_for_test("org-a", "user-a"),
                ))
                .await
                .unwrap_err(),
            // A service caller without the required scope is rejected too --
            // holding *a* service scope is not enough, it must be the right one.
            service
                .list_memory(request(
                    list_request("org-a", "user-a"),
                    VerifiedIdentity::service_for_test("org-a", &["memory:write"], false),
                ))
                .await
                .unwrap_err(),
            service
                .delete_memory(request(
                    delete_request("org-a", "user-a", "memory-1"),
                    VerifiedIdentity::service_for_test("org-a", &["memory:read"], false),
                ))
                .await
                .unwrap_err(),
        ];

        for error in &cases {
            assert_eq!(error.code(), tonic::Code::PermissionDenied);
        }
        assert_eq!(ownership.calls.load(Ordering::SeqCst), 0);
        assert!(auth.received_requests().await.unwrap().is_empty());
    }

    /// A ZDR caller calling List/Delete on their own identity is not an error
    /// -- it is a well-formed request that always gets an empty/no-op answer,
    /// per `MemoryRetention::permits_durable_memory`'s documented policy for
    /// this boundary. Neither the database nor Letta is touched.
    #[tokio::test]
    async fn list_and_delete_memory_are_noop_not_error_for_a_zdr_caller() {
        let auth = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&auth)
            .await;
        let (service, ownership) = guarded_service(&auth.uri(), "user-a");
        let zdr_caller = VerifiedIdentity::user_for_test_with_zdr("org-a", "user-a", true);

        let list = service
            .list_memory(request(list_request("org-a", "user-a"), zdr_caller.clone()))
            .await
            .expect("ZDR list is a well-formed empty response, not an error")
            .into_inner();
        assert!(list.entries.is_empty());
        assert!(list.degraded);
        assert_eq!(
            list.degradation_reason,
            "DEGRADED_LETTA_ZDR_READ_SUPPRESSED"
        );

        let delete = service
            .delete_memory(request(
                delete_request("org-a", "user-a", "memory-1"),
                zdr_caller,
            ))
            .await
            .expect("ZDR delete is a well-formed no-op, not an error")
            .into_inner();
        assert!(!delete.deleted);
        assert!(delete.degraded);
        assert_eq!(
            delete.degradation_reason,
            "DEGRADED_LETTA_ZDR_WRITE_SUPPRESSED"
        );

        assert_eq!(ownership.calls.load(Ordering::SeqCst), 0);
        assert!(auth.received_requests().await.unwrap().is_empty());
    }

    #[test]
    fn list_and_delete_memory_responses_are_machine_readable() {
        let list = list_response(Vec::new(), Some("DEGRADED_LETTA_ZDR_READ_SUPPRESSED"));
        assert!(list.degraded);
        assert_eq!(
            list.degradation_reason,
            "DEGRADED_LETTA_ZDR_READ_SUPPRESSED"
        );
        assert!(list.entries.is_empty());

        let not_degraded = list_response(Vec::new(), None);
        assert!(!not_degraded.degraded);
        assert_eq!(not_degraded.degradation_reason, "");

        let delete = delete_response(true, Some("DEGRADED_LETTA_TIMEOUT"));
        assert!(delete.deleted);
        assert!(delete.degraded);
        assert_eq!(delete.degradation_reason, "DEGRADED_LETTA_TIMEOUT");

        let clean_delete = delete_response(true, None);
        assert!(clean_delete.deleted);
        assert!(!clean_delete.degraded);
        assert_eq!(clean_delete.degradation_reason, "");
    }

    #[test]
    fn degraded_memory_responses_are_machine_readable() {
        let search = search_response(Vec::new(), Some("DEGRADED_LETTA_UNAVAILABLE"));
        assert!(search.degraded);
        assert_eq!(search.degradation_reason, "DEGRADED_LETTA_UNAVAILABLE");

        let index = index_response(
            "memory-a".to_owned(),
            Some("DEGRADED_LETTA_LEXICAL_FALLBACK"),
        );
        assert!(index.degraded);
        assert_eq!(index.degradation_reason, "DEGRADED_LETTA_LEXICAL_FALLBACK");

        let health = health_response(None);
        assert_eq!(health.status, "degraded");
        assert!(!health.ready);
        assert_eq!(health.memory_status, "DEGRADED_LETTA_NOT_CONFIGURED");
    }

    #[test]
    fn ownership_database_errors_do_not_leak_backend_details() {
        let error = map_thread_ownership_error(&sqlx::Error::RowNotFound);
        assert_eq!(error.code(), tonic::Code::Internal);
        assert_eq!(error.message(), "thread ownership lookup failed");
        assert!(!error.message().contains("row"));
    }

    mod merge {
        use super::*;
        use crate::dreaming::{MemoryProvenance, MemorySearchRow};

        fn pg_row(content: &str, score: f32, exact_match: bool, updated_at: DateTime<Utc>) -> MemorySearchRow {
            MemorySearchRow {
                id: format!("pg-{content}"),
                thread_id: "thread-a".to_owned(),
                topic: "MEMORY".to_owned(),
                content: content.to_owned(),
                score,
                updated_at,
                provenance: MemoryProvenance::Inferred,
                exact_match,
            }
        }

        fn letta_entry(content: &str, score: f32, updated_at: DateTime<Utc>) -> MemoryEntry {
            MemoryEntry {
                memory_id: format!("letta-{content}"),
                thread_id: "thread-a".to_owned(),
                topic: "MEMORY".to_owned(),
                content: content.to_owned(),
                score,
                updated_at: Some(prost_types::Timestamp {
                    seconds: updated_at.timestamp(),
                    nanos: 0,
                }),
                user_id: "user-a".to_owned(),
                provenance: 0,
            }
        }

        fn now() -> DateTime<Utc> {
            DateTime::parse_from_rfc3339("2026-09-17T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        }

        /// THE bug this whole fix exists for: a real semantic match must not
        /// be crowded out by pgstore rows that merely happen to be recent.
        /// Before this change, the semantic backend was queried only for the
        /// SHORTFALL after pgstore filled `limit` — which pgstore's own
        /// unfiltered WHERE clause did almost every time, so this exact case
        /// (a relevant Letta hit, but pgstore already has `limit` recent rows)
        /// silently dropped the relevant result on the floor.
        #[test]
        fn a_real_semantic_match_is_not_crowded_out_by_merely_recent_pgstore_rows() {
            let t = now();
            let pg_rows = vec![
                pg_row("user is based in Norway", 0.9, false, t),
                pg_row("user works in IT", 0.85, false, t),
            ];
            let letta = vec![letta_entry("the user's employer is Nordvik", 0.81, t)];

            let merged = merge_memory_search_results(pg_rows, "user-a", letta, 2);

            assert_eq!(merged.len(), 2);
            assert!(
                merged.iter().any(|e| e.content.contains("Nordvik")),
                "the semantic hit must survive the merge even though pgstore already filled the limit: {merged:?}"
            );
        }

        /// The two genuine relevance signals (an exact restatement, and a real
        /// semantic match) both outrank a row with no relevance signal at all
        /// — even one with a much higher raw `score` — because `score` on a
        /// non-exact-match pgstore row is bare self-reported confidence about
        /// the FACT, not relevance to this query, and never enters the RRF
        /// fusion at all; it is fallback filler, ranked only against other
        /// filler.
        #[test]
        fn relevance_signals_outrank_a_high_confidence_row_with_no_relevance_signal() {
            let t = now();
            let pg_rows = vec![
                pg_row("high-confidence but irrelevant fact", 0.99, false, t),
                pg_row("the literal query text", 0.5, true, t),
            ];
            let letta = vec![letta_entry("a real semantic match", 0.6, t)];

            let merged = merge_memory_search_results(pg_rows, "user-a", letta, 3);

            assert_eq!(merged.len(), 3);
            assert_eq!(
                merged[2].content, "high-confidence but irrelevant fact",
                "a row with neither an exact match nor a semantic hit must rank last: {merged:?}"
            );
            assert!(
                merged[0..2].iter().any(|e| e.content == "the literal query text"),
                "the exact-match row must be one of the top two: {merged:?}"
            );
            assert!(
                merged[0..2].iter().any(|e| e.content == "a real semantic match"),
                "the semantic-match row must be one of the top two: {merged:?}"
            );
        }

        /// A fact confirmed by BOTH signals — it is an exact restatement AND
        /// the semantic backend also independently surfaces it — earns a real
        /// RRF boost over a fact confirmed by only one signal. This is the
        /// property a hard-priority-bucket scheme cannot express: two
        /// independent signals agreeing is stronger evidence than either
        /// alone.
        #[test]
        fn a_fact_confirmed_by_both_signals_outranks_one_confirmed_by_only_one() {
            let t = now();
            let pg_rows = vec![
                pg_row("confirmed by both signals", 0.6, true, t),
                pg_row("confirmed only by exact match", 0.9, true, t),
            ];
            let letta = vec![
                letta_entry("confirmed by both signals", 0.55, t),
                letta_entry("confirmed only by semantic match", 0.95, t),
            ];

            let merged = merge_memory_search_results(pg_rows, "user-a", letta, 4);

            assert_eq!(
                merged[0].content, "confirmed by both signals",
                "double-confirmed must win even though single-signal rows had higher raw scores on their own side: {merged:?}"
            );
        }

        /// A fact stored in both pgstore and the semantic mirror (the normal
        /// case once §2's write-path fix lands) must appear once, not twice.
        #[test]
        fn the_same_fact_present_in_both_sources_is_not_duplicated() {
            let t = now();
            let pg_rows = vec![pg_row("user prefers drafts, not sent messages", 0.85, false, t)];
            let letta = vec![letta_entry("user prefers drafts, not sent messages", 0.7, t)];

            let merged = merge_memory_search_results(pg_rows, "user-a", letta, 5);

            assert_eq!(merged.len(), 1);
        }

        /// The merge respects the caller's limit even when both sources
        /// together would exceed it.
        #[test]
        fn the_result_never_exceeds_the_requested_limit() {
            let t = now();
            let pg_rows = vec![
                pg_row("a", 0.9, false, t),
                pg_row("b", 0.8, false, t),
                pg_row("c", 0.7, false, t),
            ];
            let letta = vec![letta_entry("d", 0.6, t), letta_entry("e", 0.5, t)];

            let merged = merge_memory_search_results(pg_rows, "user-a", letta, 2);

            assert_eq!(merged.len(), 2);
        }

        /// No semantic backend configured (or nothing returned) degrades to
        /// pgstore-only, still correctly bucketed.
        #[test]
        fn an_empty_semantic_result_still_returns_pgstore_rows_correctly_ranked() {
            let t = now();
            let pg_rows = vec![
                pg_row("exact query restatement", 0.5, true, t),
                pg_row("unrelated recent row", 0.99, false, t),
            ];

            let merged = merge_memory_search_results(pg_rows, "user-a", vec![], 2);

            assert_eq!(merged[0].content, "exact query restatement");
            assert_eq!(merged[1].content, "unrelated recent row");
        }
    }
}
