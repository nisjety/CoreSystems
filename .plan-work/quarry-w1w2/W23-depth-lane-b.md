# W23 depth findings, lane b

## W2-U31 (lane b)

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U31-1 | docs/ERROR_REASONS.md, openapi.yaml, CONTRACTS.md | The new 403 `details.reason = job_event_urls` on `follow=true` for a ZDR caller, and the redacted drain for ZDR, are not documented. | low | Add a row under the 403 section and a note on the three events routes (docs unit). |
| W23-b-U31-2 | crates/quarry-core/src/zdr_matrix.rs row `control_records` | Basis says a ZDR organization "reads them as before"; the event routes now redact for ZDR and refuse follow. The row stays Inert, but the basis text and the test citation are stale. | low | Reword the basis and cite `tests/job_follow_zdr.rs`. |
| W23-b-U31-3 | job_stream::redact_event_for_zdr | Allowlist drops non-allowlisted strings, including failure text of `run_failed`; a ZDR reader sees `type` only. | low | Add a closed `reason` vocabulary from control if ZDR readers need it. |
| W23-b-U31-4 | pipeline.rs ~1281 | Change event idempotency keys embed `final_url`; they reach control's event log and any non-redacted reader (non-ZDR callers, webhooks). | low | Hash the URL part of the key at the source. |
| W23-b-U31-5 | qtest-lane.sh | Every lane switch touches all sources, so interleaved lanes cost a full 10 min rebuild per run; a lane's run waited ~1.5 h behind other lanes. | process | Run all of a batch's lane-b tests in one call. |

## W2-U31 | security review

Blocker fixed in this lane (see the fix commit): `GET /v1/runs/:id/events` served the same job events unredacted to a ZDR caller (the redacted drain even hands out the `run_id`), and control's handler resolves the run by id without checking `org_id`. The edge now redacts the payload under the ZDR floor and returns 404 when any event's `org_id` is not the caller's (`tests/run_events_scope.rs`).

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U31-S1 | services/quarry-control/internal/resources/resources.go `GET /v1/runs/{id}/events` | Control resolves the run with `GetByRunID` and never compares the owner's org with `?org_id`, so any internal caller holding another org's run id reads its events. The edge now refuses mismatches (the W2-U31 security review fix), but control is the authority and should 404 itself. | medium | `if owner.OrgID != orgFromQuery(r) { 404 }`, same guard as `jobEvents`; a Go test with two orgs. |
| W23-b-U31-S2 | job_stream::ZDR_KEPT_STRING_KEYS | A kept string survives if it has no `://`. Today's producers put closed words in `reason`, `code`, `kind`, `status`, but nothing enforces that; a future producer writing a host or a page-derived string into a kept key (`reason: "dns error for host.example"`) passes. `content_type` and `lang` come from the remote server. | low | Also require a token shape for kept strings (for example `^[A-Za-z0-9_./+;= -]{1,64}$` and no dots for `reason` and `code`), and pin the producer vocabularies in a test. |
| W23-b-U31-S3 | auth::Claims::zdr_floor used by the event routes | The floor is the caller's token, so a service principal (floor never applies) of a ZDR org reads the org's job and run events in full. Consistent with `errors_for` and the matrix, but a service reader that re-serves events to a ZDR user carries the leak. | low | Document in the ZDR matrix basis; consider the org posture (X-9 event) rather than the token for read redaction. |
| W23-b-U31-S4 | resource_routes::forward_list / forward_one error path | A non-2xx control answer is returned to the caller as `control-plane GET {path} returned {status}: {body}`, echoing control's body (internal messages, ids) to the API caller. | low | Map control errors to the typed envelope without the raw body; log the body server-side only. |
| W23-b-U31-S5 | resource_routes::job_events (kind = None) | `/v1/jobs/:id/events` drain skips `ensure_job` and relies on control's `GetByOrg` in `jobEvents` for tenancy. Correct today; a regression in control would make it a cross-org read with no edge check. | low | Check each drained event's job id or call `ensure_job` for every kind. |

## W2-U25b depth rows
| D2-098b | W2-U25b | Agent events are now withheld from NATS entirely; the WS10.S4 redacted projection that would give Model Plane/Control a safe cross-plane copy is still not built. | crates/quarry-runtime/src/nats_event_bus.rs | other-plane | medium | WS10.S4 |
| D2-098c | W2-U25b | No test drives the real JetStream publish path (no broker in the test container); a bus seam (trait over the js publish) would let the gate be tested end to end. | crates/quarry-runtime/src/nats_event_bus.rs | test | low | - |

