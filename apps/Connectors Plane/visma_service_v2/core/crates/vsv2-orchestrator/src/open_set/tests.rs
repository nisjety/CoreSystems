//! Tests of the pure parts: the diff, keys, request paths and the cadence.

use std::collections::HashMap;

use serde_json::json;
use vsv2_common::PrimaryKey;

use super::fixtures::def;
use super::*;

fn key(parts: &[&str]) -> DocKey {
    DocKey::new(parts)
}

fn keys(names: &[&str]) -> Vec<DocKey> {
    names.iter().map(|name| DocKey::new([name])).collect()
}

/// The balance a document has unless a test says otherwise: the same on both sides, so that what
/// the diff finds is a difference of keys.
const BALANCE: f64 = 100.0;

fn doc(parts: &[&str], balance: Option<f64>) -> OpenDoc {
    OpenDoc::new(key(parts), balance)
}

/// Documents with one-part keys, all with [`BALANCE`].
fn docs(names: &[&str]) -> Vec<OpenDoc> {
    names
        .iter()
        .map(|name| doc(&[name], Some(BALANCE)))
        .collect()
}

// The diff ------------------------------------------------------------------------------

#[test]
fn two_empty_sides_have_nothing_to_reconcile() {
    let diff = diff_open_sets(&[], &[]);

    assert!(diff.is_empty());
    assert_eq!(diff, OpenSetDiff::default());
}

#[test]
fn identical_sets_have_nothing_to_reconcile() {
    let both = docs(&["100", "200", "300"]);

    assert!(diff_open_sets(&both, &both).is_empty());
}

#[test]
fn the_same_keys_in_another_order_are_identical() {
    let diff = diff_open_sets(&docs(&["300", "100", "200"]), &docs(&["200", "300", "100"]));

    assert!(diff.is_empty());
}

#[test]
fn a_key_open_in_staging_but_not_in_visma_is_stale() {
    let diff = diff_open_sets(&docs(&["100", "200"]), &docs(&["100"]));

    assert_eq!(diff.stale_in_staging, keys(&["200"]));
    assert!(diff.missing_in_staging.is_empty());
}

#[test]
fn a_key_open_in_visma_but_not_in_staging_is_missing() {
    let diff = diff_open_sets(&docs(&["100"]), &docs(&["100", "200"]));

    assert!(diff.stale_in_staging.is_empty());
    assert_eq!(diff.missing_in_staging, keys(&["200"]));
}

#[test]
fn both_directions_are_found_at_once_and_keys_open_on_both_sides_are_left_out() {
    let diff = diff_open_sets(&docs(&["1", "2", "3"]), &docs(&["2", "3", "4"]));

    assert_eq!(diff.stale_in_staging, keys(&["1"]));
    assert_eq!(diff.missing_in_staging, keys(&["4"]));
    assert_eq!(diff.len(), 2);
}

#[test]
fn one_empty_side_makes_everything_on_the_other_side_a_difference() {
    let some = docs(&["1", "2"]);

    let nothing_in_visma = diff_open_sets(&some, &[]);
    assert_eq!(nothing_in_visma.stale_in_staging, keys(&["1", "2"]));
    assert!(nothing_in_visma.missing_in_staging.is_empty());

    let nothing_in_staging = diff_open_sets(&[], &some);
    assert!(nothing_in_staging.stale_in_staging.is_empty());
    assert_eq!(nothing_in_staging.missing_in_staging, keys(&["1", "2"]));
}

#[test]
fn a_key_listed_twice_is_one_difference() {
    let diff = diff_open_sets(&docs(&["1", "1", "2", "2"]), &docs(&["2", "3", "3", "3"]));

    assert_eq!(diff.stale_in_staging, keys(&["1"]));
    assert_eq!(diff.missing_in_staging, keys(&["3"]));
}

#[test]
fn a_key_listed_twice_on_both_sides_is_no_difference() {
    assert!(diff_open_sets(&docs(&["7", "7"]), &docs(&["7", "7", "7"])).is_empty());
}

#[test]
fn the_differences_come_out_sorted() {
    let diff = diff_open_sets(&docs(&["9", "1", "5"]), &[]);

    assert_eq!(diff.stale_in_staging, keys(&["1", "5", "9"]));
}

