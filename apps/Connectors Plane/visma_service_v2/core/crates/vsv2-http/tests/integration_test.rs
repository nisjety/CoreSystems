use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use chrono::{TimeZone, Utc};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;
use vsv2_common::{Source, TenantId};
use vsv2_http::metadata_store::{
    CreateScheduleInput, CreateSyncOperationError, CreateSyncOperationInput, MetadataStore,
    PersistedSchedule, PersistedSyncOperation, RunsQuery, SyncOperationKind,
};
use vsv2_http::sync_runner::{EndpointSyncFailure, SyncExecutionSummary, SyncRunner};
use vsv2_http::{app, AppState};
use vsv2_orchestrator::EndpointRegistry;
use vsv2_state::{Checkpoint, CheckpointKey, RunStatus, SyncRun};

#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncCall {
    All {
        tenant: String,
        date_from: Option<String>,
        date_to: Option<String>,
    },
    Names {
        tenant: String,
        names: Vec<String>,
        date_from: Option<String>,
        date_to: Option<String>,
    },
}

#[derive(Default)]
struct MockSyncRunner {
    calls: Mutex<Vec<SyncCall>>,
    all_endpoints: Vec<String>,
    failures: BTreeMap<String, String>,
    result: u64,
    delay: Option<Duration>,
}

impl MockSyncRunner {
    fn build_summary(
        &self,
        endpoint_names: &[String],
        attempted_endpoints: usize,
    ) -> SyncExecutionSummary {
        let failed_endpoints = endpoint_names
            .iter()
            .filter_map(|endpoint| {
                self.failures
                    .get(endpoint)
                    .map(|error| EndpointSyncFailure {
                        endpoint: endpoint.clone(),
                        error: error.clone(),
                    })
            })
            .collect();

        SyncExecutionSummary {
            records_synced: self.result,
            attempted_endpoints,
            failed_endpoints,
        }
    }
}

#[derive(Default)]
struct MockMetadataStore {
    runs: Mutex<Vec<SyncRun>>,
    checkpoints: Mutex<Vec<Checkpoint>>,
    schedules: Mutex<Vec<PersistedSchedule>>,
    sync_operations: Mutex<Vec<PersistedSyncOperation>>,
    create_calls: Mutex<Vec<CreateScheduleInput>>,
    run_queries: Mutex<Vec<RunsQuery>>,
}

#[async_trait]
impl SyncRunner for MockSyncRunner {
    async fn sync_all_window(
        &self,
        tenant: &TenantId,
        date_from: Option<chrono::DateTime<Utc>>,
        date_to: Option<chrono::DateTime<Utc>>,
    ) -> anyhow::Result<SyncExecutionSummary> {
        self.calls.lock().await.push(SyncCall::All {
            tenant: tenant.to_string(),
            date_from: date_from.map(|value| value.to_rfc3339()),
            date_to: date_to.map(|value| value.to_rfc3339()),
        });
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        let attempted_endpoints = self.all_endpoints.len().max(1);
        Ok(self.build_summary(&self.all_endpoints, attempted_endpoints))
    }

    async fn sync_names_window(
        &self,
        tenant: &TenantId,
        names: &[String],
        date_from: Option<chrono::DateTime<Utc>>,
        date_to: Option<chrono::DateTime<Utc>>,
    ) -> anyhow::Result<SyncExecutionSummary> {
        self.calls.lock().await.push(SyncCall::Names {
            tenant: tenant.to_string(),
            names: names.to_vec(),
            date_from: date_from.map(|value| value.to_rfc3339()),
            date_to: date_to.map(|value| value.to_rfc3339()),
        });
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        Ok(self.build_summary(names, names.len()))
    }
}

#[async_trait]
impl MetadataStore for MockMetadataStore {
    async fn list_runs(&self, query: RunsQuery) -> anyhow::Result<Vec<SyncRun>> {
        self.run_queries.lock().await.push(query);
        Ok(self.runs.lock().await.clone())
    }

    async fn list_checkpoints(
        &self,
        _tenant: &TenantId,
        _endpoint: Option<&str>,
        _source: Option<Source>,
    ) -> anyhow::Result<Vec<Checkpoint>> {
        Ok(self.checkpoints.lock().await.clone())
    }

