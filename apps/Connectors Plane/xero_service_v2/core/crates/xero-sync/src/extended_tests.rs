use super::*;

#[test]
fn finance_requires_a_reproducible_window() {
    let options = ExtendedOptions {
        from: None,
        to: None,
        bank_account_ids: vec![],
        details: false,
    };
    assert!(options.validate(&[ExtendedFamily::Finance]).is_err());
    assert!(options.validate(&[ExtendedFamily::Assets]).is_ok());
}

#[test]
fn ordinary_and_sensitive_families_need_separate_runs() {
    let options = ExtendedOptions {
        from: None,
        to: None,
        bank_account_ids: vec![],
        details: false,
    };
    assert!(options
        .validate(&[ExtendedFamily::Assets, ExtendedFamily::Identity])
        .is_err());
    assert!(options
        .validate(&[ExtendedFamily::Assets, ExtendedFamily::Projects])
        .is_ok());
    assert!(options
        .validate(&[ExtendedFamily::BankFeeds, ExtendedFamily::Identity])
        .is_ok());
}

#[test]
fn ids_are_deduplicated_across_pages() {
    let pages = [RawPage {
        page: 1,
        body: br#"{"items":[{"projectId":"one"},{"projectId":"one"}]}"#
            .as_slice()
            .into(),
        http_status: 200,
        record_count: 2,
        fetched_at: Utc::now(),
    }];
    assert!(collect_ids(&pages, ResponseShape::Items, Some("projectId")).is_err());
}

#[test]
fn collection_snapshots_are_limited_to_complete_list_endpoints() {
    assert_eq!(
        collection_snapshot_endpoint("assets_assets"),
        Some("assets_list_snapshots")
    );
    assert_eq!(
        collection_snapshot_endpoint("projects_time"),
        Some("projects_list_snapshots")
    );
    assert_eq!(collection_snapshot_endpoint("finance_balance_sheet"), None);
    assert_eq!(collection_snapshot_endpoint("projects_time_details"), None);
}
