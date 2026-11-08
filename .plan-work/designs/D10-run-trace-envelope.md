# D10 run-trace-envelope: one canonical run trace (extend `model_plane.v1.Event`, Quarry publishes into it, real SSE replay, request ids, run origin) and ZDR-safe telemetry phase 2

Author: Sonnet design agent D10, wave W2. Date of reading: 2026-10-15. Code read from the LIVE tree `C:/dev/CoresSystem` (afb83928 plus ~324 uncommitted entries). Audit: `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Provenance tags come from `tools/provenance.sh`: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked, `[NEW]` the plan would create it. Path shorthand used below (each is a prefix of `C:/dev/CoresSystem/`):

- `Q/` = `apps/Ingestion Plane/Quarry-v2/`; `QE/` = `Q/crates/quarry-edge/src/`; `QR/` = `Q/crates/quarry-runtime/src/`; `QC/` = `Q/crates/quarry-core/src/`; `QCT/` = `Q/services/quarry-control/`.
- `MP/` = `apps/Model Plane/`; `MPR/` = `MP/rust/`; `MPG/` = `MP/go/`; `SC/` = `MPR/services/session-core/src/`; `MGW/` = `MPR/services/model-gateway/src/`; `EXC/` = `MPR/services/execution-core/src/`.
- `GW/` = `apps/Frontend Plane/verevonv3/apps/gateway/src/`; `SPA/` = `apps/Frontend Plane/verevonv3/src/`; `AP/` = `apps/Application Plane/`.

## 1. Items resolved and wave

| Item | Part resolved here | Wave | Verified verdict carried in |
|---|---|---|---|
| F-t2-014 (one canonical run trace) | The **envelope slice** only: extend `model_plane.v1.Event` with `producer_seq`, agent and origin fields; Quarry publishes browser-session, step-receipt, egress-decision and artifact events in it on NATS (persisted unless ZDR); `x-request-id` intake as a tower layer in quarry-edge; a run origin enum; real `from_seq` replay for Quarry's agent-event SSE; the Application Plane projection and the ADR topic 8 recommendation; cross-plane contract tests, volume and ordering. | W2 | "reword": extend, do not replace |
| F-t2-004 (ZDR-safe telemetry) | **Phase 2 only**: OpenTelemetry sampler that drops ZDR spans, bounded late-ZDR trace registry plus span processor, documented safe/sensitive log-field contract mirrored in Rust and the Go `quarryotel` package, explicit exporter-disable flag. Phase 1 (field sweep, log filter, lint) is D4 slices S-4..S-6 and is a hard predecessor. | W1 item, phase 2 ships with F-t2-029 (W2) | "reword": live exposure is log fields, spans barely exist |
| F-t2-029 (metrics, spans) | Only the **link**: the ZDR-safe span contract that every future span vocabulary must follow, and the order of work (log sweep, then sampler and registry, then spans). Metrics recorder, RunStatistics, drain, admission and alerts are not designed here. | W2 | n/a |

Out of scope on purpose (assumed owned elsewhere, and this design states the contract they consume): the card's "first slice" (gateway proxies for `receipts`, `egress-receipts`, `procedure`; devtools level and substring filters; the read-only `browser.console` model tool; SPA egress and procedure panels; the FleetRunConsole fix). The one gateway route this design needs itself is the agent-events SSE proxy, because Quarry's replay is only reachable by a browser through it.

Honest sizing: the card says L. Adding the F-t2-004 phase 2 work, the real total is about **47 engineer-days**, which is XL by the repo's usual scale. The Quarry-only slices S1 to S4 (about 16 days) already deliver user-visible value (request ids, real replay, origin, safe reference events) without waiting for Model Plane persistence.

## 2. Verified current state

### 2.1 Model Plane: what the "canonical envelope" really is today

- `MP/proto/model_plane/v1/events.proto` [C]: `Event` is lines 12 to 56 (fields 1 to 13 = event_id, event_type, schema_version, ts, producer, correlation_id, causation_id, idempotency_key, org_id, user_id, resource_ref, payload as `google.protobuf.Any`, zdr). `reserved 14 to 20;` at line 55. `EventType` is lines 59 to 185; browser values 140 to 146 at 172 to 181 (`EVENT_TYPE_BROWSER_ACTION_DISPATCHED` through `EVENT_TYPE_APPROVAL_CONTINUATION_VERIFIED`); `reserved 200 to 255;` at 184. The header comment (10 to 11) says every event on NATS or in the events table uses this shape. **[DRIFT]** That is only partly true: on the wire the shape is a JSON twin, not protobuf bytes. Rust `MPR/crates/mp-events/src/envelope.rs` [C] (struct 10 to 52, `zdr: bool` with `#[serde(default)]` at 49 to 50) and Go `MPG/pkg/envelope/envelope.go` [C] (struct 26 to 60, `Zdr *bool` so "undeclared" survives) serialise the same field names as snake_case JSON. The tool-completion consumer reads ad hoc keys in both spellings (`SC/nats.rs` [C] `handle_message` 128 to 193).
- A second, separate run stream exists: `OrchestrationEvent` (a `oneof`, tags 10 to 23, `MP/proto/model_plane/v1/orchestration.proto` [C] 1010 to ~1060) is what the chat panel renders. Execution-core publishes it with the additive `RecordOrchestrationEvent` gRPC (`EXC/browser_events.rs` [C] 1 to 120); session-core fans it out per run with an in-memory bounded replay buffer keyed by ULID (`SC/orchestration_grpc.rs` [M] `broadcast_event` 1762 to 1783) and persists a subset into the `events` table (`persist_orchestration_event`, INSERT at 1735, `ON CONFLICT (id) DO NOTHING`). **[DRIFT]** the sweep cited the browser tags as 17 to 22; tag 23 (`approval_continuation_verified`) also exists.
- `MP/proto/model_plane/v1/eventlog.proto` [C]: `service EventLog` (26 to 38) with `Cursor` (78 to 81) and `EventFilter` (58 to 73). **[CLAIM-FALSE as an implementation claim]**: no server exists, and `MPR/crates/mp-contracts/build.rs` [C] does not list `eventlog.proto` in `mp_protos` (the list at 36 to 56 has events, gateway, sessions, runs, capabilities, sandboxes, browser, memory, inference, execution, orchestration, verified_outcome, proof_bundle, finetune, routing_policy), so no Rust trait is even generated. `MPR/crates/mp-eventlog` [C] (a 31-line `lib.rs` plus `cursor.rs`, `filter.rs`, `idempotency.rs`; `Cursor` payload is `{ offset: u64 }`) has no dependents. It is exactly the missing server-side half and it fits an `ingest_seq` cursor.
- The only persisted run-event read path today: model-gateway `GET /v1/runs/:run_id/events/replay` (`MGW/http_routes.rs` [M] route at 218, handler `replay_run_events` 1278 to 1380) calls session-core `replay_thread` (`SC/grpc.rs` [M] 4952), which runs `ORDER BY e.ts ASC, e.id ASC` with the cursor predicate `e.id > $2` (`replay_thread_task`, 4018 to 4031). **[FINDING]** ordering key (ts, id) and cursor key (id) differ, so a page boundary can skip events whose ULID sorts below the cursor but whose ts is later; producers whose ids are not ULID-monotonic against `ts` (any non-session-core producer, or clock skew) make this real. The handler also keeps only the 7 browser types and requires `payload.run_id` to equal the run (1346 to 1357). The `events` columns (migration `SC/../migrations/0002_events_and_ordinals.sql` [C]) have no producer sequence, agent or origin.
- `events` rows are written only with a `runs` row present (`INSERT ... SELECT ... FROM runs r WHERE r.id = $n`, `SC/nats.rs` 157 to 189; `orchestration_grpc.rs` 1735). A browser session with no Model Plane run (a human browsing in the gateway) therefore cannot live in that table.
- NATS topology is deployment-owned: `MPG/services/nats-provisioner/main.go` [C] `streamConfigs()` 24 to 62 (`TOOLS_COMPLETIONS`, `MP_ORCHESTRATION_EVENTS`, `MODEL_PLANE_RUN_EVENTS` 48 h, audit, usage) and `consumerBindings()` from 64; broker permissions per principal in `MP/deploy/nats.conf` [C] (users from line 12). Model-nats has **no Quarry principal**; the pattern for a scoped external producer exists (`tool-completion-producer`, nats.conf 113 to 121). `mp.v1.run.<id>.event` is low-volume and has consumers that must not be flooded: capability-core (RUN_COMPLETED skill learning), insight-core, audit, and Convex (`AP/convex-core/nats-subscriber.js` [C] 331 to 341, 569 to 605 upserts `agentRuns`).
- Contract tests already guard this topology: `MPG/services/nats-provisioner/publishers_registry_test.go` [C] (a registry of every JetStream publisher with principal, subject, stream, source file and needle) and `manifest_contract_test.go` [C] (`TestEveryJetStreamPublishCallSiteIsRegistered` 418, `TestPublisherPrincipalsMayPublishTheirSubjects` 621, `TestEveryNATSClientModuleIsAccountedFor` 448). A new publisher that is not registered fails CI. The registry's `source` is relative to `MP/`, so a Quarry publisher needs a small harness extension (S5).
- Model Plane already has a different "origin": `threads.origin IN ('chat','space','agent_run','support','system','mail_task')` (`SC/../migrations/0034_thread_origin.sql` [C], widened by 0041). **[DRIFT]** The audit's run origin (user, agent, schedule, webhook, api) is a *trigger* vocabulary; thread origin is a *surface* vocabulary. They are different facts and must not be merged (decision D11).
- Generated-code locations (from `MP/proto/buf.gen.yaml` [C]): Go `MPG/gen/model_plane/v1/events.pb.go` [C] (managed by `buf generate`, paths=source_relative, also `eventlog.pb.go`, `eventlog_grpc.pb.go`); Python `MP/python/mp-events-py/src/mp_events/_gen/model_plane/v1/events_pb2.py` [C]; TypeScript into the legacy `apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/` [C] (not verevonv3; whether it is still built is [UNVERIFIED], check before regenerating). Rust is **not** checked in: `mp-contracts/build.rs` runs `tonic_build` at build time. `MP/proto/baseline.binpb` [C] is **0 bytes** and `.github/workflows/buf.yml` [C] runs `buf lint apps/Model Plane/proto` with an unquoted path, so both buf gates are inert (S9 headline H5).
- Rust compile-fix sites for new `Event` fields (struct literals, [UNVERIFIED] which are the proto type): `SC/grpc.rs` `replay_event_row_to_proto` 3967 to 4003 (full literal, ends `zdr: false`), `MGW/http_routes.rs` [M], `MGW/sse.rs` [M], `MGW/sse_events.rs` [M], `MPR/crates/mp-contracts/tests/proto_wire_parity.rs` [C]. Hand-written type map: `SC/grpc.rs` `event_type_to_i32` 7184 to 7204 (unknown strings become 0).
- GDPR: org erasure deletes table by table in `SC/gdpr.rs` [C] (`events` at 350 to 360, runs at 362); any new table must be added there.

### 2.2 Quarry: what the event machinery really does

- Two envelopes plus a third on the bus: `quarry_core::event::Event` (`QC/event.rs` [C] 84 to 96: `event_id` (`evt_<ULID>`), `run_id`, `job_id`, `type`, `ts`, `seq: u64`, `payload`, `idempotency_key`), the Go mirror (`Q/pkg/quarrycontracts/event.go` [C]), and the Model Plane `Event`. `docs/gap-quarry.md:347` documents the first as "the canonical envelope".
- **[DRIFT]** The `seq` is **process-global, not per run**: `EventSink` holds one `Arc<AtomicU64>` (`QR/events.rs` [C] 24, 38) incremented on every emit (74, 118) for all runs. A client sees gaps and can never tell loss from interleaving.
- **[FINDING]** The same logical event gets different identity on the two paths. The sink mints `event_id` and global `seq` for the SSE and mpsc paths (`events.rs` 63 to 110); `NatsEventBus::publish` (`QR/nats_event_bus.rs` [C] 191 to 205) builds a *new* `Event` with a new `event_id` and its own per-run counter (`next_seq` 127 to 143, a `HashMap` that is never garbage-collected). SSE and NATS disagree on both fields. The sink also fans out to NATS in a `tokio::spawn` per event (`events.rs` 90 to 99), so the bus path can reorder; its own comment says per-subject ordering is not enforced.
- **[FINDING, exposure]** Agent-class events reach Ingestion NATS with their raw payload. `ActionStarted` carries `serde_json::to_value(&request.action)` (`QR/observation.rs` [C] 386 to 396), i.e. typed text and URLs; `ObservationReady` carries the full page `url` (1000 to 1013). `emit_for_zdr` (`events.rs` 137 to 152) correctly keeps ZDR runs off the bus, but for non-ZDR runs these land on `quarry.run.<id>.<type>` and the aggregate `quarry.events.<type>` in stream `QUARRY_EVENTS` (7 days, `nats_event_bus.rs` 145 to 157). The module header (16 to 20) says payloads must not carry scraped content: **[CLAIM-FALSE as practised]** for typed text. The only consumer of `quarry.events.*` is autocomplete-core (`Ingestion Plane/autocomplete-core/src/routes.rs` [C] 291 to 293); whether it ignores agent types is [UNVERIFIED]. F-t2-021 owns capture-time redaction; this design must not widen the exposure and closes it for trace (D7, D10).
- `ActionRuntime::emit_action_event` (`QR/action_runtime.rs` [C] 195 to 208) uses `.emit()` (not `emit_for_zdr`) and puts `result.error` text in the payload. Whether `ActionRuntime` is reachable from a ZDR request today is [UNVERIFIED]; D2 notes F-t2-023 will wire it into scrape. The single-emitter rule in D8 removes this bypass.
- SSE: `agent_events_stream` (`QE/agent_routes.rs` [C] 2658 to ~2770). `AgentEventsQuery` (936 to 942) accepts `from_seq` and `max_events`; the replay is a no-op: `history_rx` is `None` and `let _ = &query.from_seq;` at 2705. **[CLAIM-FALSE]** The doc comment (2640 to 2657) says replay comes from Postgres `quarry_event_history`; the only durable history is `quarry_job_history` of `JobHistoryEvent` rows (`QR/postgres_event_history.rs` [C] 1 to 12, 38 to 58), a different type for crawl and job progress. `Last-Event-ID` is ignored; `id:` is the global `seq`; the broadcast capacity is 256 (`events.rs` 19) so a `lagged` event cannot be backfilled. Only 7 types pass `is_user_facing_event` (949 to 962).
- **[FINDING]** `EventSink::unsubscribe` is never called outside the in-process `InProcessEventBus` (grep over QE and QR), and `close_run` (`agent_routes.rs` 3562 to 3655) does not call it. The per-run broadcast sender therefore lives forever in `subscribers`, the `RecvError::Closed` arm that emits `done: run_closed` is unreachable in production, and the map grows by one entry per observed run.
- The stream requires a live run (`agent_runs` lookup then `caller_owns_live_run` 484, 2680 to 2700), so after `close_run` it is 404: replay after close must come from persisted history, which today is the owner timeline and receipts.
- Emit sites for the agent run: `AgentStarted` (`agent_routes.rs` 1642 to 1650), `AgentCompleted` (3637 to 3645), `ActionStarted` and `ObservationReady` (`observation.rs` 386, 1000), `ActionFailed` (`agent_loop.rs` 271, 328; the loop wiring `with_event_bus` is test-only, lines 559, 637, 689). Step receipt creation and the ZDR guard: `agent_routes.rs` 2338 to 2375 (`if !entry.zdr.is_active() { receipts.append(..) }`), checkpoint write 2377 to 2386.
- Durable owner state that already exists and must not be duplicated: receipts (`0007`, `0008`), checkpoints (`0006`, JSONB `state`, so new serde fields need no migration), timeline events (`0009_browser_timeline_events.sql`, index on (org_id, actor_id, run_id, occurred_at, event_id)); `BrowserTimelineEventKind` (`QR/step_receipts.rs` [C] 130 to 160), `StepReceipt` (183 to 210), `AgentRunCheckpoint` (39 to 71).
- Request ids: `RequestKind::new()` is called **65 times** in quarry-edge (`agent_routes.rs` 22, `resource_routes.rs` 18, `change_routes.rs` 8, `profile_routes.rs` 7, `schedule_routes.rs` 6, `routes.rs` 4); `Envelope.meta.request_id` (`QC/envelope.rs` [C] 16 to 19). No `x-request-id` intake exists in quarry-edge or in the gateway (`GW/` grep: only domain-local `request_id` for chat and AG-UI). quarry-control has one (`Q/services/quarry-control/internal/httpx/httpx.go` [C] 24 to 34, mounted at `cmd/control/main.go:144`). Layer order in `QE/routes.rs` [M] 297 to 306: `.layer(TraceLayer)` (303), `.layer(body_limit)` (304), `.layer(timeout)` (305); the later `.layer` is the outer one, so a request-id layer must be added after `TraceLayer` to be visible to its span.
- Run origin: none. `StartRunBody` (`agent_routes.rs` 346 to 383) has no origin or trace field. D2 adds a per-step `initiated_by` (human, agent, system) which is a *different* fact (who is acting now versus how the run was started).
- Gateway: browser router `GW/domains/browser.rs` [C] 368 to 460 (no events or receipts routes); `proxy_sse_stream` (`GW/upstream.rs` [C] 1628) already carries `Last-Event-ID` (header set at 1782 to 1784) and the pattern for an owner-checked SSE proxy is `stream_live_frames` (`browser.rs` 1730 to 1782: `owner_browser_session_projection` then `proxy_sse_stream`). Upstream request builders set `x-user-id` at `upstream.rs` 1579, 1777, 2883 and in about 20 domain-local builders (`browser.rs` 3 sites, `chat/shared.rs`, `search.rs`, ...); none forwards a request id.

### 2.3 Telemetry (F-t2-004 phase 2 baseline)