    async fn create_schedule(
        &self,
        input: CreateScheduleInput,
    ) -> anyhow::Result<PersistedSchedule> {
        self.create_calls.lock().await.push(input.clone());
        let schedule = PersistedSchedule {
            schedule_id: Uuid::new_v4(),
            tenant: input.tenant,
            name: input.name,
            cron: input.cron,
            endpoints: input.endpoints,
            created_at: Utc::now(),
            disabled_at: None,
        };
        self.schedules.lock().await.push(schedule.clone());
        Ok(schedule)
    }

    async fn list_schedules(
        &self,
        _tenant: Option<&TenantId>,
    ) -> anyhow::Result<Vec<PersistedSchedule>> {
        Ok(self.schedules.lock().await.clone())
    }

    async fn get_schedule(&self, schedule_id: Uuid) -> anyhow::Result<Option<PersistedSchedule>> {
        Ok(self
            .schedules
            .lock()
            .await
            .iter()
            .find(|schedule| schedule.schedule_id == schedule_id)
            .cloned())
    }

    async fn create_sync_operation(
        &self,
        input: CreateSyncOperationInput,
    ) -> Result<PersistedSyncOperation, CreateSyncOperationError> {
        let mut operations = self.sync_operations.lock().await;
        if let Some(existing) = operations
            .iter()
            .find(|operation| {
                is_active_status(operation.status)
                    && operation.kind == input.kind
                    && (operation.kind != SyncOperationKind::ScheduleTrigger
                        || operation.schedule_id == input.schedule_id)
            })
            .cloned()
        {
            return Err(CreateSyncOperationError::Conflict(Box::new(existing)));
        }

        let operation = PersistedSyncOperation {
            operation_id: Uuid::new_v4(),
            kind: input.kind,
            tenant: input.tenant,
            schedule_id: input.schedule_id,
            endpoints: input.endpoints,
            date_from: input.date_from,
            date_to: input.date_to,
            status: RunStatus::Pending,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            records_synced: 0,
            attempted_endpoints: 0,
            succeeded_endpoints: 0,
            failed_endpoints: Vec::new(),
            error: None,
        };
        operations.push(operation.clone());
        Ok(operation)
    }

    async fn get_sync_operation(
        &self,
        operation_id: Uuid,
    ) -> anyhow::Result<Option<PersistedSyncOperation>> {
        Ok(self
            .sync_operations
            .lock()
            .await
            .iter()
            .find(|operation| operation.operation_id == operation_id)
            .cloned())
    }

    async fn mark_sync_operation_running(&self, operation_id: Uuid) -> anyhow::Result<()> {
        let mut operations = self.sync_operations.lock().await;
        if let Some(operation) = operations
            .iter_mut()
            .find(|operation| operation.operation_id == operation_id)
        {
            operation.status = RunStatus::Running;
            operation.started_at = operation.started_at.or_else(|| Some(Utc::now()));
        }

        Ok(())
    }

    async fn complete_sync_operation(
        &self,
        operation_id: Uuid,
        summary: &SyncExecutionSummary,
    ) -> anyhow::Result<()> {
        let mut operations = self.sync_operations.lock().await;
        if let Some(operation) = operations
            .iter_mut()
            .find(|operation| operation.operation_id == operation_id)
        {
            operation.status = if summary.failed_endpoints.is_empty() {
                RunStatus::Succeeded
            } else if summary.succeeded_endpoints() == 0 {
                RunStatus::Failed
            } else {
                RunStatus::PartialSuccess
            };
            operation.started_at = operation.started_at.or_else(|| Some(Utc::now()));
            operation.finished_at = Some(Utc::now());
            operation.records_synced = summary.records_synced;
            operation.attempted_endpoints = summary.attempted_endpoints;
            operation.succeeded_endpoints = summary.succeeded_endpoints();
            operation.failed_endpoints = summary.failed_endpoints.clone();
            operation.error = None;
        }

        Ok(())
    }

    async fn fail_sync_operation(
        &self,
        operation_id: Uuid,
        error: &str,
        attempted_endpoints: usize,
    ) -> anyhow::Result<()> {
        let mut operations = self.sync_operations.lock().await;
        if let Some(operation) = operations
            .iter_mut()
            .find(|operation| operation.operation_id == operation_id)
        {
            operation.status = RunStatus::Failed;
            operation.started_at = operation.started_at.or_else(|| Some(Utc::now()));
            operation.finished_at = Some(Utc::now());
            operation.records_synced = 0;
            operation.attempted_endpoints = attempted_endpoints;
            operation.succeeded_endpoints = 0;
            operation.failed_endpoints = Vec::new();
            operation.error = Some(error.to_string());
        }

        Ok(())
    }

