//! The text and JSON reports, the webhook message and the exit-code mapping.

use serde_json::Value;

use super::*;

const ACTIVE: &str = "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd";
const OTHER: &str = "ffa4a061-7717-11ee-970e-025417856183";

fn at(text: &str) -> DateTime<Utc> {
    text.parse().expect("RFC 3339 timestamp")
}

fn evaluated() -> DateTime<Utc> {
    at("2026-10-13T12:00:00Z")
}

fn finding(tenant: &str, name: &str, check: &str, severity: Severity, detail: &str) -> Finding {
    Finding::new(
        Some(tenant),
        Some(name),
        check,
        severity,
        detail,
        Some(at("2026-10-13T11:40:00Z")),
    )
}

fn pipeline(check: &str, severity: Severity, detail: &str) -> Finding {
    Finding::new(None, None, check, severity, detail, None)
}

fn report(findings: Vec<Finding>) -> Report {
    Report::checked(evaluated(), 2, findings)
}

fn ok_only() -> Report {
    report(vec![
        finding(
            ACTIVE,
            "Aqua Pharma AS",
            "freshness_hourly",
            Severity::Ok,
            "fine",
        ),
        finding(
            OTHER,
            "Aqua Pharma Group AS",
            "freshness_hourly",
            Severity::Ok,
            "fine",
        ),
    ])
}

fn with_warning() -> Report {
    report(vec![
        finding(
            ACTIVE,
            "Aqua Pharma AS",
            "freshness_hourly",
            Severity::Ok,
            "fine",
        ),
        finding(
            OTHER,
            "Aqua Pharma Group AS",
            "failures_hourly",
            Severity::Warning,
            "customers failed once",
        ),
    ])
}

fn with_problem() -> Report {
    report(vec![
        finding(
            ACTIVE,
            "Aqua Pharma AS",
            "freshness_hourly",
            Severity::Problem,
            "five hours old",
        ),
        finding(
            ACTIVE,
            "Aqua Pharma AS",
            "failures_hourly",
            Severity::Warning,
            "customers failed once",
        ),
        finding(
            OTHER,
            "Aqua Pharma Group AS",
            "freshness_hourly",
            Severity::Ok,
            "fine",
        ),
    ])
}

fn json_of(report: &Report, strict: bool) -> Value {
    serde_json::from_str(&report.to_json(strict)).expect("the report is JSON")
}

// ── exit codes ─────────────────────────────────────────────────────────────

#[test]
fn the_exit_code_is_0_for_ok_and_warning_1_for_a_problem_and_2_for_an_unreadable_state() {
    let cases = [
        (Status::Ok, false, 0),
        (Status::Ok, true, 0),
        (Status::Warning, false, 0),
        (Status::Warning, true, 1),
        (Status::Problem, false, 1),
        (Status::Problem, true, 1),
        (Status::Unreadable, false, 2),
        (Status::Unreadable, true, 2),
    ];
    for (status, strict, expected) in cases {
        assert_eq!(
            exit_code(status, strict),
            expected,
            "{status:?}, strict {strict}"
        );
    }
}

#[test]
fn a_report_maps_its_worst_finding_to_the_exit_code() {
    assert_eq!(ok_only().exit_code(false), 0);
    assert_eq!(with_warning().exit_code(false), 0);
    assert_eq!(
        with_warning().exit_code(true),
        1,
        "--strict makes a warning fail"
    );
    assert_eq!(with_problem().exit_code(false), 1);
    assert_eq!(with_problem().exit_code(true), 1);
    assert_eq!(
        Report::unreadable(evaluated(), 2, "boom".into()).exit_code(false),
        2
    );
}

#[test]
fn the_status_is_the_worst_severity_and_unreadable_beats_everything() {
    assert_eq!(ok_only().status(), Status::Ok);
    assert_eq!(with_warning().status(), Status::Warning);
    assert_eq!(with_problem().status(), Status::Problem);
    assert_eq!(
        Report::unreadable(evaluated(), 2, "boom".into()).status(),
        Status::Unreadable
    );
    assert_eq!(report(Vec::new()).status(), Status::Ok);
}

// ── JSON ───────────────────────────────────────────────────────────────────

fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn the_json_report_has_a_summary_and_an_array_of_findings_with_fixed_field_names() {
    let json = json_of(&with_problem(), false);

    assert_eq!(keys(&json), ["findings", "summary"]);
    assert_eq!(
        keys(&json["summary"]),
        [
            "error",
            "evaluated_at",
            "exit_code",
            "ok",
            "problems",
            "status",
            "tenants",
            "warnings"
        ]
    );
    let findings = json["findings"].as_array().expect("an array");
    assert_eq!(findings.len(), 3);
    for entry in findings {
        assert_eq!(
            keys(entry),
            [
                "check",
                "detail",
                "observed_at",
                "severity",
                "tenant_id",
                "tenant_name"
            ]
        );
    }
}

