//! Catalogued, read-only Accounting GET landing job.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use uuid::Uuid;
use xero_auth::TokenProvider;
use xero_client::{
    accounting_get::{catalog, operation, AccountingGetOperation, PreparedAccountingGet},
    XeroApiClient,
};
use xero_common::GcsConfig;
use xero_gcs::{metadata, object_key, EntityOutcome, MetaArgs, RawSink, RunManifest, SyncType};

use crate::outcome::{classify_source_failure, sanitized_error};

pub struct AccountingGetSyncJob {
    auth: Arc<dyn TokenProvider>,
    sink: Arc<dyn RawSink>,
    gcs: GcsConfig,
    org_names: HashMap<String, String>,
    send_tenant_header: bool,
    #[cfg(test)]
    test_api_base_url: Option<String>,
}

impl AccountingGetSyncJob {
    pub fn new(
        auth: Arc<dyn TokenProvider>,
        sink: Arc<dyn RawSink>,
        gcs: GcsConfig,
        org_names: HashMap<String, String>,
        send_tenant_header: bool,
    ) -> Self {
        Self {
            auth,
            sink,
            gcs,
            org_names,
            send_tenant_header,
            #[cfg(test)]
            test_api_base_url: None,
        }
    }

    #[cfg(test)]
    fn with_test_api_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.test_api_base_url = Some(base_url.into());
        self
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        tenants: &[String],
        operation_id: &str,
        path: &[(String, String)],
        query: &[(String, String)],
        headers: &[(String, String)],
    ) -> Result<RunManifest> {
        let selected = operation(operation_id).map_err(anyhow::Error::msg)?;
        let prepared = selected
            .prepare(path, query, headers)
            .map_err(anyhow::Error::msg)?;
        #[cfg(test)]
        let prepared = match self.test_api_base_url.as_deref() {
            Some(base_url) => rehost_for_test(prepared, base_url)?,
            None => prepared,
        };
        let descriptor = request_descriptor(selected, path, query, headers)?;
        let endpoint = endpoint(selected);
        let started = Utc::now();
        let mut outcomes = Vec::with_capacity(tenants.len());
        for tenant in tenants {
            let result = self
                .fetch_one(
                    tenant,
                    selected,
                    &prepared,
                    &descriptor,
                    &endpoint,
                    run_id,
                    run_date,
                )
                .await;
            outcomes.push(match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    let error = error.to_string();
                    let (outcome_class, http_status) = classify_source_failure(&endpoint, &error);
                    EntityOutcome {
                        tenant: tenant.clone(),
                        endpoint: endpoint.clone(),
                        pages: 0,
                        records: 0,
                        termination: "error".into(),
                        request_key: None,
                        error: Some(sanitized_error(&error)),
                        outcome_class: Some(outcome_class),
                        http_status,
                    }
                }
            });
        }
        Ok(RunManifest {
            run_id: run_id.to_string(),
            started_at: started.to_rfc3339(),
            finished_at: Utc::now().to_rfc3339(),
            mode: "accounting_get".into(),
            window_days: None,
            entities: outcomes,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_one(
        &self,
        tenant: &str,
        operation: &AccountingGetOperation,
        prepared: &PreparedAccountingGet,
        descriptor: &str,
        endpoint: &str,
        run_id: Uuid,
        date: NaiveDate,
    ) -> Result<EntityOutcome> {
        let token = self.auth.get_valid_token(tenant).await?;
        let api = XeroApiClient::new_with_tenant_header(tenant, self.send_tenant_header);
        let response = api
            .fetch_accounting_get(&token.access_token, prepared)
            .await?;
        if response.http_status == 304 {
            return Ok(EntityOutcome {
                tenant: tenant.into(),
                endpoint: endpoint.into(),
                pages: 0,
                records: 0,
                termination: "not_modified".into(),
                request_key: None,
                error: None,
                outcome_class: None,
                http_status: None,
            });
        }
        if !operation.is_binary() {
            let value: serde_json::Value = serde_json::from_slice(&response.body)
                .context("Accounting GET returned invalid JSON")?;
            anyhow::ensure!(
                value.is_object() || value.is_array(),
                "Accounting GET returned a JSON scalar rather than an envelope"
            );
        }
        let run_id_text = run_id.to_string();
        let key = object_key(
            &self.gcs.prefix,
            tenant,
            endpoint,
            date,
            &response.fetched_at.format("%Y%m%dT%H%M%S%3fZ").to_string(),
            &run_id_text[..6],
            1,
        );
        let key = format!(
            "{}.{}",
            key.strip_suffix(".json").unwrap_or(&key),
            object_extension(operation)
        );
        let synced_at = response.fetched_at.to_rfc3339();
        let org_name = self
            .org_names
            .get(tenant)
            .map(String::as_str)
            .unwrap_or_default();
        let mut meta = metadata(&MetaArgs {
            tenant_id: tenant,
            org_name,
            endpoint,
            sync_type: SyncType::AccountingGet,
            page: 1,
            record_count: 1,
            http_status: response.http_status,
            run_id: &run_id_text,
            synced_at: &synced_at,
            ..MetaArgs::default()
        });
        meta.0.insert("x-api-family".into(), "accounting".into());
        meta.0
            .insert("x-operation-id".into(), operation.operation_id.clone());
        meta.0
            .insert("x-spec-commit".into(), catalog().spec_commit.clone());
        meta.0
            .insert("x-request-descriptor".into(), descriptor.into());
        meta.0.insert(
            "x-response-content-type".into(),
            response.content_type.clone(),
        );
        meta.0.insert(
            "x-response-binary".into(),
            operation.is_binary().to_string(),
        );
        self.sink
            .put_raw_typed(&key, &response.body, &meta, &response.content_type)
            .await?;
        Ok(EntityOutcome {
            tenant: tenant.into(),
            endpoint: endpoint.into(),
            pages: 1,
            records: 1,
            termination: "single_response".into(),
            request_key: None,
            error: None,
            outcome_class: None,
            http_status: None,
        })
    }
}

#[cfg(test)]
fn rehost_for_test(
    mut prepared: PreparedAccountingGet,
    base_url: &str,
) -> Result<PreparedAccountingGet> {
    let mut suffix = prepared.url.path().to_owned();
    if let Some(query) = prepared.url.query() {
        suffix.push('?');
        suffix.push_str(query);
    }
    prepared.url = format!("{}{}", base_url.trim_end_matches('/'), suffix)
        .parse()
        .context("invalid test Accounting API base URL")?;
    Ok(prepared)
}

pub(crate) fn endpoint(operation: &AccountingGetOperation) -> String {
    format!(
        "accounting_get_{}",
        operation.operation_id.to_ascii_lowercase()
    )
}

fn object_extension(operation: &AccountingGetOperation) -> &'static str {
    if !operation.is_binary() {
        "json"
    } else if operation
        .response_content_types
        .iter()
        .any(|kind| kind == "application/pdf")
    {
        "pdf"
    } else {
        "bin"
    }
}

