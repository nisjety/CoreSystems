//! The options of `vsv2 monitor`, the tenant list, and what must not leak into `--help` or a log.

use clap::{CommandFactory, Parser};
use vsv2_common::TenantStatus;

use super::*;

/// `MonitorArgs` is a flattened part of the CLI; this stands in for the rest of it.
#[derive(Debug, Parser)]
struct Harness {
    #[command(flatten)]
    args: MonitorArgs,
}

fn parse(flags: &[&str]) -> Result<MonitorArgs, clap::Error> {
    Harness::try_parse_from(std::iter::once("monitor").chain(flags.iter().copied()))
        .map(|harness| harness.args)
}

fn write_tenant(dir: &Path, file: &str, json: &str) {
    std::fs::write(dir.join(file), json).expect("write a tenant file");
}

const ACTIVE_JSON: &str = r#"{
    "tenant_id": "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd",
    "tenant_name": "Aqua Pharma AS"
}"#;

const HISTORICAL_JSON: &str = r#"{
    "tenant_id": "9b1fa69b-5568-11ee-960c-025417856183",
    "tenant_name": "Consult AS",
    "status": "historical",
    "valid_to": "2025-12-31"
}"#;

// ── options ────────────────────────────────────────────────────────────────

#[test]
fn the_defaults_are_the_documented_ones() {
    let args = parse(&[]).expect("no flags is valid");

    assert_eq!(args.format, Format::Text);
    assert_eq!(args.tenants_dir, "config/tenants");
    assert!(!args.strict);
    assert_eq!(args.thresholds(), Thresholds::default());
    assert_eq!(args.hourly_max_age_mins, 180);
    assert_eq!(args.daily_max_age_hours, 30);
    assert_eq!(args.full_max_age_days, 8);
    assert_eq!(args.open_set_max_age_hours, 36);
    assert_eq!(args.stuck_heartbeat_mins, 10);
    assert_eq!(args.repeat_failures, 3);
}

#[test]
fn every_flag_sets_its_value() {
    let args = parse(&[
        "--format",
        "json",
        "--tenants-dir",
        "/etc/vsv2/tenants",
        "--webhook-url",
        "https://hooks.example.com/x/y",
        "--hourly-max-age-mins",
        "60",
        "--daily-max-age-hours",
        "12",
        "--full-max-age-days",
        "3",
        "--open-set-max-age-hours",
        "6",
        "--stuck-heartbeat-mins",
        "5",
        "--repeat-failures",
        "2",
        "--strict",
    ])
    .expect("valid flags");

    assert_eq!(args.format, Format::Json);
    assert_eq!(args.tenants_dir, "/etc/vsv2/tenants");
    assert!(args.strict);
    assert_eq!(
        args.webhook().map(Secret::expose),
        Some("https://hooks.example.com/x/y")
    );
    assert_eq!(
        args.thresholds(),
        Thresholds {
            hourly_max_age: Duration::minutes(60),
            daily_max_age: Duration::hours(12),
            full_max_age: Duration::days(3),
            open_set_max_age: Duration::hours(6),
            stuck_heartbeat: Duration::minutes(5),
            repeat_failures: 2,
        }
    );
}

#[test]
fn values_that_make_no_sense_are_refused() {
    for flags in [
        &["--repeat-failures", "0"][..],
        &["--repeat-failures", "101"],
        &["--hourly-max-age-mins", "0"],
        &["--daily-max-age-hours", "0"],
        &["--full-max-age-days", "0"],
        &["--full-max-age-days", "366"],
        &["--open-set-max-age-hours", "0"],
        &["--stuck-heartbeat-mins", "0"],
        &["--hourly-max-age-mins", "-5"],
        &["--hourly-max-age-mins", "soon"],
        &["--format", "xml"],
    ] {
        assert!(parse(flags).is_err(), "{flags:?} should be refused");
    }
}

