//! The DataMart company id lookup must not hand out migration 0001's placeholder.
//!
//! Migration 0001 seeds the pilot tenant with `PENDING_PHASE_0_DISCOVERY`. The DataMart
//! adapter sends whatever `get_company_id` returns as the `companyId` variable, so returning
//! the placeholder would turn "not configured" into a confusing GraphQL error (or, worse, a
//! query for a company that does not exist). It must read as *absent* instead.
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use common::test_db::TestDb;
use vsv2_common::TenantId;

const PILOT_TENANT: &str = "33f319b9-4d57-11ee-960c-025417856183";

#[tokio::test]
async fn the_seeded_placeholder_reads_as_no_company_id() {
    let db = TestDb::create().await;

    let found = db
        .state
        .get_company_id(&TenantId::new(PILOT_TENANT))
        .await
        .expect("query company id");

    assert_eq!(
        found, None,
        "PENDING_PHASE_0_DISCOVERY must not be returned as an id"
    );
}

#[tokio::test]
async fn a_configured_company_id_is_returned() {
    let db = TestDb::create().await;
    let tenant = TenantId::new(PILOT_TENANT);

    db.state
        .upsert_company_id(&tenant, "4839725")
        .await
        .expect("store company id");

    assert_eq!(
        db.state
            .get_company_id(&tenant)
            .await
            .expect("read")
            .as_deref(),
        Some("4839725")
    );
}

#[tokio::test]
async fn an_unknown_tenant_has_no_company_id() {
    let db = TestDb::create().await;

    assert_eq!(
        db.state
            .get_company_id(&TenantId::new("no-such-tenant"))
            .await
            .expect("query company id"),
        None
    );
}