    async fn fail_stale_sync_operations(&self, error: &str) -> anyhow::Result<u64> {
        let mut operations = self.sync_operations.lock().await;
        let mut count = 0_u64;
        for operation in operations.iter_mut() {
            if is_active_status(operation.status) {
                operation.status = RunStatus::Failed;
                operation.finished_at = Some(Utc::now());
                operation.error = Some(error.to_string());
                count += 1;
            }
        }

        Ok(count)
    }
}

fn is_active_status(status: RunStatus) -> bool {
    matches!(status, RunStatus::Pending | RunStatus::Running)
}

fn workspace_endpoint_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config/endpoints")
}

fn bearer_header(token: &str) -> String {
    format!("Bearer {token}")
}

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn poll_backfill_until_status(
    app: &Router,
    backfill_id: &str,
    terminal_statuses: &[&str],
) -> serde_json::Value {
    for _ in 0..50 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/backfills/{backfill_id}"))
                    .header("authorization", bearer_header("test-token"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        let status = json["status"].as_str().unwrap();
        if terminal_statuses.contains(&status) {
            return json;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("backfill did not reach a terminal status in time");
}

async fn poll_schedule_trigger_until_status(
    app: &Router,
    schedule_id: Uuid,
    trigger_id: &str,
    terminal_statuses: &[&str],
) -> serde_json::Value {
    for _ in 0..50 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/schedules/{schedule_id}/triggers/{trigger_id}"))
                    .header("authorization", bearer_header("test-token"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        let status = json["status"].as_str().unwrap();
        if terminal_statuses.contains(&status) {
            return json;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("schedule trigger did not reach a terminal status in time");
}

#[tokio::test]
async fn health_route_returns_ok_json() {
    let state = AppState::without_pg(EndpointRegistry::new());
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn endpoints_route_lists_workspace_configs() {
    let registry = EndpointRegistry::load_dir(&workspace_endpoint_dir()).unwrap();
    let state = AppState::without_pg(registry).with_bearer_token("test-token");
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/endpoints")
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let endpoints = json["endpoints"].as_array().unwrap();

    assert!(endpoints.len() >= 70);
    assert!(endpoints
        .iter()
        .any(|endpoint| endpoint["endpoint_name"] == "customers"));
}

#[tokio::test]
async fn endpoints_route_handles_empty_registry() {
    let state = AppState::without_pg(EndpointRegistry::new()).with_bearer_token("test-token");
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/endpoints")
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["endpoints"], serde_json::json!([]));
}

#[tokio::test]
async fn backfill_route_calls_sync_all_for_all_endpoints() {
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: vec!["customers".to_string(), "customer_invoices".to_string()],
        failures: BTreeMap::new(),
        result: 73,
        delay: Some(Duration::from_millis(25)),
    });
    let state = AppState::without_pg(EndpointRegistry::new())
        .with_sync_runner(runner.clone())
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let app = app(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["all"],
                        "date_from": Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap().to_rfc3339(),
                        "date_to": Utc.with_ymd_and_hms(2026, 4, 26, 23, 59, 59).unwrap().to_rfc3339(),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let json = response_json(response).await;
    let backfill_id = json["backfill_id"].as_str().unwrap();
    assert!(Uuid::parse_str(backfill_id).is_ok());
    assert_eq!(json["status"], "pending");
    assert_eq!(json["endpoints"], serde_json::json!(["all"]));
    assert_eq!(json["poll_url"], format!("/v1/backfills/{backfill_id}"));
    assert!(json["runs_url"]
        .as_str()
        .unwrap()
        .contains("tenant=33f319b9-4d57-11ee-960c-025417856183"));

    let status_json = poll_backfill_until_status(&app, backfill_id, &["succeeded"]).await;
    assert_eq!(status_json["records_synced"], 73);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 2);

    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        SyncCall::All {
            tenant: "33f319b9-4d57-11ee-960c-025417856183".to_string(),
            date_from: Some("2026-04-20T00:00:00+00:00".to_string()),
            date_to: Some("2026-04-26T23:59:59+00:00".to_string()),
        }
    );
}

#[tokio::test]
async fn backfill_status_route_survives_app_restart_with_shared_store() {
    let metadata_store = Arc::new(MockMetadataStore::default());
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: vec!["customers".to_string()],
        failures: BTreeMap::new(),
        result: 1,
        delay: Some(Duration::from_millis(50)),
    });

    let app_a = app(AppState::without_pg(EndpointRegistry::new())
        .with_sync_runner(runner)
        .with_metadata_store(metadata_store.clone())
        .with_bearer_token("test-token"));

    let accept_response = app_a
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["all"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(accept_response.status(), StatusCode::ACCEPTED);
    let accept_json = response_json(accept_response).await;
    let backfill_id = accept_json["backfill_id"].as_str().unwrap();

    let app_b = app(AppState::without_pg(EndpointRegistry::new())
        .with_metadata_store(metadata_store)
        .with_bearer_token("test-token"));

    let response = app_b
        .oneshot(
            Request::builder()
                .uri(format!("/v1/backfills/{backfill_id}"))
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["backfill_id"], backfill_id);
    assert_eq!(json["tenant"], "33f319b9-4d57-11ee-960c-025417856183");
}

#[tokio::test]
async fn backfill_route_rejects_concurrent_backfills_with_conflict() {
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: vec!["customers".to_string()],
        failures: BTreeMap::new(),
        result: 1,
        delay: Some(Duration::from_millis(75)),
    });
    let state = AppState::without_pg(EndpointRegistry::new())
        .with_sync_runner(runner)
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let app = app(state);

    let first_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["all"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::ACCEPTED);
    let first_json = response_json(first_response).await;
    let backfill_id = first_json["backfill_id"].as_str().unwrap().to_string();

    let second_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["all"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second_response.status(), StatusCode::CONFLICT);
    let second_json = response_json(second_response).await;
    assert_eq!(second_json["error"], "backfill_already_running");
    assert!(second_json["message"]
        .as_str()
        .unwrap()
        .contains(&backfill_id));

    let status_json = poll_backfill_until_status(&app, &backfill_id, &["succeeded"]).await;
    assert_eq!(status_json["status"], "succeeded");
}

