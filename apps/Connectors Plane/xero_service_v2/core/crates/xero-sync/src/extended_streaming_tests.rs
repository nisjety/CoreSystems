use super::*;
use std::sync::Mutex;
use xero_client::{PaginationOutcome, TerminationReason};
use xero_gcs::ObjectMeta;

struct DeniedAuth;
#[async_trait::async_trait]
impl TokenProvider for DeniedAuth {
    async fn get_valid_token(&self, _: &str) -> xero_common::Result<xero_auth::TokenData> {
        Err(xero_common::Error::Auth(
            "fixture auth must not be called".into(),
        ))
    }
}
struct Sink {
    keys: Mutex<Vec<String>>,
    fail_snapshot: bool,
}
#[async_trait::async_trait]
impl RawSink for Sink {
    async fn put_raw(&self, key: &str, _: &[u8], _: &ObjectMeta) -> xero_gcs::Result<()> {
        if self.fail_snapshot && key.contains("projects_list_snapshots") {
            return Err(xero_gcs::GcsError::Config(
                "fixture snapshot upload failure".into(),
            ));
        }
        self.keys.lock().unwrap().push(key.into());
        Ok(())
    }
}
struct Source<'a> {
    fetched: u32,
    sink: &'a Sink,
    late_failure: bool,
    capped: bool,
}
#[async_trait::async_trait]
impl PageSource for Source<'_> {
    async fn next(&mut self) -> Result<Option<RawPage>, String> {
        if self.fetched == 0 {
            self.fetched += 1;
            return Ok(Some(RawPage {
                page: 1,
                body: br#"{"items":[{"projectId":"6eb4f270-0000-0000-0000-000000000000"}]}"#
                    .as_slice()
                    .into(),
                http_status: 200,
                record_count: 1,
                fetched_at: Utc::now(),
            }));
        }
        assert_eq!(
            self.sink.keys.lock().unwrap().len(),
            1,
            "page one must be uploaded before another fetch"
        );
        if self.late_failure {
            Err("fixture late-page error".into())
        } else {
            Ok(None)
        }
    }
    fn outcome(&self) -> PaginationOutcome {
        PaginationOutcome {
            pages_fetched: 1,
            termination: if self.capped {
                TerminationReason::HitMaxPagesCap
            } else {
                TerminationReason::EndedOnEmptyPage
            },
        }
    }
}

#[tokio::test]
async fn only_complete_extended_lists_publish_membership_snapshots() {
    for (late_failure, capped, fail_snapshot) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let auth = DeniedAuth;
        let sink = Sink {
            keys: Mutex::new(Vec::new()),
            fail_snapshot,
        };
        let gcs = GcsConfig {
            bucket: "fixture".into(),
            prefix: "raw/xero".into(),
        };
        let mut ctx = RunContext {
            auth: &auth,
            sink: &sink,
            gcs: &gcs,
            api: XeroApiClient::new("fixture"),
            tenant: "fixture",
            storage_tenant: "fixture",
            org_name: "",
            run_id: Uuid::new_v4(),
            run_date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
            ts: "20261003T170000Z",
            page_counters: HashMap::new(),
            record_counters: HashMap::new(),
            test_api_base_url: None,
        };
        let req = request(
            ExtendedFamily::Projects,
            "projects_projects",
            "/projects",
            ResponseShape::Items,
            true,
        );
        let mut source = Source {
            fetched: 0,
            sink: &sink,
            late_failure,
            capped,
        };
        let mut outcomes = Vec::new();
        let ids = ctx
            .record_pages(&req, Some("projectId"), None, &mut source, &mut outcomes)
            .await;
        let complete = !late_failure && !capped && !fail_snapshot;
        assert_eq!(ids.len(), usize::from(complete));
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].error.is_none(), complete);
        assert_eq!(outcomes[0].pages, 1);
        assert_eq!(outcomes[0].records, 1);
        assert_eq!(ctx.page_counters["projects_projects"], 1);
        assert_eq!(ctx.record_counters["projects_projects"], 1);
        let keys = sink.keys.lock().unwrap();
        assert_eq!(
            keys.iter()
                .any(|key| key.contains("projects_list_snapshots")),
            complete
        );
    }
}
