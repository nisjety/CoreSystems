# Zero Data Retention: what Quarry does today

Generated from `crates/quarry-core/src/zdr_matrix.rs`; do not edit by hand.
`QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix` rewrites this file and the JSON golden.

This describes behaviour. It does not change it. A **gap** is a place where ZDR is not honoured today.

| Treatment | Meaning |
|---|---|
| honored | the feature runs and nothing about the request is retained |
| inline only | the result is in the response only; nothing is stored |
| degraded | the feature runs with less than it has outside ZDR |
| refused | the request is refused with a message naming ZDR |
| inert | ZDR does not apply: nothing of the caller's content is stored, or the route is not reachable with ZDR |

6 of 30 rows are gaps.

| Feature | Routes | Treatment | Status | Log |
|---|---|---|---|---|
| `scrape` | `/v1/scrape` | inline only | enforced | canary `zdr_log_capture::a_zdr_scrape_logs_no_url_and_no_title_even_at_debug` |
| `scrape_stream` | `/v1/scrape/stream` | honored | enforced | canary `zdr_log_capture::a_zdr_streamed_scrape_logs_no_url_from_its_spawned_runner` |
| `scrape_record_read` | `/v1/scrape/{id}` | refused (`FORBIDDEN`, `scrape_record_read`) | enforced | log-unverified |
| `durable_jobs` | `/v1/crawl`<br>`/v1/batch` | refused (`FORBIDDEN`, `durable_job`) | enforced | log-unverified |
| `search` | `/v1/search` | honored | enforced | canary `zdr_log_capture::a_zdr_search_with_a_failing_provider_logs_no_query` |
| `search_verticals` | `/v1/search/images`<br>`/v1/search/videos` | inert: a provider call and a usage count only; the query is not cached, logged or stored | enforced | log-unverified |
| `search_similar` | `/v1/search/similar` | honored | enforced | log-unverified |
| `search_suggest` | `/v1/search/suggest` | degraded: the partial query is sent to the Model Plane without the ZDR posture | **gap**: unassigned: forward the posture, or refuse under ZDR | log-unverified |
| `answer` | `/v1/answer`<br>`/v1/answer/stream` | honored | enforced | log-unverified |
| `extract` | `/v1/extract` | honored | enforced | log-unverified |
| `map` | `/v1/map` | inert: reads robots and sitemaps and returns links; nothing is stored | enforced | log-unverified |
| `audio` | `/v1/audio` | honored | enforced | log-unverified |
| `artifacts` | `/v1/artifacts`<br>`/v1/artifacts/{id}` | inert: reads artifacts that already exist; a ZDR run writes none | enforced | log-unverified |
| `control_records` | `/v1/sources`<br>`/v1/sources/{id}`<br>`/v1/snapshots`<br>`/v1/{kind}/jobs`<br>`/v1/request-queues`<br>`/v1/benchmarks`<br>`/v1/team/credit-usage`<br>`/v1/team/token-usage`<br>`/v1/team/concurrency`<br>`/v1/team/queue-status`<br>`/v1/team/activity`<br>`/v1/runs/{id}/events`<br>`/v1/jobs/{id}/events` | inert: reads or proxies Control Plane records; Quarry stores nothing for the caller | enforced | log-unverified |
| `change_history` | `/v1/change/check`<br>`/v1/change/latest`<br>`/v1/change/history` | degraded: a change check keeps a baseline and history for the URL | **gap**: WS4.H: refuse under ZDR, observe first | log-unverified |
| `change_snapshot` | `/v1/change/snapshot`<br>`/v1/change/refresh` | degraded: promoting a snapshot and scheduling a refresh both retain the page | **gap**: WS4.H: refuse under ZDR, observe first | log-unverified |
| `change_record_internal` | `/v1/internal/change/record` | degraded: the change event and webhook are suppressed; the baseline is still stored | **gap**: WS4.H stage B: with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on, a body that declares zdr is refused before the baseline is stored; the flag default stays off until ADR-4 sign-off and the customer notice | log-unverified |
| `schedules` | `/v1/schedules`<br>`/v1/schedules/{id}`<br>`/v1/schedules/{id}/pause`<br>`/v1/schedules/{id}/unpause`<br>`/v1/schedules/{id}/trigger`<br>`/v1/schedules/{id}/backfill` | degraded: a schedule is a durable resource; a ZDR organization can create and run one | **gap**: WS4.H stage B: with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on, create, trigger, backfill and unpause are refused and list, get, pause and delete stay open so an organization can wind down; the flag default stays off until ADR-4 sign-off and the customer notice | log-unverified |
| `profiles` | `/v1/profiles`<br>`/v1/profiles/create`<br>`/v1/profiles/{id}`<br>`/v1/profiles/{id}/restore_probe` | degraded: a browser profile (cookies, storage) is a durable resource a ZDR organization can create | **gap**: WS4.H stage B: with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on, create, save and update are refused and list, read, restore_probe and delete stay open so profiles can be erased; the flag default stays off until ADR-4 sign-off and the customer notice | log-unverified |
| `agent_run` | `/v1/agent/runs` | honored | enforced | canary `zdr_log_capture::a_zdr_agent_start_logs_no_url` |
| `agent_persistent_profile` | (same route as another row) | refused (`BAD_REQUEST`, `persistent_profile`) | enforced | log-unverified |
| `agent_resume` | (same route as another row) | refused (`FORBIDDEN`, `resume`) | enforced | log-unverified |
| `agent_run_surface` | `/v1/agent/runs/{run_id}`<br>`/v1/agent/runs/{run_id}/step`<br>`/v1/agent/runs/{run_id}/interact`<br>`/v1/agent/runs/{run_id}/browser-session`<br>`/v1/agent/runs/{run_id}/browser-session/control`<br>`/v1/agent/runs/{run_id}/browser-session/timeline`<br>`/v1/agent/runs/{run_id}/egress-receipts`<br>`/v1/agent/runs/{run_id}/tabs`<br>`/v1/agent/runs/{run_id}/tabs/{tab_id}`<br>`/v1/agent/runs/{run_id}/tabs/{tab_id}/select`<br>`/v1/agent/runs/{run_id}/devtools`<br>`/v1/agent/runs/{run_id}/frames/ws`<br>`/v1/agent/runs/{run_id}/frames/stream`<br>`/v1/agent/runs/{run_id}/frame`<br>`/v1/agent/runs/{run_id}/live-view`<br>`/v1/agent/runs/{run_id}/events/stream` | honored | enforced | log-unverified |
| `agent_receipts_and_procedures` | `/v1/agent/runs/{run_id}/receipts`<br>`/v1/agent/runs/{run_id}/procedure`<br>`/v1/agent/procedures/replay-check`<br>`/v1/agent/procedures/quality-check`<br>`/v1/agent/procedures/impact-check` | inert: reads receipts a ZDR run never wrote; the checks are pure functions of the request body | enforced | log-unverified |
| `fleets` | `/v1/fleets`<br>`/v1/fleets/{fleet_id}`<br>`/v1/fleets/{fleet_id}/members`<br>`/v1/fleets/{fleet_id}/budget` | inert: an in-memory registry; nothing is persisted, and the console is hidden (WS2.E) | enforced | log-unverified |
| `graphql` | `/graphql`<br>`/graphql/schema`<br>`/graphql/playground` | inert: a read-only API over records that exist | enforced | log-unverified |
| `operations` | `/health`<br>`/ready`<br>`/version` | inert: process state; carries no caller content | enforced | log-unverified |
| `internal_run_page` | `/v1/internal/run_page` | honored | enforced | log-unverified |
| `internal_page_images` | `/v1/internal/page-images/{org}/{doc}/{page}/{hash}` | inert: serves a stored page image by content hash; a ZDR run stores none | enforced | log-unverified |
| `internal_queues` | `/v1/internal/queues/{queue_name}/enqueue`<br>`/v1/internal/queues/{queue_name}/pop`<br>`/v1/internal/queues/{queue_name}/ack` | inert: the worker queue behind crawl and batch, which refuse ZDR at the public edge | enforced | log-unverified |