## W2-U25b | security review

No blocker. The gate is exhaustive over `EventType`, sits in `publish_encoded` ahead of both JetStream publishes, and `publish_event` takes the type from the event itself. No other in-repo producer writes `EventType` events to JetStream (usage meter publishes counts on `usage.>`; quarry-control only subscribes for GDPR; `fleet::subjects` are helpers with no publisher).

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U25b-S1 | crates/quarry-runtime/src/events.rs `EventSink::emit`; nats_event_bus.rs module doc | Agent-class events of a non-ZDR run still go through the durable `EventPublisher` (mpsc -> control's event log) and are readable through `GET /v1/runs/:id/events`. The new module doc says they reach only the SSE stream and the replay ring, which is not true. Combined with W23-b-U31-S1 (control does not compare the run owner's org on that route) the agent payloads (URLs, element labels, action arguments) share that route's cross-org exposure. | medium | Fix the doc; decide in D10 whether agent-class events belong in control's durable log at all, and close W23-b-U31-S1 before relying on that route for agent runs. |
| W23-b-U25b-S2 | nats_event_bus.rs `is_agent_class` call site; events.rs `with_nats`; agent_loop.rs `with_event_bus` | The gate lives in one transport (`NatsEventBus`). `EventSink::with_nats` and `AgentLoop::with_event_bus` accept any `dyn EventBus`, so a second cross-plane bus implementation would carry agent events raw with no test failing. | low | Apply the classification at the sink/loop boundary as well (or wrap any cross-plane bus in a gating decorator) and test it with a recording bus. |
| W23-b-U25b-S3 | QUARRY_EVENTS stream (7-day retention) | Agent events published before this fix stay on `quarry.run.<id>.agent_*` / `quarry.events.agent_*` etc. until retention expires; any subscriber with stream access can still replay them. | low | Ship a runbook step to purge the agent-class subjects from QUARRY_EVENTS when this reaches live. |
| W23-b-U25b-S4 | nats_event_bus.rs `both_publish_entry_points_pass_through_the_agent_gate` | The source scan anchors on the text `let primary_pub`; a new `js.publish` added above it under another name would pass the test. With no metric on the drop path, a regression is invisible in production too. | low | Anchor the scan on `.publish(` (first occurrence) and add a counter for withheld agent events; D2-098c's bus seam replaces the scan. |

## W2-U38 (lane b)

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U38-1 | internal/workflows/scheduled.go | The SDK settles a child future on parent cancellation only when the child closes. Before the U38 fix the cancel signal could never reach a running child (fixed). The real CrawlJobWF/ScrapeJobWF/BatchJobWF were not checked for how they react to the cancellation request plus SignalCancel together. | low | Add a test with the real CrawlJobWF as the child once the stack session can run it. |
| W23-b-U38-2 | internal/schedules/testdata/scheduled_job_crawl_history.json | The replay fixture is synthesized, not captured. It covers only the allowed-crawl path; skipped, failed, timed-out and cancelled fires have no recorded history. | low | Capture histories from a live fire per path and add them next to it. |
| W23-b-U38-3 | go.temporal.io/sdk testsuite | The test environment ignores a child's WorkflowExecutionTimeout, so the timeout path is only tested by returning a timeout error from the child. | low | Cover the real timeout in the stack session with a short ExecutionTimeoutS. |

## W2-U31 | security review (second pass, 2026-11-02)

No new blocker. The first pass's fix (`3a47a1b19`, run events redaction and org check) holds: the follow refusal comes before control, the permit and the replay parse; the drain redacts on all three job event routes, with the flag off and on; `/v1/runs/:id/events` redacts on both the Postgres and the control-forward path; the gateway's `ingestions/evidence.rs` reads run events through the edge, so it inherits the redaction.

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U31-S6 | resource_routes::job_events (drain path) | `claims.effective_zdr(false)` is first called after `legacy_events` has read control, so the request span is marked ZDR (the `zdr_log` filter) only after the control reads. Nothing in those reads logs a URL today (the path holds only the job id), but a future log line there escapes the filter. | low | Compute `let zdr = claims.effective_zdr(false);` once at the top of `job_events` and use it for both the follow refusal and the drain. |
| W23-b-U31-S7 | resource_routes::job_events / legacy_events | The drain takes no stream permit and buffers up to `LEGACY_MAX_PAGES` (200) pages of up to 1000 events in memory per request before the first frame; any caller of an org can open many such drains on a large job. Pre-existing, not ZDR, but it sits on the route this unit touched. | low | Stream page by page instead of buffering, or take the per-org permit on the drain too. |
| W23-b-U31-S8 | tests/job_follow_zdr.rs | The ZDR tests cover the user-token floor only. No test pins that an API-key token of a ZDR org (`api_key_token_zdr_floor_counts`) gets the 403 and the redacted drain on these routes, and none pins the service-principal behaviour recorded in W23-b-U31-S3. | low | Add the API-key and service claims cases to `job_follow_zdr.rs`. |