#[test]
fn the_json_values_are_the_ones_the_findings_carry() {
    let json = json_of(&with_problem(), false);

    let first = &json["findings"][0];
    assert_eq!(first["tenant_id"], ACTIVE);
    assert_eq!(first["tenant_name"], "Aqua Pharma AS");
    assert_eq!(first["check"], "freshness_hourly");
    assert_eq!(first["severity"], "problem");
    assert_eq!(first["detail"], "five hours old");
    assert_eq!(first["observed_at"], "2026-10-13T11:40:00Z");
    assert_eq!(json["findings"][1]["severity"], "warning");
    assert_eq!(json["findings"][2]["severity"], "ok");
}

#[test]
fn the_json_summary_counts_and_carries_the_exit_code() {
    let json = json_of(&with_problem(), false);
    let summary = &json["summary"];

    assert_eq!(summary["status"], "problem");
    assert_eq!(summary["exit_code"], 1);
    assert_eq!(summary["problems"], 1);
    assert_eq!(summary["warnings"], 1);
    assert_eq!(summary["ok"], 1);
    assert_eq!(summary["tenants"], 2);
    assert_eq!(summary["evaluated_at"], "2026-10-13T12:00:00Z");
    assert!(summary["error"].is_null());
}

#[test]
fn the_json_exit_code_follows_strict() {
    assert_eq!(json_of(&with_warning(), false)["summary"]["exit_code"], 0);
    assert_eq!(json_of(&with_warning(), true)["summary"]["exit_code"], 1);
    assert_eq!(
        json_of(&with_warning(), true)["summary"]["status"],
        "warning",
        "strict changes the exit code, not the verdict"
    );
}

#[test]
fn a_finding_without_a_tenant_or_a_time_has_null_fields_not_missing_ones() {
    let report = report(vec![pipeline(
        "scheduler_ran",
        Severity::Problem,
        "not run",
    )]);

    let json = json_of(&report, false);

    let only = &json["findings"][0];
    assert!(only["tenant_id"].is_null());
    assert!(only["tenant_name"].is_null());
    assert!(only["observed_at"].is_null());
    assert_eq!(only["check"], "scheduler_ran");
}

#[test]
fn an_unreadable_state_is_reported_as_such_in_json() {
    let report = Report::unreadable(evaluated(), 12, "cannot connect: refused".into());

    let json = json_of(&report, false);

    assert_eq!(json["summary"]["status"], "unreadable");
    assert_eq!(json["summary"]["exit_code"], 2);
    assert_eq!(json["summary"]["error"], "cannot connect: refused");
    assert_eq!(json["summary"]["tenants"], 12);
    let findings = json["findings"].as_array().expect("an array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "state_database");
    assert_eq!(findings[0]["severity"], "problem");
}

// ── text ───────────────────────────────────────────────────────────────────

#[test]
fn the_text_report_leads_with_the_verdict_and_the_counts() {
    let text = with_problem().to_text();

    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("vsv2 monitor: PROBLEM at 2026-10-13T12:00:00Z (database clock)")
    );
    assert_eq!(
        lines.next(),
        Some("1 problem, 1 warning, 1 ok; 2 tenants checked")
    );
}

#[test]
fn the_text_report_lists_problems_before_warnings_with_tenant_check_and_detail() {
    let text = with_problem().to_text();

    let problem = text
        .find("PROBLEM  Aqua Pharma AS (ab0e5d06)  [freshness_hourly]")
        .expect("problem line");
    let warning = text
        .find("WARNING  Aqua Pharma AS (ab0e5d06)  [failures_hourly]")
        .expect("warning line");
    assert!(problem < warning, "{text}");
    assert!(text.contains("         five hours old"), "{text}");
    assert!(text.contains("         customers failed once"), "{text}");
}

#[test]
fn the_text_report_names_the_tenants_with_nothing_to_report() {
    let text = with_problem().to_text();

    assert!(text.contains("OK: Aqua Pharma Group AS"), "{text}");
    assert!(
        !text.contains("OK: Aqua Pharma AS"),
        "a tenant with a problem is not listed as ok: {text}"
    );
}

