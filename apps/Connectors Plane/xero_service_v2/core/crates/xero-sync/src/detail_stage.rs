use std::collections::{BTreeMap, HashSet};
use xero_auth::TokenProvider;
use xero_client::{
    accounting_get::{catalog, uuid_detail_operation},
    RawPage, XeroApiClient,
};
use xero_common::EntityType;
use xero_gcs::{EntityOutcome, MetaArgs, ObjectMeta, SyncType};

use crate::{
    accounting_get::{endpoint as accounting_get_endpoint, request_descriptor},
    detail_ids,
    outcome::{classify_source_failure, sanitized_error},
    page_spool::PageSpool,
    streaming::{write_pages, PageSource, PageWriter},
    validate_detail_count,
};

#[async_trait::async_trait]
pub(super) trait DetailFetcher: Send + Sync {
    async fn fetch(&self, id: &str, page: u32) -> Result<RawPage, String>;
}

pub(super) struct AuthenticatedDetails<'a> {
    pub api: &'a XeroApiClient,
    pub auth: &'a dyn TokenProvider,
    pub tenant: &'a str,
    pub entity: &'a EntityType,
}

#[async_trait::async_trait]
impl DetailFetcher for AuthenticatedDetails<'_> {
    async fn fetch(&self, id: &str, page: u32) -> Result<RawPage, String> {
        let token = self
            .auth
            .get_valid_token(self.tenant)
            .await
            .map_err(|e| e.to_string())?;
        self.api
            .fetch_raw_detail(&token.access_token, self.entity, id, page)
            .await
            .map_err(|e| e.to_string())
    }
}

pub(super) async fn expanded_pages(
    source: &mut dyn PageSource,
    fetcher: &dyn DetailFetcher,
    entity: &EntityType,
    max_records: usize,
    writer: &mut PageWriter<'_>,
) -> Vec<EntityOutcome> {
    let mut collection = match PageSpool::from_env() {
        Ok(spool) => spool,
        Err(error) => return vec![writer.outcome("error", Some(error))],
    };
    let (ids, detail_error, source_error) =
        stage_collection(source, &mut collection, entity, max_records).await;
    let pagination = source.outcome();
    collection.set_outcome(pagination);

    // The listed response is authoritative raw evidence in its own right. It
    // must reach durable storage before a UUID detail expansion begins.
    let collection_outcome = write_pages(&mut collection, writer).await;
    if let Some(error) = source_error {
        if collection_outcome.error.is_some() {
            return vec![collection_outcome];
        }
        let (outcome_class, http_status) =
            classify_source_failure(&collection_outcome.endpoint, &error);
        let collection_outcome = EntityOutcome {
            termination: "error".into(),
            error: Some(sanitized_error(&error)),
            outcome_class: Some(outcome_class),
            http_status,
            ..collection_outcome
        };
        let detail_endpoint = match detail_target(entity) {
            Ok((_, endpoint)) => endpoint,
            Err(_) => return vec![collection_outcome],
        };
        return vec![
            collection_outcome,
            detail_outcome(
                writer,
                &detail_endpoint,
                "not_attempted_collection_incomplete",
                Some(error),
            ),
        ];
    }
    if collection_outcome.error.is_some() {
        return vec![collection_outcome];
    }

    let (operation, detail_endpoint) = match detail_target(entity) {
        Ok(target) => target,
        Err(_) => return vec![collection_outcome],
    };
    if pagination.termination.is_suspect() {
        return vec![
            collection_outcome,
            detail_outcome(
                writer,
                &detail_endpoint,
                "not_attempted_collection_incomplete",
                Some("Collection pagination did not complete safely".into()),
            ),
        ];
    }
    if let Some(error) = detail_error {
        return vec![
            collection_outcome,
            detail_outcome(
                writer,
                &detail_endpoint,
                "not_attempted_detail_limit",
                Some(error),
            ),
        ];
    }
    if ids.is_empty() {
        return vec![
            collection_outcome,
            detail_outcome(
                writer,
                &detail_endpoint,
                "not_applicable_no_parent_id",
                None,
            ),
        ];
    }

    let detail_outcome =
        write_detail_pages(fetcher, &ids, entity, operation, &detail_endpoint, writer).await;
    vec![collection_outcome, detail_outcome]
}

