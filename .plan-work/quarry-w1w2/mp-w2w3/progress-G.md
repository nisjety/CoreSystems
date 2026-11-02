# Lane G progress (build agent G, mp-w2w3)

## Batch 6: WS8.G attach mode, model-gateway half (W2, tested)

Built (one file): `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs`
- `BrowserRunStartRequest.attach_session_id: Option<String>`.
- Flag `MODEL_GATEWAY_BROWSER_ATTACH` (`attach_flag`: only `1`/`true`/`on`, case-insensitive, trimmed). Off: a start carrying `attach_session_id` is refused 400 `browser_attach_unavailable` before any grant, thread or run write. A blank, oversized (>128) or non-printable id is 400 `attach_session_id_invalid`.
- `resolve_browser_grant` -> new `resolve_attach_grant` when attaching: ZDR parent -> 412 (`browser_run_zdr_refusal`); parent kind must be `session` or `human_browse` else 403 `browser_grant_not_derivable`; explicit non-empty hosts required (400), at most `MAX_HUMAN_BROWSE_RUN_DOMAINS` (8); session parent's host list must be canonical; human_browse parent uses the named hosts as the policy.
- `derive_run_grant`: for attach sends `purpose = "attach"` (`DERIVE_PURPOSE_ATTACH`), `resource_id = session id`, NO budget fields; accepts only a response of kind `attach` whose `resource_id` equals the session id (else 403). The derive uses the caller's browser bearer (same `authenticated_browser_request` as validate), and runs in `resolve_browser_grant`, i.e. before `prepare_managed_run_*` (thread/run write).
- `browser_derive_status_to_http` -> `attach_derive_refusal`: 412 `attach_zdr_refused`, 403 `attach_needs_owner`, 400 `attach_resource_invalid`.
- `insert_attach_fields`: tool_input gains `attach_session_id`; `profile_id` forced null. `grant_id` is already the derived attach grant. Run fingerprint includes the session id only when attaching.

Tests (all in the browser_run test module; names per spec):
`attach_derives_with_user_bearer_before_any_run_write`, `attach_requires_explicit_domains`, `attach_rejected_for_zdr`, `attach_rejected_when_parent_is_run_or_approval_kind`, `attach_derive_failure_creates_no_thread_or_run` (substring-matches the plan name `derive_failure_creates_no_thread_or_run`, which already exists for ordinary runs), `attach_tool_input_carries_attach_grant_and_session_id` (full handler via the keyed_start Harness), plus `an_attach_answer_for_another_session_or_kind_is_refused`, `the_brokers_attach_refusals_keep_their_names`, `the_attach_flag_is_off_unless_one_true_or_on` (default-off pin), `a_start_with_an_attach_session_is_refused_while_the_flag_is_off`, `attach_fields_ride_the_tool_input_only_when_attaching`, `a_retry_that_names_another_attach_session_is_a_different_request`.

Test runs (from `apps/Model Plane/rust`, `CARGO_TARGET_DIR=C:/dev/mp-w2w3-target-B CARGO_BUILD_JOBS=2`):
- `cargo test -p model-gateway --lib browser_run`: 93 passed, 0 failed (baseline 81 + 12 new).
- `cargo test -p model-gateway --lib zdr_posture`: 25 passed, 0 failed.

## Where the broker's code was followed over the spec
- The spec says errors.go "codes" `attach_zdr_refused`, `attach_needs_owner`, `attach_resource_invalid`. In the broker those strings are only metric outcome labels (`grantOutcome`); on the wire they are gRPC code + error text (FailedPrecondition / PermissionDenied / InvalidArgument). The gateway matches code AND exact text (`ErrAttachZdrRefused`, `ErrAttachNeedsOwner`, and server.go's "an attach grant is derived by a signed-in user only") and emits the names as the HTTP error codes. HTTP statuses chosen: 412, 403, 400.
- Budget: the broker's attach derive DOES accept the budget fields (same `budgetFromRequest` + `tightenBudget` as ai_run), but they are sent empty: the broker then gives the attach grant the parent's claim, and the gateway still tightens run params by the returned claim (+ minted claim when `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM=on`).
- Broker attach derive needs a `user` principal (server.go:348) and the attach TTL is capped at 15 min (`AttachMaxTTL`): a longer run than that outlives its grant (see DEPTH note below).

## Lead to land
- Compose (`apps/Model Plane/deploy/docker-compose.yml`, model-gateway environment, next to `MODEL_GATEWAY_BROWSER_POLICY_ASK`):
  ```
  # WS8.G, OFF by default: a browser run start may carry attach_session_id (steps on the caller's own
  # browser session under an `attach` grant derived with the caller's bearer). Off REFUSES such a start
  # (400 browser_attach_unavailable). `1`/`true`/`on` enables it. Needs broker BROWSER_BROKER_HUMAN_BROWSE_ENABLED
  # and Quarry QUARRY_EDGE__ATTACH on as well (R-29: a feature is on only when every layer is).
  MODEL_GATEWAY_BROWSER_ATTACH: ${MODEL_GATEWAY_BROWSER_ATTACH:-off}
  ```
- Ledger text (docs/decisions/ledger.md): "WS8.G (model-gateway half) implemented behind MODEL_GATEWAY_BROWSER_ATTACH (default off). A browser run start with attach_session_id derives an `attach` grant (purpose attach, resource_id = session id, no budget fields) with the caller's bearer before any thread/run write, from a non-ZDR session or human_browse parent only, with explicit hosts (<= 8). It dispatches execution-core's browser_agent with grant_id = the attach grant and tool_input.attach_session_id. Broker refusals map to 412 attach_zdr_refused / 403 attach_needs_owner / 400 attach_resource_invalid (matched by gRPC code + text; the broker's names are metric labels only). Decided: attach derive sends no budget claim (inherits the parent's)."
- ASK for MP-ASKS / DEPTH: attach grant TTL is capped at `AttachMaxTTL` = 15 min by the broker; a run longer than that loses its grant mid-run unless the loop renews it (renew is not wired for attach).

## Not verified
- No live broker/Quarry run; the three-layer enablement is untested end to end. The execution-core half (reads `attach_session_id`) is another lane.
- Handler tests set `MODEL_GATEWAY_BROWSER_ATTACH=1` in the process env (never set off by any test).
- `tab_mark_on_complete` passthrough (WS8.J) NOT added: not in this slice's prompt.