#[test]
fn the_supplier_key_is_the_pair_and_the_reference_number_alone_does_not_match() {
    let staged = vec![
        doc(&["Invoice", "210507"], Some(BALANCE)),
        doc(&["DebitAdj", "200089"], Some(BALANCE)),
    ];
    let listed = vec![
        doc(&["Invoice", "210507"], Some(BALANCE)),
        doc(&["CreditAdj", "200089"], Some(BALANCE)),
    ];

    let diff = diff_open_sets(&staged, &listed);

    assert_eq!(diff.stale_in_staging, vec![key(&["DebitAdj", "200089"])]);
    assert_eq!(diff.missing_in_staging, vec![key(&["CreditAdj", "200089"])]);
}

#[test]
fn the_order_of_the_parts_is_part_of_the_key() {
    assert_ne!(key(&["Invoice", "1"]), key(&["1", "Invoice"]));
}

#[test]
fn padding_does_not_make_a_different_document() {
    assert_eq!(key(&[" 023672 "]), key(&["023672"]));
    assert!(diff_open_sets(
        &[doc(&["Invoice", "5 "], Some(BALANCE))],
        &[doc(&[" Invoice", "5"], Some(BALANCE))]
    )
    .is_empty());
}

// The diff: balances ----------------------------------------------------------------------

#[test]
fn the_same_keys_with_equal_balances_have_nothing_to_fetch() {
    let staged = vec![doc(&["1"], Some(1450.5)), doc(&["2"], Some(0.01))];
    let listed = vec![doc(&["2"], Some(0.01)), doc(&["1"], Some(1450.5))];

    let diff = diff_open_sets(&staged, &listed);

    assert!(diff.is_empty(), "{diff:?}");
}

#[test]
fn a_part_paid_invoice_has_the_same_key_and_another_balance_and_is_fetched() {
    // Still Open, same lastModifiedDateTime: only the balance says a payment was made.
    let staged = vec![doc(&["1"], Some(1000.0)), doc(&["2"], Some(500.0))];
    let listed = vec![doc(&["1"], Some(1000.0)), doc(&["2"], Some(250.0))];

    let diff = diff_open_sets(&staged, &listed);

    assert_eq!(diff.balance_differs, keys(&["2"]));
    assert!(diff.stale_in_staging.is_empty());
    assert!(diff.missing_in_staging.is_empty());
    assert_eq!(diff.len(), 1);
    assert!(!diff.is_empty());
}

#[test]
fn a_balance_missing_on_either_side_is_unknown_not_equal_and_the_document_is_fetched() {
    // Nothing shows that no payment was made, so the document is asked for: one request, and the
    // by-key answer puts a balance in staging if Visma has one.
    for (staged, listed) in [(None, Some(5.0)), (Some(5.0), None), (None, None)] {
        let diff = diff_open_sets(&[doc(&["1"], staged)], &[doc(&["1"], listed)]);

        assert_eq!(
            diff.balance_differs,
            keys(&["1"]),
            "{staged:?} vs {listed:?}"
        );
    }
}

#[test]
fn balances_within_half_a_cent_are_the_same_balance_and_a_cent_is_a_payment() {
    let same = |staged: f64, listed: f64| {
        diff_open_sets(&[doc(&["1"], Some(staged))], &[doc(&["1"], Some(listed))]).is_empty()
    };

    assert!(same(100.0, 100.0));
    assert!(same(100.0, 100.004));
    assert!(same(0.1 + 0.2, 0.3), "float noise is not a payment");
    assert!(!same(100.0, 100.01));
    assert!(!same(100.0, 99.99));
    assert!(!same(0.0, 1450.5));
    assert!(!same(-5.0, 5.0));
}

#[test]
fn a_key_on_one_side_only_is_never_also_a_balance_difference() {
    let diff = diff_open_sets(&[doc(&["1"], None)], &[doc(&["2"], None)]);

    assert_eq!(diff.stale_in_staging, keys(&["1"]));
    assert_eq!(diff.missing_in_staging, keys(&["2"]));
    assert!(diff.balance_differs.is_empty());
}

#[test]
fn a_key_listed_twice_with_one_balance_is_one_document_and_with_two_is_in_doubt() {
    let same = diff_open_sets(
        &[doc(&["1"], Some(5.0)), doc(&["1"], Some(5.0))],
        &[doc(&["1"], Some(5.0))],
    );
    assert!(same.is_empty(), "{same:?}");

    // Two staged rows for one document, whose keys differ only in padding, with two balances:
    // not both can agree with Visma's.
    let torn = diff_open_sets(
        &[doc(&["1"], Some(5.0)), doc(&["1 "], Some(7.0))],
        &[doc(&["1"], Some(5.0))],
    );
    assert_eq!(torn.balance_differs, keys(&["1"]));
}

