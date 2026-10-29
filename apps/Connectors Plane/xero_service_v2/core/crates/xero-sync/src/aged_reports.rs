use chrono::{NaiveDate, Utc};
use std::collections::BTreeSet;
use tracing::warn;
use uuid::Uuid;
use xero_client::{ExtraQuery, RawPage, XeroApiClient};
use xero_common::EntityType;
use xero_gcs::{
    report_metadata, report_object_key, EntityOutcome, MetaArgs, ReportMetaArgs, RunManifest,
    SyncType,
};

use crate::{
    outcome::{classify_source_failure, sanitized_error},
    report::aged_contact_report_params,
    streaming::{AuthenticatedPages, PageSource, PageWriter, WriterArgs},
    SyncJob,
};

const DEFAULT_MAX_CONTACTS_PER_TENANT: usize = 1_000;
const DEFAULT_MAX_REPORT_REQUESTS: usize = 2_000;

/// Explicit guardrails for the potentially high-cardinality aged-report fan-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgedContactReportOptions {
    pub max_contacts_per_tenant: usize,
    pub max_report_requests: usize,
}

impl Default for AgedContactReportOptions {
    fn default() -> Self {
        Self {
            max_contacts_per_tenant: DEFAULT_MAX_CONTACTS_PER_TENANT,
            max_report_requests: DEFAULT_MAX_REPORT_REQUESTS,
        }
    }
}

