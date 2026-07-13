//! Durable `MemoryService` implementation backed by session-core `agent_memory`.

use crate::{
    auth::{identity, VerifiedIdentity},
    letta_adapter::LettaMemoryAdapter,
};
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    memory_service_server::{MemoryService, MemoryServiceServer},
    IndexMemoryRequest, IndexMemoryResponse, MemoryEntry, MemoryHealthRequest,
    MemoryHealthResponse, SearchMemoryRequest, SearchMemoryResponse,
};
use sqlx::PgPool;
use std::sync::Arc;
use tonic::{Request, Response, Status};

const MEMORY_READ_SCOPE: &str = "memory:read";
const MEMORY_WRITE_SCOPE: &str = "memory:write";
const LETTA_NOT_CONFIGURED: &str = "DEGRADED_LETTA_NOT_CONFIGURED";

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
        let rows = crate::dreaming::search_agent_memory(
            &self.pool,
            &crate::dreaming::AgentMemorySearch {
                org_id: req.org_id.trim(),
                thread_id: req.thread_id.trim(),
                owner_user_id: &owner_user_id,
                query: &req.query,
                topic_filter: &topic_filter,
                limit,
                updated_after,
            },
        )
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
                let outcome = letta
                    .search_detailed(
                        req.org_id.trim(),
                        req.thread_id.trim(),
                        &req.query,
                        &topic_filter,
                        remaining,
                    )
                    .await;
                degradation_reason = outcome.degradation_reason;
                for mut entry in outcome.entries {
                    let content = entry.content.trim();
                    if content.is_empty()
                        || entries
                            .iter()
                            .any(|existing| existing.content.trim() == content)
                    {
                        continue;
                    }
                    entry.score *= 0.85;
                    entries.push(entry);
                    if entries.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                        break;
                    }
                }
            }
        }

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

        let degradation_reason = if let Some(letta) = self.letta.as_ref() {
            letta
                .index_detailed(
                    req.org_id.trim(),
                    req.thread_id.trim(),
                    &req.topic,
                    &req.content,
                )
                .await
                .degradation_reason
        } else {
            Some(LETTA_NOT_CONFIGURED)
        };

        Ok(Response::new(index_response(memory_id, degradation_reason)))
    }

    async fn health(
        &self,
        request: Request<MemoryHealthRequest>,
    ) -> Result<Response<MemoryHealthResponse>, Status> {
        let _caller = identity(&request)?;
        Ok(Response::new(health_response(self.letta.as_ref())))
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
}