#[test]
fn balance_differences_come_out_sorted_and_count_in_the_total() {
    let staged = vec![
        doc(&["9"], Some(1.0)),
        doc(&["1"], Some(1.0)),
        doc(&["5"], Some(1.0)),
        doc(&["3"], Some(1.0)),
    ];
    let listed = vec![
        doc(&["9"], Some(2.0)),
        doc(&["1"], Some(2.0)),
        doc(&["5"], Some(2.0)),
        doc(&["7"], Some(1.0)),
    ];

    let diff = diff_open_sets(&staged, &listed);

    assert_eq!(diff.balance_differs, keys(&["1", "5", "9"]));
    assert_eq!(diff.stale_in_staging, keys(&["3"]));
    assert_eq!(diff.missing_in_staging, keys(&["7"]));
    assert_eq!(diff.len(), 5);
}

#[test]
fn a_balance_is_read_from_a_number_or_from_text_that_holds_one() {
    assert_eq!(read_balance(Some(&json!(1450.5))), Some(1450.5));
    assert_eq!(read_balance(Some(&json!(0))), Some(0.0));
    assert_eq!(read_balance(Some(&json!(-12))), Some(-12.0));
    assert_eq!(read_balance(Some(&json!("1450.50"))), Some(1450.5));
    assert_eq!(read_balance(Some(&json!(" 12.5 "))), Some(12.5));
    assert_eq!(read_balance(Some(&json!("1e-05"))), Some(0.00001));
}

#[test]
fn anything_else_is_no_balance() {
    for value in [
        json!(null),
        json!(""),
        json!("  "),
        json!("n/a"),
        json!("1,5"),
        json!([1]),
        json!({"amount": 1}),
        json!(true),
        json!("NaN"),
        json!("inf"),
        json!("-inf"),
    ] {
        assert_eq!(read_balance(Some(&value)), None, "{value}");
    }
    assert_eq!(read_balance(None), None, "a missing field");
}

// Keys from records and lines ------------------------------------------------------------

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn a_key_is_read_from_a_record_in_field_order() {
    let record = json!({"referenceNumber": " 210507", "documentType": "Invoice", "x": 1});

    let read = DocKey::from_record(&fields(&["documentType", "referenceNumber"]), &record);

    assert_eq!(read, Ok(key(&["Invoice", "210507"])));
}

#[test]
fn a_numeric_key_field_is_written_without_a_decimal_point() {
    let record = json!({"orderNbr": 4711});

    assert_eq!(
        DocKey::from_record(&fields(&["orderNbr"]), &record),
        Ok(key(&["4711"]))
    );
}

#[test]
fn a_dotted_key_field_follows_the_path() {
    let record = json!({"customer": {"number": "1001"}});

    assert_eq!(
        DocKey::from_record(&fields(&["customer.number"]), &record),
        Ok(key(&["1001"]))
    );
}

#[test]
fn a_key_field_that_is_missing_null_empty_or_not_a_scalar_is_an_error() {
    for record in [
        json!({}),
        json!({"referenceNumber": null}),
        json!({"referenceNumber": "  "}),
        json!({"referenceNumber": {"id": 1}}),
        json!({"referenceNumber": ["1"]}),
    ] {
        let error = DocKey::from_record(&fields(&["referenceNumber"]), &record).unwrap_err();
        assert!(error.contains("referenceNumber"), "{error}");
    }
}

#[test]
fn the_open_list_gives_its_keys_and_the_balance_of_each_row() {
    let rows = vec![
        json!({"status": "Open", "documentType": "Invoice", "referenceNumber": "1",
               "balance": 1450.5}),
        json!({"status": "Open", "documentType": "DebitAdj", "referenceNumber": "2",
               "balance": 20}),
    ];

    let read = docs_from_open_rows(&fields(&["documentType", "referenceNumber"]), &rows);

    assert_eq!(
        read.unwrap(),
        vec![
            doc(&["Invoice", "1"], Some(1450.5)),
            doc(&["DebitAdj", "2"], Some(20.0))
        ]
    );
}

