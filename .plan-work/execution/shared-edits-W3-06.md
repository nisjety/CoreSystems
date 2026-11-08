## EP-8-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: insert the line `pub mod locale_context;` immediately before `pub mod locale_hint;` (line 44 at this run). Required: the crate does not compile without it.

## EP-8-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs: (a) in invoke_field_names, after the line `        locale: _,` (line 270) add `        client_timezone: _,`, `        client_utc_offset_minutes: _,`, `        ui_locale: _,`. (b) in the returned vec, after `        "locale",` (line 316) add `        "client_timezone",`, `        "client_utc_offset_minutes",`, `        "ui_locale",`. (c) in FIELD_TREATMENTS, after `    ("locale", Treatment::Honored),` (line 161) add `    ("client_timezone", Treatment::Honored),`, `    ("client_utc_offset_minutes", Treatment::Honored),`, `    ("ui_locale", Treatment::Honored),`. Required: (a) is an exhaustive destructuring, so it is a compile error otherwise; the order in (b) and (c) must match because the table test compares the lists in order.

## EP-8-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs: in the make_request test helper, after `            locale: None,` (line 612) add `            client_timezone: None,`, `            client_utc_offset_minutes: None,`, `            ui_locale: None,`. Required for the test build (exhaustive struct literal).

## EP-8-S1
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs: append to the vec returned by cases(), after the persona_instructions entry and before the closing `    ]`: `        ("client_timezone", with("client_timezone", json!("Europe/Oslo")), None),`, `        ("client_utc_offset_minutes", with("client_utc_offset_minutes", json!(60)), None),`, `        ("ui_locale", with("ui_locale", json!("no")), None),`. Required: every Honored field needs a row, or every_field_has_a_row_that_matches_its_treatment fails.

## EP-8-S1
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/json_handlers.rs: after line 134 (`        shared::with_identity_context(outbound_body, &user.user_name, &org_name);`) insert `    let outbound_body = shared::with_locale_context(outbound_body);`, before the `let mut outbound_body` line.

## EP-8-S1
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ag_ui.rs: after line 126 (`    let body = shared::with_identity_context(body, &user.user_name, &org_name);`) insert `    let body = shared::with_locale_context(body);`, before the `let mut body` line.

## EP-8-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: after line 414 (`      MODEL_GATEWAY_ZDR_STREAM_COST: ${MODEL_GATEWAY_ZDR_STREAM_COST:-false}`) insert a comment line `      # EP-8-S1: the person's local date, time and app language in the prompt. Off by default; only the literal true turns it on. Nothing is stored.` and then `      MODEL_GATEWAY_USER_LOCALE_CONTEXT: ${MODEL_GATEWAY_USER_LOCALE_CONTEXT:-false}`. The file uses LF line endings.

## EP-10-S2
shared-edit docs/plans/model-plane-depth-register.md: close D2-718 with the commit. Cite the Go golden test (internal/api/write_operations_golden_test.go), the Rust contract (mp-contracts provider_writes.rs, tests/provider_write_operations.rs, execution-core catalog_writes_match_contract_table) and the two recorded mutation checks.

## EP-10-S2
shared-edit docs/plans/model-plane-chat-build-plan-2026-10-14.md: add the EP-10-S2 line to the Build ledger (built, sha pins, tests run, the two red package tests recorded above).

