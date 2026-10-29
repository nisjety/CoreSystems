//! Accounting attachment metadata sync.

use std::collections::HashSet;

use chrono::{NaiveDate, Utc};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;
use xero_client::XeroApiClient;
use xero_common::EntityType;
use xero_gcs::{metadata, object_key, EntityOutcome, MetaArgs, ObjectMeta, RunManifest, SyncType};

use crate::{
    compact_ts,
    outcome::{classify_source_failure, sanitized_error},
    SyncJob,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentDiscoveryOptions {
    pub max_scan_pages: u32,
    pub max_parents: usize,
    pub include_unflagged: bool,
}

impl AttachmentDiscoveryOptions {
    pub fn validate(self, entity: &EntityType) -> Result<(), String> {
        if !matches!(
            entity,
            EntityType::Accounts
                | EntityType::BankTransactions
                | EntityType::BankTransfers
                | EntityType::Contacts
                | EntityType::CreditNotes
                | EntityType::Invoices
                | EntityType::ManualJournals
                | EntityType::PurchaseOrders
                | EntityType::Quotes
        ) {
            return Err(format!(
                "{entity} is not supported for attachment discovery"
            ));
        }
        if self.max_scan_pages == 0 || self.max_parents == 0 {
            return Err("discovery limits must be positive".into());
        }
        if *entity == EntityType::Quotes && !self.include_unflagged {
            return Err("Quotes omit HasAttachments; use --include-unflagged".into());
        }
        Ok(())
    }
}

fn collect_discovered_ids(
    records: &[Value],
    entity: &EntityType,
    existing: &[Uuid],
    max_parents: usize,
    include_unflagged: bool,
) -> Result<Vec<Uuid>, String> {
    let mut ids = existing.to_vec();
    let mut seen: HashSet<Uuid> = existing.iter().copied().collect();
    for record in records {
        let selected = include_unflagged
            || record
                .get("HasAttachments")
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("{entity} list item lacks a boolean HasAttachments flag"))?;
        if !selected {
            continue;
        }
        let id = record
            .get(entity.id_field())
            .and_then(Value::as_str)
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .ok_or_else(|| {
                format!(
                    "{entity} attachment parent has no valid {}",
                    entity.id_field()
                )
            })?;
        if seen.insert(id) {
            ids.push(id);
            if ids.len() > max_parents {
                return Err(format!(
                    "{entity} attachment discovery exceeded max_parents={max_parents}"
                ));
            }
        }
    }
    Ok(ids)
}

fn attachment_metadata(
    base: &MetaArgs<'_>,
    parent_entity: &EntityType,
    parent_id: &str,
) -> ObjectMeta {
    let mut meta = metadata(base);
    meta.0
        .insert("x-parent-endpoint".into(), parent_entity.as_str().into());
    meta.0.insert("x-parent-id".into(), parent_id.into());
    meta
}

fn attachment_key(
    prefix: &str,
    tenant: &str,
    run_date: NaiveDate,
    ts: &str,
    run_id: &str,
    page: u32,
) -> String {
    object_key(
        prefix,
        tenant,
        "attachment_metadata",
        run_date,
        ts,
        run_id,
        page,
    )
}

fn attachment_outcome_endpoint(parent_entity: &EntityType, parent_id: Uuid) -> String {
    format!("attachment_metadata:{}:{parent_id}", parent_entity.as_str())
}

impl SyncJob {
    async fn discover_attachment_parents(
        &self,
        api: &XeroApiClient,
        tenant: &str,
        entity: &EntityType,
        options: AttachmentDiscoveryOptions,
    ) -> Result<(Vec<Uuid>, u32), String> {
        options.validate(entity)?;
        let token = self
            .auth
            .get_valid_token(tenant)
            .await
            .map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        for page in 1..=options.max_scan_pages {
            let records = api
                .fetch_discovery_page(&token.access_token, entity, page, 100)
                .await
                .map_err(|e| e.to_string())?;
            if records.is_empty() {
                return Ok((ids, page));
            }
            ids = collect_discovered_ids(
                &records,
                entity,
                &ids,
                options.max_parents,
                options.include_unflagged,
            )?;
            if matches!(entity, EntityType::Accounts | EntityType::BankTransfers) {
                return Ok((ids, page));
            }
        }
        Err(format!(
            "{entity} discovery reached max_scan_pages={} before an empty page",
            options.max_scan_pages
        ))
    }