#[test]
fn a_list_row_without_a_readable_balance_is_a_document_with_an_unknown_one() {
    let rows = vec![
        json!({"status": "Open", "referenceNumber": "1"}),
        json!({"status": "Open", "referenceNumber": "2", "balance": null}),
        json!({"status": "Open", "referenceNumber": "3", "balance": "n/a"}),
        json!({"status": "Open", "referenceNumber": "4", "balance": "7.5"}),
    ];

    let read = docs_from_open_rows(&fields(&["referenceNumber"]), &rows).unwrap();

    let balances: Vec<Option<f64>> = read.iter().map(|doc| doc.balance).collect();
    assert_eq!(balances, vec![None, None, None, Some(7.5)]);
}

#[test]
fn a_row_that_is_not_open_means_the_filter_was_ignored() {
    let rows = vec![
        json!({"status": "Open", "referenceNumber": "1"}),
        json!({"status": "Closed", "referenceNumber": "2"}),
    ];

    let error = docs_from_open_rows(&fields(&["referenceNumber"]), &rows).unwrap_err();

    assert!(error.to_string().contains("row 1"), "{error}");
    assert!(
        error.to_string().contains("did not apply the filter"),
        "{error}"
    );
}

#[test]
fn a_row_without_a_status_is_not_trusted_either() {
    let rows = vec![json!({"referenceNumber": "1"})];

    assert!(docs_from_open_rows(&fields(&["referenceNumber"]), &rows).is_err());
}

#[test]
fn a_row_without_its_key_is_an_error_not_a_hole() {
    let rows = vec![json!({"status": "Open"})];

    let error = docs_from_open_rows(&fields(&["referenceNumber"]), &rows).unwrap_err();

    assert!(error.to_string().contains("referenceNumber"), "{error}");
}

#[test]
fn an_empty_open_list_is_a_valid_answer() {
    assert_eq!(
        docs_from_open_rows(&fields(&["referenceNumber"]), &[]).unwrap(),
        vec![]
    );
}

#[test]
fn the_tool_output_is_one_object_per_line_with_the_staged_balance() {
    let output =
        "{\"documentType\":\"Invoice\",\"referenceNumber\":\"1\",\"balance\":\"1450.5\"}\n\n\
                  {\"documentType\":\"DebitAdj\",\"referenceNumber\":\"2\",\"balance\":\"20\"}\n";

    let read = parse_key_lines(output, &fields(&["documentType", "referenceNumber"]));

    assert_eq!(
        read.unwrap(),
        vec![
            doc(&["Invoice", "1"], Some(1450.5)),
            doc(&["DebitAdj", "2"], Some(20.0))
        ]
    );
}

#[test]
fn a_staged_balance_may_be_text_a_number_or_null() {
    let output = r#"{"referenceNumber":"1","balance":"12.50"}
{"referenceNumber":"2","balance":12.5}
{"referenceNumber":"3","balance":null}
{"referenceNumber":"4","balance":"not a number"}
"#;

    let read = parse_key_lines(output, &fields(&["referenceNumber"])).unwrap();

    let balances: Vec<Option<f64>> = read.iter().map(|doc| doc.balance).collect();
    assert_eq!(balances, vec![Some(12.5), Some(12.5), None, None]);
}

#[test]
fn output_of_a_tool_that_prints_no_balance_still_reads_with_unknown_balances() {
    // What the tool printed before it carried the balance: keys only.
    let output = "{\"documentType\":\"Invoice\",\"referenceNumber\":\"1\"}\n";

    let read = parse_key_lines(output, &fields(&["documentType", "referenceNumber"]));

    assert_eq!(read.unwrap(), vec![doc(&["Invoice", "1"], None)]);
}

#[test]
fn no_output_is_no_keys() {
    assert_eq!(
        parse_key_lines("", &fields(&["referenceNumber"])).unwrap(),
        vec![]
    );
    assert_eq!(
        parse_key_lines("\n\n", &fields(&["referenceNumber"])).unwrap(),
        vec![]
    );
}

#[test]
fn output_that_is_not_json_or_lacks_a_key_field_is_an_error() {
    let fields = fields(&["referenceNumber"]);

    assert!(parse_key_lines("not json", &fields).is_err());
    assert!(parse_key_lines("{\"other\": \"1\"}", &fields).is_err());
    assert!(parse_key_lines("[\"1\"]", &fields).is_err());
}

// Request paths ---------------------------------------------------------------------------

#[test]
fn the_customer_invoice_path_ends_in_the_reference_number() {
    let def = def(
        Some("/v1/customerinvoice"),
        PrimaryKey::Single("referenceNumber".into()),
    );

    assert_eq!(
        by_key_path(&def, &key(&["023672"])).unwrap(),
        "/v1/customerinvoice/023672"
    );
}