#[tokio::test]
async fn backfill_route_calls_sync_names_for_selected_endpoints() {
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: Vec::new(),
        failures: BTreeMap::new(),
        result: 11,
        delay: Some(Duration::from_millis(10)),
    });
    let registry = EndpointRegistry::load_dir(&workspace_endpoint_dir()).unwrap();
    let state = AppState::without_pg(registry)
        .with_sync_runner(runner.clone())
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let app = app(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["customers", "customer_invoices"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let json = response_json(response).await;
    let backfill_id = json["backfill_id"].as_str().unwrap();
    let status_json = poll_backfill_until_status(&app, backfill_id, &["succeeded"]).await;
    assert_eq!(status_json["status"], "succeeded");
    assert_eq!(status_json["records_synced"], 11);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 2);
    assert!(status_json["started_at"].is_string());
    assert!(status_json["finished_at"].is_string());

    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        SyncCall::Names {
            tenant: "33f319b9-4d57-11ee-960c-025417856183".to_string(),
            names: vec!["customers".to_string(), "customer_invoices".to_string()],
            date_from: None,
            date_to: None,
        }
    );
}

#[tokio::test]
async fn backfill_route_reports_partial_failure_for_selected_endpoints() {
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: Vec::new(),
        failures: BTreeMap::from([(
            "customer_invoices".to_string(),
            "upstream timeout".to_string(),
        )]),
        result: 11,
        delay: Some(Duration::from_millis(10)),
    });
    let registry = EndpointRegistry::load_dir(&workspace_endpoint_dir()).unwrap();
    let state = AppState::without_pg(registry)
        .with_sync_runner(runner)
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let app = app(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["customers", "customer_invoices"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let json = response_json(response).await;
    let backfill_id = json["backfill_id"].as_str().unwrap();
    let status_json = poll_backfill_until_status(&app, backfill_id, &["partial_success"]).await;
    assert_eq!(status_json["status"], "partial_success");
    assert_eq!(status_json["records_synced"], 11);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 1);
    assert_eq!(
        status_json["failed_endpoints"][0]["endpoint"],
        "customer_invoices"
    );
    assert_eq!(
        status_json["failed_endpoints"][0]["error"],
        "upstream timeout"
    );
}

