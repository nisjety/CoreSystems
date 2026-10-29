use std::sync::{Arc, Mutex};

use chrono::{Duration, NaiveDate, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;
use xero_auth::{TokenData, TokenProvider};
use xero_client::extended::ExtendedFamily;
use xero_common::GcsConfig;
use xero_gcs::{ObjectMeta, RawSink};

use crate::extended::{ExtendedOptions, ExtendedSyncJob};

struct FixtureAuth;

#[async_trait::async_trait]
impl TokenProvider for FixtureAuth {
    async fn get_valid_token(&self, tenant: &str) -> xero_common::Result<TokenData> {
        Ok(TokenData {
            access_token: "fixture-access-token".into(),
            refresh_token: "fixture-refresh-token".into(),
            expires_at: Utc::now() + Duration::hours(1),
            scopes: Vec::new(),
            tenant_id: tenant.into(),
        })
    }
}

#[derive(Default)]
struct RecordingSink(Mutex<Vec<(String, Vec<u8>, ObjectMeta)>>);

#[async_trait::async_trait]
impl RawSink for RecordingSink {
    async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> xero_gcs::Result<()> {
        self.0
            .lock()
            .unwrap()
            .push((key.into(), body.into(), meta.clone()));
        Ok(())
    }
}

async fn fixture_server(
    responses: Vec<&'static [u8]>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for body in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let read = stream.read(&mut request).await.unwrap();
            requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        }
        requests
    });
    (base_url, task)
}

fn job(sink: Arc<dyn RawSink>, base_url: String) -> ExtendedSyncJob {
    ExtendedSyncJob::new(
        Arc::new(FixtureAuth),
        sink,
        GcsConfig {
            bucket: "fixture-bucket".into(),
            prefix: "raw/xero".into(),
        },
        Default::default(),
        true,
    )
    .with_test_api_base_url(base_url)
}

#[tokio::test]
async fn assets_run_writes_every_collection_and_its_complete_membership_snapshots() {
    let (base_url, server) = fixture_server(vec![
        br#"{"items":[],"pagination":{"pageCount":1}}"#,
        br#"{"items":[],"pagination":{"pageCount":1}}"#,
        br#"{"items":[],"pagination":{"pageCount":1}}"#,
        br#"[]"#,
        br#"{"assetNumberSequence":1}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let manifest = job(sink.clone(), base_url)
        .run(
            Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-assets".into()],
            &[ExtendedFamily::Assets],
            &ExtendedOptions {
                from: None,
                to: None,
                bank_account_ids: Vec::new(),
                details: false,
            },
        )
        .await;

    assert_eq!(manifest.entities.len(), 5);
    assert!(manifest
        .entities
        .iter()
        .all(|outcome| outcome.error.is_none()));
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 9);
        assert_eq!(
            writes
                .iter()
                .filter(|(_, _, meta)| meta.get("x-endpoint") == Some("assets_list_snapshots"))
                .count(),
            4
        );
        assert!(writes.iter().any(|(_, _, meta)| {
            meta.get("x-endpoint") == Some("assets_settings")
                && meta.get("x-api-family") == Some("assets")
        }));
    }
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests[0].contains("GET /Assets?"));
    assert!(requests[0].contains("status=DRAFT"));
    assert!(requests[3].contains("GET /AssetTypes"));
    assert!(requests[4].contains("GET /Settings"));
}