impl AgedContactReportOptions {
    pub fn validate(self) -> Result<(), String> {
        if self.max_contacts_per_tenant == 0 || self.max_report_requests == 0 {
            return Err("aged contact report limits must be positive".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgedContactReportRequest {
    tenant: String,
    entity: EntityType,
    contact_id: Uuid,
    as_of: NaiveDate,
}

fn plan_aged_contact_reports(
    tenant: &str,
    as_of: NaiveDate,
    reports: &[EntityType],
    contacts: &[Uuid],
    max_contacts_per_tenant: usize,
    max_requests: usize,
) -> Result<Vec<AgedContactReportRequest>, String> {
    if tenant.trim().is_empty() {
        return Err("aged contact report plan requires a tenant".into());
    }
    if reports.is_empty() {
        return Err("aged contact report plan requires at least one report".into());
    }
    if max_contacts_per_tenant == 0 || max_requests == 0 {
        return Err("aged contact report limits must be positive".into());
    }
    if reports.iter().any(|entity| {
        !matches!(
            entity,
            EntityType::ReportAgedReceivablesByContact | EntityType::ReportAgedPayablesByContact
        )
    }) {
        return Err("aged contact report plan only supports AgedReceivablesByContact and AgedPayablesByContact".into());
    }

    let contacts: Vec<Uuid> = contacts
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if contacts.len() > max_contacts_per_tenant {
        return Err(format!(
            "tenant {tenant} has {} contacts, exceeding max_contacts_per_tenant={max_contacts_per_tenant}",
            contacts.len()
        ));
    }
    let request_count = contacts
        .len()
        .checked_mul(reports.len())
        .ok_or_else(|| "aged contact report request count overflow".to_string())?;
    if request_count > max_requests {
        return Err(format!(
            "tenant {tenant} needs {request_count} aged report requests, exceeding remaining request budget {max_requests}"
        ));
    }

    Ok(contacts
        .into_iter()
        .flat_map(|contact_id| {
            reports
                .iter()
                .cloned()
                .map(move |entity| AgedContactReportRequest {
                    tenant: tenant.to_owned(),
                    entity,
                    contact_id,
                    as_of,
                })
        })
        .collect())
}

fn contact_ids_from_page(page: &RawPage) -> Result<Vec<Uuid>, String> {
    let response: serde_json::Value = serde_json::from_slice(&page.body)
        .map_err(|error| format!("Contacts response is not valid JSON: {error}"))?;
    let contacts = response
        .get("Contacts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Contacts response lacks a Contacts array".to_string())?;
    contacts
        .iter()
        .map(|contact| {
            let raw = contact
                .get("ContactID")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "Contacts item lacks ContactID".to_string())?;
            Uuid::parse_str(raw).map_err(|_| format!("Contacts item has invalid ContactID {raw}"))
        })
        .collect()
}

impl SyncJob {
    /// Fetch a complete, tenant-specific Contacts source receipt then fan out
    /// the two aged reports across its validated contact UUIDs. This remains a
    /// dedicated workflow because a contact UUID must never cross tenant
    /// boundaries and because its cardinality needs an explicit operator cap.
    pub async fn run_aged_contact_reports(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        as_of: NaiveDate,
        tenants: &[String],
        reports: &[EntityType],
        options: AgedContactReportOptions,
    ) -> Result<RunManifest, String> {
        options.validate()?;
        if reports.is_empty() {
            return Err("aged contact report run requires at least one report".into());
        }
        if reports.iter().any(|entity| {
            !matches!(
                entity,
                EntityType::ReportAgedReceivablesByContact
                    | EntityType::ReportAgedPayablesByContact
            )
        }) {
            return Err("aged contact report run only supports AgedReceivablesByContact and AgedPayablesByContact".into());
        }

        let started_at = Utc::now();
        let ts = crate::compact_ts(started_at);
        let run_id_text = run_id.to_string();
        let run_id_short = &run_id_text[..run_id_text.len().min(6)];
        let mut outcomes = Vec::new();
        let mut planned_requests = 0_usize;

        for tenant in tenants {
            let api = self.api_for_tenant(tenant);
            let (contacts_outcome, contacts) = self
                .capture_tenant_contacts(
                    &api,
                    tenant,
                    run_date,
                    &ts,
                    run_id_short,
                    &run_id_text,
                    options.max_contacts_per_tenant,
                )
                .await;
            let discovery_complete = contacts_outcome.error.is_none();
            outcomes.push(contacts_outcome);
            let Some(contacts) = contacts.filter(|_| discovery_complete) else {
                continue;
            };

            let remaining_requests = options.max_report_requests.saturating_sub(planned_requests);
            let plan = match plan_aged_contact_reports(
                tenant,
                as_of,
                reports,
                &contacts,
                options.max_contacts_per_tenant,
                remaining_requests,
            ) {
                Ok(plan) => plan,
                Err(error) => {
                    let (outcome_class, http_status) =
                        classify_source_failure("aged_contact_report_plan", &error);
                    outcomes.push(EntityOutcome {
                        tenant: tenant.clone(),
                        endpoint: "aged_contact_report_plan".into(),
                        pages: 0,
                        records: contacts.len() as i64,
                        termination: "error".into(),
                        request_key: None,
                        error: Some(sanitized_error(&error)),
                        outcome_class: Some(outcome_class),
                        http_status,
                    });
                    continue;
                }
            };
            planned_requests += plan.len();

            for request in plan {
                outcomes.push(
                    self.run_aged_contact_report(
                        &api,
                        run_date,
                        &ts,
                        run_id_short,
                        &run_id_text,
                        &request,
                    )
                    .await,
                );
            }
        }

        Ok(RunManifest {
            run_id: run_id_text,
            started_at: started_at.to_rfc3339(),
            finished_at: Utc::now().to_rfc3339(),
            mode: "aged-contact-reports".into(),
            window_days: None,
            entities: outcomes,
        })
    }

    fn api_for_tenant(&self, tenant: &str) -> XeroApiClient {
        let api = XeroApiClient::new_with_tenant_header(tenant.to_owned(), self.send_tenant_header);
        #[cfg(test)]
        let api = match self.test_api_base_url.as_deref() {
            Some(base_url) => api.with_test_base_url(base_url),
            None => api,
        };
        api
    }

    #[allow(clippy::too_many_arguments)]
    async fn capture_tenant_contacts(
        &self,
        api: &XeroApiClient,
        tenant: &str,
        run_date: NaiveDate,
        ts: &str,
        run_id_short: &str,
        run_id: &str,
        max_contacts: usize,
    ) -> (EntityOutcome, Option<Vec<Uuid>>) {
        let org_name = self
            .org_names
            .get(tenant)
            .map(String::as_str)
            .unwrap_or_default();
        let synced_at = Utc::now().to_rfc3339();
        let contact_query = ExtraQuery::default();
        let mut source = AuthenticatedPages {
            reader: api.raw_page_reader(&EntityType::Contacts, None, &contact_query),
            auth: self.auth.as_ref(),
            tenant,
        };
        let mut writer = PageWriter::new(
            self.sink.as_ref(),
            WriterArgs {
                prefix: &self.gcs.prefix,
                ts,
                run_date,
                run_id_short,
                meta: MetaArgs {
                    tenant_id: tenant,
                    org_name,
                    endpoint: EntityType::Contacts.as_str(),
                    sync_type: SyncType::Master,
                    run_id,
                    synced_at: &synced_at,
                    ..MetaArgs::default()
                },
            },
        );
        let mut contacts = BTreeSet::new();

        loop {
            match source.next().await {
                Ok(Some(page)) => {
                    if let Err(error) = writer.write(&page).await {
                        return (writer.outcome("error", Some(error)), None);
                    }
                    let page_contacts = match contact_ids_from_page(&page) {
                        Ok(ids) => ids,
                        Err(error) => return (writer.outcome("error", Some(error)), None),
                    };
                    contacts.extend(page_contacts);
                    if contacts.len() > max_contacts {
                        return (
                            writer.outcome(
                                "error",
                                Some(format!(
                                    "Contacts discovery found more than max_contacts_per_tenant={max_contacts}"
                                )),
                            ),
                            None,
                        );
                    }
                }
                Ok(None) => {
                    let pagination = source.outcome();
                    if pagination.termination.is_suspect() {
                        return (
                            writer.outcome(
                                "error",
                                Some(
                                    "Contacts discovery pagination did not complete safely".into(),
                                ),
                            ),
                            None,
                        );
                    }
                    return (
                        writer.outcome(pagination.termination.as_str(), None),
                        Some(contacts.into_iter().collect()),
                    );
                }
                Err(error) => return (writer.outcome("error", Some(error)), None),
            }
        }
    }

    async fn run_aged_contact_report(
        &self,
        api: &XeroApiClient,
        run_date: NaiveDate,
        ts: &str,
        run_id_short: &str,
        run_id: &str,
        request: &AgedContactReportRequest,
    ) -> EntityOutcome {
        let request_key = format!("contact:{}", request.contact_id);
        let result = async {
            let spec =
                aged_contact_report_params(&request.entity, request.as_of, request.contact_id)?;
            let token = self
                .auth
                .get_valid_token(&request.tenant)
                .await
                .map_err(|error| error.to_string())?;
            let page = api
                .fetch_report_raw(
                    &token.access_token,
                    &request.entity,
                    &ExtraQuery {
                        extra: spec.extra.clone(),
                        ..ExtraQuery::default()
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            let key = report_object_key(
                &self.gcs.prefix,
                &request.tenant,
                request.entity.as_str(),
                run_date,
                ts,
                run_id_short,
                &spec.period_key,
            );
            let signature = spec.params_signature();
            let org_name = self
                .org_names
                .get(&request.tenant)
                .map(String::as_str)
                .unwrap_or_default();
            let synced_at = Utc::now().to_rfc3339();
            let meta = report_metadata(&ReportMetaArgs {
                tenant_id: &request.tenant,
                org_name,
                endpoint: request.entity.as_str(),
                report: request.entity.as_str(),
                report_date: spec.report_date.as_deref(),
                report_from: spec.report_from.as_deref(),
                report_to: spec.report_to.as_deref(),
                report_params: &signature,
                contact_id: Some(&request.contact_id.to_string()),
                http_status: page.http_status,
                record_count: page.record_count as i64,
                run_id,
                synced_at: &synced_at,
            });
            self.sink
                .put_raw(&key, &page.body, &meta)
                .await
                .map_err(|error| error.to_string())?;
            Ok::<i64, String>(page.record_count as i64)
        }
        .await;

        match result {
            Ok(records) => EntityOutcome {
                tenant: request.tenant.clone(),
                endpoint: request.entity.as_str().into(),
                pages: 1,
                records,
                termination: "report-snapshot".into(),
                request_key: Some(request_key),
                error: None,
                outcome_class: None,
                http_status: None,
            },
            Err(error) => {
                warn!(tenant = %request.tenant, entity = %request.entity, contact_id = %request.contact_id, %error, "aged contact report failed (isolated)");
                let (outcome_class, http_status) =
                    classify_source_failure(request.entity.as_str(), &error);
                EntityOutcome {
                    tenant: request.tenant.clone(),
                    endpoint: request.entity.as_str().into(),
                    pages: 0,
                    records: 0,
                    termination: "error".into(),
                    request_key: Some(request_key),
                    error: Some(sanitized_error(&error)),
                    outcome_class: Some(outcome_class),
                    http_status,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use chrono::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use xero_auth::{TokenData, TokenProvider};
    use xero_common::{GcsConfig, SyncConfig};
    use xero_gcs::{ObjectMeta, RawSink};

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).unwrap()
    }

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
                .map_err(|_| xero_gcs::GcsError::Upload("recording sink lock poisoned".into()))?
                .push((key.into(), body.into(), meta.clone()));
            Ok(())
        }
    }

    async fn fixture_server(
        responses: Vec<&'static [u8]>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        fixture_server_with_status(responses.into_iter().map(|body| (200, body)).collect()).await
    }

    async fn fixture_server_with_status(
        responses: Vec<(u16, &'static [u8])>,
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
                    "HTTP/1.1 {status} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len(),
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
            }
            requests
        });
        (base_url, task)
    }

    fn job(sink: Arc<dyn RawSink>, base_url: String) -> SyncJob {
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
    fn plans_only_aged_reports_in_stable_tenant_bound_contact_order() {
        let plan = plan_aged_contact_reports(
            "tenant-a",
            NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
            &[
                EntityType::ReportAgedPayablesByContact,
                EntityType::ReportAgedReceivablesByContact,
            ],
            &[
                uuid("5f3c9a2e-0000-0000-0000-000000000002"),
                uuid("5f3c9a2e-0000-0000-0000-000000000001"),
                uuid("5f3c9a2e-0000-0000-0000-000000000002"),
            ],
            3,
            10,
        )
        .unwrap();

        assert_eq!(plan.len(), 4);
        assert!(plan.iter().all(|request| request.tenant == "tenant-a"));
        assert_eq!(
            plan[0].contact_id,
            uuid("5f3c9a2e-0000-0000-0000-000000000001")
        );
        assert_eq!(plan[0].entity, EntityType::ReportAgedPayablesByContact);
        assert_eq!(
            plan[2].contact_id,
            uuid("5f3c9a2e-0000-0000-0000-000000000002")
        );
        assert_eq!(
            plan[0].as_of,
            NaiveDate::from_ymd_opt(2026, 10, 18).unwrap()
        );
    }

    #[test]
    fn rejects_non_aged_reports_and_request_budgets_before_http() {
        let contact = uuid("5f3c9a2e-0000-0000-0000-000000000001");
        let date = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        assert!(plan_aged_contact_reports(
            "tenant-a",
            date,
            &[EntityType::ReportBalanceSheet],
            &[contact],
            1,
            10,
        )
        .is_err());
        assert!(plan_aged_contact_reports(
            "tenant-a",
            date,
            &[EntityType::ReportAgedReceivablesByContact],
            &[contact, uuid("5f3c9a2e-0000-0000-0000-000000000002")],
            1,
            10,
        )
        .is_err());
    }

    #[tokio::test]
    async fn aged_reports_are_contact_scoped_per_tenant_and_keep_raw_contact_receipts() {
        let contact_a = "5f3c9a2e-0000-0000-0000-000000000001";
        let contact_b = "5f3c9a2e-0000-0000-0000-000000000002";
        let (base_url, server) = fixture_server(vec![
            br#"{"Contacts":[{"ContactID":"5f3c9a2e-0000-0000-0000-000000000001"}]}"#,
            br#"{"Contacts":[]}"#,
            br#"{"Reports":[{"ReportName":"AgedReceivablesByContact","Rows":[]}]}"#,
            br#"{"Reports":[{"ReportName":"AgedPayablesByContact","Rows":[]}]}"#,
            br#"{"Contacts":[{"ContactID":"5f3c9a2e-0000-0000-0000-000000000002"}]}"#,
            br#"{"Contacts":[]}"#,
            br#"{"Reports":[{"ReportName":"AgedReceivablesByContact","Rows":[]}]}"#,
            br#"{"Reports":[{"ReportName":"AgedPayablesByContact","Rows":[]}]}"#,
        ])
        .await;
        let sink = Arc::new(RecordingSink::default());
        let date = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let manifest = job(sink.clone(), base_url)
            .run_aged_contact_reports(
                uuid("5f3c9a2e-0000-0000-0000-000000000099"),
                date,
                date,
                &["tenant-a".into(), "tenant-b".into()],
                &[
                    EntityType::ReportAgedReceivablesByContact,
                    EntityType::ReportAgedPayablesByContact,
                ],
                AgedContactReportOptions {
                    max_contacts_per_tenant: 10,
                    max_report_requests: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(manifest.entities.len(), 6);
        assert!(manifest
            .entities
            .iter()
            .all(|outcome| outcome.error.is_none()));
        let report_outcomes: Vec<_> = manifest
            .entities
            .iter()
            .filter(|outcome| outcome.endpoint.starts_with("report_aged_"))
            .collect();
        assert_eq!(report_outcomes.len(), 4);
        assert_eq!(
            report_outcomes
                .iter()
                .filter_map(|outcome| outcome.request_key.as_deref())
                .collect::<BTreeSet<_>>()
                .len(),
            2
        );

        let writes = sink.0.lock().unwrap();
        assert_eq!(writes.len(), 8);
        let report_writes: Vec<_> = writes
            .iter()
            .filter(|(_, _, meta)| meta.get("x-sync-type") == Some("report-snapshot"))
            .collect();
        assert_eq!(report_writes.len(), 4);
        assert!(report_writes
            .iter()
            .all(|(key, _, meta)| meta.get("x-contact-id").is_some() && key.contains("contact_")));
        assert_eq!(
            report_writes
                .iter()
                .map(|(key, _, _)| key.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
        assert!(writes.iter().any(|(_, _, meta)| {
            meta.get("x-endpoint") == Some("contacts") && meta.get("x-record-count") == Some("1")
        }));
        drop(writes);

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 8);
        let request_for = |contact_id: &str| {
            requests
                .iter()
                .filter(|request| request.contains(contact_id))
                .collect::<Vec<_>>()
        };
        let reports_a = request_for(contact_a);
        assert_eq!(reports_a.len(), 2);
        assert!(reports_a.iter().all(|request| request
            .to_ascii_lowercase()
            .contains("xero-tenant-id: tenant-a")));
        let reports_b = request_for(contact_b);
        assert_eq!(reports_b.len(), 2);
        assert!(reports_b.iter().all(|request| request
            .to_ascii_lowercase()
            .contains("xero-tenant-id: tenant-b")));
    }

    #[tokio::test]
    async fn failed_contact_report_isolated_without_stopping_later_contacts() {
        let failed_contact = "5f3c9a2e-0000-0000-0000-000000000001";
        let completed_contact = "5f3c9a2e-0000-0000-0000-000000000002";
        let (base_url, server) = fixture_server_with_status(vec![
            (
                200,
                br#"{"Contacts":[{"ContactID":"5f3c9a2e-0000-0000-0000-000000000001"},{"ContactID":"5f3c9a2e-0000-0000-0000-000000000002"}]}"#,
            ),
            (200, br#"{"Contacts":[]}"#),
            (400, br#"{"Elements":[{"Message":"fixture contact rejection"}]}"#),
            (200, br#"{"Reports":[{"Rows":[]}]}"#),
            (200, br#"{"Reports":[{"Rows":[]}]}"#),
            (200, br#"{"Reports":[{"Rows":[]}]}"#),
        ])
        .await;
        let sink = Arc::new(RecordingSink::default());
        let date = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let manifest = job(sink.clone(), base_url)
            .run_aged_contact_reports(
                uuid("5f3c9a2e-0000-0000-0000-000000000099"),
                date,
                date,
                &["tenant-a".into()],
                &[
                    EntityType::ReportAgedReceivablesByContact,
                    EntityType::ReportAgedPayablesByContact,
                ],
                AgedContactReportOptions {
                    max_contacts_per_tenant: 10,
                    max_report_requests: 10,
                },
            )
            .await
            .unwrap();

        let failed: Vec<_> = manifest
            .entities
            .iter()
            .filter(|outcome| outcome.error.is_some())
            .collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(
            failed[0].request_key.as_deref(),
            Some("contact:5f3c9a2e-0000-0000-0000-000000000001")
        );
        assert_eq!(
            manifest
                .entities
                .iter()
                .filter(|outcome| outcome.request_key.as_deref()
                    == Some("contact:5f3c9a2e-0000-0000-0000-000000000002"))
                .count(),
            2
        );
        assert_eq!(sink.0.lock().unwrap().len(), 5);

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 6);
        assert!(requests.last().unwrap().contains(completed_contact));
        assert!(requests
            .iter()
            .any(|request| request.contains(failed_contact)));
    }
}
