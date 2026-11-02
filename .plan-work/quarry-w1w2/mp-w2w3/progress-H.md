# Lane H progress (batch 6)

## WS8.G proto and event plumbing (W2, TESTED)

### Step 1+2 (proto + mp-orchestration), committed
- `proto/model_plane/v1/orchestration.proto`: `BrowserRunPaused.reason = 3`, `BrowserRunResumed.reason = 3` (plain proto3 string, empty by default); new message `BrowserAttachSharedJar { run_id=1, plan_id=2, attach_id=3, session_run_id=4, tab_id=5 }` (ids only, no url/title) as oneof arm `browser_attach_shared_jar = 30`.
  - Tag choice: 29 is `RunUsageProgress` and 26 is `ToolRetryScheduled` on branch `model-plane` (not yet merged here), so 30 is the first tag free on every branch (checked `model-plane`, `space`, this branch).
- `events.proto`: `EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR = 160` was ALREADY there (landed by WS10.S2, 65aae102b, already in go/gen). No change; `reserved 200 to 255` untouched.
- `buf breaking . --against baseline.binpb` (from `apps/Model Plane/proto`): clean, exit 0.
- `mp-orchestration`: `OrchestrationEvent::BrowserRunPaused/Resumed` gain `reason: String` (`#[serde(default, skip_serializing_if = "String::is_empty")]`, so old JSON and empty-reason JSON are byte-identical), new variant `BrowserAttachSharedJar { run_id, plan_id, attach_id, session_run_id, tab_id, at }`, new kind `BrowserAttachSharedJar`; `proto_shim.rs` maps both directions.
- Tests: `cargo test -p mp-orchestration` -> lib 66 passed (incl. new `pause_reason_is_omitted_when_empty_and_defaults_on_old_json`, `attach_shared_jar_carries_ids_only`, `round_trip_browser_attach_shared_jar`, `attach_shared_jar_is_oneof_tag_30_and_reason_is_field_3`), `proto_wire_parity` 10 passed.

### Literal breaks left for other lanes (domain `OrchestrationEvent` / proto structs now need `reason`)
- `services/execution-core/src/browser_events.rs:212` and `:224`: proto literals `BrowserRunPaused { .. }` / `BrowserRunResumed { .. }` lack `reason` (E0063). Owner: execution-core lane. Fix: add `reason: String::new()` (or the real reason) or `..Default::default()`.
- `services/model-gateway/src/sse.rs` has two non-exhaustive matches (12944, 13132): fixed by step 4 of this lane (not yet committed when this note was written).

### Step 4+5 (SSE, session-core, taxonomy, go/python gen), committed
- `model-gateway/src/sse.rs`: SSE frame `browser_attach_shared_jar` (keys run_id, plan_id, attach_id, session_run_id, tab_id), optional `reason` key on `browser_run_paused`/`browser_run_resumed` (only when non-empty, so old frames are byte-identical); step-update text distinguishes `user_took_over` / `control_returned`; the shared-jar event is not a chat step.
- `session-core/src/orchestration_grpc.rs`: persisted payload adds `reason` when set (`pause_resume_payload`), new persisted arm `BROWSER_ATTACH_SHARED_JAR` (already mapped to 160 in `event_type_to_i32`), `event_run_id` routes it to the run stream. `orchestration_nats.rs`: test literal only (the NATS bridge passes the new variant through via the domain->proto shim; no code change needed).
- `docs/sse-event-taxonomy.md` regenerated with `scripts/gen-sse-event-taxonomy.py`; `--check` passes (28 events), CRLF kept.
- Go: `go/gen/model_plane/v1/orchestration.pb.go` regenerated with native buf + local protoc-gen-go v1.36.12 (CRLF kept); `orchestration_grpc.pb.go` byte-identical. `GOWORK=off go build ./... && go vet ./... && go test ./...` in `go/gen`: ok.
- Python: `python/mp-events-py/.../orchestration_pb2.py` regenerated (buf protoc_builtin python with local protoc 35.1; header kept at the 7.36.0 the repo had, since the descriptor bytes do not depend on it). events_pb2 already had 160.
- TS (Frontend Plane, forbidden): NOT regenerated, asked in MP-ASKS.md.
- Tests: `cargo test -p mp-orchestration` lib 66 + proto_wire_parity 10 passed; `cargo test -p model-gateway --lib sse::` 292 passed (incl. 2 new); `cargo test -p mp-events` 28+11+11+4+5 passed; `cargo test -p mp-contracts --test proto_wire_parity` 5 passed; `cargo test -p session-core --bin session-core orchestration` 128 passed, 56 ignored (DB-backed); `cargo check --tests -p execution-core -p model-gateway -p session-core` clean at the time of the run.
- `scripts/tests/zdr-contract-sync.test.sh`: 140 passed, 10 failed; all 10 are digest/vendored-copy mismatches of ZDR goldens (CRLF noise in the worktree), none touches orchestration/events/sse; nothing to land from this slice.

### Not verified / assumed
- Persisted `reason` payload and the `BROWSER_ATTACH_SHARED_JAR` insert are not DB-tested (the DB tests are ignored without the harness).
- Plain proto3 `string reason` (not `optional`): empty means "no reason".
- Lead to land: nothing in the ledger beyond what WS10.S2 already records (oneof tag 30 for BrowserAttachSharedJar, reason=3 on pause/resume): suggested text for the WS10.S2 ledger entry: "WS8.G: orchestration.proto BrowserRunPaused/Resumed.reason = 3; OrchestrationEvent oneof browser_attach_shared_jar = 30 (29 is RunUsageProgress on model-plane); EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR = 160."
