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
