//! An endpoint queried one financial period at a time never resumes from its checkpoint.
//!
//! `/v1/projecttransaction` carries no top-level modification timestamp, so the only thing its
//! checkpoint can hold is the end of an earlier explicit window. A scheduled run that resumed from
//! it would walk only the periods since then and lose every document posted to an older period
//! (the same hole a current-month-only window leaves). Each run without an explicit `date_from`
//! therefore starts from the endpoint's configured first period, which the adapter reads.
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use common::fake_adapter::{FakeAdapter, Step};
use common::{erp_def, utc, Harness};
use serde_json::json;
use vsv2_orchestrator::registry::{EndpointDefinition, PeriodScanConfig};

fn project_transactions_def() -> EndpointDefinition {
    let mut def = erp_def("project_transactions");
    def.api_endpoint = Some("/v1/projecttransaction".into());
    def.primary_key = vsv2_common::PrimaryKey::Composite(vec!["module".into(), "refNbr".into()]);
    def.modified_field = None;
    def.sync_pattern = Some("scoped_wrapper_only".into());
    def.period_scan = Some(PeriodScanConfig {
        first_period: "202301".into(),
        query_params: vec!["fromPeriod".into(), "toPeriod".into()],
        look_ahead_months: 0,
        period_field: None,
        resume: false,
    });
    def.validation_fields = Some(vec!["module".into(), "refNbr".into()]);
    def
}

fn document(module: &str, number: &str) -> serde_json::Value {
    json!({"module": module, "refNbr": number, "status": "Released", "lines": []})
}

#[tokio::test]
async fn a_run_after_an_explicit_window_does_not_resume_from_that_windows_end() {
    let h = Harness::new(project_transactions_def(), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![document("ModuleAP", "1400001")]),
    );
    h.fake.push(
        "default",
        Step::Records(vec![document("ModuleAP", "1400002")]),
    );

    h.sync_window(
        Some(utc("2026-08-10T00:00:00Z")),
        Some(utc("2026-08-20T00:00:00Z")),
    )
    .await
    .expect("explicit window");
    assert!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark
            .is_some(),
        "the window leaves a watermark behind: that is the trap this test guards"
    );

    h.sync().await.expect("scheduled run");

    assert_eq!(
        h.fake.calls()[1].watermark,
        None,
        "a scheduled run must not shrink to the periods since the last window"
    );
}

#[tokio::test]
async fn an_explicit_window_still_names_its_own_start() {
    let h = Harness::new(project_transactions_def(), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![document("ModuleAP", "1400001")]),
    );

    h.sync_window(
        Some(utc("2026-08-10T00:00:00Z")),
        Some(utc("2026-08-20T00:00:00Z")),
    )
    .await
    .expect("explicit window");

    assert_eq!(
        h.fake.calls()[0].watermark,
        Some(utc("2026-08-10T00:00:00Z"))
    );
}

fn resuming_def() -> EndpointDefinition {
    let mut def = erp_def("general_ledger_transactions");
    def.period_scan = Some(PeriodScanConfig {
        first_period: "202001".into(),
        query_params: vec!["fromPeriod".into(), "toPeriod".into()],
        look_ahead_months: 0,
        period_field: None,
        resume: true,
    });
    def
}

#[tokio::test]
async fn a_resuming_period_scan_continues_from_its_checkpoint_and_a_full_pass_does_not() {
    let h = Harness::new(resuming_def(), FakeAdapter::single_partition()).await;
    for code in ["1000", "2000", "3000"] {
        h.fake.push(
            "default",
            Step::Records(vec![common::account(code, "2026-03-10T09:00:00Z")]),
        );
    }

    h.sync().await.expect("first sync");
    h.sync().await.expect("second sync");
    h.executor
        .sync_one_full(&h.tenant, &h.endpoint)
        .await
        .expect("full pass");

    let calls = h.fake.calls();
    assert_eq!(calls[0].watermark, None, "nothing to resume from yet");
    assert!(
        calls[1].watermark.is_some(),
        "the checkpoint becomes the modification filter inside each period"
    );
    assert_eq!(calls[2].watermark, None, "a full pass asks for everything");
}
