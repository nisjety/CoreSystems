//! Check 4: `open_set_<endpoint>`.

use super::*;

#[test]
fn a_recent_open_set_run_that_succeeded_is_ok() {
    let (passes, runs) = healthy(ACTIVE);

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Ok);
    assert!(
        finding.detail.contains("succeeded 5 h ago"),
        "{}",
        finding.detail
    );
    assert!(finding.detail.contains("limit 36 h"), "{}", finding.detail);
}

#[test]
fn an_open_set_run_exactly_at_the_limit_is_fresh_and_a_second_older_is_a_problem() {
    let (passes, mut runs) = healthy(ACTIVE);
    runs[0].started_at = ago(hours(36));
    let at_limit = check(&[active()], passes.clone(), runs.clone());
    runs[0].started_at = ago(hours(36) + Duration::seconds(1));
    let over = check(&[active()], passes, runs);

    assert_eq!(
        severity(&at_limit, ACTIVE, "open_set_customer_invoices"),
        Severity::Ok
    );
    let finding = find(&over, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(
        finding.detail.contains("over the limit of 36 h"),
        "{}",
        finding.detail
    );
    assert_eq!(
        severity(&over, ACTIVE, "open_set_supplier_invoices"),
        Severity::Ok,
        "the other endpoint has its own runs"
    );
}

#[test]
fn a_missing_open_set_run_is_a_warning_until_the_first_hourly_pass_is_a_day_old() {
    let (mut passes, _) = healthy(ACTIVE);
    passes.retain(|p| p.tier != "hourly");
    let cases = [
        (hours(23), Severity::Warning),
        (hours(24), Severity::Warning),
        (hours(24) + Duration::seconds(1), Severity::Problem),
        (days(3), Severity::Problem),
    ];
    for (first_pass_age, expected) in cases {
        let mut with_hourly = passes.clone();
        with_hourly.push(done(ACTIVE, "hourly", ago(first_pass_age)));

        let findings = check(&[active()], with_hourly, Vec::new());

        assert_eq!(
            severity(&findings, ACTIVE, "open_set_customer_invoices"),
            expected,
            "first hourly pass {first_pass_age} old"
        );
        assert_eq!(
            severity(&findings, ACTIVE, "open_set_supplier_invoices"),
            expected
        );
    }
}

#[test]
fn a_missing_open_set_run_before_any_hourly_pass_is_a_warning() {
    let passes = vec![
        done(ACTIVE, "daily", ago(days(5))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, Vec::new());

    let finding = find(&findings, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(finding.detail.contains("no hourly pass has run yet"));
    assert_eq!(finding.observed_at, None);
}

#[test]
fn a_missing_open_set_run_says_why_it_is_due() {
    let (mut passes, _) = healthy(ACTIVE);
    passes[0] = done(ACTIVE, "hourly", ago(days(2)));

    let findings = check(&[active()], passes, Vec::new());

    let detail = &find(&findings, ACTIVE, "open_set_customer_invoices").detail;
    assert!(
        detail.contains("first hourly pass began 2 d ago"),
        "{detail}"
    );
    assert!(detail.contains("BigQuery staging"), "{detail}");
}

#[test]
fn a_failed_open_set_run_is_a_warning_and_three_in_a_row_are_a_problem() {
    let (passes, _) = healthy(ACTIVE);
    let one = vec![
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Succeeded,
            ago(hours(30)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(5)),
        ),
        open_run(
            ACTIVE,
            "supplier_invoices",
            RunStatus::Succeeded,
            ago(hours(5)),
        ),
    ];
    let three = vec![
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Succeeded,
            ago(hours(30)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(9)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(7)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(5)),
        ),
        open_run(
            ACTIVE,
            "supplier_invoices",
            RunStatus::Succeeded,
            ago(hours(5)),
        ),
    ];

    let warned = check(&[active()], passes.clone(), one);
    let problem = check(&[active()], passes, three);

    let finding = find(&warned, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(
        finding.detail.contains("1 run in a row failed"),
        "{}",
        finding.detail
    );
    let finding = find(&problem, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(
        finding.detail.contains("3 runs in a row failed"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_success_between_failed_open_set_runs_ends_the_run_of_failures() {
    let (passes, _) = healthy(ACTIVE);
    let runs = vec![
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(11)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(9)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Succeeded,
            ago(hours(7)),
        ),
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(5)),
        ),
        open_run(
            ACTIVE,
            "supplier_invoices",
            RunStatus::Succeeded,
            ago(hours(5)),
        ),
    ];

    let findings = check(&[active()], passes, runs);

    assert_eq!(
        severity(&findings, ACTIVE, "open_set_customer_invoices"),
        Severity::Warning
    );
}

#[test]
fn a_partial_open_set_run_is_a_warning() {
    let (passes, _) = healthy(ACTIVE);
    let runs = vec![
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::PartialSuccess,
            ago(hours(2)),
        ),
        open_run(
            ACTIVE,
            "supplier_invoices",
            RunStatus::Succeeded,
            ago(hours(2)),
        ),
    ];

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(
        finding.detail.contains("ended partially"),
        "{}",
        finding.detail
    );
}

#[test]
fn an_old_failed_open_set_run_is_reported_as_stale() {
    let (passes, _) = healthy(ACTIVE);
    let runs = vec![
        open_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            ago(hours(40)),
        ),
        open_run(
            ACTIVE,
            "supplier_invoices",
            RunStatus::Succeeded,
            ago(hours(2)),
        ),
    ];

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "open_set_customer_invoices");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(finding.detail.contains("(failed)"), "{}", finding.detail);
}

#[test]
fn open_set_runs_of_another_tenant_do_not_count() {
    let (mut passes, mut runs) = healthy(ACTIVE);
    let (other_passes, _) = healthy(OTHER);
    passes.extend(other_passes);
    runs.retain(|run| run.tenant_id == ACTIVE);

    let findings = check(&[active(), other()], passes, runs);

    assert_eq!(
        severity(&findings, ACTIVE, "open_set_customer_invoices"),
        Severity::Ok
    );
    assert_eq!(
        severity(&findings, OTHER, "open_set_customer_invoices"),
        Severity::Warning,
        "the other tenant's first hourly pass is only 20 minutes old"
    );
}

#[test]
fn the_open_set_limit_follows_the_threshold_that_is_passed_in() {
    let (passes, mut runs) = healthy(ACTIVE);
    runs[0].started_at = ago(hours(7));
    let short = Thresholds {
        open_set_max_age: hours(6),
        ..Thresholds::default()
    };

    let findings = check_with(&[active()], passes, runs, &short);

    assert_eq!(
        severity(&findings, ACTIVE, "open_set_customer_invoices"),
        Severity::Problem
    );
}

#[test]
fn the_monitored_open_set_endpoints_are_the_ones_the_endpoint_configs_mark() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../config/endpoints");
    let registry = EndpointRegistry::load_dir(&dir).expect("load config/endpoints");

    let mut marked: Vec<String> = registry
        .iter()
        .filter(|def| def.open_set && def.enabled)
        .map(|def| def.endpoint_name.clone())
        .collect();
    marked.sort();

    assert_eq!(
        marked,
        OPEN_SET_ENDPOINTS,
        "an endpoint with `open_set: true` must be listed in OPEN_SET_ENDPOINTS (and the other way \
         round), or its reconciliation is not monitored"
    );
}

#[test]
fn the_run_labels_are_the_names_the_open_set_run_records() {
    assert_eq!(
        open_set_run_labels(),
        ["customer_invoices:open_set", "supplier_invoices:open_set"]
    );
}