#[test]
fn the_supplier_invoice_path_takes_the_document_type_before_the_reference_number() {
    let def = def(
        Some("/v1/supplierInvoice"),
        PrimaryKey::Composite(vec!["documentType".into(), "referenceNumber".into()]),
    );

    assert_eq!(
        by_key_path(&def, &key(&["Invoice", "210507"])).unwrap(),
        "/v1/supplierInvoice/Invoice/210507"
    );
}

#[test]
fn a_key_value_cannot_change_the_path_it_is_put_into() {
    let def = def(
        Some("/v1/supplierInvoice/"),
        PrimaryKey::Single("ref".into()),
    );

    assert_eq!(
        by_key_path(&def, &key(&["A/B ?#æ"])).unwrap(),
        "/v1/supplierInvoice/A%2FB%20%3F%23%C3%A6"
    );
}

#[test]
fn a_key_part_that_is_a_way_up_the_path_is_refused() {
    let def = def(
        Some("/v1/supplierInvoice"),
        PrimaryKey::Composite(vec!["documentType".into(), "referenceNumber".into()]),
    );

    for part in ["..", ".", "...", ""] {
        let error = by_key_path(&def, &key(&["Invoice", part])).unwrap_err();
        assert!(
            error.to_string().contains("path segment"),
            "{part:?}: {error}"
        );
    }
    assert!(
        by_key_path(&def, &key(&["Invoice", "1.5"])).is_ok(),
        "a dot inside a name is fine"
    );
}

#[test]
fn a_key_with_the_wrong_number_of_parts_is_refused() {
    let def = def(
        Some("/v1/supplierInvoice"),
        PrimaryKey::Composite(vec!["documentType".into(), "referenceNumber".into()]),
    );

    assert!(by_key_path(&def, &key(&["210507"])).is_err());
}

#[test]
fn an_endpoint_without_a_path_has_no_by_key_path() {
    let def = def(None, PrimaryKey::Single("referenceNumber".into()));

    assert!(by_key_path(&def, &key(&["1"])).is_err());
    assert!(list_path(&def).is_err());
}

#[test]
fn a_comma_separated_single_key_counts_as_several_fields() {
    let def = def(
        Some("/v1/x"),
        PrimaryKey::Single("documentType, referenceNumber".into()),
    );

    assert_eq!(
        key_fields(&def),
        fields(&["documentType", "referenceNumber"])
    );
}

#[test]
fn the_query_has_the_default_query_in_key_order_with_arrays_repeated() {
    let default_query = HashMap::from([
        ("expandNote".to_string(), json!("true")),
        ("expandApproval".to_string(), json!(true)),
        ("size".to_string(), json!(5)),
        ("tags".to_string(), json!(["a", "b"])),
        ("skipped".to_string(), Value::Null),
    ]);

    let pairs = query_pairs(&default_query).unwrap();

    assert_eq!(
        pairs,
        vec![
            ("expandApproval".to_string(), "true".to_string()),
            ("expandNote".to_string(), "true".to_string()),
            ("size".to_string(), "5".to_string()),
            ("tags".to_string(), "a".to_string()),
            ("tags".to_string(), "b".to_string()),
        ]
    );
}

#[test]
fn a_query_value_that_is_an_object_is_refused() {
    let default_query = HashMap::from([("bad".to_string(), json!({"a": 1}))]);

    assert!(query_pairs(&default_query).is_err());
}

// Cadence ---------------------------------------------------------------------------------

fn at(text: &str) -> DateTime<Utc> {
    text.parse().expect("RFC3339 timestamp")
}

fn state(when: &str, outcome: RunOutcome) -> Option<RunState> {
    Some(RunState {
        at: at(when),
        outcome,
    })
}

#[test]
fn an_endpoint_that_never_ran_is_due() {
    assert!(is_due(None, at("2026-10-12T10:00:00Z")));
}

#[test]
fn after_a_success_the_next_run_is_due_after_24_hours_and_not_before() {
    let last = state("2026-10-11T10:00:00Z", RunOutcome::Succeeded);

    assert!(!is_due(last, at("2026-10-12T09:59:59Z")));
    assert!(is_due(last, at("2026-10-12T10:00:00Z")));
}