fn detail_target(
    entity: &EntityType,
) -> Result<
    (
        &'static xero_client::accounting_get::AccountingGetOperation,
        String,
    ),
    String,
> {
    let operation = uuid_detail_operation(entity).map_err(|error| error.to_string())?;
    Ok((operation, accounting_get_endpoint(operation)))
}

fn detail_outcome(
    collection_writer: &PageWriter<'_>,
    endpoint: &str,
    termination: &str,
    error: Option<String>,
) -> EntityOutcome {
    let (outcome_class, http_status, error) = match error.as_deref() {
        Some(error) => {
            let (outcome_class, http_status) = classify_source_failure(endpoint, error);
            (
                Some(outcome_class),
                http_status,
                Some(sanitized_error(error)),
            )
        }
        None if termination == "not_applicable_no_parent_id" => {
            (Some(xero_gcs::OutcomeClass::NotApplicable), None, None)
        }
        None => (None, None, None),
    };
    EntityOutcome {
        tenant: collection_writer.args().meta.tenant_id.into(),
        endpoint: endpoint.into(),
        pages: 0,
        records: 0,
        termination: termination.into(),
        request_key: None,
        error,
        outcome_class,
        http_status,
    }
}

async fn stage_collection(
    source: &mut dyn PageSource,
    spool: &mut PageSpool,
    entity: &EntityType,
    max_records: usize,
) -> (Vec<String>, Option<String>, Option<String>) {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut detail_error = None;
    loop {
        let page = match source.next().await {
            Ok(Some(page)) => page,
            Ok(None) => return (ids, detail_error, None),
            Err(error) => return (ids, detail_error, Some(error)),
        };
        if let Err(error) = spool.append(&page).await {
            return (ids, detail_error, Some(error));
        }
        if detail_error.is_none() {
            if let Err(error) = append_ids(&page, entity, max_records, &mut ids, &mut seen) {
                detail_error = Some(error);
            }
        }
    }
}

fn append_ids(
    page: &RawPage,
    entity: &EntityType,
    max_records: usize,
    ids: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    for id in detail_ids(entity, std::slice::from_ref(page))? {
        if !seen.contains(&id) {
            validate_detail_count(entity, ids.len().saturating_add(1), max_records)?;
            seen.insert(id.clone());
            ids.push(id);
        }
    }
    Ok(())
}

