//! Registering the tenants in `config/tenants` writes their DataMart company to the state store.
//!
//! A DataMart sync asks the store for the tenant's company id. A tenant that is not registered
//! cannot be synced from DataMart; a tenant registered with another tenant's company would be
//! synced with that tenant's data. Registering must therefore be repeatable, must change only
//! what differs, and must leave a tenant without a discovered company untouched (the pilot's row
//! holds a placeholder until discovery has run).
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use std::path::Path;

use common::test_db::TestDb;
use vsv2_common::{load_tenants, TenantConfig, TenantId, TenantStatus};
use vsv2_state::Registration;

const PILOT: &str = "33f319b9-4d57-11ee-960c-025417856183";

fn tenant(id: &str, name: &str, company: Option<&str>) -> TenantConfig {
    TenantConfig {
        tenant_id: id.into(),
        tenant_name: name.into(),
        datamart_company_id: company.map(String::from),
        status: TenantStatus::Active,
        valid_to: None,
        bu_label: None,
        notes: None,
    }
}

#[tokio::test]
async fn a_new_tenant_is_created_and_then_found_by_a_datamart_sync() {
    let db = TestDb::create().await;
    let sensilist = tenant(
        "eda0dd16-fa25-11ed-a0e0-0693d8a7c3dd",
        "Sensilist AS",
        Some("4734974"),
    );

    let outcome = db
        .state
        .register_tenant(&sensilist)
        .await
        .expect("register");

    assert_eq!(outcome, Registration::Created);
    let found = db
        .state
        .get_company_id(&TenantId::new(&sensilist.tenant_id))
        .await
        .expect("read");
    assert_eq!(found.as_deref(), Some("4734974"));
}

#[tokio::test]
async fn registering_twice_changes_nothing_the_second_time() {
    let db = TestDb::create().await;
    let sensilist = tenant(
        "eda0dd16-fa25-11ed-a0e0-0693d8a7c3dd",
        "Sensilist AS",
        Some("4734974"),
    );

    db.state.register_tenant(&sensilist).await.expect("first");
    let again = db.state.register_tenant(&sensilist).await.expect("second");

    assert_eq!(again, Registration::Unchanged);
}

#[tokio::test]
async fn a_changed_company_or_name_is_updated() {
    let db = TestDb::create().await;
    let id = "eda0dd16-fa25-11ed-a0e0-0693d8a7c3dd";
    db.state
        .register_tenant(&tenant(id, "Sensilist AS", Some("1111")))
        .await
        .expect("first");

    let moved = db
        .state
        .register_tenant(&tenant(id, "Sensilist AS", Some("4734974")))
        .await
        .expect("new company");
    let renamed = db
        .state
        .register_tenant(&tenant(id, "Sensilist Norge AS", Some("4734974")))
        .await
        .expect("new name");

    assert_eq!(
        (moved, renamed),
        (Registration::Updated, Registration::Updated)
    );
    let rows = db.state.list_tenant_companies().await.expect("list");
    let row = rows.iter().find(|r| r.tenant.as_str() == id).expect("row");
    assert_eq!(row.company_id, "4734974");
    assert_eq!(row.company_name.as_deref(), Some("Sensilist Norge AS"));
}

#[tokio::test]
async fn a_tenant_without_a_company_is_skipped_and_the_pilot_placeholder_survives() {
    let db = TestDb::create().await;
    let pilot = tenant(PILOT, "Aquatiq AS", None);

    let outcome = db.state.register_tenant(&pilot).await.expect("register");

    assert_eq!(outcome, Registration::SkippedNoCompanyId);
    let rows = db.state.list_tenant_companies().await.expect("list");
    let row = rows
        .iter()
        .find(|r| r.tenant.as_str() == PILOT)
        .expect("seeded row");
    assert_eq!(row.company_id, "PENDING_PHASE_0_DISCOVERY");
    assert_eq!(
        db.state
            .get_company_id(&TenantId::new(PILOT))
            .await
            .expect("read"),
        None,
        "still reads as not configured"
    );
}

#[tokio::test]
async fn a_dry_run_plan_writes_nothing() {
    let db = TestDb::create().await;
    let sensilist = tenant(
        "eda0dd16-fa25-11ed-a0e0-0693d8a7c3dd",
        "Sensilist AS",
        Some("4734974"),
    );

    let planned = db.state.plan_tenant(&sensilist).await.expect("plan");

    assert_eq!(planned, Registration::Created);
    assert_eq!(
        db.state
            .get_company_id(&TenantId::new(&sensilist.tenant_id))
            .await
            .expect("read"),
        None
    );
}

#[tokio::test]
async fn the_repository_tenants_register_and_each_one_reads_back_its_own_company() {
    let db = TestDb::create().await;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../config/tenants");
    let tenants = load_tenants(&dir).expect("tenant files");

    for tenant in &tenants {
        db.state.register_tenant(tenant).await.expect("register");
    }

    for tenant in &tenants {
        let found = db
            .state
            .get_company_id(&TenantId::new(&tenant.tenant_id))
            .await
            .expect("read");
        assert_eq!(found, tenant.datamart_company_id, "{}", tenant.tenant_name);
    }
}