#[test]
fn a_restart_straight_after_a_run_does_not_repeat_it() {
    let last = state("2026-10-12T10:00:00Z", RunOutcome::Succeeded);

    assert!(!is_due(last, at("2026-10-12T10:00:05Z")));
    assert!(!is_due(last, at("2026-10-12T11:00:00Z")));
}

#[test]
fn a_failed_or_partial_run_is_tried_again_after_two_hours() {
    for outcome in [RunOutcome::Failed, RunOutcome::Partial] {
        let last = state("2026-10-12T10:00:00Z", outcome);

        assert!(!is_due(last, at("2026-10-12T11:59:59Z")), "{outcome:?}");
        assert!(is_due(last, at("2026-10-12T12:00:00Z")), "{outcome:?}");
    }
}

#[test]
fn a_last_run_a_few_minutes_ahead_is_two_clocks_disagreeing_and_is_not_due() {
    for outcome in [RunOutcome::Succeeded, RunOutcome::Failed] {
        let last = state("2026-10-12T10:04:00Z", outcome);

        assert!(!is_due(last, at("2026-10-12T10:00:00Z")), "{outcome:?}");
    }
}

#[test]
fn a_last_run_dated_further_ahead_than_the_clocks_can_differ_did_not_happen() {
    for outcome in [
        RunOutcome::Succeeded,
        RunOutcome::Partial,
        RunOutcome::Stalled,
        RunOutcome::Failed,
    ] {
        let last = state("2026-10-13T10:00:00Z", outcome).unwrap();

        assert!(is_from_the_future(&last, at("2026-10-12T10:00:00Z")));
        assert!(
            is_due(Some(last), at("2026-10-12T10:00:00Z")),
            "{outcome:?}"
        );
    }
    let edge = state("2026-10-12T10:05:00Z", RunOutcome::Succeeded).unwrap();
    assert!(
        !is_from_the_future(&edge, at("2026-10-12T10:00:00Z")),
        "five minutes is within the tolerance"
    );
}

#[test]
fn a_stalled_run_waits_a_day_like_a_success_because_asking_again_gets_the_same_answers() {
    let last = state("2026-10-12T10:00:00Z", RunOutcome::Stalled);

    assert!(!is_due(last, at("2026-10-13T09:59:59Z")));
    assert!(is_due(last, at("2026-10-13T10:00:00Z")));
}

#[test]
fn the_marker_in_the_cadence_row_round_trips() {
    for outcome in [
        RunOutcome::Succeeded,
        RunOutcome::Partial,
        RunOutcome::Stalled,
        RunOutcome::Failed,
    ] {
        assert_eq!(RunOutcome::from_marker(Some(outcome.as_str())), outcome);
    }
    assert_eq!(
        RunOutcome::from_marker(None),
        RunOutcome::Succeeded,
        "a row without a marker says nothing went wrong"
    );
}

#[test]
fn a_marker_this_version_does_not_know_is_a_failed_run_retried_in_two_hours_not_a_success() {
    for marker in [
        "whatever",
        "",
        "Succeeded",
        " failed",
        "succeeded ",
        "paused",
    ] {
        let outcome = RunOutcome::from_marker(Some(marker));

        assert_eq!(outcome, RunOutcome::Failed, "{marker:?}");
        let last = Some(RunState {
            at: at("2026-10-12T10:00:00Z"),
            outcome,
        });
        assert!(!is_due(last, at("2026-10-12T11:59:59Z")), "{marker:?}");
        assert!(is_due(last, at("2026-10-12T12:00:00Z")), "{marker:?}");
    }
}

#[test]
fn a_run_is_recorded_under_its_own_name() {
    assert_eq!(run_label("customer_invoices"), "customer_invoices:open_set");
}

#[test]
fn notes_are_joined_in_the_order_they_were_made() {
    let mut report = OpenSetReport::default();
    report.note("first");
    report.note("second");

    assert_eq!(report.error.as_deref(), Some("first; second"));
    assert_eq!(report.differences(), 0);
}

#[test]
fn the_documents_that_differed_are_those_of_all_three_kinds() {
    let report = OpenSetReport {
        stale_in_staging: 2,
        missing_in_staging: 3,
        balance_differs: 4,
        ..OpenSetReport::default()
    };

    assert_eq!(report.differences(), 9);
}

#[test]
fn a_run_may_take_four_minutes_because_it_holds_the_tenants_lock() {
    assert_eq!(RUN_TIMEOUT, Duration::from_secs(240));
    assert_eq!(Limits::default().timeout, RUN_TIMEOUT);
}