#[tokio::test]
async fn backfill_route_reports_failed_status_when_all_endpoints_fail() {
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: vec!["customers".to_string(), "customer_invoices".to_string()],
        failures: BTreeMap::from([
            (
                "customers".to_string(),
                "finance host unavailable".to_string(),
            ),
            (
                "customer_invoices".to_string(),
                "finance host unavailable".to_string(),
            ),
        ]),
        result: 0,
        delay: Some(Duration::from_millis(10)),
    });
    let state = AppState::without_pg(EndpointRegistry::new())
        .with_sync_runner(runner)
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let app = app(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["all"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let json = response_json(response).await;
    let backfill_id = json["backfill_id"].as_str().unwrap();
    let status_json = poll_backfill_until_status(&app, backfill_id, &["failed"]).await;
    assert_eq!(status_json["status"], "failed");
    assert_eq!(status_json["records_synced"], 0);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 0);
    assert_eq!(status_json["failed_endpoints"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn backfill_route_rejects_mixed_all_and_named_endpoints() {
    let response =
        app(AppState::without_pg(EndpointRegistry::new()).with_bearer_token("test-token"))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sync/backfill")
                    .header("authorization", bearer_header("test-token"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                            "endpoints": ["all", "customers"],
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn protected_routes_require_bearer_token() {
    let response =
        app(AppState::without_pg(EndpointRegistry::new()).with_bearer_token("test-token"))
            .oneshot(
                Request::builder()
                    .uri("/v1/endpoints")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn backfill_route_rejects_invalid_tenant_uuid() {
    let response =
        app(AppState::without_pg(EndpointRegistry::new()).with_bearer_token("test-token"))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sync/backfill")
                    .header("authorization", bearer_header("test-token"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "tenant": "tenant-invalid",
                            "endpoints": ["all"],
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn backfill_route_rejects_unknown_endpoints() {
    let state =
        AppState::without_pg(EndpointRegistry::load_dir(&workspace_endpoint_dir()).unwrap())
            .with_bearer_token("test-token")
            .with_sync_runner(Arc::new(MockSyncRunner::default()));
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["customers", "not_real"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn backfill_route_rejects_inverted_date_range() {
    let response =
        app(AppState::without_pg(EndpointRegistry::new()).with_bearer_token("test-token"))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sync/backfill")
                    .header("authorization", bearer_header("test-token"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                            "endpoints": ["all"],
                            "date_from": "2026-04-30T00:00:00Z",
                            "date_to": "2026-04-20T00:00:00Z"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn backfill_route_times_out_long_running_syncs() {
    let state = AppState::without_pg(EndpointRegistry::new())
        .with_bearer_token("test-token")
        .with_sync_runner(Arc::new(MockSyncRunner {
            calls: Mutex::new(Vec::new()),
            all_endpoints: vec!["customers".to_string()],
            failures: BTreeMap::new(),
            result: 1,
            delay: Some(Duration::from_millis(50)),
        }))
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_backfill_timeout(Duration::from_millis(1));
    let app = app(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/backfill")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "endpoints": ["all"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let json = response_json(response).await;
    let backfill_id = json["backfill_id"].as_str().unwrap();
    let status_json = poll_backfill_until_status(&app, backfill_id, &["succeeded"]).await;
    assert_eq!(status_json["status"], "succeeded");
}

#[tokio::test]
async fn backfill_status_route_returns_not_found_for_unknown_handle() {
    let state = AppState::without_pg(EndpointRegistry::new())
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/backfills/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn runs_route_lists_runs_from_store() {
    let metadata_store = Arc::new(MockMetadataStore::default());
    metadata_store.runs.lock().await.push(SyncRun {
        run_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
        tenant: TenantId::new("33f319b9-4d57-11ee-960c-025417856183"),
        endpoint: "customers".to_string(),
        source: Source::ErpRest,
        status: RunStatus::Succeeded,
        started_at: Utc.with_ymd_and_hms(2026, 5, 1, 20, 0, 0).unwrap(),
        finished_at: Some(Utc.with_ymd_and_hms(2026, 5, 1, 20, 1, 0).unwrap()),
        records_in: 15,
        records_written: 15,
        error: None,
    });

    let state = AppState::without_pg(EndpointRegistry::new())
        .with_bearer_token("test-token")
        .with_metadata_store(metadata_store.clone());
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/runs?tenant=33f319b9-4d57-11ee-960c-025417856183&endpoint=customers&limit=5")
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["runs"][0]["endpoint"], "customers");
    assert_eq!(metadata_store.run_queries.lock().await.len(), 1);
}

#[tokio::test]
async fn checkpoints_route_lists_checkpoints_from_store() {
    let metadata_store = Arc::new(MockMetadataStore::default());
    metadata_store.checkpoints.lock().await.push(Checkpoint {
        key: CheckpointKey::for_partition(
            TenantId::new("33f319b9-4d57-11ee-960c-025417856183"),
            "customers".to_string(),
            Source::ErpRest,
            "default".to_string(),
        ),
        last_modified_watermark: Some(Utc.with_ymd_and_hms(2026, 5, 1, 20, 0, 0).unwrap()),
        last_updated_in_dm: None,
        last_id: None,
        last_sync_at: Some(Utc.with_ymd_and_hms(2026, 5, 1, 20, 1, 0).unwrap()),
        records_seen: 50,
    });

    let state = AppState::without_pg(EndpointRegistry::new())
        .with_bearer_token("test-token")
        .with_metadata_store(metadata_store);
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/checkpoints?tenant=33f319b9-4d57-11ee-960c-025417856183&endpoint=customers")
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["checkpoints"][0]["key"]["endpoint"], "customers");
}

#[tokio::test]
async fn schedules_route_creates_and_lists_persisted_schedules() {
    let metadata_store = Arc::new(MockMetadataStore::default());
    let state =
        AppState::without_pg(EndpointRegistry::load_dir(&workspace_endpoint_dir()).unwrap())
            .with_bearer_token("test-token")
            .with_metadata_store(metadata_store.clone());

    let create_response = app(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/schedules")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "name": "daily-all-2000",
                        "cron": "0 0 20 * * *",
                        "endpoints": ["all"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let list_response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/schedules?tenant=33f319b9-4d57-11ee-960c-025417856183")
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["schedules"][0]["name"], "daily-all-2000");
    assert_eq!(metadata_store.create_calls.lock().await.len(), 1);
}

#[tokio::test]
async fn schedule_trigger_route_runs_persisted_schedule() {
    let schedule_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
    let metadata_store = Arc::new(MockMetadataStore::default());
    metadata_store
        .schedules
        .lock()
        .await
        .push(PersistedSchedule {
            schedule_id,
            tenant: TenantId::new("33f319b9-4d57-11ee-960c-025417856183"),
            name: "daily-all-2000".to_string(),
            cron: "0 0 20 * * *".to_string(),
            endpoints: vec!["all".to_string()],
            created_at: Utc::now(),
            disabled_at: None,
        });
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: vec!["customers".to_string(), "customer_invoices".to_string()],
        failures: BTreeMap::new(),
        result: 120,
        delay: Some(Duration::from_millis(10)),
    });

    let app = app(AppState::without_pg(EndpointRegistry::new())
        .with_bearer_token("test-token")
        .with_metadata_store(metadata_store)
        .with_sync_runner(runner.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/schedules/{schedule_id}/trigger"))
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "date_from": "2026-04-27T00:00:00Z",
                        "date_to": "2026-04-27T23:59:59Z"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = response_json(response).await;
    let trigger_id = json["trigger_id"].as_str().unwrap();
    assert!(Uuid::parse_str(trigger_id).is_ok());
    assert_eq!(json["schedule_id"], schedule_id.to_string());
    assert_eq!(json["status"], "pending");
    assert_eq!(
        json["poll_url"],
        format!("/v1/schedules/{schedule_id}/triggers/{trigger_id}")
    );

    let status_json =
        poll_schedule_trigger_until_status(&app, schedule_id, trigger_id, &["succeeded"]).await;
    assert_eq!(status_json["records_synced"], 120);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 2);
    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        SyncCall::All {
            tenant: "33f319b9-4d57-11ee-960c-025417856183".to_string(),
            date_from: Some("2026-04-27T00:00:00+00:00".to_string()),
            date_to: Some("2026-04-27T23:59:59+00:00".to_string()),
        }
    );
}

#[tokio::test]
async fn schedule_trigger_route_reports_partial_failure() {
    let schedule_id = Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
    let metadata_store = Arc::new(MockMetadataStore::default());
    metadata_store
        .schedules
        .lock()
        .await
        .push(PersistedSchedule {
            schedule_id,
            tenant: TenantId::new("33f319b9-4d57-11ee-960c-025417856183"),
            name: "daily-targeted".to_string(),
            cron: "0 0 20 * * *".to_string(),
            endpoints: vec!["customers".to_string(), "customer_invoices".to_string()],
            created_at: Utc::now(),
            disabled_at: None,
        });
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: Vec::new(),
        failures: BTreeMap::from([(
            "customer_invoices".to_string(),
            "upstream timeout".to_string(),
        )]),
        result: 33,
        delay: Some(Duration::from_millis(10)),
    });

    let app = app(AppState::without_pg(EndpointRegistry::new())
        .with_bearer_token("test-token")
        .with_metadata_store(metadata_store)
        .with_sync_runner(runner));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/schedules/{schedule_id}/trigger"))
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = response_json(response).await;
    let trigger_id = json["trigger_id"].as_str().unwrap();
    let status_json =
        poll_schedule_trigger_until_status(&app, schedule_id, trigger_id, &["partial_success"])
            .await;
    assert_eq!(status_json["status"], "partial_success");
    assert_eq!(status_json["records_synced"], 33);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 1);
    assert_eq!(
        status_json["failed_endpoints"][0]["endpoint"],
        "customer_invoices"
    );
    assert_eq!(
        status_json["failed_endpoints"][0]["error"],
        "upstream timeout"
    );
}

#[tokio::test]
async fn schedule_trigger_route_reports_failed_status_when_all_endpoints_fail() {
    let schedule_id = Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap();
    let metadata_store = Arc::new(MockMetadataStore::default());
    metadata_store
        .schedules
        .lock()
        .await
        .push(PersistedSchedule {
            schedule_id,
            tenant: TenantId::new("33f319b9-4d57-11ee-960c-025417856183"),
            name: "daily-targeted".to_string(),
            cron: "0 0 20 * * *".to_string(),
            endpoints: vec!["customers".to_string(), "customer_invoices".to_string()],
            created_at: Utc::now(),
            disabled_at: None,
        });
    let runner = Arc::new(MockSyncRunner {
        calls: Mutex::new(Vec::new()),
        all_endpoints: Vec::new(),
        failures: BTreeMap::from([
            (
                "customers".to_string(),
                "finance host unavailable".to_string(),
            ),
            (
                "customer_invoices".to_string(),
                "finance host unavailable".to_string(),
            ),
        ]),
        result: 0,
        delay: Some(Duration::from_millis(10)),
    });

    let app = app(AppState::without_pg(EndpointRegistry::new())
        .with_bearer_token("test-token")
        .with_metadata_store(metadata_store)
        .with_sync_runner(runner));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/schedules/{schedule_id}/trigger"))
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = response_json(response).await;
    let trigger_id = json["trigger_id"].as_str().unwrap();
    let status_json =
        poll_schedule_trigger_until_status(&app, schedule_id, trigger_id, &["failed"]).await;
    assert_eq!(status_json["status"], "failed");
    assert_eq!(status_json["records_synced"], 0);
    assert_eq!(status_json["attempted_endpoints"], 2);
    assert_eq!(status_json["succeeded_endpoints"], 0);
    assert_eq!(status_json["failed_endpoints"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn schedule_trigger_status_route_returns_not_found_for_unknown_handle() {
    let state = AppState::without_pg(EndpointRegistry::new())
        .with_metadata_store(Arc::new(MockMetadataStore::default()))
        .with_bearer_token("test-token");
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri(
                    "/v1/schedules/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/triggers/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                )
                .header("authorization", bearer_header("test-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn schedules_route_rejects_invalid_cron() {
    let metadata_store = Arc::new(MockMetadataStore::default());
    let state =
        AppState::without_pg(EndpointRegistry::load_dir(&workspace_endpoint_dir()).unwrap())
            .with_bearer_token("test-token")
            .with_metadata_store(metadata_store);

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/schedules")
                .header("authorization", bearer_header("test-token"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": "33f319b9-4d57-11ee-960c-025417856183",
                        "name": "broken-schedule",
                        "cron": "not a cron",
                        "endpoints": ["all"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