    /// Scan a bounded parent collection, then look up selected parents.
    /// An incomplete scan is a failed outcome.
    pub async fn run_discovered_attachments(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        tenants: &[String],
        parent_entity: &EntityType,
        options: AttachmentDiscoveryOptions,
    ) -> RunManifest {
        let started_at = Utc::now();
        let mut outcomes = Vec::new();
        for tenant in tenants {
            let api = XeroApiClient::new_with_tenant_header(tenant, self.send_tenant_header);
            #[cfg(test)]
            let api = match self.test_api_base_url.as_deref() {
                Some(base_url) => api.with_test_base_url(base_url),
                None => api,
            };
            match self
                .discover_attachment_parents(&api, tenant, parent_entity, options)
                .await
            {
                Ok((ids, pages)) => {
                    let termination = if matches!(
                        parent_entity,
                        EntityType::Accounts | EntityType::BankTransfers
                    ) {
                        "collection-end"
                    } else {
                        "empty-page"
                    };
                    outcomes.push(EntityOutcome {
                        tenant: tenant.clone(),
                        endpoint: format!("attachment_discovery:{}", parent_entity.as_str()),
                        pages,
                        records: ids.len() as i64,
                        termination: termination.into(),
                        request_key: None,
                        error: None,
                        outcome_class: None,
                        http_status: None,
                    });
                    if !ids.is_empty() {
                        let manifest = self
                            .run_attachments(
                                run_id,
                                run_date,
                                std::slice::from_ref(tenant),
                                parent_entity,
                                &ids,
                            )
                            .await;
                        outcomes.extend(manifest.entities);
                    }
                }
                Err(error) => {
                    warn!(tenant, entity = %parent_entity, %error, "attachment discovery failed");
                    let endpoint = format!("attachment_discovery:{}", parent_entity.as_str());
                    let (outcome_class, http_status) = classify_source_failure(&endpoint, &error);
                    outcomes.push(EntityOutcome {
                        tenant: tenant.clone(),
                        endpoint,
                        pages: 0,
                        records: 0,
                        termination: "error".into(),
                        request_key: None,
                        error: Some(sanitized_error(&error)),
                        outcome_class: Some(outcome_class),
                        http_status,
                    });
                }
            }
        }
        RunManifest {
            run_id: run_id.to_string(),
            started_at: started_at.to_rfc3339(),
            finished_at: Utc::now().to_rfc3339(),
            mode: "attachment_discovery".into(),
            window_days: None,
            entities: outcomes,
        }
    }

