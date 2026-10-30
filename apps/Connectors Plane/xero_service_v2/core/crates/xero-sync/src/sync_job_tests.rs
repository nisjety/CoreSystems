use super::*;

use std::sync::Mutex;

use chrono::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xero_auth::TokenData;
use xero_common::{EntityType, Error};
use xero_gcs::{ObjectMeta, RawSink};

struct FixtureAuth {
    failing_tenant: Option<&'static str>,
}

#[async_trait::async_trait]
impl TokenProvider for FixtureAuth {
    async fn get_valid_token(&self, tenant: &str) -> xero_common::Result<TokenData> {
        if self.failing_tenant == Some(tenant) {
            return Err(Error::Auth("fixture token unavailable".into()));
        }
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
    let base_url = format!("http://{}/api.xro/2.0", listener.local_addr().unwrap());
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

fn config() -> (GcsConfig, SyncConfig) {
    (
        GcsConfig {
            bucket: "fixture-bucket".into(),
            prefix: "raw/xero".into(),
        },
        SyncConfig {
            window_days: 3,
            rolling_full_tight_days: 30,
            rolling_full_wide_days: 90,
            max_concurrent: 1,
        },
    )
}

fn job(auth: Arc<dyn TokenProvider>, sink: Arc<dyn RawSink>, base_url: String) -> SyncJob {
    let (gcs, cfg) = config();
    SyncJob::new(auth, sink, gcs, cfg, Default::default(), true).with_test_api_base_url(base_url)
}

#[tokio::test]
async fn sync_job_streams_pages_and_isolates_a_tenant_token_failure() {
    let (base_url, server) = fixture_server(vec![
        br#"{"Invoices":[{"InvoiceID":"one"}]}"#,
        br#"{"Invoices":[]}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let sync = job(
        Arc::new(FixtureAuth {
            failing_tenant: Some("tenant-failed"),
        }),
        sink.clone(),
        base_url,
    );
    let manifest = sync
        .run(
            Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-failed".into(), "tenant-ok".into()],
            &[EntityType::Invoices],
            SyncMode::Master,
            1,
        )
        .await;

    assert_eq!(manifest.entities.len(), 2);
    assert_eq!(
        manifest.entities[0].error.as_deref(),
        Some("Xero source request failed")
    );
    assert_eq!(manifest.entities[1].pages, 2);
    assert_eq!(manifest.entities[1].records, 1);
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].1, br#"{"Invoices":[{"InvoiceID":"one"}]}"#);
        assert_eq!(writes[0].2.get("x-sync-type"), Some("master"));
    }
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("GET /api.xro/2.0/Invoices?"));
    assert!(requests[0].contains("page=1"));
    assert!(requests[0]
        .to_ascii_lowercase()
        .contains("xero-tenant-id: tenant-ok"));
}

#[tokio::test]
async fn sync_job_writes_a_report_snapshot_with_resolved_metadata() {
    let (base_url, server) = fixture_server(vec![
        br#"{"Reports":[{"ReportName":"BalanceSheet","Rows":[]}]}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let sync = job(
        Arc::new(FixtureAuth {
            failing_tenant: None,
        }),
        sink.clone(),
        base_url,
    );
    let run_date = NaiveDate::from_ymd_opt(2026, 10, 4).unwrap();
    let manifest = sync
        .run(
            Uuid::parse_str("aeb4f270-0000-0000-0000-000000000000").unwrap(),
            run_date,
            &["tenant-report".into()],
            &[EntityType::ReportBalanceSheet],
            SyncMode::Reports { as_of: run_date },
            1,
        )
        .await;

    assert_eq!(manifest.entities[0].termination, "report-snapshot");
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert!(writes[0].0.contains("report_balance_sheet"));
        assert_eq!(writes[0].2.get("x-sync-type"), Some("report-snapshot"));
        assert_eq!(writes[0].2.get("x-report-date"), Some("2026-10-04"));
    }
    let request = server.await.unwrap().pop().unwrap();
    assert!(request.contains("GET /api.xro/2.0/Reports/BalanceSheet?date=2026-10-04"));
}

#[tokio::test]
async fn default_budget_detail_expansion_preserves_collection_and_catalogued_detail() {
    let budget_id = "5f3c9a2e-0000-0000-0000-000000000000";
    let (base_url, server) = fixture_server(vec![
        br#"{"Budgets":[{"BudgetID":"5f3c9a2e-0000-0000-0000-000000000000"}]}"#,
        br#"{"Budgets":{"BudgetID":"5f3c9a2e-0000-0000-0000-000000000000","BudgetLines":[{"Amount":42}]}}"#,
    ])
    .await;
    let sink = Arc::new(RecordingSink::default());
    let sync = job(
        Arc::new(FixtureAuth {
            failing_tenant: None,
        }),
        sink.clone(),
        base_url,
    );
    let manifest = sync
        .run(
            Uuid::new_v4(),
            NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            &["tenant-budget".into()],
            &[EntityType::Budgets],
            SyncMode::Master,
            1,
        )
        .await;

    assert_eq!(manifest.entities.len(), 2);
    assert_eq!(manifest.entities[0].termination, "ended_on_empty_page");
    assert_eq!(manifest.entities[0].records, 1);
    assert_eq!(manifest.entities[1].endpoint, "accounting_get_getbudget");
    assert_eq!(manifest.entities[1].termination, "single_response");
    assert_eq!(manifest.entities[1].records, 1);
    {
        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert!(std::str::from_utf8(&writes[0].1)
            .unwrap()
            .contains("BudgetID"));
        assert!(std::str::from_utf8(&writes[1].1)
            .unwrap()
            .contains("BudgetLines"));
        assert_eq!(writes[1].2.get("x-operation-id"), Some("getBudget"));
        assert_eq!(writes[1].2.get("x-parent-record-id"), Some(budget_id));
    }
    let requests = server.await.unwrap();
    assert!(requests[0].contains("GET /api.xro/2.0/Budgets"));
    assert!(requests[1].contains(&format!("GET /api.xro/2.0/Budgets/{budget_id}")));
}