## W2-U64 (lane b)

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U64-1 | scheduled.go / CrawlInputFromParams | The scheduled-job child crawl builds its input with CrawlInputFromParams, not crawlInputFromJob, so the D2-073 refusal (absent scope with scope_mode enforce) is not applied to scheduled crawls; they still run legacy-unscoped. The dispatcher path is covered. Moving the check into CrawlInputFromParams (inputs.go) covers both. | medium | Next round: move the check to inputs.go with a test in inputs_test.go. |
| W23-b-U64-2 | activities.CrawlReport.ScopeApplied | ScopeApplied is a plain bool, so an absent field and an explicit false are the same value; the tolerant check treats both as 'not proven' (observe: go on unscoped, enforce: refuse). A *bool would tell a skewed edge from a broken one, and let the log name which. | low | Optional; needs crawl_types.go. |
| W23-b-U64-3 | CrawlJobWF tolerant path | A tolerated page leaves only a workflow log line (no run event), because a new event would change the event payloads. A crawl that ran unscoped through a skewed edge is visible in logs only, not in the run's terminal summary. | low | Decide whether the terminal event should carry a scope_unapplied_pages count (new field, observe only). |
| W23-b-U64-4 | quarry-edge crawl_request.rs scope_skew | The Rust rule is pinned by unit tests but nothing in the edge calls it: the edge never judges a report. It is the table both sides are tested against, not a runtime gate. | low | Wire it if an edge-side consumer of the report appears, else leave as the shared table. |
| W23-b-U64-5 | sed -i on CRLF files | MSYS sed -i rewrote three CRLF Go files to LF in the middle of the unit (caught by the CRLF count, restored). Anyone scripting edits on this tree should not use sed -i. | low | Already restored; a note for the other lanes. |
| W23-b-U65-1 | KnowledgeComposer crawl-options CSS | The new include/exclude text inputs reuse `dashboard-knowledge-composer__crawl-options` but only `input[type=number]` is styled in global.css, so the text inputs render with browser defaults. Needs a rule for `input[type=text]` (and a wider width). | low | Add the CSS rule in global.css in a UI polish unit. |
| W23-b-U65-2 | KnowledgeComposer pattern validation | Over-long and surplus patterns are dropped silently and invalid globs/regexes are only judged by the edge; the form gives no inline hint. The edge `requestWarnings` echoed in `CrawlEffectiveScope` are not shown anywhere in the composer. | medium | Show the handoff's effective scope and warnings on the crawl job row. |
| W23-b-U65-3 | CrawlPagePicker / crawlSelectedPages | The selected-pages crawl (batch) does not take the scope options and cannot take screenshots/PDFs. | low | Decide if a picked list needs include/exclude (probably not). |
| W23-b-U65-4 | Lane worktree node_modules | The lane has no node_modules and `pnpm install` fails on the Quarry SDK file: path (resolves under Frontend Plane/Ingestion Plane). A junction to the main checkout's node_modules was used for tests; it is untracked and not committed. | low | Fix the file: path or document the junction for lane runs. |
| W23-b-U58-1 | profile load failure wiring | `ProfileLoadError::restore_report()` exists but `load_first_tab_snapshot` (chromiumoxide.rs) does not attach it to the session, and `ProfileRestoreSummary` has no `load_failure` field, so the corrupt-load signal is still log-only on the wire. | medium | Add `load_failure: Option<String>` to the contract (serde default), store the report on `SessionPages`, merge it into the browser-session projection. |
| W23-b-U58-2 | cookie read-back live check | The `Storage.getCookies` read-back match (host spelling for host-only, domain and IPv6 cookies; partitioned cookies) is unit-tested with a mock only. A spelling mismatch would show as false `cookies_dropped_after_accept`. | medium | Run the live restore tests with Chromium and check the dropped count is 0 for a clean snapshot. |