## Basis

- `scrape`: no artifact, page-cache entry, scrape record or durable event; the text comes back in the response (default format markdown), cut at max_inline_bytes Tests: `zdr_claim_floor::scrape_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service`, `zdr_log_capture::scrape_meter_metadata_under_zdr_has_no_url_or_fingerprint`, `routes::tests::a_zdr_scrape_has_no_scrape_id_and_stores_no_artifact`.
- `scrape_stream`: the run's posture is the request flag raised by the token floor; page events go to live subscribers only Tests: `zdr_claim_floor::scrape_stream_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service`.
- `scrape_record_read`: scrape_routes::get_scrape refuses a reader whose organization is under ZDR, even for a record written before it was Tests: `routes::tests::a_zdr_floored_reader_is_refused_even_for_an_earlier_record`.
- `durable_jobs`: routes::refuse_durable_job_under_zdr: the job record and per-page results would be retained Tests: `zdr_claim_floor::durable_crawl_and_batch_are_refused_for_a_zdr_org_user_and_open_otherwise`, `zdr_gap_rows::enforced_refusals_hold_for_durable_jobs`.
- `search`: paid providers closed, search cache bypassed, no durable SearchIssued event; the provider failure is logged as a class only Tests: `zdr_claim_floor::a_user_token_from_a_zdr_org_runs_search_as_zdr_even_when_the_body_says_false`, `zdr_claim_floor::a_user_token_from_a_zdr_org_runs_search_as_zdr_when_the_body_omits_the_flag`.
- `search_verticals`: search_routes::images and ::videos touch no store; meter_vertical_search records user, kind and result count, not the query
- `search_similar`: the page fetches run with the token's ZDR floor (effective_zdr_mode); the request has no zdr field of its own
- `search_suggest`: search_routes::suggest calls the Model Plane and never reads the token's ZDR floor Tests: `zdr_gap_rows::gap_rows_currently_violate_search_suggest`.
- `answer`: the answer pipeline runs with the effective ZDR posture: no paid providers, no durable search event Tests: `zdr_claim_floor::answer_runs_as_zdr_for_a_zdr_org_user_and_as_before_otherwise`, `zdr_claim_floor::answer_stream_runs_as_zdr_for_a_zdr_org_user_and_as_before_otherwise`.
- `extract`: the page runs carry the effective ZDR posture, so no artifact or durable page event is written Tests: `zdr_claim_floor::extract_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service`.
- `map`: map_routes::map touches no cache, artifact store, index or event sink
- `audio`: audio_routes::raise_to_zdr_floor forwards the posture to the Model Plane, which owns retention for audio Tests: `zdr_claim_floor::audio_forwards_the_floored_posture_to_the_model_plane`, `zdr_claim_floor::audio_transcription_forwards_the_floored_posture_to_the_model_plane`.
- `artifacts`: resource_routes list and get are tenant-bound reads of the artifact store
- `control_records`: resource_routes forwards to Control Plane with the verified identity and keeps no copy; EXCEPT POST /v1/sources with monitor:true, which registers a recurring change-monitor schedule in Control Plane and is classified by the schedules row (it is refused or observed under the same flag and stamps the posture on the schedule)
- `change_history`: change_routes::check calls zdr_refusal::refuse behind QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES (default off: observe, zdr.would_refuse, served as before); latest and history are reads and stay open Tests: `zdr_gap_rows::gap_rows_currently_violate_change_history`.
- `change_snapshot`: change_routes::snapshot and ::refresh call zdr_refusal::refuse behind QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES (default off: observe, zdr.would_refuse, served as before) Tests: `zdr_gap_rows::gap_rows_currently_violate_change_snapshot`.
- `change_record_internal`: record_internal calls zdr_refusal::refuse on the body's declared zdr before the store (flag on: 403; flag off: zdr.would_refuse and the baseline is stored as before); emit_change_side_effects suppresses the change event and webhook for a declared zdr in both modes. The orchestrator declares the posture quarry-control stamped on the schedule at create (migration 023); a schedule created before that (NULL) or before its org turned ZDR on declares nothing or false and is stored (pinned by zdr_posture_stamp::record_trusts_the_stamp_not_the_current_posture); the orchestrator also declares the stamp on the run_page fetch that precedes the record call, so a stamped-ZDR monitor's fetch runs in ZDR mode and persists no page artifact, event or local-index write-back; ChangeMonitorWF still emits run_started and change_detected with the URL. No test observes the baseline write, because the harness wires no baseline store
- `schedules`: schedule_routes call zdr_refusal::refuse on create, unpause, trigger and backfill (default off: observe, zdr.would_refuse, forwarded as before); POST /v1/sources with monitor:true is covered by this row; the caller's posture is stamped on the change-monitor schedule at create (quarry-control migration 023) from the verified claims, raised (never lowered) by a zdr:true declared in the body. Known gaps: an org that turns ZDR on later keeps its stamp, the legacy non-preset create is not stamped Tests: `zdr_gap_rows::gap_rows_currently_violate_schedules`.
- `profiles`: profile_routes call zdr_refusal::refuse on create, save and update (default off: observe, zdr.would_refuse, served as before); the agent start refuses a persistent profile in both modes Tests: `zdr_gap_rows::gap_rows_currently_violate_profiles`.
- `agent_run`: a ZDR run writes no checkpoint, timeline row or receipt and logs under a ZDR span Tests: `agent_routes::tests::zdr_expiry_writes_no_timeline_row_and_no_checkpoint`.
- `agent_persistent_profile`: same route as agent_run: lifecycle::start_run refuses persist_profile or a profile id under ZDR Tests: `zdr_claim_floor::a_zdr_org_users_agent_run_cannot_take_a_persistent_profile`, `zdr_gap_rows::a_service_principal_with_body_zdr_gets_the_same_persistent_profile_refusal`.
- `agent_resume`: same route as agent_run: lifecycle::start_run refuses a resume of a ZDR checkpoint (none is written)
- `agent_run_surface`: every step, tab and control change skips receipts and timeline rows when the run is ZDR (step.rs, control.rs, projection.rs) Tests: `agent_routes::tests::zdr_expiry_writes_no_timeline_row_and_no_checkpoint`.
- `agent_receipts_and_procedures`: receipts.rs lists from the receipt store; replay, quality and impact checks hold no state
- `fleets`: fleet_routes keep fleets in the process (new_fleet_state)
- `graphql`: the GraphQL schema resolves queries from existing stores and mutates none
- `operations`: routes::health, ready and version
- `internal_run_page`: routes::internal_run_page builds the runner with ZdrMode from the orchestrator's request
- `internal_page_images`: resource_routes::get_page_image reads the content-addressed store
- `internal_queues`: queue_routes are internal and carry no ZDR flag; nothing ZDR can reach them through the public routes Tests: `zdr_claim_floor::durable_crawl_and_batch_are_refused_for_a_zdr_org_user_and_open_otherwise`.