#[test]
fn the_largest_values_do_not_overflow_a_duration() {
    let args = parse(&[
        "--hourly-max-age-mins",
        "525600",
        "--daily-max-age-hours",
        "8760",
        "--full-max-age-days",
        "365",
        "--open-set-max-age-hours",
        "8760",
        "--stuck-heartbeat-mins",
        "525600",
    ])
    .expect("the largest values are valid");

    let limits = args.thresholds();

    assert_eq!(limits.full_max_age, Duration::days(365));
    assert_eq!(limits.hourly_max_age, Duration::days(365));
}

// ── the webhook URL stays out of sight ─────────────────────────────────────

#[test]
fn the_webhook_url_is_read_from_the_environment_and_its_value_is_hidden_from_help() {
    let command = Harness::command();
    let option = command
        .get_arguments()
        .find(|arg| arg.get_id() == "webhook_url")
        .expect("the --webhook-url option");

    assert_eq!(
        option.get_env().and_then(|name| name.to_str()),
        Some("VSV2_MONITOR_WEBHOOK_URL")
    );
    assert!(
        option.is_hide_env_values_set(),
        "--help would print the URL otherwise"
    );
}

#[test]
fn the_arguments_never_print_the_webhook_url() {
    let url = "https://hooks.example.com/services/T0/B0/topsecretT0kenValue";
    let args = parse(&["--webhook-url", url]).expect("valid flags");

    let shown = format!("{args:?}");

    assert!(!shown.contains("topsecretT0kenValue"), "{shown}");
    assert!(!shown.contains("hooks.example.com"), "{shown}");
}

#[test]
fn a_blank_webhook_url_counts_as_none() {
    let args = parse(&["--webhook-url", "   "]).expect("valid flags");

    assert!(args.webhook().is_none());
}

// ── the tenant list ────────────────────────────────────────────────────────

#[test]
fn the_tenant_list_comes_from_the_tenant_files_with_their_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_tenant(dir.path(), "aqua_pharma_as.json", ACTIVE_JSON);
    write_tenant(dir.path(), "consult_as.json", HISTORICAL_JSON);
    write_tenant(
        dir.path(),
        "_example.json",
        "not json: files with _ are skipped",
    );

    let tenants = load_tenant_list(dir.path().to_str().expect("utf-8 path")).expect("a list");

    let listed: Vec<(&str, TenantStatus)> = tenants
        .iter()
        .map(|tenant| (tenant.name.as_str(), tenant.status))
        .collect();
    assert_eq!(
        listed,
        [
            ("Aqua Pharma AS", TenantStatus::Active),
            ("Consult AS", TenantStatus::Historical)
        ]
    );
    assert_eq!(
        tenants[0].id.as_str(),
        "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd"
    );
}

#[test]
fn a_missing_tenant_directory_is_an_error_that_names_it() {
    let error = load_tenant_list("/nonexistent/vsv2/tenants").expect_err("no such directory");

    assert!(
        format!("{error:#}").contains("/nonexistent/vsv2/tenants"),
        "{error:#}"
    );
}

#[test]
fn a_directory_without_tenant_files_is_an_error_not_an_empty_all_clear() {
    let dir = tempfile::tempdir().expect("tempdir");

    let error =
        load_tenant_list(dir.path().to_str().expect("utf-8 path")).expect_err("nothing to check");

    assert!(
        format!("{error:#}").contains("no tenant files"),
        "{error:#}"
    );
}

#[test]
fn a_broken_tenant_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_tenant(dir.path(), "broken.json", r#"{"tenant_id": 7}"#);

    let error = load_tenant_list(dir.path().to_str().expect("utf-8 path")).expect_err("broken");

    assert!(format!("{error:#}").contains("broken.json"), "{error:#}");
}

#[test]
fn the_real_tenant_files_of_this_repository_load() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../config/tenants");

    let tenants = load_tenant_list(dir.to_str().expect("utf-8 path")).expect("config/tenants");

    assert!(tenants.len() >= 12, "{} tenants", tenants.len());
    assert!(tenants
        .iter()
        .any(|tenant| tenant.status == TenantStatus::Historical));
    assert!(tenants
        .iter()
        .any(|tenant| tenant.status == TenantStatus::Active));
}