async fn write_detail_pages(
    fetcher: &dyn DetailFetcher,
    ids: &[String],
    entity: &EntityType,
    operation: &xero_client::accounting_get::AccountingGetOperation,
    endpoint: &str,
    collection_writer: &PageWriter<'_>,
) -> EntityOutcome {
    let args = collection_writer.args();
    let mut writer = collection_writer.scoped(crate::streaming::WriterArgs {
        prefix: args.prefix,
        ts: args.ts,
        run_date: args.run_date,
        run_id_short: args.run_id_short,
        meta: MetaArgs {
            tenant_id: args.meta.tenant_id,
            org_name: args.meta.org_name,
            endpoint,
            sync_type: SyncType::AccountingGet,
            run_id: args.meta.run_id,
            synced_at: args.meta.synced_at,
            ..MetaArgs::default()
        },
    });
    for (index, id) in ids.iter().enumerate() {
        let number = match u32::try_from(index + 1) {
            Ok(number) => number,
            Err(_) => {
                return writer.outcome(
                    "partial_detail_failure",
                    Some("Too many detail records".into()),
                )
            }
        };
        let page = match fetcher.fetch(id, number).await {
            Ok(page) => page,
            Err(error) => return writer.outcome("partial_detail_failure", Some(error)),
        };
        if page.http_status == 304 {
            return writer.outcome(
                "partial_detail_failure",
                Some("Unexpected not-modified detail response".into()),
            );
        }
        let path = [(entity.id_field().to_string(), id.clone())];
        let descriptor = match request_descriptor(operation, &path, &[], &[]) {
            Ok(descriptor) => descriptor,
            Err(error) => return writer.outcome("partial_detail_failure", Some(error.to_string())),
        };
        let metadata = ObjectMeta(BTreeMap::from([
            ("x-api-family".into(), "accounting".into()),
            ("x-operation-id".into(), operation.operation_id.clone()),
            ("x-spec-commit".into(), catalog().spec_commit.clone()),
            ("x-request-descriptor".into(), descriptor),
            ("x-response-content-type".into(), "application/json".into()),
            ("x-response-binary".into(), "false".into()),
            ("x-source-endpoint".into(), entity.as_str().into()),
            ("x-parent-record-id".into(), id.clone()),
        ]));
        if let Err(error) = writer.write_with_metadata(&page, Some(&metadata)).await {
            return writer.outcome("partial_detail_failure", Some(error));
        }
    }
    writer.outcome("single_response", None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::WriterArgs;
    use chrono::Utc;
    use std::{collections::VecDeque, sync::Mutex};
    use xero_client::{PaginationOutcome, TerminationReason};
    use xero_gcs::{MetaArgs, ObjectMeta, RawSink};

    struct Source(VecDeque<Result<RawPage, String>>);
    #[async_trait::async_trait]
    impl PageSource for Source {
        async fn next(&mut self) -> Result<Option<RawPage>, String> {
            self.0.pop_front().transpose()
        }
        fn outcome(&self) -> PaginationOutcome {
            PaginationOutcome {
                pages_fetched: 2,
                termination: TerminationReason::EndedOnEmptyPage,
            }
        }
    }
    struct Details {
        fail: bool,
    }
    #[async_trait::async_trait]
    impl DetailFetcher for Details {
        async fn fetch(&self, _: &str, page: u32) -> Result<RawPage, String> {
            if self.fail {
                return Err("fixture detail failure".into());
            }
            Ok(raw(
                page,
                br#"{"Budgets":{"BudgetID":"one","BudgetLines":[]}}"#,
            ))
        }
    }
    type WrittenObject = (String, Vec<u8>, ObjectMeta);

    struct Sink(Mutex<Vec<WrittenObject>>);
    #[async_trait::async_trait]
    impl RawSink for Sink {
        async fn put_raw(
            &self,
            key: &str,
            body: &[u8],
            metadata: &ObjectMeta,
        ) -> xero_gcs::Result<()> {
            self.0
                .lock()
                .unwrap()
                .push((key.into(), body.into(), metadata.clone()));
            Ok(())
        }
    }
    fn raw(page: u32, body: &'static [u8]) -> RawPage {
        RawPage {
            page,
            body: body.into(),
            http_status: 200,
            record_count: 1,
            fetched_at: Utc::now(),
        }
    }
    fn args() -> WriterArgs<'static> {
        WriterArgs {
            prefix: "raw/xero",
            ts: "20261003T170000Z",
            run_date: chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
            run_id_short: "abcdef",
            meta: MetaArgs {
                tenant_id: "fixture",
                endpoint: "budgets",
                ..MetaArgs::default()
            },
        }
    }
    #[tokio::test]
    async fn full_details_preserve_collection_and_emit_catalogued_direct_responses() {
        for fail in [false, true] {
            let collection = raw(1, br#"{ "Budgets": [ { "BudgetID": "one" } ] }"#);
            let original = collection.body.clone();
            let mut source = Source(VecDeque::from([Ok(collection)]));
            let sink = Sink(Mutex::new(Vec::new()));
            let mut writer = PageWriter::new(&sink, args());
            let outcomes = expanded_pages(
                &mut source,
                &Details { fail },
                &EntityType::Budgets,
                10,
                &mut writer,
            )
            .await;
            assert_eq!(outcomes.len(), 2);
            assert_eq!(outcomes[0].termination, "ended_on_empty_page");
            assert!(outcomes[0].error.is_none());
            let written = sink.0.lock().unwrap();
            assert_eq!(written[0].1, original);
            if fail {
                assert_eq!(written.len(), 1);
                assert_eq!(outcomes[1].endpoint, "accounting_get_getbudget");
                assert_eq!(outcomes[1].termination, "partial_detail_failure");
                assert!(outcomes[1].error.is_some());
            } else {
                assert_eq!(written.len(), 2);
                assert_eq!(outcomes[1].endpoint, "accounting_get_getbudget");
                assert_eq!(outcomes[1].termination, "single_response");
                assert_eq!(written[1].2.get("x-operation-id"), Some("getBudget"));
                assert_eq!(written[1].2.get("x-parent-record-id"), Some("one"));
                assert!(written[1].0.contains("accounting_get_getbudget"));
                assert!(std::str::from_utf8(&written[1].1)
                    .unwrap()
                    .contains("BudgetLines"));
            }
        }
    }
    #[tokio::test]
    async fn late_collection_failure_persists_staged_pages_without_claiming_completion() {
        let mut source = Source(VecDeque::from([
            Ok(raw(1, br#"{"Budgets":[{"BudgetID":"one"}]}"#)),
            Err("fixture page two failure".into()),
        ]));
        let sink = Sink(Mutex::new(Vec::new()));
        let mut writer = PageWriter::new(&sink, args());
        let outcomes = expanded_pages(
            &mut source,
            &Details { fail: false },
            &EntityType::Budgets,
            10,
            &mut writer,
        )
        .await;
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].pages, 1);
        assert_eq!(outcomes[0].records, 1);
        assert_eq!(outcomes[0].termination, "error");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("Xero source request failed")
        );
        assert_eq!(
            outcomes[1].termination,
            "not_attempted_collection_incomplete"
        );
    }

    #[tokio::test]
    async fn detail_limit_or_invalid_ids_preserve_original_collection() {
        for (body, limit) in [
            (
                br#"{"Budgets":[{"BudgetID":"one"},{"BudgetID":"two"}]}"#.as_slice(),
                1,
            ),
            (br#"{"Budgets":[{}]}"#.as_slice(), 10),
        ] {
            let mut source = Source(VecDeque::from([Ok(raw(1, body))]));
            let sink = Sink(Mutex::new(Vec::new()));
            let mut writer = PageWriter::new(&sink, args());
            let outcomes = expanded_pages(
                &mut source,
                &Details { fail: false },
                &EntityType::Budgets,
                limit,
                &mut writer,
            )
            .await;
            assert_eq!(outcomes[0].termination, "ended_on_empty_page");
            assert_eq!(outcomes[1].termination, "not_attempted_detail_limit");
            assert!(outcomes[1].error.is_some());
            assert_eq!(sink.0.lock().unwrap()[0].1, body);
        }
    }

    #[tokio::test]
    async fn empty_collection_never_fans_out_and_duplicate_ids_are_bounded() {
        let body = br#"{"Budgets":[]}"#;
        let mut source = Source(VecDeque::from([Ok(raw(1, body))]));
        let sink = Sink(Mutex::new(Vec::new()));
        let mut writer = PageWriter::new(&sink, args());
        let outcomes = expanded_pages(
            &mut source,
            &Details { fail: true },
            &EntityType::Budgets,
            0,
            &mut writer,
        )
        .await;
        assert!(outcomes[0].error.is_none());
        assert_eq!(outcomes[1].termination, "not_applicable_no_parent_id");
        assert_eq!(
            outcomes[1].outcome_class,
            Some(xero_gcs::OutcomeClass::NotApplicable)
        );
        assert_eq!(sink.0.lock().unwrap()[0].1, body);
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let page = raw(1, br#"{"Budgets":[{"BudgetID":"one"}]}"#);
        append_ids(&page, &EntityType::Budgets, 1, &mut ids, &mut seen).unwrap();
        append_ids(&page, &EntityType::Budgets, 1, &mut ids, &mut seen).unwrap();
        assert_eq!(ids, vec!["one"]);
    }

    struct NotModified;
    #[async_trait::async_trait]
    impl DetailFetcher for NotModified {
        async fn fetch(&self, _: &str, page: u32) -> Result<RawPage, String> {
            Ok(RawPage {
                http_status: 304,
                ..raw(page, b"")
            })
        }
    }
    #[tokio::test]
    async fn not_modified_detail_cannot_replace_collection_with_zero_data() {
        let body = br#"{"Budgets":[{"BudgetID":"one"}]}"#;
        let mut source = Source(VecDeque::from([Ok(raw(1, body))]));
        let sink = Sink(Mutex::new(Vec::new()));
        let mut writer = PageWriter::new(&sink, args());
        let outcomes = expanded_pages(
            &mut source,
            &NotModified,
            &EntityType::Budgets,
            10,
            &mut writer,
        )
        .await;
        assert_eq!(outcomes[0].termination, "ended_on_empty_page");
        assert_eq!(outcomes[1].termination, "partial_detail_failure");
        assert_eq!(sink.0.lock().unwrap()[0].1, body);
    }
}