#[test]
fn an_all_ok_text_report_lists_every_tenant_and_no_details() {
    let text = ok_only().to_text();

    assert!(text.starts_with("vsv2 monitor: OK at "), "{text}");
    assert!(
        text.contains("0 problems, 0 warnings, 2 ok; 2 tenants checked"),
        "{text}"
    );
    assert!(
        text.contains("OK: Aqua Pharma AS, Aqua Pharma Group AS"),
        "{text}"
    );
    assert!(
        !text.contains("fine"),
        "ok findings are not spelled out: {text}"
    );
}

#[test]
fn an_unreadable_text_report_says_so_and_gives_the_reason() {
    let text = Report::unreadable(evaluated(), 12, "cannot connect: refused".into()).to_text();

    assert!(text.starts_with("vsv2 monitor: UNREADABLE at "), "{text}");
    assert!(text.contains("reason: cannot connect: refused"), "{text}");
    assert!(text.contains("[state_database]"), "{text}");
    assert!(text.contains("nothing was checked"), "{text}");
}

#[test]
fn a_finding_without_a_tenant_is_shown_as_the_pipeline() {
    let text = report(vec![pipeline(
        "scheduler_ran",
        Severity::Problem,
        "not run",
    )])
    .to_text();

    assert!(
        text.contains("PROBLEM  pipeline  [scheduler_ran]"),
        "{text}"
    );
}

// ── the webhook message ────────────────────────────────────────────────────

#[test]
fn nothing_is_posted_when_there_is_no_problem() {
    assert_eq!(ok_only().webhook_text(), None);
    assert_eq!(
        with_warning().webhook_text(),
        None,
        "a warning is not worth a post"
    );
    assert_eq!(report(Vec::new()).webhook_text(), None);
}

#[test]
fn the_message_is_the_counts_then_one_line_per_problem() {
    let text = with_problem().webhook_text().expect("a problem is posted");

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "vsv2 monitor: 1 problem, 1 warning (2 tenants)");
    assert_eq!(
        lines[1],
        "- Aqua Pharma AS: freshness_hourly: five hours old"
    );
    assert_eq!(lines.len(), 2, "the warning is counted, not listed: {text}");
}

#[test]
fn the_message_says_pipeline_for_a_finding_without_a_tenant() {
    let text = report(vec![pipeline(
        "scheduler_ran",
        Severity::Problem,
        "not run",
    )])
    .webhook_text()
    .expect("posted");

    assert!(
        text.contains("- pipeline: scheduler_ran: not run"),
        "{text}"
    );
}

#[test]
fn chat_markup_in_the_message_is_escaped() {
    let text = report(vec![finding(
        ACTIVE,
        "Tom & Jerry <!channel>",
        "freshness_hourly",
        Severity::Problem,
        "see <https://example.com|here> & weep",
    )])
    .webhook_text()
    .expect("posted");

    assert!(!text.contains("<!channel>"), "{text}");
    assert!(!text.contains('<'), "{text}");
    assert!(text.contains("Tom &amp; Jerry &lt;!channel&gt;"), "{text}");
    assert!(
        text.contains("&lt;https://example.com|here&gt; &amp; weep"),
        "{text}"
    );
}

#[test]
fn a_long_list_of_problems_is_cut_and_counted() {
    let findings: Vec<Finding> = (0..20)
        .map(|n| {
            finding(
                ACTIVE,
                "Aqua Pharma AS",
                &format!("check_{n:02}"),
                Severity::Problem,
                "broken",
            )
        })
        .collect();

    let text = report(findings).webhook_text().expect("posted");

    assert!(
        text.starts_with("vsv2 monitor: 20 problems, 0 warnings"),
        "{text}"
    );
    assert!(text.contains("check_14"), "{text}");
    assert!(!text.contains("check_15"), "{text}");
    assert!(text.contains("... and 5 more"), "{text}");
}

#[test]
fn an_unreadable_state_is_posted_without_the_reason() {
    let text = Report::unreadable(
        evaluated(),
        12,
        "cannot connect: password for hunter2".into(),
    )
    .webhook_text()
    .expect("an unreadable state is a problem");

    assert!(text.contains("state_database"), "{text}");
    assert!(text.contains("cannot be read"), "{text}");
    assert!(
        !text.contains("hunter2"),
        "the reason stays out of the message: {text}"
    );
    assert!(!text.contains("cannot connect"), "{text}");
}

#[test]
fn the_timestamp_is_whole_seconds_in_utc() {
    assert_eq!(
        timestamp(at("2026-10-13T08:15:03.987654Z")),
        "2026-10-13T08:15:03Z"
    );
}