    /// Look up attachment metadata for exact Accounting parent IDs. Requests
    /// are sequential per run and share the normal per-tenant API limiter.
    pub async fn run_attachments(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        tenants: &[String],
        parent_entity: &EntityType,
        parent_ids: &[Uuid],
    ) -> RunManifest {
        let started_at = Utc::now();
        let ts = compact_ts(started_at);
        let run_id_str = run_id.to_string();
        let mut outcomes = Vec::with_capacity(tenants.len() * parent_ids.len());
        for tenant in tenants {
            let api = XeroApiClient::new_with_tenant_header(tenant, self.send_tenant_header);
            #[cfg(test)]
            let api = match self.test_api_base_url.as_deref() {
                Some(base_url) => api.with_test_base_url(base_url),
                None => api,
            };
            let org_name = self
                .org_names
                .get(tenant)
                .map(String::as_str)
                .unwrap_or_default();
            for (index, parent_id) in parent_ids.iter().enumerate() {
                let result: Result<usize, String> = async {
                    let token = self
                        .auth
                        .get_valid_token(tenant)
                        .await
                        .map_err(|e| e.to_string())?;
                    let listing = api
                        .fetch_attachment_list_raw(
                            &token.access_token,
                            parent_entity,
                            &parent_id.to_string(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    let page = (index + 1) as u32;
                    let key =
                        attachment_key(&self.gcs.prefix, tenant, run_date, &ts, &run_id_str, page);
                    let meta = attachment_metadata(
                        &MetaArgs {
                            tenant_id: tenant,
                            org_name,
                            endpoint: "attachment_metadata",
                            sync_type: SyncType::AttachmentMetadata,
                            page,
                            record_count: listing.record_count as i64,
                            http_status: 200,
                            run_id: &run_id_str,
                            synced_at: &listing.fetched_at.to_rfc3339(),
                            ..MetaArgs::default()
                        },
                        parent_entity,
                        &parent_id.to_string(),
                    );
                    self.sink
                        .put_raw(&key, &listing.body, &meta)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(listing.record_count)
                }
                .await;
                let (pages, records, termination, error, outcome_class, http_status) = match result
                {
                    Ok(count) => (
                        1,
                        count as i64,
                        "attachment-list".to_string(),
                        None,
                        None,
                        None,
                    ),
                    Err(error) => {
                        warn!(tenant, parent_id = %parent_id, %error, "attachment metadata lookup failed");
                        let (outcome_class, http_status) =
                            classify_source_failure("attachment_metadata", &error);
                        (
                            0,
                            0,
                            "error".to_string(),
                            Some(sanitized_error(&error)),
                            Some(outcome_class),
                            http_status,
                        )
                    }
                };
                outcomes.push(EntityOutcome {
                    tenant: tenant.clone(),
                    endpoint: attachment_outcome_endpoint(parent_entity, *parent_id),
                    pages,
                    records,
                    termination,
                    request_key: None,
                    error,
                    outcome_class,
                    http_status,
                });
            }
        }
        RunManifest {
            run_id: run_id_str,
            started_at: started_at.to_rfc3339(),
            finished_at: Utc::now().to_rfc3339(),
            mode: "attachment_metadata".into(),
            window_days: None,
            entities: outcomes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use chrono::Duration;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use xero_auth::{TokenData, TokenProvider};
    use xero_common::{EntityType, GcsConfig, SyncConfig};
    use xero_gcs::RawSink;

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
        responses: Vec<(&'static str, &'static [u8])>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/api.xro/2.0", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(responses.len());
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0_u8; 16 * 1024];
                let read = stream.read(&mut request).await.unwrap();
                requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
                let headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
            }
            requests
        });
        (base_url, task)
    }

    fn fixture_job(sink: Arc<dyn RawSink>, base_url: String) -> SyncJob {
        SyncJob::new(
            Arc::new(FixtureAuth),
            sink,
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
            Default::default(),
            true,
        )
        .with_test_api_base_url(base_url)
    }

    #[test]
    fn attachment_sidecar_identifies_its_parent() {
        let meta = attachment_metadata(
            &MetaArgs {
                tenant_id: "tenant",
                org_name: "Org",
                endpoint: "attachment_metadata",
                sync_type: SyncType::AttachmentMetadata,
                page: 2,
                record_count: 3,
                http_status: 200,
                run_id: "run",
                synced_at: "2026-10-01T00:00:00Z",
                ..MetaArgs::default()
            },
            &EntityType::Invoices,
            "5f3c9a2e-0000-0000-0000-000000000000",
        );
        assert_eq!(meta.get("x-endpoint"), Some("attachment_metadata"));
        assert_eq!(meta.get("x-parent-endpoint"), Some("invoices"));
        assert_eq!(
            meta.get("x-parent-id"),
            Some("5f3c9a2e-0000-0000-0000-000000000000")
        );
        assert_eq!(meta.get("x-record-count"), Some("3"));
    }

    #[test]
    fn attachment_keys_and_outcomes_keep_full_parent_identity() {
        let run = "5f3c9a2e-0000-0000-0000-000000000000";
        let parent = Uuid::parse_str("aeb4f270-0000-0000-0000-000000000000").unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let key = attachment_key("raw/xero", "tenant", date, "20261001T000000Z", run, 1);
        assert!(key.contains(&run.replace('-', "_")));
        assert_eq!(
            attachment_outcome_endpoint(&EntityType::Invoices, parent),
            format!("attachment_metadata:invoices:{parent}")
        );
    }

    #[test]
    fn discovery_supports_unpaged_parents_and_requires_quote_fanout() {
        let options = AttachmentDiscoveryOptions {
            max_scan_pages: 10,
            max_parents: 100,
            include_unflagged: false,
        };
        assert!(options.validate(&EntityType::Invoices).is_ok());
        assert!(options.validate(&EntityType::Accounts).is_ok());
        assert!(options.validate(&EntityType::BankTransfers).is_ok());
        assert!(options.validate(&EntityType::Quotes).is_err());
        assert!(AttachmentDiscoveryOptions {
            include_unflagged: true,
            ..options
        }
        .validate(&EntityType::Quotes)
        .is_ok());
        assert!(AttachmentDiscoveryOptions {
            max_scan_pages: 0,
            ..options
        }
        .validate(&EntityType::Invoices)
        .is_err());
    }

    #[test]
    fn discovery_collects_unique_flagged_parent_ids_and_fails_on_drift() {
        let first = "5f3c9a2e-0000-0000-0000-000000000000";
        let second = "aeb4f270-0000-0000-0000-000000000000";
        let records = vec![
            json!({"InvoiceID": first, "HasAttachments": true}),
            json!({"InvoiceID": second, "HasAttachments": false}),
            json!({"InvoiceID": first, "HasAttachments": true}),
        ];
        let ids = collect_discovered_ids(&records, &EntityType::Invoices, &[], 2, false).unwrap();
        assert_eq!(ids, vec![Uuid::parse_str(first).unwrap()]);
        let all = collect_discovered_ids(&records, &EntityType::Invoices, &[], 2, true).unwrap();
        assert_eq!(
            all,
            vec![
                Uuid::parse_str(first).unwrap(),
                Uuid::parse_str(second).unwrap()
            ]
        );
        assert!(collect_discovered_ids(
            &[json!({"InvoiceID": first})],
            &EntityType::Invoices,
            &ids,
            2,
            false
        )
        .is_err());
        assert!(collect_discovered_ids(
            &[json!({"InvoiceID": second, "HasAttachments": true})],
            &EntityType::Invoices,
            &ids,
            1,
            false
        )
        .is_err());
        let quotes = vec![json!({"QuoteID": second})];
        assert!(collect_discovered_ids(&quotes, &EntityType::Quotes, &[], 2, false).is_err());
        assert_eq!(
            collect_discovered_ids(&quotes, &EntityType::Quotes, &[], 2, true).unwrap(),
            vec![Uuid::parse_str(second).unwrap()]
        );
    }

    #[tokio::test]
    async fn attachment_lookup_writes_metadata_and_isolates_parent_failure() {
        let first = Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").unwrap();
        let second = Uuid::parse_str("aeb4f270-0000-0000-0000-000000000000").unwrap();
        let (base_url, server) = fixture_server(vec![
            (
                "200 OK",
                br#"{"Attachments":[{"AttachmentID":"9c4b1e6d-0000-0000-0000-000000000000"}]}"#,
            ),
            ("404 Not Found", br#"{"Message":"missing"}"#),
        ])
        .await;
        let sink = Arc::new(RecordingSink::default());
        let manifest = fixture_job(sink.clone(), base_url)
            .run_attachments(
                Uuid::new_v4(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
                &["tenant-attachments".into()],
                &EntityType::Invoices,
                &[first, second],
            )
            .await;

        assert_eq!(manifest.entities.len(), 2);
        assert_eq!(manifest.entities[0].records, 1);
        assert!(manifest.entities[0].error.is_none());
        assert_eq!(
            manifest.entities[1].error.as_deref(),
            Some("Xero source request failed (HTTP 404)")
        );
        assert!(!manifest.entities[1]
            .error
            .as_deref()
            .unwrap()
            .contains(&second.to_string()));
        {
            let writes = sink.0.lock().unwrap();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].2.get("x-parent-endpoint"), Some("invoices"));
            assert_eq!(
                writes[0].2.get("x-parent-id"),
                Some(first.to_string().as_str())
            );
        }
        let requests = server.await.unwrap();
        assert!(requests[0].contains(&format!("GET /api.xro/2.0/Invoices/{first}/Attachments")));
        assert!(requests[1].contains(&format!("GET /api.xro/2.0/Invoices/{second}/Attachments")));
    }

    #[tokio::test]
    async fn attachment_discovery_selects_flagged_parents_then_writes_their_sidecar() {
        let parent = "5f3c9a2e-0000-0000-0000-000000000000";
        let (base_url, server) = fixture_server(vec![
            (
                "200 OK",
                br#"{"Invoices":[{"InvoiceID":"5f3c9a2e-0000-0000-0000-000000000000","HasAttachments":true},{"InvoiceID":"aeb4f270-0000-0000-0000-000000000000","HasAttachments":false}]}"#,
            ),
            ("200 OK", br#"{"Invoices":[]}"#),
            ("200 OK", br#"{"Attachments":[]}"#),
        ])
        .await;
        let sink = Arc::new(RecordingSink::default());
        let manifest = fixture_job(sink.clone(), base_url)
            .run_discovered_attachments(
                Uuid::new_v4(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
                &["tenant-discovery".into()],
                &EntityType::Invoices,
                AttachmentDiscoveryOptions {
                    max_scan_pages: 2,
                    max_parents: 10,
                    include_unflagged: false,
                },
            )
            .await;

        assert_eq!(manifest.entities.len(), 2);
        assert_eq!(
            manifest.entities[0].endpoint,
            "attachment_discovery:invoices"
        );
        assert_eq!(manifest.entities[0].records, 1);
        assert_eq!(manifest.entities[1].records, 0);
        assert!(manifest
            .entities
            .iter()
            .all(|outcome| outcome.error.is_none()));
        assert_eq!(sink.0.lock().unwrap().len(), 1);
        let requests = server.await.unwrap();
        assert!(requests[0].contains("GET /api.xro/2.0/Invoices?page=1&pageSize=100"));
        assert!(requests[1].contains("GET /api.xro/2.0/Invoices?page=2&pageSize=100"));
        assert!(requests[2].contains(&format!("GET /api.xro/2.0/Invoices/{parent}/Attachments")));
    }
}