- `QE/telemetry.rs` [C] (64 lines; declared in `main.rs` only, not in `lib.rs`): `init_telemetry` 25 to 56 installs `fmt().json()` always and the OTLP layer only when `OTEL_EXPORTER_OTLP_ENDPOINT` is non-empty (29 to 33), `SdkTracerProvider::builder().with_batch_exporter(..)` with the default sampler (`try_init_otel` 58 to 64). **[CLAIM-FALSE, partly]** the card says both layers are unconditional; only the JSON layer is. No `zdr` string anywhere in the file. Versions: opentelemetry 0.29, opentelemetry_sdk 0.29, tracing-opentelemetry 0.30 (`Q/Cargo.toml` [C] 92 to 95).
- `Q/pkg/quarryotel/quarryotel.go` [C] (101 lines): `Init` 39 to 93, no-op provider when the endpoint is empty, `sdktrace.ParentBased(sdktrace.AlwaysSample())` at 80; **no tests**; not in the CI Go matrix (`quarry-v2-ci.yml` [C] matrix lists quarry-control, quarry-orchestrator, quarrycontracts at 245 to 249 and 313 to 317). Callers: quarry-control `main.go:39`, orchestrator `main.go:34`; no `tracer.Start` anywhere under `services/` or `pkg/`.
- Model Plane `MPR/crates/mp-telemetry/src/lib.rs` [C] (75 lines) uses the **older** OpenTelemetry 0.26 API (`opentelemetry_sdk::trace::TracerProvider`, `Config::default().with_resource`); `MPR/Cargo.toml` [C] pins opentelemetry 0.26. A sampler written for Quarry's 0.29 does not compile there. The gateway has **no OpenTelemetry dependency at all** (`GW/../Cargo.toml` [C]): its phase 2 is empty, only D4's log filter applies.
- Posture carriers that already exist and must be reused (D4 3.10 agrees): token floor plus body flag (`QE/auth.rs` [C] 121 to 138, service claim ignored 104 to 120), `emit_for_zdr`, the NATS envelope `zdr` key (`MPG/pkg/natsx/publisher.go` [C] 56 to 60). D4 hands this design the `zdr` span field and `ZdrMarkLayer`, the single-file `SENSITIVE_FIELDS` constant (to be pinned by a golden), the `error_class` vocabulary and the lint baseline.
- **[UNVERIFIED, decides the registry's shape]** When tracing-opentelemetry 0.30 asks the SDK sampler to decide for a tracing span. If it builds the OTel span when the tracing span *closes*, a `zdr` field recorded after creation is visible to the sampler and processor; if at creation, it is not. S8 starts with a spike test that pins this (D14).

### 2.4 Uncommitted ([M]/[U]) files in the area, and whether in-flight diffs conflict

| File | Tag | In-flight hunks (live) | Conflict with this design |
|---|---|---|---|
| `QE/main.rs` | [M] | `+47`, `+244..253`, `984`, `1022`, `1061..1232`, `1395`, `1463` | NATS wiring (405 to 445) and shutdown (84 to 111) are untouched; S5 adds one block next to 405 to 445. Low. |
| `QE/lib.rs` | [M] | one line at 18 (`pub mod map_routes;`) | S1, S4, S8 each add a `pub mod`; trivial rebase. |
| `QE/routes.rs` | [M] | one test line at 1329 | None for 297 to 306 (TraceLayer region). Real hotspot is with D4 S-5 (`make_request_span`) and D2. |
| `QE/config.rs` | [M] | 187 lines (vector budget at 51 to 62, rerank at 327) | Avoided on purpose: new knobs live in an env-only module (as D2 D13). |
| `QE/state.rs`, `QR/lib.rs`, `QE/tests/support/mod.rs` | [M] | small (`QR/lib.rs` +42, +160) | S3 adds exports to `QR/lib.rs` and a field/test helper in `support`; rebase only. |
| `Q/docs/openapi.yaml` | [M] | in flight | S3 and S4 add the SSE path and fields; shared with D2, D5. |
| `MPR/services/session-core/src/grpc.rs` | [M] | many hunks, none in 3967 to 4045 or 7184 to 7204 (nearest: `MemoryRetention` at ~4101 to 4160) | S2 and S6 edit exactly those untouched ranges. Low. |
| `SC/orchestration_grpc.rs` | [M] | hunks at 1188, 1944, 2259, 2651, 4355, 6078 | `persist_orchestration_event` (1735) is clear; S6c (optional dual-write) sits between two hunks. Low-medium. |
| `SC/main.rs` | [M] | one `mod` line at 35 | S6 adds `mod trace_nats; mod eventlog_grpc;` and two spawns. Trivial. |
| `MGW/http_routes.rs` | [M] | many (824 to 4906) | Route table 218 and `replay_run_events` 1278 are clear; S6 adds one route and one handler. Low. |
| `MGW/sse.rs`, `MGW/sse_events.rs`, `EXC/grpc.rs`, `MGW/browser_run.rs` | [M] | many | Only compile-fix literals (S2). Add `..Default::default()`; avoid reformatting. |
| `SC/../migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` (+ `revert/*.down.sql`) | [U] | whole files | **Hard**: S6's migration is `0046_run_trace_events.sql` and references `runs.retention_class` (`'zdr'`, `'persistent'`, `'zdr_waived'`) from 0044. They must be committed first. |
| `scripts/tests/zdr-contract-sync.test.sh`, `scripts/tests/zdr-contract-schema.mjs` | [M] | 95 and 24 changed lines | S2, S8 add GOLDENS and VENDORED rows; the 28 untracked fixtures (S9 H1) must land with it. |
| `.github/workflows/model-plane-rust-ci.yml`, `verevonv3-ci.yml` | [M] | extra jobs | S6 adds a Postgres-backed `trace_` job modelled on the uncommitted `session-core-receipts` job (lines 127 to 169); S1 gateway tests ride the existing gateway job. |
| `Ingestion Plane/docker-compose.yml` | [M] | switch defaults 129, 152 | S5 adds three env lines to the quarry-edge service; no overlap with the changed defaults. |
| `MP/api/contracts/zdr-refusals.json` and Rust twin | [U] | whole files | Adjacent only: ZDR refusal vocabulary (D4 3.8). No dependency here. |

No in-flight diff touches the ranges this design edits in `QR/events.rs`, `QR/nats_event_bus.rs`, `QE/agent_routes.rs`, `QR/step_receipts.rs`, `Q/pkg/quarryotel/quarryotel.go`, `MP/proto/**`, the nats-provisioner or `deploy/nats.conf` (all [C]). The conflicts that matter are with sibling designs (section 5).

## 3. Design decisions

Tags: **ADR-GATED n** means the audit's section 9.3 ADR topic n must be ratified by the user before the work ships to production; "proceeds on the recommended default" means building can start now because the default is the audit's own recommendation and is additive or reversible.

### D1. One envelope: extend `model_plane.v1.Event` (ADR-GATED 8, part 1; proceeds on the recommended default)

- Options: (a) add fields to the existing `Event` (the audit's recommendation and ADR topic 8); (b) define a new `TraceEvent` message (the card's first summary); (c) adopt Quarry's `quarry_core::event::Event` as the cross-plane shape and map Model events into it.
- **Recommendation: (a).** Rationale: `Event` already carries `event_id`, `schema_version`, `producer`, `correlation_id`, `causation_id`, `org_id`, `user_id`, `resource_ref`, `zdr` and a reserved block for exactly this purpose; both JSON twins, the Go and Python generated types, the `events` row mapping and the golden wire test exist; a second envelope would need a second twin pair and a mapping between them. (b) duplicates 9 of 13 fields. (c) makes the Model Plane depend on an Ingestion Plane type and lacks `org_id`, `user_id`, `producer`, `zdr`.
- Scope limit: this is the envelope for the **run trace**. `OrchestrationEvent` (the oneof stream the chat panel renders) is not replaced; it keeps serving live Model-side progress. Merging the two streams is explicitly not attempted (decision D13 and section 9).

### D2. Field allocation inside the reserved block (proceeds; ledger entry required)

Exact diff in S2. Summary: `producer_seq = 14` (uint64), `agent = 15` (`AgentRef`), `origin = 16` (`RunOrigin`), `run_id = 17` (string), `request_id = 18` (string); `reserved 19 to 20;` stays. New `EventType` values 150 to 156. Options for numbering: use the front of the reserved range (chosen), or leave the range intact and use 21 and up. The reserved range was created "for future envelope fields" (events.proto:54), so using it is the intended path; the only risk is that `buf breaking` (rule set `FILE`) may flag shrinking a `reserved` range as `RESERVED_MESSAGE_NO_DELETE` **[UNVERIFIED]**. Resolution: run it locally against a regenerated baseline first (S2a repairs the gate); if it fires, add a one-line `ignore_only` for that file and rule in `MP/proto/buf.yaml` with a comment citing the ledger entry; the fallback numbering (21 to 25) has identical semantics. Record the allocation in `MP/docs/decisions/ledger.md` (newest first), following D1's convention of allocating by convention rather than adding `reserved` statements that block other designers.

### D3. `producer_seq` semantics: dense, per (run_id, producer), 0 means "not numbered"

- Options: global per org; per run across all producers; per (run, producer); per (run, producer, agent) as in Firecrawl (monotonic per emitting agent, consumers group by agent id and then order [SRC, contract only]).
- **Recommendation: per (run_id, producer), starting at 1, dense, assigned by the producer.** A producer is a service class (`quarry-edge`, `session-core`, `execution-core`), not an agent. `agent` is grouping metadata for the UI. A gap in a producer's sequence means events were lost between producer and consumer (queue overflow, broker outage), which is the whole point: it makes loss visible. There is no cross-producer total order; consumers merge by `ts` with `causation_id` as an anchor and show the ordering as "approximate across producers". Per-agent sequences would need agent identity on every producer and would hide loss when an agent id is empty; Quarry does not know agent ids unless the caller says so.
- `0` means the producer does not number its events (every existing producer): consumers must not treat 0 as a gap or an order.
- Resume: Quarry stores the last assigned sequence in the run checkpoint (`AgentRunCheckpoint.trace_seq`, serde-default, no migration) so a resumed run continues the sequence; if the checkpoint is lost, the resumed run restarts numbering at 1 and the consumer's unique key (S6) rejects the collisions instead of silently overwriting, which is the correct loud failure.

### D4. What "run" means in the envelope: the Model Plane run; Quarry events without one are not published

- Quarry has its own run id (`run_<ULID>`). The trace's `run_id` (field 17) is the **root Model Plane run id** when the Quarry run was started by a Model Plane service; `resource_ref` carries `browser_run/<quarry run id>`; `correlation_id` equals the Model run id (the session-core default) so existing consumers that group by correlation keep working.
- A Quarry run started by a human in the gateway has no Model run. Its events stay in Quarry's owner timeline plus live SSE (already owner-scoped) and are **not** published to the Model Plane (nothing to attach them to; the `events` and `run_trace_events` tables are keyed by run). This is a deliberate scope statement, not a gap: the card's "one run timeline" is the AI-run timeline.
- Trust: the trace context (`run_id`, `agent`) is honoured only from a service principal (`claims.is_service()`); a user token cannot set it. Even then it is a *label*: the consumer re-checks `runs.id = run_id AND runs.org_id = org_id` (S6) and rejects mismatches, so a mislabelled event cannot land in another org's or another run's trace. It never feeds an authorization decision in Quarry.

### D5. Transport: a scoped Quarry principal on Model-nats, a new subject and stream (ADR-GATED 8 touches ownership; credential needs a security review)

- Options: (a) publish on Ingestion NATS `quarry.>` and bridge to Model-nats; (b) Quarry holds a second NATS client for Model-nats with a publish-only principal limited to one subject family; (c) Quarry calls session-core `RecordOrchestrationEvent` over gRPC; (d) reuse `mp.v1.run.<id>.event`.
- **Recommendation: (b), subject `mp.v1.trace.run.<model_run_id>`, new stream `MODEL_PLANE_RUN_TRACE`.** Reasons: (a) adds a bridge service nobody owns and keeps raw payload risk on the Ingestion bus; (c) is a cross-plane write RPC into session-core's storage path, which blurs "Quarry never writes Model Plane storage" and puts a synchronous dependency on the step path; (d) would flood capability-core, insight-core, audit and Convex, which all subscribe to that subject and ignore everything but lifecycle. (b) has precedent: `tool-completion-producer` (`nats.conf` 113 to 121) and the Ingestion publisher principal on the Application broker (`AP/nats.conf` [C] 116 to 121, `ingestion-integration-publisher` publishes `verevon.ingestion.>` only). The principal is `quarry-trace-producer`: publish `mp.v1.trace.run.*` only, subscribe `_INBOX.QUARRY_TRACE_PRODUCER.>` only (needed for the JetStream PubAck), no `$JS.API` rights, so it cannot create or alter topology and cannot publish `mp.v1.run.*.event`, `tools.completions.*` or audit subjects. Quarry's run id token must be a single subject token; the publisher validates it (`[A-Za-z0-9_-]{1,64}`).
- Ordering and loss: one bounded queue and one publisher task per process, so publish order equals sequence order (the existing `NatsEventBus` spawns a task per event and cannot promise this); a full queue drops the new event and counts it, which shows up as a `producer_seq` gap downstream.

### D6. Persistence: a new `run_trace_events` table behind `EventLog.QueryEvents`, not the `events` table and not `replay_thread`

- Options: (a) write trace events into `events` and extend `replay_run_events`; (b) new table in session-core, implement the existing `EventLog.QueryEvents` contract over it with `mp-eventlog`'s cursor; (c) Quarry serves the persisted trace from its own tables.
- **Recommendation: (b).** (a) inherits the `ts`/`id` cursor flaw (section 2.1), puts hundreds of high-churn rows per run into a table the compaction and ordinal triggers also read (`SC/compaction.rs`, `assign_event_step_ordinal`), and has no place for the new columns without altering a large table. (c) makes Quarry the owner of the run trace, contradicting the plane rule (Model Plane owns runs and sessions). (b) gives an `ingest_seq BIGINT GENERATED ALWAYS AS IDENTITY` cursor that is strictly monotonic per commit order, a `UNIQUE (run_id, producer, producer_seq)` that makes redelivery idempotent and duplicates detectable, a bounded payload, a retention sweeper and GDPR coverage in one place, and finally gives `mp-eventlog` (Cursor `{offset}`, `EventFilter`) and `eventlog.proto` their first server. Deviation to record: `eventlog.proto`'s service comment says it is a facade over the `events` table; the ledger entry amends it to "facade over the run trace log".
- Model-side events (browser action dispatched, observation, approvals) join the same table by an optional dual-write at `persist_orchestration_event` (S6c) so the single read returns the whole story; until then the read merges nothing and the SPA keeps its existing run-event sources for Model-side events.

### D7. What Quarry publishes, and in what shape

- Quarry publishes only what the Model Plane cannot know: `BROWSER_SESSION_OPENED`, `BROWSER_SESSION_CLOSED` (with `reason`: closed, expired, failed, cancelled; the D2 sweeper's `agent.expired` maps here), `BROWSER_CONTROL_CHANGED`, `BROWSER_TAB_CHANGED`, `BROWSER_STEP_RECEIPT_RECORDED`, `BROWSER_EGRESS_DECIDED`, `BROWSER_ARTIFACT_RECORDED`. It does **not** republish `ActionStarted` or `ObservationReady`: execution-core already emits the Model-side dispatch and observation events (`EVENT_TYPE_BROWSER_ACTION_DISPATCHED` 140, `EVENT_TYPE_BROWSER_OBSERVATION_RECEIVED` 141), and duplicating them would double the volume and the leak surface.
- Payloads are **reference-based and allowlisted per type** (exact field lists in S4). They never contain typed text, full URLs, page text, DOM, console text, cookies, headers or error messages. The only page-derived field is `url_origin` (scheme, host, port), present only on non-ZDR events. The allowlist is applied at one function (`trace_projection::to_model_envelope`), by dropping unknown fields rather than by redacting known bad ones, so a new payload field is invisible to the Model Plane until someone adds it to the allowlist and the golden.
- Egress decisions are summarised, not streamed: one `BROWSER_EGRESS_DECIDED` event per step with `{allowed, blocked, egress_seq_from, egress_seq_to}` plus at most 20 individual `block` events per step (category only, origin only). The per-request detail stays in the owner-only `GET /v1/agent/runs/:id/egress-receipts` (up to 512 per observation, `QR/observation.rs` 589 to 602).
- Payload cap 4096 bytes serialised; an over-cap payload is replaced by `{ "truncated": true }` plus the id fields, never partially cut.

### D8. One mint, per-run sequence, a bounded replay ring: refactor `EventSink`, do not add a second bus

- Today three mints exist (global seq in the sink, per-run seq in `NatsEventBus`, event id in both). The refactor makes `EventSink` the **only** minter of `event_id`, `seq` (now per run for agent runs opened with `open_run`) and `ts`; the NATS bus and the new Model publisher receive the already-minted `Event`; legacy callers (crawl, scrape, search events) that never call `open_run` keep today's global counter, so no per-run map grows for runs that never close.
- Per-run state `RunTrace { next_seq: AtomicU64, ring: Mutex<VecDeque<Arc<Event>>>, meta: RunTraceMeta }`. Emission order is: lock ring, assign `seq`, push, broadcast `send`, unlock, then the `.await` points (mpsc, publisher queue). A reader subscribes to the broadcast first, then snapshots the ring under the same lock; events are therefore in the snapshot, on the receiver, or both, and the reader de-duplicates by `seq`. This is what makes replay-then-tail gap-free without a database.
- Replay semantics: `from_seq` (query) or `Last-Event-ID` (header; query wins if both) means "events with seq greater than N". If N+1 is older than the oldest retained event the stream first emits `event: replay_gap` with `{requested, oldest_retained, fallback: "browser-session/timeline"}` and then continues from the oldest retained event. Ring size 512 events and 2 MiB per run by default.
- `close_run` (and the D2 sweeper through its single `finish_run`) calls `EventSink::close_run`, which removes the broadcast sender and the ring; subscribers then see `Closed` and the existing `done: run_closed` branch finally fires, and the map stops growing (fixes the section 2.2 leak).
- Options rejected: a Postgres table of `Event` rows for replay (`quarry_job_history` is a different type; a new table is only needed for replay after the run is gone, and that is served by the owner timeline, receipts and, for AI runs, the Model trace); Redis streams (new dependency).

### D9. ZDR: live-only everywhere; the in-memory ring is allowed; the consumer fails closed (ADR-GATED 4, confirmation of the default)

- ZDR runs publish nothing to NATS (the publisher refuses at its door and `emit_for_zdr` already routes them to the live path), write no `run_trace_events` row, and their step receipts are not appended (existing guard, `agent_routes.rs` 2360). The only thing a ZDR run keeps is the bounded in-memory ring, which holds the same events the 256-slot broadcast channel already holds in memory (`events.rs` 19) and which is dropped at `close_run` and at process exit. This lets a ZDR user's flaky SSE reconnect without a gap. Recommended default: allowed; the ADR 4 matrix (D4 S-7) records it as `browser_trace_replay: in-memory, live run only`. If ADR 4 prefers "refused", the ring is not created for ZDR runs and ZDR reconnects simply resume the live tail (a one-line change in `open_run`).
- Defence in depth at the consumer: session-core accepts a trace event only when the JSON carries an **explicit** `"zdr": false` key (the Rust `Envelope` twin defaults an absent key to `false`, which is the unsafe direction; the Go twin keeps a pointer for this reason), and also skips persistence when `runs.retention_class = 'zdr'` (migration 0044 [U]). An absent or `true` flag is terminated and counted, not retried.
- Telemetry (F-t2-004 phase 2) follows the same rule: ZDR runs export zero spans (D14).

### D10. Stop putting raw agent payloads on the Ingestion bus (proceeds; owner to confirm autocomplete-core)

Exclude the agent-class event types (`AgentStarted`, `AgentCompleted`, `AgentFailed`, `ActionStarted`, `ActionCompleted`, `ActionFailed`, `ObservationReady` and the new trace types) from `NatsEventBus::publish`, because the Model publisher is now the governed path and the Ingestion subjects carry raw action JSON and URLs for 7 days. Keep them on the in-process SSE and the owner stores. The only observed consumer of `quarry.events.*` is autocomplete-core, which ingests search and host types; confirm it ignores agent types (a one-line check in its handler) before merging. This is the smallest correct fix and removes the dual-mint problem for agent events (D8).

### D11. Run origin is a trigger enum, derived, descriptive, and never an authorization input

- `RunOrigin`: `USER` (a human action in a Verevon surface), `AGENT` (a Model Plane agent or another agent started it), `SCHEDULE` (cron or routine), `WEBHOOK` (an inbound event such as mail-task), `API` (programmatic service caller). Proto adds `RUN_ORIGIN_UNSPECIFIED = 0` for lint and for "unknown".
- Derivation in Quarry (`QC/trace.rs` [NEW], `derive_origin`): a user token yields `USER`, or `AGENT` when the caller hints `agent` (tighten-only in spirit, the same rule D2 uses for `initiated_by`); other hints from a user token are ignored. A service token yields its hint (any of the five) or `API` when absent. Quarry stores it on the run entry, the checkpoint (serde default), the `Lifecycle` timeline event and every trace event; it never reads it in `admission`, budgets or policy (the source-scan test `origin_is_not_authz` in S4 enforces that).
- Mapping for Model Plane callers (owned by sibling (b), listed so both sides agree): chat, space, support and agent-run-console turns by a human → `USER`; capability-core cron and routines (`0028_space_scoped_cron.sql`) → `SCHEDULE`; background review and dreaming → `AGENT`; mail-task (`mail_task` thread origin) → `WEBHOOK`; direct service calls → `API`. Thread `origin` stays the surface vocabulary and is not touched.
- Go mirror: constants in `Q/pkg/quarrycontracts` (S4). Persisting origin on Go job rows (crawl, batch, schedule) is a follow-up that needs a quarry-control migration whose number must be allocated at integration (014 is the latest, 015 is already claimed by D4 S-10); it is not needed for the agent-run trace.

### D12. Request id: adopt if valid, else mint; never an authorization, tenancy or idempotency key

- Rule: accept an inbound `x-request-id` only if 1 to 128 bytes and every byte in `[A-Za-z0-9._:-]`; otherwise mint `req_<ULID>` (`RequestKind::new()`); never reply 400 for a bad id. Always echo `x-request-id` in the response, record it as the span field `request_id`, store it in `Envelope.meta.request_id`, forward it on every internal call, and put it on trace events (`request_id`, field 18).
- Because ids are caller-chosen they are **not unique across tenants**; support lookups always filter by (org_id, request_id). Never used for idempotency (Firecrawl's provider-tool routes do reuse it as a retry key [SRC]; Verevon does not). A kill switch `QUARRY_EDGE__REQUEST_ID_ADOPT=0` (default 1) makes the edge always mint, for an incident where a client abuses the header.
- Mechanism: an `axum::middleware::from_fn` layer plus a `tokio::task_local!` so the 65 existing `RequestKind::new()` sites can be replaced mechanically by `request_id::current()` without threading an argument. Spawned tasks lose the task-local and mint their own id (acceptable; background work is not a request).
- Gateway: its own layer mints or adopts, echoes the header, sets the span field, and forwards through a small helper at the central builders (`upstream.rs` 1579, 1777, 2883) and the browser and chat domain builders first; the other ~20 domain-local builders adopt in a follow-up sweep.

### D13. Application Plane projection and Space sharing: present the options and recommend (ADR-GATED 8, part 2)

| Option | What it is | Verdict |
|---|---|---|
| A | Model Plane `run_trace_events` is canonical; the Application Plane keeps a **coarse, debounced projection** in Convex (last activity, control mode, pending approval, blocked-egress flag, approximate counters) fed by its own JetStream consumer on `mp.v1.trace.run.*`; per-event realtime tail stays a direct owner-checked SSE; sharing a run with a Space is a read-through that re-checks the owning plane at read time | **Recommended** |
| B | Convex becomes the realtime store of the full trace and the UI subscribes with `useQuery` | Rejected: write amplification (a run is 100 to 200 events), payload copies in a second store, and a second ACL |
| C | A Space grant copies or re-publishes trace events into the Space | Rejected: widens the owning plane's ACL by copy; violates matrix rule 10 ("Application projections only narrow access", `apps/master-ownership-matrix.md:166`) |
| D | No projection | Acceptable fallback; dashboards and notifications then poll the REST trace |

- Option A keeps one authority per fact: Model Plane for the trace, Quarry for proof (receipts, artifacts), Control Plane for membership. The projection stores ids and counters only and uses the existing `requireViewerMembership` (`AP/convex-core/convex/authz.ts`). It tolerates loss by design (latest-wins fields, counters marked approximate) and carries no payloads, so ZDR is trivially safe (nothing is published for ZDR runs).
- Space sharing is not designed here (sibling (a), F-t2-072, resource-taxonomy ADR). The constraint this design places on it: a Space member's read of a run trace must be authorised by the owning plane at read time through a Control Plane membership decision; the Application Plane may cache that decision only to narrow it.
- Slice S7 is therefore ADR-GATED 8 and blocked until the user answers section 9 question 1.

### D14. F-t2-004 phase 2: contract first, flag, sampler, registry, processor; spans only after (ADR-GATED 4 for the ZDR-per-feature wording)

- Order, from the audit's dependency note 8 and D4: (1) D4 S-4..S-6 log sweep, carrier and lint (**hard predecessor**); (2) S8a contract plus exporter flag (no spans needed); (3) S8b/S8c sampler, late-ZDR registry, span processor in Rust and Go, proven with synthetic spans; (4) only then F-t2-029's span vocabulary. If D4 keeps its custom request span at INFO level, enabling OTLP would start exporting a request span per request including ZDR ones; the sampler must therefore land before `OTEL_EXPORTER_OTLP_ENDPOINT` is set in any environment, or the span stays at DEBUG. Both are stated as acceptance checks.
- Sampler (Rust, `QE/otel_zdr.rs` [NEW], in `lib.rs` so integration tests can import it): `ZdrSampler` implements `opentelemetry_sdk::trace::ShouldSample` and returns `Drop` when the attribute `zdr` is `true`, when the parent context's trace id is in the registry, or when `QUARRY_TELEMETRY_EXPORT=off`; otherwise delegates to `ParentBased(AlwaysOn)`.
- Late-ZDR registry: `ZdrTraceRegistry`, a bounded **FIFO** (not LRU) set of trace ids, capacity 100,000 (env `QUARRY_TELEMETRY_ZDR_REGISTRY_CAP`), oldest evicted first, as in Firecrawl's `otel-tracer.ts` [SRC] (a Set with oldest-first eviction at 100,000). It is flagged from the D4 `ZdrMarkLayer` hook and from Go `quarryotel.MarkZDR(ctx)`.
- Span processor: `ZdrSpanProcessor<P: SpanProcessor>` wraps the batch processor; `on_end` drops spans whose trace id is registered or whose `zdr` attribute is true, and removes attributes whose key is in the contract's `zdr_sensitive` or `never` lists from every other span (Rust receives an owned `SpanData`, so it can edit before forwarding). Go wraps its exporter instead (a `ReadOnlySpan` cannot be edited, so a thin wrapper type overrides `Attributes()`).
- Where each is needed, honestly: for Rust spans created through tracing-opentelemetry the sampler may already see a late-recorded `zdr` field (section 2.3 spike), making the registry belt and braces; for Go spans created with `tracer.Start` at request begin the sampler decides before the job body is parsed, so the registry is genuinely required there.
- Contract (S8a): `Q/docs/TELEMETRY_FIELD_CONTRACT.md` [NEW] plus a machine-readable golden `Q/crates/quarry-runtime/contracts/telemetry-field-contract.json` [NEW] with three lists: `safe` (ids, enum outcomes, durations, counts, byte sizes, status codes, driver names, `request_id`, `run_id`, `step`), `zdr_sensitive` (dropped from logs and span attributes when the scope is ZDR: url, uri, source_url, target_url, query, selector, host, domain, origin, title, snippet, text, markdown, excerpt, error, err, reason_text; equals D4's `SENSITIVE_FIELDS`) and `never` (dropped everywhere, non-ZDR included: authorization, cookie, set-cookie, token, password, otp, secret, api_key, typed text). `error_class` is the sanctioned replacement for error text. The Rust constants, the Go constants and any later `mp-telemetry` copy are pinned to the golden by the repo's existing golden mechanism (`scripts/tests/zdr-contract-sync.test.sh` GOLDENS and VENDORED rows), so drift fails CI. No shared crate across planes: each plane owns its telemetry init (F-t2-004 constraint).
- Exporter-disable flag: `QUARRY_TELEMETRY_EXPORT` with values `on` (default; exports only if an OTLP endpoint is set) and `off` (never build an exporter, even with the endpoint set; JSON logs, filtered by D4, continue). Read by `telemetry.rs`, `quarryotel.Init` and (follow-up) `mp-telemetry`. This is explicit, unlike today's only switch (leaving the endpoint unset).
- Not done here: Model Plane `mp-telemetry` adoption (otel 0.26 API, different traits) is a sibling (b) follow-up using the same golden; the gateway has no OTLP, so its phase 2 is empty.

### D15. Event volume and ordering controls (summary; numbers in the risks of S5 and S9)

Per run about 2 to 4 published events per step, 100 to 200 per run at the 25-step ceiling used today; payload at most 4 KiB; at 100 concurrent runs stepping every 5 s that is about 60 events per second into JetStream and Postgres, comfortably inside both. Controls: egress summarisation (D7), payload cap, a 2,048-slot bounded queue with drop-and-count, `MODEL_PLANE_RUN_TRACE` capped at 512 MiB with `DiscardOld` and 48 h, `run_trace_events` retention 30 days with a batch sweeper, Convex writes debounced to one per run per 2 s (D13). Ordering: per (run, producer) total order by `producer_seq`; JetStream per-subject FIFO plus the single publisher task preserve it end to end; cross-producer order is approximate by `ts` and anchored by `causation_id`.

### D16. Gateway and SPA stay thin

The gateway proxies and normalises (`{data, meta}`), it holds no trace state and does no cross-plane DB access. New gateway routes in this design: `GET /api/v1/browser/sessions/{session_id}/events/stream` (SSE, owner-checked, `Last-Event-ID` passthrough) and `GET /api/v1/runs/{run_id}/trace` (cursor read, Model Plane backed). Add the typed actions to `SPA/shared/actions` before wiring any UI operation, as the repo rule requires; the SPA work itself (panels, merge-by-producer, replay-gap banner) belongs to the UI workstream and is described only as a consumer contract in section 7.

## 4. Slices (PR-sized, in build order)

Build order and parallelism at a glance (details in section 5):

| # | Slice | Plane(s) | Effort | Needs |
|---|---|---|---|---|
| S1 | `x-request-id` intake (Quarry layer, 65-site codemod, gateway adopt and forward, Go parity) | Ingestion, Frontend | S/M, 3.0 d | nothing |
| S2 | Envelope contract: proto diff, codegen, JSON twins, golden, buf gate repair | Model Plane | M, 4.0 d | nothing (ratify ADR 8 part 1) |
| S3 | Quarry trace core: one mint, per-run seq, replay ring, real `from_seq`, `close_run` fix, gateway events proxy | Ingestion, Frontend | M, 5.0 d | S1 soft |
| S4 | Run origin, trace context, new trace event types, allowlisted projection, Ingestion-bus exclusion | Ingestion | M, 4.5 d | S2, S3 |
| S5 | Quarry to Model Plane publisher and NATS topology | Ingestion, Model Plane | M, 4.5 d | S2, S4 |
| S6 | session-core persistence, `EventLog.QueryEvents`, model-gateway and gateway trace read | Model Plane, Frontend | L, 10 d | S5, migrations 0044/0045 committed |
| S7 | Application Plane projection (ADR-GATED 8) | Application | M, 4.5 d | S5, ADR 8 part 2 |
| S8 | F-t2-004 phase 2: contract, flag, sampler, registry, processor (Rust and Go) | Ingestion | M/L, 8.0 d | D4 S-4..S-6 (hard); independent of S1 to S7 |
| S9 | Cross-plane contract tests and volume soak | all | M, 4.0 d | S5, S6 (partly earlier) |

Sum about 47.5 engineer-days. S1, S2 and S8a can start on day one in parallel; S3 and S8b/c are the other independent tracks.

Shared test and run conventions for every slice (from CLAUDE.md and the S9 sweep):

- Quarry Rust: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p <crate> <filter>` (CI runs `cargo test --workspace`, `quarry-v2-ci.yml` line 205). On the Windows host the Quarry crates do not build natively (boring-sys2); use the `rust:1` container with the `quarry-cargo`, `quarry-go`, `quarry-target` volumes and `bash -c` (not `-lc`); a full `-p quarry-core -p quarry-runtime -p quarry-edge` run exceeds 10 minutes (memory: Quarry Rust tests in Docker).
- Quarry Go: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`, same for `services/quarry-orchestrator`, `pkg/quarrycontracts`, and (new in S8) `pkg/quarryotel`; `GOWORK=off` when a module gains a dependency. Go tests run in CI only as report-only coverage jobs (`quarry-v2-ci.yml` 313 to 333, `continue-on-error`), so S8 adds a hard job.
- Model Plane Rust: `cd "apps/Model Plane/rust" && cargo test --workspace` (CI: `--no-fail-fast`, `model-plane-rust-ci.yml` 119); on Windows `cargo test -p <crate> --lib`; two cargo targets fail pre-existing on Windows (memory: model-plane-build-and-deploy). Postgres-backed session-core tests are `#[ignore]` and run with `DATABASE_URL=postgres://session_core:session_core@localhost:5432/session_core_test cargo test -p session-core --bin session-core <filter> -- --ignored --test-threads=1`.
- Model Plane Go: build and test per service from `apps/Model Plane/go` (`go test ./services/<name>/...`); the nats-provisioner is its own module: `cd "apps/Model Plane/go/services/nats-provisioner" && go test ./...`. No Go CI exists for the Model Plane (S9 H8), so S5 and S9 add one job for the provisioner contract tests.
- Gateway: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test <filter>` (CI also runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`, `verevonv3-ci.yml` 132 to 141); the repo rule adds `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` from the SPA root. SPA: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run <file>`, `pnpm typecheck`, `pnpm lint`. Any `pnpm` call inside a worktree prunes `pnpm-lock.yaml`; restore it afterwards (memory).
- Deploy evidence recipes: Model Plane `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service>` (needs the absolute `MODEL_PLANE_RUNTIME_ENV_FILE`); Ingestion `cd "apps/Ingestion Plane" && docker compose --env-file ../../.env --env-file .env -f docker-compose.yml up -d --build --no-deps quarry-edge` (use the longer env-file chain from the memory recipe if an interpolation is unset); gateway image swap plus frontend restart per the Ingestion deploy recipe; `make test-endpoints` as the stack smoke. A proto edit must rebuild `mp-contracts` from a clean cache: the build.rs comment documents that a BuildKit-normalised mtime can leave a stale cached artifact, so use `--no-cache` for the first image build after a proto change.

---

### S1. `x-request-id` intake (Quarry tower layer, codemod, gateway adopt and forward, Go parity)

**Goal.** One request id travels gateway to Quarry (and to quarry-control), appears in the response header, the JSON log span, `Envelope.meta.request_id` and, from S4, on trace events. It is never an authorization, tenancy or idempotency key.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QR/request_context.rs` | `REQUEST_ID` task-local, `scope(id, fut)`, `current_opt()` | new: a `tokio::task_local!` visible to the runtime crate so `EventSink` (S4) can stamp events | [NEW] |
| `QR/lib.rs` | `pub mod request_context;` | one line | [M] |
| `QE/request_id.rs` | `RequestId(pub String)`, `is_valid_inbound(&str) -> bool` (1 to 128 bytes, every byte in `[A-Za-z0-9._:-]`), `layer(req, next)` (`axum::middleware::from_fn`), `current() -> String` (task-local or `RequestKind::new().to_string()`), `make_request_span(&Request<Body>) -> Span` (method, path without query, `request_id`) | new. Reads `QUARRY_EDGE__REQUEST_ID_ADOPT` once (default `1`). Valid inbound id adopted, otherwise minted, never 400. Inserts `Extension<RequestId>`, wraps `next.run` in `request_context::scope`, sets response header `x-request-id` (safe: charset-checked) | [NEW] |
| `QE/lib.rs` | `pub mod request_id;` | one line (file has one in-flight line at 18) | [M] |
| `QE/routes.rs` | `router()` lines 297 to 306 | `.layer(TraceLayer::new_for_http().make_span_with(crate::request_id::make_request_span))` then, immediately after it, `.layer(middleware::from_fn(crate::request_id::layer))` so the id layer is outside `TraceLayer` and visible to its span. **Shared hunk with D4 S-5** (which replaces the span function by `zdr_log::make_request_span`; whichever lands first creates it, the other edits it and must keep the `request_id` field) | [M] |
| S1b (separate codemod PR): `QE/agent_routes.rs` (22 sites), `QE/resource_routes.rs` (18), `QE/change_routes.rs` (8), `QE/profile_routes.rs` (7), `QE/schedule_routes.rs` (6), `QE/routes.rs` (4) | `RequestKind::new().to_string()` | mechanical replacement by `crate::request_id::current()`; diff-only PR so reviewers can skim; after it `meta.request_id` equals the header | [C]/[M] |
| `Q/services/quarry-control/internal/httpx/httpx.go` | `RequestID` 24 to 34 | **[FINDING]** it adopts any inbound `X-Request-Id` with no length or charset check and logs it; add `validInboundID` with the same rule, mint `req_<uuid>` otherwise; test in `httpx_test.go` | [C] |
| `Q/docs/CONTRACTS.md` | section 2 (envelope) | document `meta.request_id` semantics and the rule | [C] |
| `GW/request_id.rs` | `layer`, `REQUEST_ID` task-local, `apply(RequestBuilder) -> RequestBuilder`, same validity rule, mint `req_<ULID-like>` | new; echoes `x-request-id`; records the span field | [NEW] |
| `GW/main.rs` | router assembly (TraceLayer at the end of the chain, ~164), `make_http_trace_span` 194 to 202 | `mod request_id;`; add `.layer(axum::middleware::from_fn(request_id::layer))` after `TraceLayer` (outermost); the span function reads `Extension<RequestId>` and records `request_id` | [C] |
| `GW/upstream.rs` | request builders at 1579 (`proxy_bearer_json_with_headers`), 1777 (SSE builder), 2883 (GET builder) | `req = crate::request_id::apply(req)` next to the `x-user-id` header | [C] |
| `GW/domains/browser.rs`, `GW/domains/chat/shared.rs` | the three browser builders and the chat/model-gateway builder (`grep 'header("x-user-id"'` finds them) | same `apply` call. The other ~20 domain-local builders (`search.rs`, `knowledge/*`, `spaces.rs`, `orgs/settings.rs`, ...) adopt in a follow-up sweep; this slice covers the run and browser paths only | [C] |
| `GW/config.rs` | `build_cors_layer` 303 | `expose_headers(["x-request-id"])` so the SPA can show a support id | [C] |

**API / contract changes.** Request header `x-request-id` (optional, validated). Response header `x-request-id` on every Quarry and gateway response. No proto, no migration, no OpenAPI path change. Env: `QUARRY_EDGE__REQUEST_ID_ADOPT` (default `1`; `0` always mints).

**Feature flag and rollout.** Always on; `QUARRY_EDGE__REQUEST_ID_ADOPT=0` is the safety valve. Rollout by image rebuild (no data). Rollback is a revert; the codemod PR reverts independently.

**Tests**
- Unit (`QE/request_id.rs`): `adopts_a_valid_inbound_id`, `mints_when_the_inbound_id_is_longer_than_128_bytes`, `mints_when_the_inbound_id_has_spaces_or_control_characters`, `never_returns_400_for_a_bad_id`, `echoes_the_header_on_success_and_error_responses`, `adopt_flag_off_always_mints`.
- Router-level (`QE/tests/request_id.rs` [NEW], harness `tests/support`): `meta_request_id_equals_the_response_header` (a 404 and a 200), `the_request_id_is_not_an_auth_or_tenancy_key` (the same inbound id from two orgs does not share a run, a cache entry or an ownership decision; a foreign run id with a matching request id still 404s).
- Go: `TestRequestIDRejectsOverlongAndForeignCharset`, `TestRequestIDAdoptsValid` (`httpx_test.go`).
- Gateway: `request_id::tests::*` (same rules), `browser_proxy_forwards_request_id_to_quarry` and `invalid_inbound_request_id_is_replaced_not_forwarded` (wiremock header match, the idiom at `upstream.rs` 2146).
- Commands: `cargo test -p quarry-edge request_id`; `cd services/quarry-control && go test ./internal/httpx/...`; `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test request_id browser_proxy`.

**Definition of done.** All tests green; `grep -rn "RequestKind::new()" "apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src"` returns only `request_id.rs`; clippy and fmt clean for the gateway; CONTRACTS.md updated.

**Deployment evidence.** Rebuild `quarry-edge`, `quarry-control` and the gateway images. Through the real gateway: `curl -i -H "x-request-id: e2e-d10-<uuid>" -b <session cookie> -X POST .../api/v1/browser/sessions` (or the dev-bypass path). Assert (1) the response header echoes the id; (2) `docker logs quarry-edge` contains a JSON line with `"request_id":"e2e-d10-<uuid>"`; (3) a deliberately failing call returns an envelope whose `meta.request_id` equals the header; (4) `-H "x-request-id: $(printf 'a%.0s' {1..300})"` yields a server-minted `req_` id, not the 300-character one.

**Effort.** S/M, 3.0 engineer-days (Quarry layer and tests 1.0, codemod 0.5, Go parity 0.5, gateway 1.0). **Risks.** Task-local lost across `tokio::spawn` (accepted, documented); two layers editing the same router line (hotspot with D4 and D2, merge order stated); log cardinality of arbitrary client ids (charset and cap bound it).

---

### S2. Envelope contract: proto diff, codegen, JSON twins, golden, buf gate repair

**Goal.** The Model Plane `Event` carries `producer_seq`, `agent`, `origin`, `run_id`, `request_id`; seven browser trace event types exist; every generated and hand-written twin agrees; the buf gates actually run.

**Exact proto diff** (`MP/proto/model_plane/v1/events.proto` [C])

```diff
   // Zero Data Retention mode. When true, this event must not be persisted durably.
   bool zdr = 13;

-  // Reserved for future envelope fields.
-  reserved 14 to 20;
+  // Position in the producer's own sequence for the run: scoped to
+  // (run_id, producer), starting at 1 and dense. A jump means events were lost
+  // between the producer and a consumer. 0 means the producer does not number
+  // its events. There is no order across producers.
+  uint64 producer_seq = 14;
+
+  // The agent on whose behalf the event was produced. Grouping metadata for
+  // timelines; never an authorization input.
+  AgentRef agent = 15;
+
+  // How the run this event belongs to was started (a trigger, not a surface).
+  RunOrigin origin = 16;
+
+  // The root run this event belongs to (the Model Plane run id). Empty for
+  // events that are not part of a run. resource_ref still names the emitting
+  // resource.
+  string run_id = 17;
+
+  // The validated x-request-id of the HTTP request that caused the event (at
+  // most 128 characters from [A-Za-z0-9._:-]). A correlation hint only: never
+  // an authorization, tenancy or idempotency key, and not unique across tenants.
+  string request_id = 18;
+
+  // Reserved for future envelope fields.
+  reserved 19 to 20;
 }
+
+// Identity of the agent that produced, or is acting in, an event.
+message AgentRef {
+  // Stable agent identifier (persona or subagent id). Empty when unknown.
+  string id = 1;
+  // Role in the run, for example "primary", "subagent" or "browser".
+  string role = 2;
+  // Identifier of the agent that spawned this one. Empty for the root agent.
+  string parent_id = 3;
+}
+
+// How a run was started. A trigger vocabulary, distinct from a thread's
+// surface ("origin" on threads in session-core).
+enum RunOrigin {
+  // Unknown or not declared.
+  RUN_ORIGIN_UNSPECIFIED = 0;
+  // A human action in a Verevon surface.
+  RUN_ORIGIN_USER = 1;
+  // A Model Plane agent, or another agent, started the run.
+  RUN_ORIGIN_AGENT = 2;
+  // A schedule, cron or routine fired.
+  RUN_ORIGIN_SCHEDULE = 3;
+  // An inbound webhook or external event started the run.
+  RUN_ORIGIN_WEBHOOK = 4;
+  // A programmatic caller (service or API client) started the run.
+  RUN_ORIGIN_API = 5;
+}
```

and after `EVENT_TYPE_APPROVAL_CONTINUATION_VERIFIED = 146;`:

```diff
+  // Quarry-produced browser trace events. Quarry (Ingestion Plane) publishes
+  // them on mp.v1.trace.run.<run_id> and Session Core persists them. Payloads
+  // are reference-based and content-free (ids, enums, counts, durations, url
+  // origin). They are never published for Zero Data Retention runs.
+  // A browser session was opened for the run.
+  EVENT_TYPE_BROWSER_SESSION_OPENED = 150;
+  // A browser session ended (reason: closed, expired, failed or cancelled).
+  EVENT_TYPE_BROWSER_SESSION_CLOSED = 151;
+  // Control of the browser session moved between a human and the agent.
+  EVENT_TYPE_BROWSER_CONTROL_CHANGED = 152;
+  // A tab was opened, selected or closed.
+  EVENT_TYPE_BROWSER_TAB_CHANGED = 153;
+  // An immutable step receipt was recorded by Quarry.
+  EVENT_TYPE_BROWSER_STEP_RECEIPT_RECORDED = 154;
+  // Browser egress decisions for a step (blocked requests and a summary).
+  EVENT_TYPE_BROWSER_EGRESS_DECIDED = 155;
+  // A governed artifact (screenshot, evidence delta, download) was recorded.
+  EVENT_TYPE_BROWSER_ARTIFACT_RECORDED = 156;
```

**Compatibility notes.** Wire: all additions are optional proto3 fields and new enum numbers; old readers ignore them, old writers leave defaults (`producer_seq` 0, `agent` unset, `origin` `RUN_ORIGIN_UNSPECIFIED`, strings empty). The golden wire test at `MPG/gen/model_plane/v1/proto_wire_parity_test.go` (`goldenEventWireHex`) stays valid because fields 1 to 13 are untouched. JSON twins: names are snake_case; `origin` is the enum name without the `RUN_ORIGIN_` prefix (`"AGENT"`), `event_type` is the name without `EVENT_TYPE_` (`"BROWSER_STEP_RECEIPT_RECORDED"`, the existing convention at `SC/grpc.rs` 7184); `producer_seq` is a JSON number (proto3-JSON would use a string for 64-bit integers; these twins use numbers, a JS consumer loses precision above 2^53, which a per-run counter never reaches). Unknown enum values: Go keeps the number; prost exposes `i32` and `EventType::try_from` fails, so every consumer must tolerate it (`MGW/http_routes.rs` 1343 to 1345 already maps unknown to `UNSPECIFIED`). `buf breaking` (rule set `FILE`): additions are safe; shrinking `reserved 14 to 20` may trip `RESERVED_MESSAGE_NO_DELETE` **[UNVERIFIED]**, resolved as in D2.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `.github/workflows/buf.yml` | steps at 21 to 23 | **S2a (separate PR)**: quote the paths (`buf lint "apps/Model Plane/proto"`, `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"`); the unquoted form splits at the space (S9 H5) | [C] |
| `MP/proto/baseline.binpb` | whole file | **S2a**: regenerate from the PRE-change commit: `docker run --rm -v "$PWD:/w" -w /w bufbuild/buf build "apps/Model Plane/proto" -o "apps/Model Plane/proto/baseline.binpb"`; it is 0 bytes today, so no breaking check has ever run. Also fix any pre-existing `buf lint` findings first (`COMMENTS` is in the rule set and several existing enum values at 175 to 181 have no comment) | [C] |
| `MP/proto/model_plane/v1/events.proto` | `Event`, `AgentRef`, `RunOrigin`, `EventType` | the diff above | [C] |
| `MP/proto/buf.yaml` | `breaking.ignore_only` | only if the reserved-range rule fires, one entry with a comment citing the ledger entry | [C] |
| `MPG/gen/model_plane/v1/events.pb.go` | generated | `docker run --rm -v "$PWD:/w" -w "/w/apps/Model Plane/proto" bufbuild/buf generate` (buf is not on the host, needs the BSR remote plugins); only `events.pb.go` changes (eventlog is unchanged) | [C] |
| `MP/python/mp-events-py/src/mp_events/_gen/model_plane/v1/events_pb2.py` | generated | same `buf generate` run | [C] |
| `apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/events_pb.ts` | generated (legacy app) | regenerate only if that app is still built **[UNVERIFIED]**; otherwise remove the TS plugin entry in a follow-up | [C] |
| `MPR/crates/mp-contracts/build.rs` | `mp_protos` list 36 to 56 | add `"model_plane/v1/eventlog.proto"` (needed by S6; lands here so codegen changes once). Rust code is generated at build time and never checked in | [C] |
| `MPR/services/session-core/src/grpc.rs` | `replay_event_row_to_proto` 3967 to 4003 | add `..Default::default()` (or the five new fields) to the `pb::Event` literal; `event_type_to_i32` 7184 to 7204: add the seven new names (150 to 156) | [M] (hunks clear of both ranges) |
| `MPR/services/model-gateway/src/{http_routes,sse,sse_events}.rs`, `MPR/crates/mp-contracts/tests/proto_wire_parity.rs` | `Event { .. }` literals | compile fixes with `..Default::default()`; do not reformat | [M]/[C] |
| `MPR/crates/mp-events/src/envelope.rs` | `Envelope` 10 to 52 | add `#[serde(default, skip_serializing_if = "Option::is_none")]` fields `producer_seq: Option<u64>`, `agent: Option<AgentRef>`, `origin: Option<String>`, `run_id: Option<String>`, `request_id: Option<String>`; new `AgentRef` struct; `validate()` unchanged; doc note that `zdr: bool` defaults to false when absent so new producers **must** write `"zdr"` explicitly | [C] |
| `MPG/pkg/envelope/envelope.go` | `Envelope` 26 to 60 | same fields with `omitempty` (`ProducerSeq uint64`, `Agent *AgentRef`, `Origin string`, `RunID string`, `RequestID string`) | [C] |
| `MP/api/contracts/run-trace-envelope.golden.json` and `.sha256` | golden | one canonical `BROWSER_STEP_RECEIPT_RECORDED` envelope (all 18 fields, explicit `"zdr": false`), pinned in `scripts/tests/zdr-contract-sync.test.sh` GOLDENS and VENDORED rows (copies under `Q/crates/quarry-runtime/tests/fixtures/model-trace/` and `MPR/services/session-core/tests/fixtures/`) | [NEW] |
| `MP/docs/phase-0/event-schemas.md`, `MP/docs/phase-0/proto-contracts.md` | docs | document the new fields, the numbering allocation and the JSON naming; `MP/docs/decisions/ledger.md` gets a newest-first entry (allocation of 14 to 18, event types 150 to 156, the `eventlog.proto` comment amendment) | [C] |

**Feature flag and rollout.** None (additive contract). Rollout order matters only for deployment: ship S2 first, every consumer ignores unknown fields; no producer sets them until S4 and S5. Rollback: revert the generated files; because no data uses the new fields until S5, nothing is stranded.

**Tests**
- Go: `TestEventDecodeWithTraceFieldsFromGoldenHex` (a second frozen hex produced once by `proto.Marshal` in a helper test and then pinned), `TestEventDecodeStillAcceptsPreTraceGoldenHex` in `MPG/gen/model_plane/v1/proto_wire_parity_test.go`; in `MPG/pkg/envelope`: `TestEnvelopeRoundTripsTraceFields`, `TestEnvelopeOmitsEmptyTraceFields`, `TestEnvelopeStillDecodesPreTraceJSON`, `TestRunTraceGoldenDecodes`.
- Rust: `MPR/crates/mp-events`: `envelope_round_trips_trace_fields`, `envelope_decodes_pre_trace_json`, `envelope_omits_none_trace_fields`, `trace_event_type_names_match_the_proto_enum` (iterate the generated enum's `as_str_name()`), `run_trace_golden_decodes` (`tests/run_trace_golden.rs` [NEW]); `session-core`: `event_type_to_i32_knows_the_trace_types`.
- Gates: `buf lint "apps/Model Plane/proto"`, `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"`, `bash scripts/tests/zdr-contract-sync.test.sh`, `node --test scripts/tests/zdr-contract-schema.test.mjs`.
- Commands: `cd "apps/Model Plane/go/gen" && go test ./model_plane/v1/...`; `cd "apps/Model Plane/go/pkg/envelope" && go test ./...`; `cd "apps/Model Plane/rust" && cargo test -p mp-events -p mp-contracts` (Windows: `--lib` where integration targets fail pre-existing); full: `cargo test --workspace`.

**Definition of done.** The repaired buf workflow is green on `main` (lint and breaking both run and a deliberate breaking edit on a throwaway branch turns it red); generated diffs touch only the files above; golden pinned and the sync script passes; a clean `cargo build` of the Model Plane workspace succeeds; no existing test changes meaning.

**Deployment evidence.** `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build --no-cache session-core model-gateway` (the `--no-cache` is the stale `mp-contracts` guard), then `make`-style health: session-core and model-gateway start, `GET /v1/runs/:id/events/replay` for an existing run still returns the same page as before the change (diff the JSON), and a fresh chat browser turn still streams. Attach the CI run showing the buf gate executing (it was inert before).

**Effort.** M, 4.0 engineer-days (S2a gate repair 1.0, proto and codegen 1.0, twins and compile fixes 1.0, golden, docs, ledger 1.0). **Risks and mitigations.** Compile fixes land in four [M] files with large in-flight diffs: use `..Default::default()` only, one hunk each, rebase daily. Remote buf plugins need network: pin the buf image digest in the PR. The legacy TS output may be dead code: decide before regenerating.

---

### S3. Quarry trace core: one mint, per-run sequence, replay ring, real `from_seq`, `close_run` fix, gateway events proxy

**Goal.** The agent-event SSE can resume: `from_seq` and `Last-Event-ID` replay missed events from a bounded in-memory ring then continue live without gap or duplicate; the sequence is per run and dense; SSE and NATS carry the same `event_id` and `seq`; `close_run` really ends the stream and frees state; the gateway can proxy the stream.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QR/events.rs` | `EventSink` (20 to 62), `emit` 67, `emit_ephemeral` 111, `emit_for_zdr` 137 | new field `runs: Arc<DashMap<RunKind, Arc<RunTrace>>>` and `ring: RingLimits`; private `mint(&self, run_id, event_type, payload, key) -> Arc<Event>` is the **only** place that assigns `event_id`, `seq` and `ts`; for a run opened with `open_run` the seq comes from `RunTrace.next_seq` (starts at 1, per run), otherwise the legacy global counter (so runs that never close add no map entry); emission is `lock ring, mint, push, broadcast.send, unlock`, then the awaits | [C] |
| `QR/events.rs` | `open_run(&self, run_id, start_seq: u64)`, `close_run(&self, run_id)`, `subscribe_with_replay(&self, run_id, from_seq: Option<u64>) -> Option<ReplaySubscription>`, `ReplaySubscription { replay: Vec<Arc<Event>>, gap: Option<ReplayGap>, rx: broadcast::Receiver<Event> }`, `ReplayGap { requested, oldest_retained }`, `RingLimits { max_events, max_bytes }`, `with_ring_limits` | new. `subscribe_with_replay` subscribes first, then snapshots under the ring lock. `close_run` removes the run, the broadcast sender (receivers get `Closed`) and the ring | [C] |
| `QR/event_bus.rs` | `EventBus` | new provided method `publish_event(&self, event: Event) -> QuarryResult<()>` defaulting to the old `publish` for compatibility; `InProcessEventBus` unchanged | [C] |
| `QR/nats_event_bus.rs` | `publish` 191 to ~262 | add `publish_event` that publishes the **given** event (no second `event_id` or `seq`); keep `publish` for legacy callers; GC `seq_per_run` in `unsubscribe` | [C] |
| `QR/events.rs` (inside `emit`) | the `tokio::spawn` at 90 to 99 | pass the minted `Event` to `bus.publish_event`; the spawn stays (the Ingestion bus is best-effort), ordering guarantee is only claimed for the Model publisher (S5) | [C] |
| `QR/lib.rs` | exports | `pub use events::{ReplayGap, ReplaySubscription, RingLimits};` | [M] |
| `QE/trace_config.rs` | `TraceConfig::from_env()` | new env-only module (D2's D13 convention, avoids the 187-line `config.rs` diff): `QUARRY_EDGE__TRACE_RING_EVENTS` (default 512), `QUARRY_EDGE__TRACE_RING_MAX_BYTES` (default 1 MiB); unparsable value aborts boot | [NEW] |
| `QE/lib.rs`, `QE/main.rs` | `pub mod trace_config;`; `EventSink::new(event_tx)` at ~411 | `.with_ring_limits(trace_cfg.ring)` | [M] |
| `QE/agent_routes.rs` (post-D2-split: `lifecycle.rs`, `live.rs`) | `start_run` 1273 (emit at 1642 to 1650) | call `state.event_sink.open_run(&run_id, 1)` immediately before the `AgentStarted` emit (resume passes the checkpoint's `trace_seq` from S4) | [C] |
| same | `close_run` 3562 to 3655 | after the `AgentCompleted` emit call `state.event_sink.close_run(&rid)`; the D2 sweeper's single `finish_run` gets the same one-liner | [C] |
| same | `AgentEventsQuery` 936 to 942, `agent_events_stream` 2658 to ~2770 | rewrite the body: read `from_seq` (query) else `Last-Event-ID` (header); `subscribe_with_replay`; emit a `replay_gap` SSE event when `gap` is set; emit replay events (same `is_user_facing_event` filter) with `id:` = per-run `seq`; then live with `seq > last_emitted`; on `Lagged` re-snapshot the ring for `seq > last_emitted` instead of asking the client to backfill (only a lost ring emits `replay_gap`); fix the doc comment (the `quarry_event_history` claim is false) | [C] |
| `Q/docs/openapi.yaml` | new path `/v1/agent/runs/{run_id}/events/stream` | document query `from_seq`, `max_events`, header `Last-Event-ID`, event names, `replay_gap`, `done` (the route is currently undocumented) | [M] |
| `Q/docs/CONTRACTS.md` | section 3 | note: `seq` is per run for agent runs; `id:` equals `seq`; the dotted event names in the frozen list (`page.fetched`) are not the wire spelling (snake_case) **[DRIFT in the doc]** | [C] |
| `GW/domains/browser.rs` | router 368 to 460; new `stream_agent_events` modelled on `stream_live_frames` 1730 to 1782 | route `GET /api/v1/browser/sessions/{session_id}/events/stream` -> `owner_browser_session_projection` then `proxy_sse_stream` to `/v1/agent/runs/{id}/events/stream{query}` passing `Last-Event-ID` and the ZDR flag from the projection | [C] |
| `SPA/shared/api/browser-client.ts` | new `streamBrowserEvents(sessionId, { fromSeq })` | typed reader only; no UI operation is wired here (add the `src/shared/actions` contract first when a panel uses it) | [C] |

**API / data-model changes.** SSE: `id:` is now the per-run `seq` (previously the process-global counter, which no consumer used); new SSE event `replay_gap` with data `{"requested":N,"oldest_retained":M,"fallback":"browser-session/timeline"}`; `done` with `run_closed` now fires for real. No migration, no proto.

**Feature flag and rollout.** `QUARRY_EDGE__TRACE_RING_EVENTS=0` disables the ring and restores the old no-replay behaviour (replay returns `replay_gap`). Per-run seq has no flag (additive in meaning, unused before). Rollback: set the knob to 0 or revert.

**Tests**
- Unit (`QR/events.rs` tests): `agent_run_seq_starts_at_one_and_is_dense`, `two_runs_have_independent_seq`, `a_run_that_was_never_opened_keeps_the_global_counter_and_adds_no_map_entry`, `ring_is_bounded_by_events_and_by_bytes`, `replay_returns_only_events_after_from_seq`, `replay_then_live_has_no_gap_or_duplicate_under_concurrent_emit` (8 emitters, one reader), `replay_reports_a_gap_when_from_seq_is_older_than_the_ring`, `a_lagged_receiver_recovers_from_the_ring`, `close_run_closes_subscribers_and_frees_state`, `a_zdr_event_never_reaches_nats_or_mpsc_but_is_replayable_from_the_ring`, `nats_receives_the_same_event_id_and_seq_as_the_sse_path`.
- Edge integration (`QE/tests/agent_events_replay.rs` [NEW], same harness as `tests/sse_stream.rs`): `reconnect_with_last_event_id_replays_missed_events_then_tails`, `from_seq_wins_over_last_event_id`, `a_non_owner_gets_404_not_a_replay`, `a_closed_run_stream_ends_with_done_run_closed`.
- Gateway: `browser_events_stream_proxies_last_event_id_and_query_to_quarry`, `browser_events_stream_refuses_a_non_owner`.
- Commands: `cargo test -p quarry-runtime events`; `cargo test -p quarry-edge --test agent_events_replay --test sse_stream`; `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test browser_events_stream`; `pnpm typecheck` for the client type.

**Definition of done.** All the above green; the SSE doc comment is true; with `RUST_LOG=quarry_runtime=debug` the edge logs `open_runs=<n>` at `close_run` and the count returns to 0 after a batch of runs close (no leaked senders).

**Deployment evidence.** Rebuild `quarry-edge` and the gateway. Flow: create a browser session in the UI (or `POST /api/v1/browser/sessions`), `curl -N -H "accept: text/event-stream" .../events/stream` in terminal A, drive five governed steps from the UI; kill curl after seeing `id: 3`; reconnect with `-H "Last-Event-ID: 3"`; assert the first event has `id: 4`, ids are consecutive, nothing repeats; reconnect with `Last-Event-ID: 0` after 600 steps worth of events (loop) and assert a `replay_gap` first; close the session and assert the stream ends with `event: done` and `data: run_closed` (impossible before this change). Attach the captured transcripts.

**Effort.** M, 5.0 engineer-days (sink refactor and ring 2.0, handler and tests 1.5, gateway proxy and client 1.0, docs 0.5). **Risks and mitigations.** Memory: ring cap is per run (default 1 MiB), runs per org are capped by D2 S4, so the worst case is bounded; ring lock contention is negligible (one writer per run, held for microseconds). Changing `id:` semantics: no in-repo consumer used it (the gateway did not proxy the stream, the SPA never read it). `agent_routes.rs` conflicts: write against the D2 module names, cite pre-split ranges, keep the handler body self-contained so it moves as one block.

---

### S4. Run origin, trace context, new trace event types, allowlisted projection, Ingestion-bus exclusion

**Goal.** Quarry knows how each agent run was started (`RunOrigin`), which Model Plane run and agent it belongs to (trace context, service principals only), emits the browser-session, step-receipt, egress, artifact, control and tab events as typed `Event`s, converts them to the Model Plane envelope through one allowlisting function (proven against the S2 golden), and stops putting raw agent payloads on the Ingestion bus. Nothing leaves the process yet (S5 adds the publisher).

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QC/trace.rs` | `RunOrigin` (serde snake_case: user, agent, schedule, webhook, api; `wire() -> "USER"`), `AgentRef { id, role, parent_id }`, `RunTraceContext { run_id, agent, origin }` (request body), `EventTrace { origin, agent, request_id, run_id, causation_event_id }`, `derive_origin(is_service: bool, hint: Option<RunOrigin>) -> RunOrigin`, `valid_token(&str, max) -> bool` | new. `derive_origin`: user token gives `User`, or `Agent` when hinted `agent`, other hints ignored; service token gives its hint or `Api` | [NEW] |
| `QC/event.rs` | `Event` (84 to 96), `EventType` | `Event` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub trace: Option<EventTrace>`; `EventType` gains `BrowserSessionOpened`, `BrowserSessionClosed`, `BrowserControlChanged`, `BrowserTabChanged`, `StepReceiptRecorded`, `EgressDecided`, `ArtifactRecorded` | [C] |
| `QC/lib.rs` | `pub mod trace;` | one line | [C] |
| `Q/pkg/quarrycontracts/event.go` | constants, `RunOrigin` type | mirror the seven types and the enum (parity checklist: Rust enum, Go constants, CONTRACTS.md, SSE name table) | [C] |
| `QR/trace_projection.rs` | `PublishCtx { org_id, user_id, model_run_id, browser_run_id }`, `model_event_type(&EventType) -> Option<&'static str>`, `to_model_envelope(&Event, &PublishCtx) -> Option<serde_json::Value>`, per-type allowlists, `MAX_PAYLOAD_BYTES = 4096`, `is_agent_class(&EventType) -> bool` | new. Mapping: `AgentStarted` to `BROWSER_SESSION_OPENED`, `AgentCompleted`/`AgentFailed` to `BROWSER_SESSION_CLOSED`, the five new types map one to one; `ActionStarted`, `ActionCompleted`, `ActionFailed`, `ObservationReady` and every crawl or scrape type map to `None` (Model Plane already emits dispatch and observation events). Builds the envelope with bare-ULID `event_id` (`event.event_id.ulid().to_string()`), `schema_version` 1, `producer` `"quarry-edge"`, `correlation_id` = Model run id, `idempotency_key` = `quarry:<browser_run_id>:<seq>`, `resource_ref` = `browser_run/<id>`, explicit `"zdr": false`, `producer_seq` = `event.seq` | [NEW] |
| `QR/events.rs` | `open_run` meta, `mint` | `RunTraceMeta { start_seq, origin, model_run_id, agent, org_id, actor_id, browser_run_id, zdr }` stored in `RunTrace`; `mint` stamps `event.trace` from it plus `request_context::current_opt()`; `emit`/`emit_for_zdr` now return the minted `EventKind` (callers ignore it today) so egress and artifact events can set `causation_event_id` to the receipt event | [C] |
| `QR/nats_event_bus.rs` | `publish_event` | return `Ok(())` without publishing when `trace_projection::is_agent_class(event.event_type)` (D10). Crawl, search and host events are untouched | [C] |
| `QR/step_receipts.rs` | `AgentRunCheckpoint` 39 to 71, `BrowserTimelineEventKind::Lifecycle` | checkpoint gains `#[serde(default)] trace_seq: u64`, `#[serde(default)] origin: Option<RunOrigin>`, `#[serde(default)] model_run_id: Option<String>` (JSONB `state`, no migration); `Lifecycle { state, #[serde(default, skip_serializing_if = "Option::is_none")] origin }`. **Shared with D2 S3** (adds `initiated_by`, `control_epoch` to the same structs, plus migration 0010): both are additive; merge order D2 first, or one combined hunk | [C] |
| `QE/agent_routes.rs` (post-split: `types.rs`, `lifecycle.rs`, `step.rs`, `control.rs`, `tabs.rs`, `live.rs`, `projection.rs`) | `StartRunBody` 346 to 383 | `#[serde(default)] pub trace: Option<RunTraceContext>`; unknown or invalid tokens are dropped with a safe-field log (`reason` only), never a 400 (the context is descriptive) | [C] |
| same | `RunEntry` 77 to 107 | add `origin: RunOrigin`, `model_run_id: Option<String>`, `agent: Option<AgentRef>` | [C] |
| same | `start_run` 1273 to 1663 | honour `trace.run_id` and `trace.agent` only when `claims.is_service()`; `origin = derive_origin(claims.is_service(), body.trace.origin)`; build `RunTraceMeta` (`zdr` from the effective ZDR, `start_seq` from the checkpoint on resume else 1); extend the `AgentStarted` payload to `{ org_id, execution_tier, driver, profile_scope, control_mode }` (additive to the legacy `{ org_id }`); write `origin`, `model_run_id`, `trace_seq` into the checkpoint | [C] |
| same | `step` 2228 to 2409 (receipt block 2338 to 2375) | after the receipt is built (and appended when not ZDR) emit `StepReceiptRecorded { receipt_id, step, action, outcome, latency_ms, error_code, cost_micro_usd, url_origin, verified }` through `emit_for_zdr` (so a ZDR run reaches live subscribers only); the `action` field is the serde tag of `AgentAction` only, never its fields; after the observation emit `EgressDecided` (one summary plus up to 20 `block` events) and one `ArtifactRecorded` per non-null id among `screenshot_artifact_id`, `visual_observation_artifact_id`, `evidence_delta_artifact_id` (kinds `screenshot`, `visual_observation`, `evidence_delta`), all with `causation_event_id` = the receipt event id. ZDR runs have no artifacts (existing withholding), so none appear | [C] |
| same | `set_browser_session_control` 1733, tab routes (`new_tab`, `select_tab`, `close_tab`), `record_browser_timeline_event` 488 to 509 | one helper `record_timeline_and_trace` calls the existing timeline writer and emits `BrowserControlChanged` or `BrowserTabChanged`, so each transition has one call site | [C] |
| same | `close_run` 3562 to 3655 (and the D2 sweeper) | `AgentCompleted` payload becomes `{ reason: "closed" \| "expired" \| "failed" \| "cancelled", steps, duration_ms }` (the sweeper passes `expired`) | [C] |
| same | `is_user_facing_event` 949, `event_name` 965 | add the new types and SSE names `browser.session.opened`, `browser.session.closed`, `browser.control`, `browser.tab`, `browser.receipt`, `browser.egress`, `browser.artifact` | [C] |
| same | `BrowserSessionProjectionData` (`projection.rs`) | additive `runOrigin` (camelCase like its siblings) | [C] |
| `QE/tests/origin_is_not_authz.rs` | source scan | fails if the identifier `origin` (case-sensitive, as a field or variable of `RunOrigin`) appears in `admission.rs`, `budget.rs` or, before the D2 split, the gate functions in `agent_routes.rs` ranges 314 to 339, 2257 to 2309 | [NEW] |
| `Q/docs/openapi.yaml`, `Q/docs/CONTRACTS.md` | `StartRunRequest.trace`, projection `runOrigin`, event table | document; note the frozen-list uses dotted names that differ from the snake_case wire names | [M]/[C] |
| `GW/domains/browser.rs`, `SPA/shared/api/browser-client.ts` | projection passthrough | `runOrigin` optional field | [C] |

**Payload allowlists** (every string at most 256 characters; unknown keys are dropped, not redacted; serialised payload at most 4096 bytes, else `{ "truncated": true }` plus the ids)

| Model event type | Allowed payload keys |
|---|---|
| `BROWSER_SESSION_OPENED` | `browser_run_id`, `execution_tier`, `driver`, `profile_scope`, `control_mode` |
| `BROWSER_SESSION_CLOSED` | `browser_run_id`, `reason`, `steps`, `duration_ms` |
| `BROWSER_CONTROL_CHANGED` | `browser_run_id`, `mode`, `initiated_by`, `control_epoch` |
| `BROWSER_TAB_CHANGED` | `browser_run_id`, `operation`, `tab_id`, `active_tab_id` |
| `BROWSER_STEP_RECEIPT_RECORDED` | `browser_run_id`, `receipt_id`, `step`, `action`, `outcome`, `latency_ms`, `error_code`, `cost_micro_usd`, `url_origin`, `verified`, `initiated_by`, `control_epoch` |
| `BROWSER_EGRESS_DECIDED` | `browser_run_id`, `step`, `decision` (`block` or `summary`), `policy`, `origin`, `tab_id`, `egress_seq_from`, `egress_seq_to`, `allowed`, `blocked` |
| `BROWSER_ARTIFACT_RECORDED` | `browser_run_id`, `step`, `artifact_id`, `kind`, `bytes`, `content_type` |

`url_origin` and `origin` are scheme plus host plus port computed with `url::Url::origin().ascii_serialization()`; the path, query and fragment never leave the process. `initiated_by` and `control_epoch` appear only after D2 S3 lands (absent keys are fine). `verified` mirrors the observation's action outcome flag (confirm the exact `ActionOutcome` variant names against `QC/contracts.rs` 65 while implementing, **[UNVERIFIED]**).

**API / data-model changes.** `POST /v1/agent/runs` request gains optional `trace: { run_id?, agent?: { id?, role?, parent_id? }, origin? }` (strings at most 64 characters from `[A-Za-z0-9_-]`; `origin` one of `user | agent | schedule | webhook | api`). SSE gains the seven new event names. Checkpoint JSON gains three optional keys. Projection gains `runOrigin`. No SQL migration and no proto change in this slice.

**Feature flag and rollout.** The new emissions are always on locally (they only feed SSE and the not-yet-existing publisher). The Ingestion-bus exclusion has a kill switch `QUARRY_EDGE__NATS_AGENT_EVENTS=1` that restores the old behaviour for an operator who discovers a consumer; default `0`. Rollback: revert, or set that flag.

**Tests**
- Unit (`QC/trace.rs`): `derive_origin_user_token_ignores_schedule_and_webhook_hints`, `derive_origin_user_token_may_claim_agent`, `derive_origin_service_token_defaults_to_api`, `derive_origin_service_token_accepts_each_hint`, `valid_token_rejects_dots_spaces_and_overlong`.
- Unit (`QR/trace_projection.rs`): one allowlist test per type (`step_receipt_projection_keeps_only_allowlisted_keys`, ...), `unknown_payload_keys_are_dropped`, `an_over_cap_payload_becomes_the_truncated_marker`, `agent_class_types_map_to_none_or_session_types_only`, `mapper_output_matches_the_run_trace_golden` (normalising `event_id` and `ts`), `a_zdr_false_key_is_always_written` (the Rust twin's default is the unsafe direction), and a proptest `no_typed_text_url_path_or_query_in_any_envelope` over arbitrary `AgentAction` values and arbitrary receipts: a marker string placed in `Type.text`, a URL query, a selector and an error message must never appear in any serialised envelope.
- Edge integration (`QE/tests/agent_trace_events.rs` [NEW]): `a_service_start_with_trace_context_stamps_events`, `a_user_token_trace_context_is_ignored`, `invalid_trace_tokens_are_dropped_not_rejected`, `a_step_emits_receipt_then_egress_then_artifact_with_causation`, `egress_summary_caps_individual_blocks_at_twenty`, `resume_continues_trace_seq_from_the_checkpoint`, `a_zdr_run_emits_trace_events_only_to_live_subscribers`, `agent_class_events_are_not_published_to_the_ingestion_bus` (recording `EventBus`), `the_nats_agent_events_flag_restores_old_behaviour`.
- Source scan: `origin_is_not_authz`.
- Commands: `cargo test -p quarry-core trace`; `cargo test -p quarry-runtime trace_projection events`; `cargo test -p quarry-edge --test agent_trace_events --test origin_is_not_authz`; `cd "apps/Ingestion Plane/Quarry-v2/pkg/quarrycontracts" && go test ./...`.

**Definition of done.** Tests green; the golden test proves the mapper output equals the Model Plane golden; the property test cannot be satisfied by redaction (it passes because unknown keys are dropped); `grep` for `emit(` on agent-class events outside `EventSink` finds nothing; the SSE shows the seven new event names.

**Deployment evidence.** Rebuild `quarry-edge`. With a service token standing in for execution-core (F-t2-001 may not be finished): `POST /v1/agent/runs` with `{"trace":{"run_id":"<ULID>","agent":{"id":"general-v1","role":"primary"},"origin":"agent"}}`, then three steps, one of them `type` with the marker text `D10PROBE-<uuid>` into a form and one navigation to a URL carrying `?q=D10PROBE-<uuid>`. Assert on the SSE capture: `browser.receipt` events with `trace.origin = "agent"` and `trace.request_id` equal to the request's `x-request-id` (S1), a `browser.egress` summary and a `browser.artifact` per screenshot, each artifact event with `causation_event_id` = the receipt's id; the marker appears only in the legacy owner events (`action.started`), never in any `browser.*` event. Then `nats stream view QUARRY_EVENTS --subjects "quarry.>"` (nats CLI container on the Ingestion network) shows **no** `action_started` or `observation_ready` for that run, and the marker is absent (before this slice it was present: record the before/after).

**Effort.** M, 4.5 engineer-days (types and derivation 1.0, projection and tests 1.5, emit sites 1.5, docs and parity 0.5). **Risks and mitigations.** Emit-site sprawl in `agent_routes.rs` (D2 module split first or write against its names); `step_receipts.rs` struct literals (D2 lists them: lines 501, 519, 540, 742 and `browser_procedure.rs:231`); allowlist drift (the golden and the property test fail on drift); the `trace.run_id` label could be wrong or hostile (never used in Quarry for authority, re-validated by the consumer in S6).

---

### S5. Quarry to Model Plane publisher and NATS topology

**Goal.** Non-ZDR, run-attached trace events travel from Quarry to a dedicated JetStream stream on Model-nats, in order, with loss visible as a sequence gap, using a publish-only principal. The provisioner and its contract tests know about the new publisher.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QR/model_trace_publisher.rs` | `ModelTraceConfig { url, user, password, subject_prefix: "mp.v1.trace.run", queue_cap: 2048, ack_window: 32 }`, `trait TraceTransport { async fn publish(&self, subject: String, msg_id: String, body: Bytes) -> QuarryResult<()> }`, `NatsTraceTransport` (async-nats client plus `jetstream::Context::publish_with_headers` with header `Nats-Msg-Id: <event_id>` so JetStream de-duplicates inside its window), `ModelTracePublisher { tx, dropped: AtomicU64, published: AtomicU64 }`, `try_enqueue(&Event, &PublishCtx)` | new. One bounded `mpsc` queue and **one** publisher task, so publish order equals sequence order; the task keeps up to 32 un-acked publishes (it sends in order, awaits acks in order). Defence in depth: refuses when `ctx.zdr`, when `model_run_id` is missing or not a single subject token, and when `to_model_envelope` returns `None`. A full queue drops the new event and increments `dropped` (the gap is visible downstream). `shutdown(deadline)` flushes the queue for at most 2 s at `close_run` and at process exit | [NEW] |
| `QR/events.rs` | `EventSink::with_model_trace(Arc<ModelTracePublisher>)`, hook in `emit` | after `mint`, for non-ZDR runs whose meta has a `model_run_id`, call `try_enqueue` (never `await` on the broker) | [C] |
| `QR/lib.rs` | exports | `pub use model_trace_publisher::{ModelTracePublisher, ModelTraceConfig};` | [M] |
| `QE/trace_config.rs` | `ModelTraceEnv::from_env()` | `QUARRY_EDGE__MODEL_TRACE_NATS_URL`, `QUARRY_EDGE__MODEL_TRACE_NATS_USER`, `QUARRY_EDGE__MODEL_TRACE_NATS_PASSWORD`, `QUARRY_EDGE__MODEL_TRACE_QUEUE` (2048), `QUARRY_EDGE__MODEL_TRACE_SUBJECT_PREFIX` (`mp.v1.trace.run`); unset URL means no publisher; URL set with a missing credential aborts boot (fail loud) | [C] (created in S3) |
| `QE/main.rs` | next to the NATS wiring at 405 to 445 | build the publisher when configured, `event_sink.with_model_trace(..)`; log `model trace publisher: connected` (no credential values); connection failure falls back to no publisher with a warning (best-effort, never blocks boot) | [M] |
| `Ingestion Plane/docker-compose.yml` | quarry-edge service, near lines 69 to 71 | add the three env lines (`QUARRY_EDGE__MODEL_TRACE_NATS_URL: nats://model-nats:4222`, user `quarry-trace-producer`, password `${MODEL_QUARRY_TRACE_NATS_PASSWORD:?...}`); the service already joins `inter-plane-bus` (line 239) | [M] |
| `MP/deploy/nats.conf` | new user block | `quarry-trace-producer` (password `$MODEL_QUARRY_TRACE_NATS_PASSWORD`): `publish: ["mp.v1.trace.run.*"]`, `subscribe: ["_INBOX.QUARRY_TRACE_PRODUCER.>"]`, nothing else (no `$JS.API`) | [C] |
| same | `session-core-runtime` block (35 to 58) | add `"$JS.API.STREAM.INFO.MODEL_PLANE_RUN_TRACE"`, `"$JS.API.CONSUMER.INFO.MODEL_PLANE_RUN_TRACE.session-core-trace"`, `"$JS.API.CONSUMER.MSG.NEXT.MODEL_PLANE_RUN_TRACE.session-core-trace"`, `"$JS.ACK.MODEL_PLANE_RUN_TRACE.session-core-trace.>"` (the pattern of the two existing durables) | [C] |
| `MP/deploy/docker-compose.yml` | nats service env (line ~54 pattern) | pass `MODEL_QUARRY_TRACE_NATS_PASSWORD` into the broker the same way as the other principals | [C] |
| `scripts/bootstrap_runtime_environment.sh` | secret generation | generate `MODEL_QUARRY_TRACE_NATS_PASSWORD` once and surface it to both the Model and Ingestion env files; `scripts/tests/bootstrap_runtime_environment.test.sh` gets an assertion (never hand-edit the ledgers; memory: bootstrap secrets ledger) | [C] |
| `MPG/services/nats-provisioner/main.go` | `streamConfigs()` 24 to 62, `consumerBindings()` | stream `MODEL_PLANE_RUN_TRACE`: subjects `mp.v1.trace.run.*`, `LimitsPolicy`, `FileStorage`, `MaxAge` 48 h, `MaxBytes` 512 MiB, `Replicas` 1, `DiscardOld`; consumer `session-core-trace` (pull, `AckExplicit`, `AckWait` 30 s, `MaxDeliver` 5, `FilterSubject` `mp.v1.trace.run.*`, `ReplayInstant`, `DeliverAll`) | [C] |
| `MPG/services/nats-provisioner/publishers_registry_test.go`, `manifest_contract_test.go` | `publisherRow`, `TestRegistryRowsPointAtRealSource` 347, `TestPublisherPrincipalsMayPublishTheirSubjects` 621 | add a row (`service: "quarry-edge"`, `natsUser: "quarry-trace-producer"`, `inbox: "_INBOX.QUARRY_TRACE_PRODUCER"`, subject `mp.v1.trace.run.x`, `transport: jetStreamAck`, `stream: "MODEL_PLANE_RUN_TRACE"`) and a new optional field `sourceRoot` (`"repo"`) so the source path may be `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/model_trace_publisher.rs` with needle `mp.v1.trace.run`; new `TestQuarryTracePrincipalCanOnlyPublishTraceSubjects` (parses the `nats.conf` block: publish equals exactly `["mp.v1.trace.run.*"]`, subscribe only the inbox) | [C] |
| `MPG/services/nats-provisioner/main_test.go` | `TestTopologySpecsCoverRuntimeBindings` 11 | the consumer binding must be referenced by session-core source: needle `session-core-trace` in `SC/trace_nats.rs` (S6) | [C] |
| `.github/workflows/model-plane-rust-ci.yml` | new small job | `cd apps/Model Plane/go/services/nats-provisioner && go test ./...` (the provisioner contract tests have no CI at all today, S9 H8) | [M] |

**API / contract changes.** New subject family `mp.v1.trace.run.<model_run_id>` carrying the S2 JSON envelope, one message per event, header `Nats-Msg-Id` = bare ULID `event_id`. New NATS principal, stream and consumer. Registry rows are the contract.

**Feature flag and rollout.** Presence of `QUARRY_EDGE__MODEL_TRACE_NATS_URL` enables publishing. Staged: (1) provision stream, principal and consumer with nothing publishing; (2) enable Quarry publishing (messages accumulate in the stream for 48 h while no consumer persists: "shadow" mode); (3) S6 consumer goes live and drains. Rollback: unset the URL (publishing stops immediately, in-flight queue drains or drops); topology can stay.

**Tests**
- Unit (`QR/model_trace_publisher.rs`, fake `TraceTransport`): `publishes_in_sequence_order`, `a_full_queue_drops_new_events_and_counts_them`, `a_zdr_event_is_never_enqueued`, `an_event_without_a_model_run_is_never_enqueued`, `a_run_id_with_a_dot_is_refused_as_a_subject_token`, `over_cap_payload_is_replaced_by_the_truncated_marker`, `shutdown_flushes_within_the_deadline`, `a_transport_error_does_not_block_the_step_path`, `msg_id_header_equals_the_event_id`.
- Go contract (`nats-provisioner`): the three updated or new tests above plus `TestEveryJetStreamPublisherSubjectGetsAPubAck` (existing, now covering the new row).
- Smoke script `scripts/tests/model-trace-nats-smoke.sh` [NEW] (uses the nats CLI container): as `quarry-trace-producer` publish to `mp.v1.trace.run.smoke` succeeds; publish to `mp.v1.run.smoke.event`, `tools.completions.smoke` and `verevon.audit.v2.model.x` is refused; a subscribe on `mp.v1.>` is refused.
- Commands: `cargo test -p quarry-runtime model_trace_publisher`; `cd "apps/Model Plane/go/services/nats-provisioner" && go test ./...`; `bash scripts/tests/model-trace-nats-smoke.sh`; `bash scripts/tests/bootstrap_runtime_environment.test.sh`.

**Definition of done.** Provisioner contract tests green in CI; smoke script green; with the publisher on and a service-token run of 30 steps, `nats stream info MODEL_PLANE_RUN_TRACE` shows the messages and, per run, `producer_seq` is dense with no gap at nominal rate.

**Deployment evidence.** `docker compose` restart of `model-nats` (new conf) and a run of the provisioner one-shot (`MP/deploy/docker-compose.yml` `nats-provisioner`), rebuild `quarry-edge`. Run a non-ZDR browser run through Model Plane (needs F-t2-001 working end to end; until then the service-token stand-in of S4). Assert with `nats stream view MODEL_PLANE_RUN_TRACE`: envelopes carry `producer: quarry-edge`, `origin`, `run_id`, `request_id`, `"zdr": false`, dense `producer_seq` starting at 1, and no `D10PROBE` marker. Run a ZDR run (`zdr: true` in the start body): the stream's message count does not change. Kill `model-nats` mid-run and restart it: Quarry keeps serving steps, the gap shows up as a `producer_seq` jump and `dropped` is logged once (rate limited).

**Effort.** M, 4.5 engineer-days (Quarry publisher 2.0, Model Plane topology, conf, provisioner and registry tests 1.5, compose and secrets 0.5, smoke script 0.5). **Risks and mitigations.** A new cross-plane credential (section 9 question 2, security review); `nats.conf` changes need a broker restart (document in the deploy evidence); the provisioner's registry harness extension is a test-code change in a Go service with no CI (this slice adds the CI job); pub-ack latency (acks are pipelined, the step path never awaits the broker); if Model-nats is down for more than 48 h the stream loses messages (`DiscardOld`, by design).

---

### S6. session-core persistence, `EventLog.QueryEvents`, model-gateway and gateway trace read (Model Plane owner: sibling (b); contract specified here)

**Goal.** Trace events are persisted idempotently and tenant-checked in session-core, and a run owner can read the trace by cursor through the gateway. ZDR runs never appear. This also gives the long-unimplemented `EventLog` contract its first server.

**Hard predecessors.** Migrations `0044_run_retention_receipt.sql` and `0045_zdr_waived_runs_org_index.sql` (both [U] today) must be committed first: the consumer reads `runs.retention_class`. S2 (twins, `eventlog.proto` compiled) and S5 (stream and consumer exist).

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `MPR/services/session-core/migrations/0046_run_trace_events.sql` and `.../revert/0046_run_trace_events.down.sql` | table `run_trace_events` | see SQL below; the revert file drops the table. Next free number: 0045 is the latest and is untracked, so treat it as taken | [NEW] |
| `SC/trace_nats.rs` | consts `STREAM_NAME = "MODEL_PLANE_RUN_TRACE"`, `DURABLE_NAME = "session-core-trace"`, `SUBJECT_FILTER = "mp.v1.trace.run.*"`; `run(pool, nats_url)`; pure `validate_trace_envelope(&Value) -> Result<TraceRow, Reject>`; `persist(&Pool, &TraceRow) -> Outcome` | new, modelled on `nats.rs` (pre-provisioned stream and durable, runtime credential cannot mutate topology). Validation: JSON object; `zdr` key **present and false** (absent or true is `Reject::ZdrNotExplicitFalse`); `event_type` in the seven trace names (anything else rejected); `producer == "quarry-edge"`; `producer_seq >= 1`; `run_id` a single subject token; payload at most 8 KiB. Insert: `INSERT ... SELECT ... FROM runs r WHERE r.id = $run AND r.org_id = $org AND r.retention_class <> 'zdr' ON CONFLICT (run_id, producer, producer_seq) DO NOTHING`; `user_id` is taken from the run row, never from the envelope. Zero rows then a lookup distinguishes `Duplicate` (ack), `RunUnknown` (`Nak` with a 2 s delay, because the run row can lag the first event; after `MaxDeliver` 5 it is dropped and counted), and `OrgMismatch` and `ZdrRun` (`AckKind::Term` immediately plus a labelled counter; `OrgMismatch` also logs ids only, it is a security signal). A validation `Reject` is also `Term`. A database error is `Nak` with a 2 s delay | [NEW] |
| `SC/eventlog_grpc.rs` | `EventLogService`, `query_events`, `append_event`, `subscribe_events` | new. `query_events`: `identity`, `authorize_operation(&caller, "session:read")`, `mp_eventlog::EventFilter` validation (**`run_id` required**, no cross-run listing in v1), resolve `thread_id` from `runs` and call `authorize_thread_owner(.., OwnerIntent::Read)` exactly as `replay_thread` does (4952 to 4960), cursor via `mp_eventlog::Cursor { offset = ingest_seq }`, `WHERE run_id = $1 AND org_id = $2 AND ingest_seq > $3 [AND event_type = ANY($4)] ORDER BY ingest_seq ASC LIMIT $5` (fetch limit+1 for `has_more`; `reverse` flips both), limit clamped to [1, 500]; rows map to `pb::Event` with the new fields and `payload` as `Any { type_url: "type.googleapis.com/model_plane.v1.BrowserTraceEvent", value: <JSON bytes> }` (the existing replay convention of JSON inside `Any`). `append_event` and `subscribe_events` return `Status::unimplemented` in v1 (the live tail is Quarry's SSE and the OrchestrationEvent stream) | [NEW] |
| `SC/main.rs` | `mod trace_nats; mod eventlog_grpc;`, supervised spawn of `trace_nats::run`, a retention sweeper task | add alongside the `nats::run` supervision at 92 to 110; sweeper deletes rows older than `SESSION_CORE_TRACE_RETENTION_DAYS` (default 30) in batches of 5,000 | [M] (one `mod` hunk at 35 in flight) |
| `SC/grpc.rs` | `HEALTH_SERVICE_NAMES` (the array at ~44 to 57, a hunk is in flight at 56), `serve` | register `EventLogServer`, add `"model_plane.v1.EventLog"` to the health names (array length 7 becomes 8) | [M] |
| `MPR/services/session-core/Cargo.toml`, `MPR/Cargo.toml` | dependency | add `mp-eventlog` | [C] |
| `SC/gdpr.rs` | org purge 230 to 400 | add `DELETE FROM run_trace_events WHERE org_id = $1` before `runs` (FK cascade would also do it; the explicit statement feeds `summary.trace_events`), extend the summary struct and its total (147 to 194). Per-user erasure is a deliberate no-op in session-core today (`gdpr.rs` 20 to 26), trace follows the same rule | [C] |
| `MGW/http_routes.rs` | route table near 218, new `get_run_trace` | `GET /v1/runs/:run_id/trace?cursor=&limit=&types=` calls `EventLog.QueryEvents` through the existing authenticated session client pattern (`authenticated_session_request`), returns `{ run_id, events: [<envelope JSON>], next_cursor, has_more }`; `grpc_status_to_http` for errors | [M] (route table and handler ranges are clear of the in-flight hunks) |
| `GW/domains/chat.rs`, `GW/domains/chat/streams.rs` | route table 74 to 78, new `run_trace` handler modelled on `run_events_replay` | `GET /api/v1/runs/{run_id}/trace` normalised to the repo envelope `{ data: { runId, events, hasMore }, meta: { cursor }, links }`; owner enforcement stays in Model Plane (the gateway forwards the verified session bearer), the gateway holds no trace state | [C] |
| `SPA/shared/api/run-console-client.ts` | new `getRunTrace(runId, { cursor, limit })` | typed reader; merging and panels are the UI workstream's | [C] |
| `MP/docs/decisions/ledger.md`, `MP/proto/model_plane/v1/eventlog.proto` | comments | amend the service comment (facade over the run trace log, `AppendEvent` and `SubscribeEvents` unimplemented in v1) | [C] |

**SQL** (`0046_run_trace_events.sql`)

```sql
CREATE TABLE IF NOT EXISTS run_trace_events (
    ingest_seq      BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id        TEXT        NOT NULL,
    run_id          TEXT        NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    org_id          TEXT        NOT NULL CHECK (org_id <> ''),
    user_id         TEXT        NOT NULL DEFAULT '',
    producer        TEXT        NOT NULL,
    producer_seq    BIGINT      NOT NULL CHECK (producer_seq > 0),
    event_type      TEXT        NOT NULL,
    schema_version  INTEGER     NOT NULL DEFAULT 1,
    occurred_at     TIMESTAMPTZ NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    resource_ref    TEXT        NOT NULL,
    correlation_id  TEXT        NOT NULL DEFAULT '',
    causation_id    TEXT        NOT NULL DEFAULT '',
    request_id      TEXT        NOT NULL DEFAULT '',
    agent_id        TEXT        NOT NULL DEFAULT '',
    agent_role      TEXT        NOT NULL DEFAULT '',
    agent_parent_id TEXT        NOT NULL DEFAULT '',
    origin          TEXT        NOT NULL DEFAULT ''
                    CHECK (origin IN ('', 'USER', 'AGENT', 'SCHEDULE', 'WEBHOOK', 'API')),
    payload         JSONB       NOT NULL DEFAULT '{}'::jsonb
                    CHECK (octet_length(payload::text) <= 8192),
    CONSTRAINT run_trace_events_producer_seq_uq UNIQUE (run_id, producer, producer_seq),
    CONSTRAINT run_trace_events_event_id_uq UNIQUE (event_id)
);
CREATE INDEX IF NOT EXISTS run_trace_events_run_idx ON run_trace_events (org_id, run_id, ingest_seq);
CREATE INDEX IF NOT EXISTS run_trace_events_received_idx ON run_trace_events (received_at);
```

**Optional sub-slice S6c: Model-side events join the same table.** Dual-write at `persist_orchestration_event` (`SC/orchestration_grpc.rs` [M] INSERT at 1735): in addition to the `events` insert, write the same browser and approval events into `run_trace_events` with `producer = 'session-core'` and a DB-assigned sequence (`BEFORE INSERT` trigger `assign_trace_producer_seq` filling `producer_seq = COALESCE(MAX(producer_seq), 0) + 1` per (run, producer), the single-writer pattern migration 0002 already relies on). This makes one read return the whole run. It is optional because it touches an [M] file owned by sibling (b); without it the SPA keeps reading Model-side events from the existing run-event stream and replay and reads Quarry-side events from the trace.

**API / contract changes.** `model_plane.v1.EventLog.QueryEvents` implemented (the first server); `GET /v1/runs/:run_id/trace` (model-gateway) and `GET /api/v1/runs/{run_id}/trace` (gateway). Response events use the S2 envelope; the cursor is opaque (`base64` of `{ "offset": ingest_seq }` by `mp-eventlog`, clients never parse it). Unknown event types must be tolerated by clients.

**Feature flag and rollout.** `SESSION_CORE_TRACE_CONSUMER=0` stops the consumer (default `1` once S5 shadow mode has been observed healthy); `SESSION_CORE_TRACE_RETENTION_DAYS` (30). The read routes are additive. Rollback: stop the consumer (messages wait in the stream for 48 h), revert routes; the table can stay.

**Tests**
- Pure (`SC/trace_nats.rs`): `validate_accepts_the_run_trace_golden`, `validate_rejects_an_absent_zdr_key`, `validate_rejects_zdr_true`, `validate_rejects_a_foreign_event_type`, `validate_rejects_a_foreign_producer`, `validate_rejects_producer_seq_zero`, `validate_rejects_an_oversized_payload`, `validate_rejects_a_dotted_run_id`.
- Postgres-backed, `#[ignore]` (the repo's pattern; run with `-- --ignored --test-threads=1`): `trace_persist_inserts_one_row_per_event`, `trace_persist_is_idempotent_on_redelivery`, `trace_persist_detects_a_producer_seq_gap` (insert 1, 2, 4 then read back the jump), `trace_persist_refuses_an_org_mismatch`, `trace_persist_refuses_an_unknown_run`, `trace_persist_refuses_a_zdr_run`, `trace_query_pages_by_ingest_seq_without_skips_or_repeats` (interleaved producers, reverse flag), `trace_query_requires_a_run_id`, `trace_query_denies_a_non_owner` (org B cannot read org A's run, same shape as `replay_thread`), `gdpr_org_purge_removes_trace_events`, `retention_sweeper_deletes_only_old_rows_in_batches`.
- model-gateway: `run_trace_route_requires_the_verified_bearer`, `run_trace_route_maps_unimplemented_and_not_found`. Gateway: `run_trace_route_normalises_the_envelope_and_cursor`, `run_trace_route_never_fabricates_an_empty_page_for_a_missing_run` (the same rule as `replay_run_events`).
- CI: a new Postgres-backed job modelled on the uncommitted `session-core-receipts` job (`model-plane-rust-ci.yml` [M] 127 to 169): `DATABASE_URL=postgres://session_core:session_core@localhost:5432/session_core_test cargo test -p session-core --bin session-core trace_ -- --ignored --test-threads=1`, failing below the expected count of passing tests.
- Commands: `cd "apps/Model Plane/rust" && cargo test -p session-core --bin session-core trace_` (non-ignored) and the ignored form above; `cargo test -p model-gateway run_trace`; `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test run_trace`.

**Definition of done.** All tests green including the Postgres job in CI; a run's trace is readable end to end by its owner and 404 for anyone else; killing and restarting session-core mid-run loses nothing and duplicates nothing.

**Deployment evidence.** Rebuild `session-core`, `model-gateway` and the gateway images with `--no-cache` for the first build (proto change). In a stack with Postgres: run a non-ZDR browser run with 10 steps (F-t2-001 working, or the S4 stand-in with a real Model run row); `GET /api/v1/runs/{run_id}/trace` returns events with `producer: quarry-edge`, dense `producer_seq` 1..N, matching `origin` and `request_id`; page with `limit=3` following `next_cursor` and assert no repeats or skips; restart session-core during the run and re-read: row count equals the published count minus `dropped`; read as a second org: 404; run a ZDR run: no rows (`SELECT count(*) FROM run_trace_events WHERE run_id = ...` is 0, verified through a service-owned query, not through a cross-plane read path).

**Effort.** L, 10 engineer-days (consumer and validation 2.5, migration, gdpr and retention 1.5, EventLog server 2.5, model-gateway and gateway routes 2.0, CI job and tests 1.5). **Risks and mitigations.** Identity-column ordering is commit order, so a slow transaction can expose a later `ingest_seq` before an earlier one commits (a reader paging by cursor could skip the late row); mitigation: the consumer inserts one row per statement (autocommit), single consumer, and the read uses `ingest_seq` only for pages already below the smallest in-flight id (document the residual risk; a reader that wants strictness re-reads the last page). Run row missing when the first events arrive (the Model run exists before the browser run starts, so this is rare; the `Nak` retries cover a short lag, and a permanent drop after `MaxDeliver` 5 is counted). A 0044/0045 merge-order mistake breaks the build (hard predecessor). `grpc.rs` and `main.rs` have in-flight hunks (health array, mod line): rebase carefully.

---

### S7. Application Plane projection (ADR-GATED 8, part 2; blocked until section 9 question 1 is answered)

**Goal.** Dashboards, inboxes and notifications can show "the agent is browsing: control, blocked egress, last activity, session state" reactively without anyone copying trace payloads into a second store. Option A of D13.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `MP/deploy/nats.conf` | `application-convex-model` (140 to 144) | add `"mp.v1.trace.run.*"` to `subscribe` (a plain subscription, no JetStream consumer, no provisioner change) | [C] |
| `AP/convex-core/convex/schema.ts` | `agentRuns` 430 to 460 | additive optional `browserActivity: v.optional(v.object({ lastProducerSeq: v.number(), lastEventType: v.string(), lastEventAt: v.number(), sessionState: v.optional(v.union("open","closed","expired","failed","cancelled")), control: v.optional(v.union("agent","human")), blockedEgress: v.optional(v.boolean()), steps: v.optional(v.number()), artifacts: v.optional(v.number()) }))`; counters documented as approximate | [C] |
| `AP/convex-core/convex/agentRuns.ts` | new `upsertBrowserActivity` (`internalMutation`) | patch an existing row only (never create a run row from a trace event); org check `args.externalOrgId === row.externalOrgId` else reject; monotonic guard `lastProducerSeq` only advances; reads stay behind the existing `getByRunId` with `requireViewerMembership` (`authz.ts`) | [C] |
| `AP/convex-core/nats-subscriber.js` | new `handleTraceEvent`, `subscribeToTopicOn(this.ncModelPlane, "mp.v1.trace.run.*", ...)` next to the run subscription at 331 to 341 | in-memory reducer per run (bounded `Map`, 5,000 runs, oldest evicted), flush at most once per run per 2 s and once on `BROWSER_SESSION_CLOSED`; parses only `event_type`, `producer_seq`, `run_id`, `org_id` and `payload.decision`, `payload.mode`, `payload.reason`; never stores payload text | [C] |
| `AP/convex-core/test/trace-projection.test.cjs` | tests | new | [NEW] |

**Tests** (`cd "apps/Application Plane/convex-core" && npx tsx --test test/trace-projection.test.cjs`, plus `npx tsx --test test/nats-subscriber.test.cjs`): `reducer_flushes_at_most_once_per_window`, `final_state_flushes_on_session_closed`, `an_org_mismatch_is_rejected`, `lastProducerSeq_never_moves_backwards`, `missing_events_leave_counters_approximate_but_state_correct`, `a_trace_event_for_an_unknown_run_creates_nothing`, `projection_holds_no_payload_text` (a schema assertion), plus `pnpm typecheck` and `pnpm lint` in `convex-core`.

**Definition of done.** A reactive `useQuery(api.agentRuns.getByRunId)` shows `browserActivity` changing while a run browses; membership removal revokes visibility (existing `requireViewerMembership`, matrix rule 10); no payload text exists in Convex.

**Deployment evidence.** Rebuild `convex-core` (memory: `docker restart convex-gateway` re-pushes functions; wait for "Convex functions ready!"), restart `model-nats` for the permission, run a browser run, watch the Convex row update at most every 2 s, remove the viewer from the org and assert the query now denies.

**Effort.** M, 4.5 engineer-days. **Risks.** Convex write amplification (debounce bounds it to ~0.5 writes per run per second); ordering across subjects (latest-wins monotonic guard); Space visibility is deliberately not built here (sibling (a)).

---

### S8. F-t2-004 phase 2: telemetry contract, exporter flag, ZDR sampler, late-ZDR registry, span processor (Rust and Go)

**Precondition (hard).** D4 S-4 (known leaks), S-5 (carrier: `zdr` span field, `ZdrMarkLayer`, event filter, path-only request span) and S-6 (reqwest URL stripping, lint and baseline) are merged: "ship the log-field sweep first". Phase 2 must also land before `OTEL_EXPORTER_OTLP_ENDPOINT` is set in any real environment and before F-t2-029's span vocabulary. Independent of S1 to S7.

**S8a. Contract and flag (2.0 days)**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `Q/docs/TELEMETRY_FIELD_CONTRACT.md` | doc | the three lists (D14), the `error_class` vocabulary, the rule "span attributes use the same vocabulary", the `QUARRY_TELEMETRY_EXPORT` flag, and how each plane mirrors it | [NEW] |
| `Q/crates/quarry-runtime/contracts/telemetry-field-contract.json` and `.sha256` | golden `{ version: 1, safe: [...], zdr_sensitive: [...], never: [...] }` | pinned through `scripts/tests/zdr-contract-sync.test.sh` GOLDENS and VENDORED rows (copies for `QE/zdr_log.rs` constants test and `Q/pkg/quarryotel/testdata/`) | [NEW] |
| `QE/zdr_log.rs` (D4) | `SENSITIVE_FIELDS` | a test asserts the constant equals the golden's `zdr_sensitive` | [NEW] (D4's file) |
| `QE/telemetry.rs` | `init_telemetry` 25 to 56 | read `QUARRY_TELEMETRY_EXPORT` (`on` default, `off`): `off` never calls `try_init_otel` even when the endpoint is set and logs one line `telemetry export disabled by flag` | [C] |
| `Q/pkg/quarryotel/quarryotel.go` | `Init` 39 to 93 | same flag: `off` returns the no-op shutdown after setting only the propagator | [C] |
| `MPR/crates/mp-telemetry/src/lib.rs` | `init` | follow-up owned by sibling (b) with the same flag name and golden (0.26 API) | [C] (not in this slice) |

**S8b. Rust sampler, registry, processor (3.5 days)**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QE/otel_zdr.rs` | `ZdrTraceRegistry` (bounded FIFO set, default cap 100,000, `QUARRY_TELEMETRY_ZDR_REGISTRY_CAP`), `ZdrSampler` (implements `opentelemetry_sdk::trace::ShouldSample`; `Drop` when attribute `zdr` is true, the parent trace id is registered, or export is off; else `ParentBased(AlwaysOn)`), `ZdrSpanProcessor<P: SpanProcessor>` (`on_end` drops registered or `zdr = true` spans, strips attributes whose keys are in `zdr_sensitive` or `never` from all others), `flag(trace_id)` | new, declared in `lib.rs` so integration tests can import it | [NEW] |
| `QE/lib.rs` | `pub mod otel_zdr;` | one line | [M] |
| `QE/telemetry.rs` | `try_init_otel` 58 to 64 | build the provider with `.with_sampler(ZdrSampler::new(..))` and `.with_span_processor(ZdrSpanProcessor::new(BatchSpanProcessor::builder(exporter).build()))` instead of `.with_batch_exporter(exporter)` (exact 0.29 builder and trait signatures to confirm against the pinned crate, **[UNVERIFIED]**) | [C] |
| `QE/zdr_log.rs` (D4) | `ZdrMarkLayer` | on recording `zdr = true` call `ZdrTraceRegistry::flag(trace_id)` (trace id read from the span's OTel context) and, on creation of a child span of a ZDR scope, add the attribute `zdr = true` to the tracing-opentelemetry `OtelData.builder` so both the sampler and the processor see it. The layer must sit **after** the OTel layer in the registry chain | [NEW] (D4's file) |
| `QE/Cargo.toml` | dev-dependencies | `opentelemetry_sdk` with its testing in-memory exporter feature (name to confirm for 0.29, **[UNVERIFIED]**) | [C] |

First test in S8b is the **spike** that decides the registry's necessity for Rust: `sampler_sees_a_zdr_field_recorded_after_span_creation` (passes or fails with the pinned tracing-opentelemetry 0.30 and SDK 0.29). Result recorded in the PR; the registry ships either way because Go needs it.

**S8c. Go mirror (2.5 days)**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `Q/pkg/quarryotel/zdr.go` | `ZdrSampler` (wraps `ParentBased(AlwaysSample())`, drops on attribute `zdr=true` or registered trace id), `zdrProcessor` (an `sdktrace.SpanProcessor` wrapper that does not forward `OnEnd` for registered or flagged spans), `MarkZDR(ctx)` (sets attribute `zdr=true` on the current span and registers its trace id in a bounded FIFO set, cap 100,000), `redactingExporter` (wraps `SpanExporter`; a thin type embedding `sdktrace.ReadOnlySpan` overrides `Attributes()` to remove contract keys) | new | [NEW] |
| `Q/pkg/quarryotel/fieldcontract.go` | constants from the golden | new | [NEW] |
| `Q/pkg/quarryotel/quarryotel.go` | `Init` | `WithSampler(ZdrSampler{...})`, `WithSpanProcessor(zdrProcessor{sdktrace.NewBatchSpanProcessor(redactingExporter{exporter}, ...)})` | [C] |
| `Q/pkg/quarryotel/quarryotel_test.go`, `zdr_test.go` | tests | new (there are none today); uses `tracetest.NewInMemoryExporter` from the existing `otel/sdk` module (no new dependency) | [NEW] |
| `Q/services/quarry-control/internal/...` (job and webhook handlers) | call sites | where a ZDR posture is parsed after a span started, call `quarryotel.MarkZDR(ctx)` (the places that read the body or claim `zdr` flag; list with `grep -rn "zdr\|Zdr" services/quarry-control/internal`) | [C] |
| `.github/workflows/quarry-v2-ci.yml` | matrices at 245 to 249 and 313 to 317, new hard job | add `pkg/quarryotel` to the govulncheck and coverage matrices and a **non-`continue-on-error`** job `quarryotel-test` running `go test ./...` there | [C] |

**Tests (S8 overall).** Rust: `zdr_attribute_span_is_dropped`, `registered_trace_spans_are_dropped_on_end`, `children_of_a_registered_trace_are_dropped`, `sensitive_attributes_are_stripped_from_non_zdr_spans`, `never_class_attributes_are_stripped_even_when_not_zdr`, `registry_evicts_oldest_first_at_capacity` (cap 3, FIFO not LRU: touching an id does not refresh it), `registry_is_bounded`, `export_off_builds_no_exporter`, `field_constants_equal_the_golden`; integration `QE/tests/zdr_span_export.rs` [NEW]: a ZDR scrape request and a ZDR agent step produce zero spans in the in-memory exporter while the non-ZDR twin produces spans, and no captured log line contains the probe URL. Go: `TestZdrSamplerDropsFlaggedSpans`, `TestMarkZDRDropsTheWholeTraceAtEnd`, `TestRegistryFifoEviction`, `TestRedactingExporterStripsContractKeys`, `TestExportOffIsNoop`, `TestFieldConstantsEqualGolden`. Commands: `cargo test -p quarry-edge otel_zdr zdr_span_export`; `cd "apps/Ingestion Plane/Quarry-v2/pkg/quarryotel" && go test ./...`; `bash scripts/tests/zdr-contract-sync.test.sh`.

**Definition of done.** ZDR scrape and ZDR browser step export zero spans and no URL-bearing log line (the audit's own acceptance); the CI job is hard; the contract doc and golden exist and are pinned; with `QUARRY_TELEMETRY_EXPORT=off` no exporter is constructed.

**Deployment evidence.** Add a throwaway compose override `Q/deploy/compose/docker-compose.otel-test.yml` [NEW] with an OpenTelemetry Collector exposing a debug or file exporter, set `OTEL_EXPORTER_OTLP_ENDPOINT` on `quarry-edge` and `quarry-control`, rebuild both, run one non-ZDR and one ZDR scrape plus browser step (identities from the D4 S-4 probe flow). Assert the collector output contains spans for the non-ZDR request and **none** for the ZDR trace ids, and `docker logs` contains no probe URL for the ZDR identities. Repeat with `QUARRY_TELEMETRY_EXPORT=off`: the collector receives nothing at all.

**Effort.** M/L, 8.0 engineer-days (8a 2.0, 8b 3.5, 8c 2.5). **Risks.** Rust sampler timing (spike above); tracing-opentelemetry internals (`OtelData` is public but version-pinned); partial adoption gives false confidence, so the integration test and the CI job are the enforcement; over-suppression hides incidents (keep content-free counters outside the filter: the gateway already has live Prometheus counters, Quarry's arrive with F-t2-029). Ordering with F-t2-029: no span may be added before S8b merges; F-t2-029's span vocabulary PR must add its attribute names to the contract golden in the same change.

---

### S9. Cross-plane contract tests and volume soak

**Goal.** One place proves the four planes agree on the envelope, the subject permissions, the ZDR rule and the read API, and that volume and ordering hold at a realistic load.

| Contract test | Proves | Where | Runs in |
|---|---|---|---|
| C1 Envelope golden | proto, Rust twin, Go twin, Quarry mapper, session-core validator all accept and agree on one canonical envelope | `MP/api/contracts/run-trace-envelope.golden.json` plus consumers (S2, S4, S6) | `contract-goldens.yml` (sync script), `cargo test`, `go test` |
| C2 Mapper vs golden | Quarry's `to_model_envelope` output equals the golden for the same logical event | `QR/trace_projection.rs` test (S4) | `quarry-v2-ci.yml` |
| C3 Principal least privilege | `quarry-trace-producer` publishes only `mp.v1.trace.run.*` | `TestQuarryTracePrincipalCanOnlyPublishTraceSubjects` and the NATS smoke script (S5) | new provisioner CI job |
| C4 Registry completeness | every JetStream publisher including Quarry's is registered | provisioner tests (S5) | same |
| C5 ZDR row | ZDR runs produce no NATS message, no `run_trace_events` row, no span; replay is in-memory only | Quarry `a_zdr_run_*` tests (S3, S4, S5), session-core `trace_persist_refuses_a_zdr_run`, S8 `zdr_span_export`; plus one new row in D4's ZDR feature matrix (`browser_trace_publish`: refused under ZDR; `browser_trace_replay`: in-memory, live run only) | Quarry and session-core CI |
| C6 Read API shape | model-gateway and gateway responses match a fixture the SPA type is generated against | `GW` test and `SPA/shared/api/run-console-client.ts` type test (S6) | `verevonv3-ci.yml` |
| C7 Request id end to end | one id appears in the gateway response, Quarry log, trace event and session-core row | scripted E2E below | manual evidence, then a Playwright spec `tests/e2e/run-trace.spec.ts` [NEW] (Playwright has no CI gate today, S9 H8, so the spec is documentation of the flow) |

**Soak test** `scripts/tests/run-trace-soak.sh` [NEW] plus a Rust test binary `Q/lab/evals/src/bin/quarry-trace-soak.rs` [NEW] (the `lab/evals` crate already hosts benchmark bins; build via `cargo run -p quarry-evals --bin quarry-trace-soak`): 50 concurrent fake runs times 40 steps against the stub driver with NATS up, one injected 5-second NATS outage. Assertions: per run `producer_seq` dense except the logged gap; zero duplicates in `run_trace_events`; row count equals published minus `dropped`; consumer lag p99 under 2 s; Quarry step latency p99 unchanged (within 5 percent) with the publisher on versus off; session-core CPU and Postgres write rate recorded.

**Effort.** M, 4.0 engineer-days. **Deployment evidence for the whole workstream (the repo counts a claim only with images, a running stack and an end-to-end flow).** With all images rebuilt: start the stack, sign in as a non-ZDR user, start a chat browser turn that browses three pages (requires F-t2-001; before it, the S4 service-token stand-in with a real Model run), send `x-request-id: e2e-d10-<uuid>` on the start call. Observable assertions: (1) `GET /api/v1/runs/{run_id}/trace` returns Quarry events with that request id, `origin` `AGENT`, `agent.id` set, dense `producer_seq`; (2) the Quarry SSE `GET /api/v1/browser/sessions/{id}/events/stream` with a mid-run reconnect shows no gap; (3) Convex `agentRuns.getByRunId` shows `browserActivity` (if ADR 8 is ratified); (4) repeat as a ZDR user: SSE works live, trace GET returns an empty page, `nats stream info` count unchanged, collector shows no spans, logs show no URL.

## 5. Dependencies and ordering

### 5.1 Hard and soft dependencies

| This design | Depends on | Kind | Detail |
|---|---|---|---|
| S8 (F-t2-004 phase 2) | D4 S-4, S-5, S-6 (F-t2-004 phase 1: known leaks, `zdr` span field and `ZdrMarkLayer`, event filter, lint) | **Hard** | Audit dependency note 8: ship the log-field sweep first. S8's `ZdrMarkLayer` hook, `SENSITIVE_FIELDS` golden and `error_class` vocabulary are D4's outputs |
| S8 | F-t2-029 span vocabulary | **Ordering rule** | No span may be added before S8b merges; F-t2-029 adds its attribute names to the contract golden in the same change. Enabling `OTEL_EXPORTER_OTLP_ENDPOINT` anywhere before S8b is forbidden (or the request span stays at DEBUG) |
| S6 | session-core migrations `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` committed (both [U] today) | **Hard** | The consumer reads `runs.retention_class`; also the numbering of 0046 |
| S6 | S2 (twins, `eventlog.proto` compiled) and S5 (stream, durable, principal) | **Hard** | |
| S4 | S2 (golden) and S3 (per-run seq, `open_run`) | **Hard** | |
| S5 | S4 (mapper and allowlist) | **Hard** | The publisher must never see an un-projected payload |
| S7 | S5, and ADR 8 part 2 (section 9 question 1) | **Hard** | |
| End-to-end evidence for any AI run (S4 to S9) | F-t2-001 production grants and delegated bearer (D1) | **Hard for the AI flow only** | Production Quarry rejects runs without a grant (`main.rs` 183 to 201), so a Model-driven run cannot be exercised end to end before D1. Build and unit and integration tests do not depend on it; the S4 service-token stand-in covers the interim |
| S3, S4 | D2 S2 (module split of `agent_routes.rs`) | Soft | Same code can be written in the monolith at more merge cost; D2 asks for a two-working-day freeze, merge D10 after it |
| S4 | D2 S3 (initiator, epoch, `StepReceipt` and checkpoint and timeline fields, migration 0010) | Soft | Both add optional fields to the same structs; merge D2 first or write one combined hunk. `initiated_by` and `control_epoch` appear in trace payloads only after D2 S3 |
| S3, S4 | D2 S4 (`finish_run` single body, run registry, `agent.expired`) | Soft | `EventSink::close_run` is one line inside `finish_run`; the sweeper passes `reason: "expired"` |
| S1 | D4 S-5 (`make_request_span`) | Soft | Shared hunk at `QE/routes.rs` 303 (see 5.2) |
| S4 | F-t2-021 capture-time redaction | Soft | The allowlist projection does not need it. The owner-only SSE still carries legacy raw `action.started` payloads until F-t2-021 lands |
| S5 (`dropped` counter) | F-t2-029 phase 1 (metrics recorder) | Soft | Until a recorder exists the counter is a rate-limited log field |
| S4 onward | D3 (driver stability) | None | No shared code; D3's blocked-state plumbing (its S9) can ride the same envelope later |
| S6 read API | the card's first slice (gateway proxies for `receipts`, `egress-receipts`, `procedure`; SPA panels) | Parallel | Different routes in the same router block; no data dependency |

### 5.2 Merge-conflict hotspots (files and ranges shared with other workstreams)

| File | Range(s) this design edits | Shared with |
|---|---|---|
| `QE/agent_routes.rs` [C] (becomes `agent_routes/` after D2 S2) | `RunEntry` 77 to 107; `StartRunBody` 346 to 383; `is_user_facing_event` and `event_name` 949 to 975; `start_run` 1273 to 1663 (emit 1642 to 1650); `set_browser_session_control` 1733; `step` 2228 to 2409 (receipts 2338 to 2386); `agent_events_stream` 2658 to ~2770; tab routes; `close_run` 3562 to 3655 | D2 (S1, S3, S4: `step`, `start_run`, `close_run`, timeline), D1 (`validate_run_grant` 314 to 339, grant revalidation in `step`), F-t2-008, F-t2-011, F-t2-021, F-t2-036 (receipt construction in `step`) |
| `QE/routes.rs` [M] | 297 to 306 (layer chain) | D4 S-5 (`make_request_span`, `mark_floor` at 286), D2 |
| `QE/main.rs` [M] | ~405 to 445 (NATS wiring, S3 and S5 blocks) | D2 S4 (323, 337, 1284 to 1290), F-t2-029 (84 to 111, 1513 to 1517), in-flight hunks at 244 and 984 to 1232 |
| `QE/telemetry.rs` [C] | 25 to 64 | D4 S-5 (fmt layer), F-t2-029 (recorder), S8 |
| `QE/lib.rs` [M] | one `pub mod` line per slice | D2, D4, in-flight line 18 |
| `QR/events.rs` [C] | whole `EventSink` | D4 (marks the span at posture decision points, not here), F-t2-029 (lease drain calls `close_run`) |
| `QR/step_receipts.rs` [C] | 39 to 71, 130 to 160, 183 to 210 | D2 S3 (migration 0010 fields), F-t2-021, F-t2-036 |
| `QC/event.rs` [C] | `Event`, `EventType` | F-t2-029 (new event names are unlikely but enum edits conflict textually) |
| `Q/docs/openapi.yaml` [M] | SSE path, `StartRunRequest`, projection | D2 (263 to 345, 2517), D5, F-t2-064 |
| `Q/pkg/quarryotel/quarryotel.go` [C] | `Init` 39 to 93 | F-t2-029 phase 3 (span vocabulary) |
| `GW/domains/browser.rs` [C] | router 368 to 460 (new route), 1730 to 1782 (stream pattern) | D2 S3 (`run_action` 663 to 760, `forward_quarry_failure` 3873), D1 (session grant 1235), the card's first-slice owner (receipts, egress, procedure routes in the same router block): agree one route-table PR order |
| `GW/main.rs` [C] | router tail ~164, `make_http_trace_span` 194 to 202 | D4 S-5 (tracing init 47 to 50, same span function), F-t2-046 (rate-limit layer 143 to 148) |
| `GW/upstream.rs` [C] | 1579, 1777, 2883 | F-t2-003 (`authorized_org_id` 324 to 329, `x-zdr` 1787 to 1789), D1 |
| `MP/proto/model_plane/v1/events.proto` [C] | `Event`, `EventType` | sibling (b) and D8 (shared browser backend, which proposes run events `BrowserRunPausedByUser`, `BrowserRunResumed`, `BrowserAttachSharedJar` without numbers): allocation convention, 14 to 18 and 150 to 159 are D10's; new event types from other workstreams take 160 and up and any further envelope field takes 21 and up |
| `MP/deploy/nats.conf` [C], `MPG/services/nats-provisioner/main.go` [C] | new principal block, `session-core-runtime` block 35 to 58, `application-convex-model` 140 to 144, stream and consumer lists | sibling (b), D1 (broker is gRPC, no NATS change expected), Application Plane NATS owners |
| `SC/grpc.rs` [M] | 3967 to 4003, 7184 to 7204, health array ~44 to 57 and `serve` | sibling (b) (many in-flight hunks; the health array line has one) |
| `SC/main.rs` [M], `SC/gdpr.rs` [C], `SC/orchestration_grpc.rs` [M] | `mod` list, spawns; org purge 230 to 400; INSERT 1735 (S6c only) | sibling (b) |
| `MGW/http_routes.rs` [M] | route table 218, new handler near 1278 | sibling (b) |
| `scripts/tests/zdr-contract-sync.test.sh` [M] | GOLDENS 48 to 118, VENDORED rows | D4 (matrix golden), D5, the in-flight QB-T09/T10 rows; the 28 untracked fixtures must land together (S9 H1) |
| `.github/workflows/model-plane-rust-ci.yml` [M], `quarry-v2-ci.yml` [C] | new jobs only | in-flight `session-core-receipts` job |
| `AP/convex-core/{schema.ts,agentRuns.ts,nats-subscriber.js}` [C] | `agentRuns` table, new mutation, new subscription | sibling (a) (Space presence and read markers live in the same Convex project) |

### 5.3 Ordering

Day-one parallel starts: S1, S2 (including S2a gate repair), S8a (after D4 phase 1 is merged). Then: S3 (after S1 is merged or in parallel on separate files), S8b and S8c (independent track), S4 (after S2 and S3), S5 (after S4), S6 (after S5 and the 0044/0045 commit), S7 (after S5 and ADR 8 part 2), S9 closes. A single engineer needs about 9 to 10 weeks; with an Ingestion engineer on S1, S3 to S5, S8, a Model Plane engineer on S2 and S6 and a gateway or Application engineer on the S1 gateway half and S7, the critical path is S2, S3, S4, S5, S6 at about 28 engineer-days, roughly 6 calendar weeks.

---

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

Statement per architecture rule (CLAUDE.md and `apps/master-ownership-matrix.md`):

- **Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, entitlements.** Untouched. Identity in Quarry still comes only from the verified token (`claims.org_id`, `actor_id`, `is_service()`); the request id, `trace.run_id`, `agent` and `origin` are labels and never an identity or membership source. Session-core takes `org_id` and `user_id` for a trace row from the run row, not from the envelope.
- **Data Plane v2 owns documents, chunks, embeddings, retrieval, graph, wiki, source traces.** Untouched. Trace events are run telemetry, not knowledge; they carry ids, enums, counts and a URL origin, no page content. (The Data Plane's own "source traces" are a different concept and are not written.)
- **Ingestion Plane captures evidence and persists durable knowledge through Data Plane contracts only.** Quarry persists nothing new: the replay ring is memory, the new checkpoint keys live in Quarry's own JSONB `state`, and proof stays in its own receipts and timeline tables. Nothing here writes knowledge.
- **Provider actions go through integration-corev2's actions surface.** Untouched.
- **Model Plane owns reasoning, sessions and runs, inference, execution loops, capabilities, sandboxes, browser grants, cost.** It owns the run trace: the contract (`model_plane.v1.Event`), the stream, the persistence table, the read API. Quarry **publishes** on NATS with a publish-only principal and never writes Model Plane storage; session-core is the only writer of `run_trace_events` and the only reader through `EventLog.QueryEvents`.
- **Application Plane owns collaborative and realtime workspace projections and notifications.** S7 is a projection that only narrows access (matrix rule 10), holds no payloads, and never becomes a grant. ADR-GATED 8.
- **Frontend Plane owns Verevon v3 UI plus the same-origin gateway and BFF.** The gateway proxies, normalises envelopes (`{data, meta, links}`) and forwards a request id; it keeps no trace state and no upstream secrets.
- **Support Plane.** Not involved (no session, user or billing state).
- **Channel Plane.** Not involved; nothing here builds against it.
- **No direct database crossing.** Quarry to Model Plane is NATS only; session-core reads its own table; Convex is fed from NATS; the gateway calls model-gateway over HTTP; no plane reads another plane's tables. Evidence queries that count rows are run as service-owned queries inside the owning plane, not through a cross-plane read path.
- **No independent embeddings or reranking.** N/A.
- **Quarry executes or rejects, Model proposes.** The trace carries **consequences** of Quarry decisions (receipts, egress decisions, artifacts, control changes), never a request that Quarry would execute, never a grant, never a command channel back. `trace.agent` and `origin` do not change what Quarry accepts: `origin_is_not_authz` (S4) fails the build if admission or budget code reads `origin`. A rejected action is *more* visible (a `Skipped` or failed receipt becomes an event). Risk to watch: a UI that reads `origin = USER` as proof of human authority; the field documentation says it is a descriptive trigger and D2's per-step `initiated_by` plus control epoch is the control fact.
- **ZDR must propagate through every content-persisting boundary.** Boundary by boundary:
  - NATS: the publisher refuses ZDR at its door, `emit_for_zdr` already routes ZDR to the live path, the envelope declares `"zdr": false` explicitly (the Rust twin defaults an absent key to false, so the consumer treats absence as a violation), and Model-nats `natsx` drops `zdr: true` envelopes anyway (`natsx/publisher.go` 56 to 60).
  - Database: the consumer terminates `zdr` envelopes and also skips `runs.retention_class = 'zdr'`; no row is ever written for ZDR; receipts for ZDR runs are still not appended (`agent_routes.rs` 2360).
  - Memory: the bounded replay ring holds what the 256-slot broadcast channel already holds and is freed at `close_run` (ADR 4 confirmation, D9).
  - Logs and spans: D4 S-4..S-6 plus S8 (sampler, registry, processor, exporter flag, contract); the request id is a non-content field and is allowed in ZDR logs.
  - Convex: nothing is published for ZDR runs, so nothing is projected.
  - Caches: none added.
  - Rules at risk: (1) the existing Ingestion-bus exposure of raw agent payloads for non-ZDR runs (closed by D10 in S4, with a kill switch); (2) the in-memory ring if ADR 4 prefers "refused" (one-line change); (3) allowlist drift (golden and property tests); (4) `ActionRuntime::emit_action_event` using `emit()` (the single minting `EventSink` and its tests close it, and F-t2-023 must route through `emit_for_zdr`); (5) a new cross-plane NATS credential (section 9 question 2).

---

## 7. Cross-session interfaces

### (a) Verevon Space

| Shared piece | Proposed owner | What (a) consumes or must respect | What the user must reconcile |
|---|---|---|---|
| Trace read authorization | Model Plane (session-core `authorize_thread_owner`) | v1 is owner-only (the run's thread owner). A Space member reading a teammate's run is **not** granted by this design | Whether a Space member may read a teammate's AI run trace and its Quarry events (blocks S7 and any wider ACL; ADR 8 part 2, F-t2-072) |
| Space sharing of a run | (a) with Control Plane | Read-through only: the owning plane re-checks membership via a Control Plane decision at read time; the Application Plane may cache a decision only to narrow it (matrix rule 10); never copy trace rows into a Space | The resource taxonomy name for the shared object (`quarry_run` is org-team-shared today; a trace resource would be `model_run`) |
| Presence and live "agent is browsing" chips | Application Plane (S7 projection) | Reads `agentRuns.getByRunId().browserActivity` through `requireViewerMembership`; Space presence already lives in the same Convex project (`AP/convex-core/test/space-presence.test.cjs`) | Whether Space presence and the run chip share one table or stay separate |
| ZDR-safe telemetry contract | This design (S8a) owns `TELEMETRY_FIELD_CONTRACT.md` and the golden | (a) uses the same three lists and `error_class` for any Space telemetry; each plane mirrors, never shares code | Nothing, unless (a) needs extra safe fields: propose them as additions to the golden |
| Space-scoped agent PC and browser | (a) | Quarry runs started inside a Space carry the Model run id like any other; `trace.origin` for Space-triggered runs is `USER` or `AGENT` by who started them | Whether a Space-owned (not user-owned) browser run is allowed; D2 ADR 3 (shared sessions) decides ownership, this design follows |

### (b) Model Plane and chat/agents

| Shared piece | Proposed owner | What (b) implements | What the user must reconcile |
|---|---|---|---|
| Start-run trace context | (b), in `EXC/quarry_agent.rs` `StartRunRequest` (180 to 192) | Send `trace: { run_id, agent: { id, role, parent_id }, origin }` on `POST /v1/agent/runs`; send `x-request-id` on start, step and close; send `initiated_by: "agent"` (D2); never send `trace` from a user token | Where the origin mapping of D11 lives in the Model Plane start path (one function) |
| Run origin mapping | (b) | chat, space, support and agent-run-console turns by a human then `USER`; cron and routines (`0028_space_scoped_cron.sql`) then `SCHEDULE`; background review and dreaming then `AGENT`; `mail_task` then `WEBHOOK`; direct service callers then `API`. Thread `origin` (surface) is untouched | Whether `SCHEDULE` runs are allowed to browse at all before the automation ADR (topic 9: unattended runs are draft-only through Wave 2 by owner ruling) |
| session-core trace consumer, `EventLog` server, retention, GDPR | (b) staffs S6; D10 specifies | Implement S6 as written; keep `0046` after the committed 0044/0045 | **Who staffs S6** (L, 10 days, Model Plane code in files with in-flight diffs) and whether the optional S6c dual-write (Model-side events into the same table) is wanted |
| model-gateway and gateway trace routes | (b) for model-gateway, Frontend for the gateway | `GET /v1/runs/:id/trace`, `GET /api/v1/runs/{id}/trace` | None |
| Request id in Model Plane services | (b) | model-gateway and execution-core adopt and forward `x-request-id` (the same validity rule) | None |
| Live Model-side stream | (b) | `OrchestrationEvent` (tags 10 to 23) stays the chat panel's live source. Optional later: session-core fans trace events into it as a new tag 24 `TraceEventRecorded`; D10 recommends **not** now: the SPA merges the Model stream and the Quarry SSE by (producer, producer_seq) and `ts`, `causation_id` | Whether the chat panel needs one stream (then tag 24 plus S6c) or may merge two on the client (recommended) |
| `mp-telemetry` adoption (otel 0.26 API) | (b) | Same flag name `QUARRY_TELEMETRY_EXPORT` (or a Model-prefixed twin), same golden, sampler written for 0.26 traits | None |
| Event numbering | D10 owns `EVENT_TYPE_*` 150 to 159 and `Event` fields 14 to 18; (b) uses 160 and up and 21 and up | Record in the ledger | None |
| Approvals context (F-t2-033) | (b), later | An approval card can show the egress decisions and receipt of the step through the trace; no contract change needed beyond this design | None |

SPA consumer contract (UI workstream, not designed here): render by `agent.id` groups, order within a producer by `producer_seq`, show a gap marker on a `producer_seq` jump and a banner on SSE `replay_gap`, label ordering across producers as approximate, never display `origin` as an authority claim, add the `src/shared/actions` contract before wiring any new operation.

---

## 8. What can proceed WITHOUT an ADR or decision, and what is blocked

| Work | Status | Decision it waits for |
|---|---|---|
| S1 request id (Quarry, Go parity, gateway) | **Proceeds now** | none |
| S3 replay ring, per-run seq, `close_run` fix, gateway events proxy | **Proceeds now** | none (ZDR ring default is allowed; ADR 4 can flip it with one line) |
| S8a/b/c telemetry contract, flag, sampler, registry, processor | **Proceeds once D4 phase 1 is merged** | none (ADR 4 wording only) |
| S2 envelope contract (proto, twins, golden, gate repair) | **Proceeds on the audit's recommended default** | Ratify ADR 8 part 1 (extend `model_plane.v1.Event`); additive, but field numbers are a permanent allocation, so record the ledger entry |
| S4 origin, trace context, projection, bus exclusion | Proceeds after S2 and S3 | none beyond the ADR 8 part 1 ratification; owner to confirm autocomplete-core ignores agent types |
| S5 publisher and NATS topology | **Build proceeds; enabling in a shared environment is blocked** | Security review of the publish-only Model-nats principal (section 9 question 2) |
| S6 persistence and read API | Proceeds after S5, 0044/0045 committed and a staffed Model Plane owner | ADR 8 part 1 ratified before production; ownership (question 4) |
| S7 Application Plane projection | **Blocked** | ADR 8 part 2 (question 1) |
| Sharing a run with Space members, non-owner trace reads | **Blocked, not designed** | ADR 8 part 2, resource taxonomy (F-t2-072) |
| Merging Model-side events into the trace (S6c) and a live fan-in tag 24 | Optional, **needs a decision** | question 4 |
| Persisting origin on Go job rows (crawl, batch, schedule) | Deferred | needs a migration number allocated at integration |
| F-t2-004 phase 1 | Not this design (D4) | none |

---

## 9. Open questions for the user (only real blockers)

1. **ADR 8 part 2.** May a Space member (or any org teammate) read another user's AI run trace, and is the Application Plane projection of D13 option A (coarse Convex summary, read-through ACL) acceptable? Blocks S7 and every non-owner read. Default if unanswered: owner-only reads, no projection.
2. **Cross-plane credential.** Is a publish-only principal `quarry-trace-producer` on the Model-nats broker (subject `mp.v1.trace.run.*` only, no JetStream API rights) acceptable for Quarry, or must the trace go through a bridge? Blocks enabling S5 outside a dev stack; the recommendation is the principal because a bridge adds an unowned service and keeps raw payloads on the Ingestion bus.
3. **ZDR and the replay ring (ADR 4 confirmation).** Is an in-memory, live-run-only replay ring acceptable for ZDR runs (recommended), or must ZDR runs have no replay at all? A one-line flip either way; asked because ZDR contracts are strict.
4. **Ownership of S6.** Who builds the session-core trace consumer, the `EventLog` server and the Model Plane routes (about 10 engineer-days in Model Plane files that carry uncommitted diffs), and is the optional dual-write (S6c) wanted so one read returns Model-side and Quarry-side events together? Blocks scheduling S6.

## 10. INTERFACE SUMMARY (for the integrating architect)

**Files touched.**
- Ingestion: quarry-edge (`request_id.rs`, `trace_config.rs`, `otel_zdr.rs` new; `agent_routes.rs`, `routes.rs`, `main.rs`, `telemetry.rs`, `lib.rs`); quarry-runtime (`events.rs`, `event_bus.rs`, `nats_event_bus.rs`, `step_receipts.rs`; `trace_projection.rs`, `model_trace_publisher.rs`, `request_context.rs` new); quarry-core (`event.rs`; `trace.rs` new); Go `quarryotel`, `httpx`, `quarrycontracts`; compose, openapi, CONTRACTS.
- Model Plane: `events.proto`, generated Go and Python (legacy TS if still built), `mp-events` and `go/pkg/envelope` twins, `mp-contracts/build.rs`; session-core (`trace_nats.rs`, `eventlog_grpc.rs` new; `grpc.rs`, `main.rs`, `gdpr.rs`; migration `0046_run_trace_events.sql`); model-gateway `http_routes.rs`; `nats.conf`; nats-provisioner and registry tests; compose; bootstrap script.
- Frontend: gateway (`request_id.rs` new; `browser.rs`, `chat.rs`, `upstream.rs`, `main.rs`, `config.rs`); SPA clients only.
- Application: convex `schema.ts`, `agentRuns.ts`, `nats-subscriber.js`.
- CI: `buf.yml`, `quarry-v2-ci.yml`, `model-plane-rust-ci.yml`.

**New contracts.** Proto fields 14 to 18 (`producer_seq`, `agent`, `origin`, `run_id`, `request_id`), `AgentRef`, `RunOrigin`, event types 150 to 156. NATS subject `mp.v1.trace.run.<run>`, stream `MODEL_PLANE_RUN_TRACE`, consumer `session-core-trace`, principal `quarry-trace-producer`. Table `run_trace_events` (0046). `EventLog.QueryEvents`; `GET /v1/runs/:id/trace`, `GET /api/v1/runs/{id}/trace`, `GET /api/v1/browser/sessions/{id}/events/stream`. SSE `replay_gap`, seven `browser.*` names, per-run `id:`. `x-request-id`; `StartRunBody.trace`. Telemetry field-contract golden. Flags: `QUARRY_EDGE__REQUEST_ID_ADOPT`, `__TRACE_RING_EVENTS`, `__MODEL_TRACE_NATS_*`, `__NATS_AGENT_EVENTS`; `SESSION_CORE_TRACE_CONSUMER`, `_RETENTION_DAYS`; `QUARRY_TELEMETRY_EXPORT`.

**Dependencies.** Hard: D4 S-4..S-6 (for S8), session-core 0044 and 0045 committed (for S6), F-t2-001 (only for AI-run end-to-end evidence). Soft: D2 S2, S3, S4; D4 S-5 shared span function.

**Effort.** S1 3.0, S2 4.0, S3 5.0, S4 4.5, S5 4.5, S6 10, S7 4.5, S8 8.0, S9 4.0: about 47.5 engineer-days (XL by repo scale; card says L). S1 to S4 (16.5 days) deliver value without Model Plane persistence. Critical path S2, S3, S4, S5, S6 about 28 days.

**ADR gates.** Topic 8 (canonical envelope, ratify part 1; Application projection and Space reads, part 2 blocks S7); topic 4 (ZDR replay ring, telemetry wording); security review of the new NATS principal.

**Parallelisable with.** S1, S2, S8a on day one; S3 and S8b/c independent tracks; the card's first slice (gateway proxies, devtools filters) in parallel.

**Conflicts with.** `agent_routes.rs` (D2, D1, F-t2-008/011/021/036), `routes.rs` 297 to 306 and `telemetry.rs` (D4), `step_receipts.rs` (D2 migration 0010), `openapi.yaml`, `zdr-contract-sync.test.sh`, gateway `browser.rs` route table, session-core `grpc.rs` and `main.rs` (in-flight diffs), Convex project (sibling (a)).

<!-- COMPLETE -->