#[tokio::test]
async fn assets_detail_fanout_records_each_discovered_asset() {
    let asset_id = "5f3c9a2e-0000-0000-0000-000000000000";
    let (base_url, server) = fixture_server(vec![
        br#"{"items":[{"assetId":"5f3c9a2e-0000-0000-0000-000000000000"}],"pagination":{"pageCount":1}}"#,
        br#"{"items":[],"pagination":{"pageCount":1}}"#,
        br#"{"items":[],"pagination":{"pageCount":1}}"#,
        br#"[]"#,
        br#"{"assetNumberSequence":1}"#,
        br#"{"assetId":"5f3c9a2e-0000-0000-0000-000000000000","status":"REGISTERED"}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let manifest = job(sink.clone(), base_url)
        .run(
            Uuid::new_v4(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-assets-detail".into()],
            &[ExtendedFamily::Assets],
            &ExtendedOptions {
                from: None,
                to: None,
                bank_account_ids: Vec::new(),
                details: true,
            },
        )
        .await;

    assert_eq!(manifest.entities.len(), 6);
    assert!(manifest
        .entities
        .iter()
        .all(|outcome| outcome.error.is_none()));
    {
        let writes = sink.0.lock().unwrap();
        assert!(writes.iter().any(|(_, _, meta)| {
            meta.get("x-endpoint") == Some("assets_asset_details")
                && meta.get("x-api-family") == Some("assets")
        }));
    }
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests[5].contains(&format!("GET /Assets/{asset_id}")));
}

#[tokio::test]
async fn identity_runs_once_per_application_without_a_tenant_header() {
    let (base_url, server) = fixture_server(vec![br#"[{"id":"connection-one"}]"#]).await;
    let sink = Arc::new(RecordingSink::default());
    let manifest = job(sink.clone(), base_url)
        .run(
            Uuid::parse_str("aeb4f270-0000-0000-0000-000000000000").unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-first".into(), "tenant-second".into()],
            &[ExtendedFamily::Identity],
            &ExtendedOptions {
                from: None,
                to: None,
                bank_account_ids: Vec::new(),
                details: false,
            },
        )
        .await;

    assert_eq!(manifest.entities.len(), 1);
    assert_eq!(manifest.entities[0].tenant, "_application");
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert!(writes
            .iter()
            .all(|(_, _, meta)| meta.get("x-tenant-id") == Some("_application")));
    }
    let request = server.await.unwrap().pop().unwrap();
    assert!(request.contains("GET /connections"));
    assert!(!request.to_ascii_lowercase().contains("xero-tenant-id"));
}

#[tokio::test]
async fn projects_detail_fanout_records_parent_scoped_tasks_and_time_entries() {
    let project_id = "5f3c9a2e-0000-0000-0000-000000000000";
    let task_id = "aeb4f270-0000-0000-0000-000000000000";
    let time_entry_id = "9c4b1e6d-0000-0000-0000-000000000000";
    let (base_url, server) = fixture_server(vec![
        br#"{"items":[{"projectId":"5f3c9a2e-0000-0000-0000-000000000000"}],"pagination":{"pageCount":1}}"#,
        br#"{"items":[],"pagination":{"pageCount":1}}"#,
        br#"{"projectId":"5f3c9a2e-0000-0000-0000-000000000000"}"#,
        br#"{"items":[{"taskId":"aeb4f270-0000-0000-0000-000000000000"}],"pagination":{"pageCount":1}}"#,
        br#"{"items":[{"timeEntryId":"9c4b1e6d-0000-0000-0000-000000000000"}],"pagination":{"pageCount":1}}"#,
        br#"{"taskId":"aeb4f270-0000-0000-0000-000000000000"}"#,
        br#"{"timeEntryId":"9c4b1e6d-0000-0000-0000-000000000000"}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let manifest = job(sink.clone(), base_url)
        .run(
            Uuid::new_v4(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-projects".into()],
            &[ExtendedFamily::Projects],
            &ExtendedOptions {
                from: None,
                to: None,
                bank_account_ids: Vec::new(),
                details: true,
            },
        )
        .await;

    assert_eq!(manifest.entities.len(), 7);
    assert!(manifest
        .entities
        .iter()
        .all(|outcome| outcome.error.is_none()));
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 11);
        assert_eq!(
            writes
                .iter()
                .filter(|(_, _, meta)| meta.get("x-endpoint") == Some("projects_list_snapshots"))
                .count(),
            4
        );
    }
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 7);
    assert!(requests[0].contains("GET /Projects?"));
    assert!(requests[2].contains(&format!("GET /Projects/{project_id}")));
    assert!(requests[3].contains(&format!("GET /Projects/{project_id}/Tasks?")));
    assert!(requests[4].contains(&format!("GET /Projects/{project_id}/Time?")));
    assert!(requests[5].contains(&format!("GET /Projects/{project_id}/Tasks/{task_id}")));
    assert!(requests[6].contains(&format!("GET /Projects/{project_id}/Time/{time_entry_id}")));
}

#[tokio::test]
async fn bankfeeds_detail_fanout_keeps_connection_and_statement_identity() {
    let connection_id = "5f3c9a2e-0000-0000-0000-000000000000";
    let statement_id = "aeb4f270-0000-0000-0000-000000000000";
    let (base_url, server) = fixture_server(vec![
        br#"{"items":[{"id":"5f3c9a2e-0000-0000-0000-000000000000"}],"pagination":{"pageCount":1}}"#,
        br#"{"items":[{"id":"aeb4f270-0000-0000-0000-000000000000"}],"pagination":{"pageCount":1}}"#,
        br#"{"id":"5f3c9a2e-0000-0000-0000-000000000000"}"#,
        br#"{"id":"aeb4f270-0000-0000-0000-000000000000"}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let manifest = job(sink.clone(), base_url)
        .run(
            Uuid::new_v4(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-bankfeeds".into()],
            &[ExtendedFamily::BankFeeds],
            &ExtendedOptions {
                from: None,
                to: None,
                bank_account_ids: Vec::new(),
                details: true,
            },
        )
        .await;

    assert_eq!(manifest.entities.len(), 4);
    assert!(manifest
        .entities
        .iter()
        .all(|outcome| outcome.error.is_none()));
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 6);
        assert_eq!(
            writes
                .iter()
                .filter(|(_, _, meta)| meta.get("x-endpoint") == Some("bankfeeds_list_snapshots"))
                .count(),
            2
        );
    }
    let requests = server.await.unwrap();
    assert!(requests[0].contains("GET /FeedConnections?"));
    assert!(requests[1].contains("GET /Statements?"));
    assert!(requests[2].contains(&format!("GET /FeedConnections/{connection_id}")));
    assert!(requests[3].contains(&format!("GET /Statements/{statement_id}")));
}

#[tokio::test]
async fn finance_run_fetches_all_snapshots_for_the_requested_window() {
    let bank_account = Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").unwrap();
    let (base_url, server) = fixture_server(vec![
        br#"[]"#, br#"{}"#, br#"{}"#, br#"{}"#, br#"{}"#, br#"{}"#, br#"{}"#, br#"{}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let manifest = job(sink.clone(), base_url)
        .run(
            Uuid::new_v4(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-finance".into()],
            &[ExtendedFamily::Finance],
            &ExtendedOptions {
                from: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
                to: Some(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
                bank_account_ids: vec![bank_account],
                details: false,
            },
        )
        .await;

    assert_eq!(manifest.entities.len(), 8);
    assert!(manifest
        .entities
        .iter()
        .all(|outcome| outcome.error.is_none()));
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 8);
        assert!(writes.iter().all(|(_, _, meta)| {
            meta.get("x-api-family") == Some("finance")
                && meta.get("x-sync-type") == Some("report-snapshot")
                && meta.get("x-snapshot-date") == Some("2026-01-31")
        }));
    }
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 8);
    assert!(requests[0].contains("GET /CashValidation?balanceDate=2026-01-31"));
    assert!(requests[2]
        .contains("GET /FinancialStatements/Cashflow?startDate=2026-01-01&endDate=2026-01-31"));
    assert!(requests[7].contains(&format!(
        "GET /BankStatementsPlus/statements?BankAccountID={bank_account}&FromDate=2026-01-01&ToDate=2026-01-31"
    )));
}
