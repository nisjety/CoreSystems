# W2 Rust lane: one run at HEAD a6a9b44c7 (2026-10-24)

The W2 close workflow only re-read the W1 test log for the Rust lane; this is the real run on the W2 tree: `cargo test --no-fail-fast -p quarry-core -p quarry-transform -p quarry-security -p quarry-runtime -p quarry-edge -p quarry-browser` through qtest (default features; Redis, Postgres and live Chromium suites skip).

**Result: 3560 passed, 64 failed over 94 result groups; 23 test targets failed (QTEST_RC=101).** Wave strategy: no fixing here; every failure is a depth item for the user to prioritise.

Failing targets: -p quarry-edge --test web_policy_agent; -p quarry-edge --test zdr_feature_matrix; -p quarry-edge --test zdr_gap_rows; -p quarry-edge --test zdr_refusal_modes; -p quarry-edge --test zdr_span_export; -p quarry-runtime --lib; -p quarry-transform --lib

## Failing tests (59 distinct)

- every_registered_route_has_a_matrix_row
- gap_rows_currently_violate_profiles
- observe_mode_serves_a_zdr_org_as_before_and_logs_one_content_free_event_per_action
- enforced_zdr_org_is_refused_every_durable_write_and_control_plane_sees_nothing
- enforced_standard_org_is_unchanged
- enforced_zdr_org_can_still_wind_down
- non_zdr_twin_exports_spans_and_keeps_no_contract_key
- observation::tests::an_explicit_screenshot_action_is_refused_when_the_image_is_withheld
- observation_redaction::tests::the_dom_summary_masks_shapes_and_keeps_the_fingerprint_id
- robots_gate::tests::query_aware_rules_deny_the_session_url
- pdf::tests::a_blank_page_is_empty_and_a_short_text_page_is_text
- pdf::tests::a_text_page_and_a_scanned_page_are_told_apart_in_page_order
- robots::tests::a_named_group_with_no_rules_allows_everything
- robots::tests::query_aware_rules_match_path_and_query
- capability_wire_tests::capabilities_flag_defaults_false_and_is_additive_on_the_wire
- chromiumoxide::bounded_calls_and_launch_dirs::cdp_calls_are_bounded
- session::tests::metadata_save_load_roundtrip
- stitch::tests::tile_plan_respects_max_tiles_and_marks_truncated
- crawl_scope::tests::denial_samples_cap_per_code_and_per_page
- agent_routes::tests::a_budget_refusal_comes_first_and_burns_no_single_use_approval
- crawl_request::tests::pattern_total_boundary_is_8192_characters
- policy_baseline::tests::require_claim_flag_defaults_off_and_parses_strictly
- routes::tests::a_malformed_body_and_a_bad_option_return_the_envelope_with_field_paths
- scrape_contract::tests::bad_option_values_come_back_as_field_errors
- step_refuses_evaluate_and_consumes_nothing
- interact_is_the_same_refusal
- a_human_label_does_not_open_the_door
- zero_max_pages_is_the_default_never_unlimited
- enforce_keeps_the_fixture_a_pages_and_reports_the_denials
- observe_fetches_the_disallowed_page_and_keeps_every_link
- enforce_refuses_a_disallowed_page_with_the_typed_403
- a_503_robots_txt_fetches_when_the_policy_is_allow
- bad_regex_is_a_400_with_the_exact_pattern_index
- an_allowed_page_reports_the_crawl_delay_in_milliseconds
- batch_pages_return_no_links
- ignore_and_a_request_without_crawl_do_not_touch_robots
- a_robots_txt_redirect_to_the_metadata_address_counts_as_unreachable
- without_crawl_the_response_has_no_report
- a_503_robots_txt_blocks_every_page_after_two_attempts
- observe_returns_every_href_and_reports_what_enforce_would_drop
- malformed_crawl_is_a_400_that_names_the_field
- runs::a_busy_run_does_not_block_the_drain
- a_run_grant_makes_a_users_run_an_agent_run_when_claims_are_enforced
- zdr_caller_can_cancel_but_not_read_errors
- cancel_is_org_checked_and_409_on_terminal
- the_workspace_stays_within_the_log_field_baseline
- enforce_denies_an_asked_domain_with_the_request_as_source
- import_over_cap_is_413
- attach_of_unknown_profile_id_creates_it_owned_by_caller
- second_user_cannot_claim_existing_profile_id_by_saving
- non_owner_gets_404_on_every_route
- org_shared_member_can_attach_but_not_rename_or_delete
- owner_can_use_user_private
- a_stale_type_ref_error_body_has_no_secret
- ws_action_receipt_is_masked
- receipts_endpoint_returns_masked_typed_text
- the_shutdown_drain_releases_every_idle_runs_session_and_leaves_a_busy_one
- the_browser_socket_refuses_the_same_capability_through_the_same_gate
- a_disabled_capability_is_refused_on_rest_step_and_consumes_nothing

Known unrelated compile error: `lab/evals/src/browser_benchmark.rs:221` misses `tab_pin: None` (WS8.E added the field); it breaks `cargo check --workspace`, not these package runs.

First reading (not verified): the failures cluster in WS7.C/WS7.D web-policy gates, WS14 crawl scope and robots (many `enforce_*`/`observe_*` tests), the ZDR refusal and matrix pins (`every_registered_route_has_a_matrix_row`: new W2 routes without matrix rows), profile guard and ownership tests (WS17.P0/P2), receipts masking, and PDF/robots/stitch unit tests: i.e. W2 units whose tests were written but never run until now. Each is a likely wrong expectation or an unwired piece, not necessarily a product bug.