pub(crate) fn request_descriptor(
    operation: &AccountingGetOperation,
    path: &[(String, String)],
    query: &[(String, String)],
    headers: &[(String, String)],
) -> Result<String> {
    let descriptor = serde_json::json!({
        "operation_id": operation.operation_id,
        "path": path,
        "query": query,
        "headers": headers,
    });
    let text = serde_json::to_string(&descriptor)?;
    anyhow::ensure!(
        text.len() <= 2048,
        "request descriptor exceeds GCS metadata limit"
    );
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use chrono::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use xero_auth::TokenData;
    use xero_client::accounting_get::operation;
    use xero_gcs::{ObjectMeta, RawSink};

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

    type RecordedObject = (String, Vec<u8>, ObjectMeta, String);

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<RecordedObject>>);

    #[async_trait::async_trait]
    impl RawSink for RecordingSink {
        async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> xero_gcs::Result<()> {
            self.0.lock().unwrap().push((
                key.into(),
                body.into(),
                meta.clone(),
                "application/json".into(),
            ));
            Ok(())
        }

        async fn put_raw_typed(
            &self,
            key: &str,
            body: &[u8],
            meta: &ObjectMeta,
            content_type: &str,
        ) -> xero_gcs::Result<()> {
            self.0.lock().unwrap().push((
                key.into(),
                body.into(),
                meta.clone(),
                content_type.into(),
            ));
            Ok(())
        }
    }

    async fn fixture_server(
        status: &'static str,
        content_type: &'static str,
        body: &'static [u8],
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
            request
        });
        (base_url, task)
    }

    fn fixture_job(sink: Arc<dyn RawSink>, base_url: String) -> AccountingGetSyncJob {
        AccountingGetSyncJob::new(
            Arc::new(FixtureAuth),
            sink,
            GcsConfig {
                bucket: "fixture-bucket".into(),
                prefix: "raw/xero".into(),
            },
            HashMap::from([("tenant-accounting-get".into(), "Fixture Org".into())]),
            true,
        )
        .with_test_api_base_url(base_url)
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.into(), value.into())
    }

    #[test]
    fn descriptor_preserves_operation_and_all_request_parameters() {
        let op = operation("getInvoiceHistory").unwrap();
        let path = [pair("InvoiceID", "5f3c9a2e-0000-0000-0000-000000000000")];
        let descriptor = request_descriptor(op, &path, &[], &[]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&descriptor).unwrap();
        assert_eq!(value["operation_id"], "getInvoiceHistory");
        assert_eq!(value["path"][0][0], "InvoiceID");
    }

    #[test]
    fn binary_and_json_extensions_match_existing_landing_paths() {
        assert_eq!(
            object_extension(operation("getInvoiceHistory").unwrap()),
            "json"
        );
        assert_eq!(
            object_extension(operation("getInvoiceAsPdf").unwrap()),
            "pdf"
        );
        assert_eq!(
            object_extension(operation("getInvoiceAttachmentById").unwrap()),
            "bin"
        );
        assert_eq!(
            endpoint(operation("getInvoiceHistory").unwrap()),
            "accounting_get_getinvoicehistory"
        );
    }

    #[test]
    fn descriptor_rejects_gcs_metadata_overflow() {
        let op = operation("getInvoices").unwrap();
        let query = [pair("where", &"a".repeat(2050))];
        assert!(request_descriptor(op, &[], &query, &[]).is_err());
    }

    #[tokio::test]
    async fn catalogued_json_get_lands_raw_response_with_request_metadata() {
        let invoice_id = "5f3c9a2e-0000-0000-0000-000000000000";
        let body = br#"{"Invoices":[{"InvoiceID":"5f3c9a2e-0000-0000-0000-000000000000"}]}"#;
        let (base_url, server) = fixture_server("200 OK", "application/json", body).await;
        let sink = Arc::new(RecordingSink::default());
        let manifest = fixture_job(sink.clone(), base_url)
            .run(
                Uuid::new_v4(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
                &["tenant-accounting-get".into()],
                "getInvoice",
                &[pair("InvoiceID", invoice_id)],
                &[],
                &[],
            )
            .await
            .unwrap();

        assert_eq!(manifest.entities[0].termination, "single_response");
        {
            let writes = sink.0.lock().unwrap();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].1, body);
            assert!(writes[0].0.ends_with(".json"));
            assert_eq!(writes[0].2.get("x-operation-id"), Some("getInvoice"));
            assert_eq!(writes[0].2.get("x-org-name"), Some("Fixture Org"));
            assert_eq!(writes[0].3, "application/json");
        }
        let request = server.await.unwrap();
        assert!(request.contains(&format!("GET /api.xro/2.0/Invoices/{invoice_id}")));
        let request = request.to_ascii_lowercase();
        assert!(request.contains("authorization: bearer fixture-access-token"));
        assert!(request.contains("xero-tenant-id: tenant-accounting-get"));
    }

    #[tokio::test]
    async fn catalogued_get_preserves_not_modified_without_writing_an_object() {
        let (base_url, server) = fixture_server("304 Not Modified", "application/json", b"").await;
        let sink = Arc::new(RecordingSink::default());
        let manifest = fixture_job(sink.clone(), base_url)
            .run(
                Uuid::new_v4(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
                &["tenant-accounting-get".into()],
                "getInvoices",
                &[],
                &[],
                &[],
            )
            .await
            .unwrap();

        assert_eq!(manifest.entities[0].termination, "not_modified");
        assert_eq!(manifest.entities[0].pages, 0);
        assert!(sink.0.lock().unwrap().is_empty());
        assert!(server.await.unwrap().contains("GET /api.xro/2.0/Invoices"));
    }

    #[tokio::test]
    async fn catalogued_pdf_get_lands_a_binary_object_with_pdf_content_type() {
        let invoice_id = "aeb4f270-0000-0000-0000-000000000000";
        let pdf = b"%PDF-fixture";
        let (base_url, server) = fixture_server("200 OK", "application/pdf", pdf).await;
        let sink = Arc::new(RecordingSink::default());
        let manifest = fixture_job(sink.clone(), base_url)
            .run(
                Uuid::new_v4(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
                &["tenant-accounting-get".into()],
                "getInvoiceAsPdf",
                &[pair("InvoiceID", invoice_id)],
                &[],
                &[],
            )
            .await
            .unwrap();

        assert_eq!(manifest.entities[0].termination, "single_response");
        {
            let writes = sink.0.lock().unwrap();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].1, pdf);
            assert!(writes[0].0.ends_with(".pdf"));
            assert_eq!(writes[0].2.get("x-response-binary"), Some("true"));
            assert_eq!(writes[0].3, "application/pdf");
        }
        let request = server.await.unwrap();
        assert!(request.contains(&format!("GET /api.xro/2.0/Invoices/{invoice_id}")));
        assert!(request
            .to_ascii_lowercase()
            .contains("accept: application/pdf"));
    }
}
