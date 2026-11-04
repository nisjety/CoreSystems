# W2-Q-S4a status: WS15.S4a fire-time preflight (lane a, branch w23/lane-a)

## Built
All of the scope, code only, not compiled. The gap: `schedule_preflight.rs` was the S2 always-allow stub (posture `unchecked`, entitlement `floor`); route registration, auth, binding and validation already existed and are unchanged. Built into it:
- `OrgCorePosture`: org-core `GET /internal/orgs/:org/retention-posture` with `X-Service-Id: quarry-edge` / `X-Service-Token`, 3 s timeout, no cache (one read per fire); effective ZDR = `zdrIntent && entitled`. Any failure (timeout, unreachable, 401/403/404/5xx, decode, no credential) is `PostureRead::Unknown(class)` and answers `skipped_posture_unknown` (fail closed).
- Entitlement from `state.limits` (WS6.D `LimitsResolver`; the static source answers `floor`). A spent allowance for the kind (crawl -> crawl_pages, batch -> batch_urls) or a resolver refusal (fail mode closed) answers `skipped_no_entitlement`; `clamped_units` reports what is left.
- Closed vocabularies as consts: posture `unchecked|standard|zdr|unknown`, entitlement `floor|open|entitlement|unknown`, reasons `skipped_zdr, skipped_posture_unknown, skipped_no_entitlement, skipped_overlap, skipped_kind_disabled, skipped_disabled`.
- Response `{allow, reason?, posture, entitlement, zdr?, clamped_units?, observed_reason?}`; under `off` the wire shape is exactly S2's.
- Request accepts optional `target_kind` and `enabled` (serde default, so today's orchestrator body still decodes).
- `skipped_zdr` under `enforce` spawns `schedule_routes::pause_for_posture` (new `pub(crate)`, the same signed control forward as `pause_schedule`); `observe` logs `would_pause`.
- Compose (`apps/Ingestion Plane/docker-compose.yml`, quarry-edge): `QUARRY_EDGE__SCHEDULE_POSTURE` (default `observe`), `QUARRY_EDGE__ORG_CORE_URL` (default `http://org-core:8080`), `QUARRY_EDGE__ORG_CORE_SERVICE_TOKEN: ${QUARRY_ORG_CORE_SERVICE_TOKEN:-}` (W2-CP-R13-2 name). Mirror required: the token is minted in Control's ledger and must be copied into Ingestion's.
- Tests (in-file, run with qtest by the integration step): `preflight_table` (10 rows incl. unknown posture fails closed, intent-without-entitlement is standard, disabled/kind/zdr/entitlement precedence), `preflight_unknown_posture_fails_closed`, `preflight_denies_zdr_org_under_enforce`, `preflight_observe_mode_allows_and_logs`, `preflight_entitlement_floor_is_reported`, `preflight_never_caches_posture`, `preflight_off_mode_reads_nothing`, `s2_answer_wire_shape_is_unchanged`, `mode_parses_and_defaults_to_observe`, `request_without_optional_fields_still_decodes`, and two wiremock tests of the org-core client (credential headers, reads every time; 404/500/decode/unreachable are unknown).

## Commits (w23/lane-a)
- 40662f985 feat(quarry-edge): fire-time schedule preflight checks ZDR posture and entitlement (WS15.S4a, observe default)
- cc82ad342 feat(ingestion): compose env for the quarry-edge schedule preflight (WS15.S4a)
- docs(plan-work) commit with this file and the lane a depth rows

## Code only, not compiled
No cargo build or test was run (owner order). The integration step compile-checks.

## Flag and default
`QUARRY_EDGE__SCHEDULE_POSTURE=off|observe|enforce`, default `observe` (an unparseable value logs a warning and uses observe). Observe never denies and never pauses. Rollback: `off` (the S2 answer, no reads).

## Assumed decisions
- Mode, token and URL are read once from env into a `OnceLock` in `schedule_preflight.rs` (not an `AppState` field), so the unit stays inside its three files; no boot refusal of `enforce` without a token (depth D-S4A-2).
- Token env name on the edge is the plan's `QUARRY_EDGE__ORG_CORE_SERVICE_TOKEN`, fed from `QUARRY_ORG_CORE_SERVICE_TOKEN` (R13-2), as the profile-keys line already does.
- A 404 from org-core (org missing) is unknown, so fail closed.
- A `LimitsResolver` refusal (fail mode closed) maps to `skipped_no_entitlement` with entitlement `unknown`; a spent allowance counts only for crawl and batch (scrape and change_monitor have no metered allowance on `OrgLimits`).
- Check order: disabled, kind, ZDR, posture unknown, entitlement; the first reason wins.
- `skipped_kind_disabled` answers a sent kind outside `scrape|crawl|batch|change_monitor`; `skipped_overlap` is vocabulary only (the orchestrator writes it).
- The pause on `skipped_zdr` happens only under `enforce`, best effort, off the fire path.

## MP asks
None (no Model Plane surface). Nothing appended to MP-ASKS.md.

## Not verified
- Not compiled, no tests run, no compose render, no live org-core read.
- The orchestrator does not yet send `target_kind`/`enabled` nor read the new response fields (depth D-S4A-1); ChangeMonitorWF preflight (monitor-preflight-v1) is not part of this unit.
- `Quarry-v2/deploy/compose/docker-compose.yml` (plan [C] row) and RUNBOOKS/openapi not edited (outside the unit's files; depth D-S4A-4).
- Control's pause route accepting the edge's signed, user-less forward was assumed from `pause_schedule` (same forward), not checked live.
