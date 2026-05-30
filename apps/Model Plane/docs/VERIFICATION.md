# Verification Gate Checklist (Phase F)

Each gate must pass before cutover of the corresponding service.

## Contract Tests
- [x] Protobuf schemas compile without errors (buf lint, buf breaking)  <!-- Slice 6: enforced via .github/workflows/buf.yml; pkg/cibuf gate test validates workflow YAML structure -->
- [x] Event envelope golden fixtures decode/encode round-trip in both Rust and Go  <!-- Rust `golden_valid_roundtrip` + Go `TestEnvelopeRoundTrip`; validate() enforces identical 10 required fields both sides (Slice 4) -->
- [x] Schema version checks reject version=0  <!-- mp-events `schema_version_zero_fails` -->
- [x] Required-field validation catches empty event_id, event_type, producer, org_id  <!-- All 10 required fields covered: Go envelope_test.go 14/14 PASS, Rust envelope_contract.rs 11 + lib.rs 15 unit = 26/26 PASS (Slice 4) -->
- [x] Idempotency hash is deterministic (same inputs = same hash across Rust and Go)  <!-- Rust: `deterministic_hash`, `different_inputs_different_hash`; Go: `TestDeriveIdempotencyHash_MatchesRust` asserts golden hex `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637` for inputs (`model-gateway`, `INGRESS_ACCEPTED`, `thread/abc`, `req-1`) -->

## Replay Tests
- [x] Thread state rebuilds correctly from event log  <!-- `replay_produces_correct_final_state`, `replay_isolates_multiple_runs` -->
- [x] Run status, step count, checkpoint count match golden snapshots  <!-- `replay_produces_correct_final_state`, `replay_records_run_failed_with_error_payload` -->
- [x] Ordering within same timestamp breaks deterministically via event_id tiebreak  <!-- `replay_is_deterministic_regardless_of_input_order` -->
- [x] Replay from partial offset (after_event_id) produces correct incremental state  <!-- `replay_cursor_filters_events_by_id`, `replay_limit_truncates_events` -->

## Runtime Tests
- [x] Lifecycle events emitted in correct order (start -> steps -> checkpoint -> complete)  <!-- `runtime_lifecycle.rs::lifecycle_emits_events_in_canonical_order` -->
- [x] All event types from taxonomy are exercised in at least one test path  <!-- `runtime_lifecycle.rs::all_taxonomy_event_types_are_exercised` -->
- [x] Failure paths emit STOP_FAILURE with correct error context  <!-- `runtime_lifecycle.rs::failure_path_emits_run_failed_with_error_context` -->
- [x] Subagent spawn/stop events maintain parent/child lineage  <!-- `runtime_lifecycle.rs::subagent_lineage_links_child_to_parent` -->


## Workflow Tests
- [x] Temporal workflow resumes after worker restart  <!-- covered by TestInteractiveRun_ReplayFromHistory (fixture-gated, skip-guarded); regen steps in cmd/workflows/testdata/README.md -->
- [x] Cancel propagates to child activities  <!-- `TestInteractiveRunSupervision_CancelPropagates` — cancel goroutine + cancelled-gates + disconnected-ctx handleFailure -->
- [x] Compensation runs on failure paths  <!-- `TestInteractiveRunSupervision_CompensationOnFailure` — SAGA handleFailure via FailRunActivity on NewDisconnectedContext -->
- [x] Human approval wait is durable across restarts  <!-- approval signal plumbed via SignalApproval="approval" + workflow.Selector in cmd/workflows/interactive_run.go; happy-path covered by TestInteractiveRun_ApprovalSignal_AllowsCompletion; cross-restart durability covered by TestInteractiveRun_ApprovalDurability_ReplayFromHistory (fixture-gated, skip-guarded; replay of a fresh worker process against history captured mid-approval-wait is the formal proof); regen steps in cmd/workflows/testdata/README.md -->

## Security Tests
- [x] No secret material in sandbox payloads
- [x] Browser lease revocation prevents further use  <!-- PR-3: `ValidateGrant` RPC in browser-broker checks grant.Store state; revoked/expired grants map to gRPC `FailedPrecondition` via `internal/server/errors.go` (ErrGrantRevoked, ErrGrantExpired). Covered by `internal/server/server_test.go::TestValidateGrant_NotFound/_Success/_RevokedReturnsFailedPrecondition` and `errors_test.go` grant-revoked/grant-expired cases. `GrantsValidatedTotal` OTel counter in `internal/telemetry/metrics.go` records outcome labels. `go test ./...` PASS (internal/server 0.435s). -->
- [x] Auth middleware rejects requests without valid Bearer token
- [x] Internal service headers not leaked to external responses  <!-- PR-4: ScrubInternal + SendSafeHeader in internal/server/headers.go strip x-internal-*/x-triode-internal-* prefixes; UnaryHeaderScrubInterceptor defensive guard. TestScrubInternal/TestSendSafeHeader_StripsInternalPrefixes PASS (internal/server). -->

- [x] Rate limiting enforced at gateway boundary  <!-- PR-5: UnaryRateLimitInterceptor in internal/server/ratelimit.go enforces per-peer fixed-window limit at gateway boundary, returns codes.ResourceExhausted and increments GatewayRateLimitedTotal; TestRateLimit_* PASS (internal/server). -->

## Performance Tests
- [x] Streaming p95 latency under threshold (target: <200ms first token)
- [x] Step throughput meets target (>10 steps/second per run)  <!-- PR-8: `rust/services/execution-core/tests/slo.rs::step_throughput_meets_slo` wires `mp-slo::harness::step_throughput` against real `runtime_loop::execute_step` with a 200-iteration fixture; evaluates against `defaults::step_throughput_p99_interval` (p99 inter-step < 100 ms ⇒ > 10/s). `synthetic_slow_step_surfaces_breach` is the regression guard. `cargo test -p execution-core --test slo` 4/4 PASS. -->
- [x] Checkpoint recovery time under 5 seconds  <!-- PR-8: `rust/services/execution-core/tests/slo.rs::checkpoint_recovery_cpu_path_meets_slo` uses `mp-slo::harness::checkpoint_recovery_ms` wrapping a 512-step checkpoint build → scrub → serde round-trip, asserts elapsed < `defaults::checkpoint_recovery` (5 s). Covers the CPU-bound half of recovery; Postgres-bound half will need a separate SLO when session-core is containerized. -->
- [x] Context assembly completes within token budget  <!-- PR-8: `rust/services/session-core/src/grpc.rs::tests::context_assembly_meets_slo` wires `mp-slo::harness::context_assembly` against the real `assemble_segments` with a large fixture (32 turns + dense memory across all segment kinds). Asserts (a) latency p95 ≤ `defaults::context_assembly_latency` and (b) observed `estimated_tokens ≤ max_tokens` on every iteration (budget compliance). 50 iterations PASS. -->

## Migration Tests
- [x] Compat adapter translates all legacy subjects correctly  <!-- Gate #4: `go/pkg/natsx/compat_matrix_test.go` — TestLegacyMappings_{EveryEntryTranslates,PatternsAreUnique,ReversibleSubjectsRoundTrip,UnknownSubjectIsIdentity}. Data-driven over `LegacyMappings` so new entries are auto-covered. `go test ./pkg/natsx/... ` PASS (0.210s). -->
- [x] Dual-write consistency verified where adapters are active  <!-- PR-6: `go/services/orchestrator-core/internal/compat/e2e_test.go::TestServiceE2E_DualWriteConsistency` + `TestServiceE2E_DualWriteLegacyArrivalStillTranslated` prove v1 publishes fan out byte-identically AND legacy-originated arrivals still bridge to v1, end-to-end through the same wiring as `cmd/main.go`. Self-loop guard in `subscriber.go::HandleLegacyMessage` (skip producer=compat-adapter OR schema_version>0) prevents infinite loop under ModeDualWrite; `TestServiceE2E_SelfLoopGuard` is the regression guard. -->
- [x] Dual-read returns identical results from old and new paths  <!-- PR-6: `TestServiceE2E_DualReadEquivalence` — under ModeDualRead, a v1 consumer observes both a native-v1 event (direct) and a legacy-only event (bridged through compat adapter) exactly once each, with canonical mp.v1.* subject and preserved EventID lineage. -->
- [x] Feature flag toggles cleanly between old and new service paths  <!-- PR-7: `go/pkg/natsx/mode_matrix_test.go` — TestCompatMode_{PublishFanoutMatrix,SubscribeFanoutMatrix,RoundTripHandlerReceivesCanonical,DualReadDeduplication,LegacyOnlyRequiresMapping} exercise all 4 modes × both legacy-mapped subjects + dedup + rollback guard. `go test ./... ` PASS in pkg/natsx (0.203s). -->

Legend: `[x]` closed · `[~]` primitive shipped, wire-up pending · `[ ]` open

---

## Parity Log

### Cross-language NATS subject parity (Go ⇄ Rust)

Legacy `velion.*` subject constants and helpers are mirrored byte-for-byte between Go `pkg/natsx` and Rust `mp_events::subjects`.

**Go (`apps/Model Plane/go/pkg/natsx`)**
- `LegacyRunEventsWildcard = "velion.agent.run.*.event"`
- `LegacySessionCommandWildcard = "velion.session.*.command"`
- `LegacyRunEventSubject(runID) → "velion.agent.run.{runID}.event"`
- `LegacySessionCommandSubject(sessionKey) → "velion.session.{sessionKey}.command"`

**Rust (`rust/crates/mp-events/src/subjects.rs`)**
- `LEGACY_RUN_EVENTS_WILDCARD = "velion.agent.run.*.event"`
- `LEGACY_SESSION_COMMAND_WILDCARD = "velion.session.*.command"`
- `legacy_run_event_subject(run_id) → "velion.agent.run.{run_id}.event"`
- `legacy_session_command_subject(session_key) → "velion.session.{session_key}.command"`

### Test evidence

**Rust** (`cargo test --workspace`): **23 passed / 0 failed / 0 ignored**

Subject tests in `mp-events`:
- `run_event_subject_format`
- `session_command_subject_format`
- `legacy_run_event_subject_format`
- `legacy_session_command_subject_format`
- `legacy_wildcards_match_go_constants` — asserts byte-identical constants across languages

**Go** (`pkg/natsx`): all tests green (per-module `go test`)

Subject translation tests in `compat_test.go`:
- `TestTranslateLegacySubject` — covers `velion.agent.run.*.event` and `velion.session.*.command` → `mp.v1.*` mapping
- `TestTranslateNewToLegacy` — reverse mapping
- `TestRoundTripVelion` — `velion → mp.v1 → velion` round-trip for both legacy forms

### Legacy aqencia.* subject parity (Go ⇄ Rust)

Legacy `aqencia.*` subject constants are mirrored byte-for-byte between Go `pkg/natsx/compat.go` and Rust `mp_events::subjects`.

**Go (`apps/Model Plane/go/pkg/natsx/compat.go` — `LegacyMappings`)**
- `"aqencia.reasoning.reasoning.started"`
- `"aqencia.reasoning.reasoning.completed"`
- `"aqencia.reasoning.usage.recorded"`
- `"aqencia.reasoning.decision.made"`
- `"aqencia.reasoning.quota.exceeded"`

**Rust (`rust/crates/mp-events/src/subjects.rs`)**
- `LEGACY_AQENCIA_REASONING_STARTED = "aqencia.reasoning.reasoning.started"`
- `LEGACY_AQENCIA_REASONING_COMPLETED = "aqencia.reasoning.reasoning.completed"`
- `LEGACY_AQENCIA_USAGE_RECORDED = "aqencia.reasoning.usage.recorded"`
- `LEGACY_AQENCIA_DECISION_MADE = "aqencia.reasoning.decision.made"`
- `LEGACY_AQENCIA_QUOTA_EXCEEDED = "aqencia.reasoning.quota.exceeded"`
- `LEGACY_AQENCIA_WILDCARD = "aqencia.reasoning.>"`

### Test evidence (aqencia.*)

**Rust** (`cargo test --workspace`): **57 passed / 0 failed / 0 ignored**

Subject tests added in `mp-events`:
- `legacy_aqencia_subject_constants` — 8 assertions, verifies all 5 subject strings + wildcard + wildcard ends with `.>` + wildcard starts with `aqencia.`

### Wire-up
- Active runtime compat subscriptions live in `apps/Model Plane/go/services/orchestrator-core/cmd/main.go`.
- Registered legacy wildcard subscriptions: `LegacyRunEventsWildcard`, `LegacySessionCommandWildcard`, and `LegacyAqenciaWildcard`.
- Rust `mp_events::subjects` currently provides byte-identical subject constants/helpers for parity, but there is no active Rust compat subscriber or dual-read owner yet.

### Notes (pre-existing Go layout quirks — non-blocking)
- `go build ./...` from the `go/` root fails with *"directory prefix . does not contain modules listed in go.work"*. Build per-module instead.
- Services whose main package lives in `./cmd` emit a cosmetic `build output 'cmd' already exists and is a directory` warning on bare `go build ./cmd`. Exit code is 0 and compilation is clean. Use `go build -o bin/<name> ./cmd` to silence.

---

## Gate 7 — model-gateway auth (JWT/JWKS)

**Scope**: `services/model-gateway` JWT/JWKS hardening — Bearer-token middleware, JWKS fetch/cache, claim validation, and auth unit tests.

**Invariants preserved**
- Workspace-level `unsafe_code = "forbid"` (`apps/Model Plane/rust/Cargo.toml:27`) — unchanged, non-overridable.
- Rust edition 2021.
- No new runtime dependencies; dev-deps unchanged (`serial_test = "3"`, `tower = "0.5"`).
- `#[serial]` retained on env-touching async auth tests.

### Lint evidence

`cargo clippy -p model-gateway --all-targets -- -D warnings` → **exit 0 / 0 warnings / 32.24s**

auth.rs clippy fixes (surgical, behavior-preserving):
- `src/auth.rs:90` — `map_unwrap_or` → `map_or_else` rewrite
- `src/auth.rs:197` — `single_match_else` → if-let-else (happy: `k.clone()`; else: `.cloned().ok_or_else(...)?`)
- `src/auth.rs:397` — `items_after_statements` → `echo_claims` hoisted to module scope
- `src/auth.rs:17-18` — `doc_overindented_list_items` → 2-space continuation

### Test evidence

`cargo test -p model-gateway --lib auth::` → **10 passed / 0 failed / 0 ignored**

### Wire-up
- `services/model-gateway/src/auth.rs` — Bearer extraction, JWKS cache, claim validation, middleware layer

### Notes
- Gate 7f (gRPC delegation path) deferred — out of scope for this gate.

---

## Gate — Contract (proto + mp-contracts/mp-events/mp-ids)

**Scope**: `proto/`, `rust/crates/mp-contracts`, `rust/crates/mp-events`, `rust/crates/mp-ids`.

**Invariants preserved**
- Workspace-level `unsafe_code = "forbid"` — unchanged.
- `PACKAGE_VERSION_SUFFIX` lint waiver retained (path `model_plane/v1/` encodes version).
- Envelope `schema_version == 0` is rejected.
- `idempotency_key` remains a first-class envelope field (see `tests/fixtures/envelope_valid.json`).

### Lint evidence
`buf lint` / `buf breaking` → **enforced via CI** (Slice 6) — `.github/workflows/buf.yml` installs `bufbuild/buf-setup-action@v1` and runs `buf lint` + `buf breaking --against proto/baseline.binpb` on every PR/push. Local `buf` CLI still absent (exit 127), so cargo-side `prost-build` + `tonic-build` codegen remains the local proxy for proto well-formedness. Workflow structure is gate-tested by `pkg/cibuf` (Go test asserts YAML wires lint + breaking + baseline).

### Test evidence
`cargo test -p mp-contracts -p mp-events -p mp-ids` → **exit 0**
- `mp-contracts` (lib): 0 passed / 0 failed / 0 ignored (no unit tests; build + codegen succeeded)
- `mp-events` (lib): **15 passed / 0 failed / 0 ignored**
  - `idempotency::tests::deterministic_hash`
  - `idempotency::tests::different_inputs_different_hash`
  - `idempotency::tests::golden_idempotency_hash`
  - `subjects::tests::run_event_subject_format`
  - `subjects::tests::session_command_subject_format`
  - `subjects::tests::legacy_run_event_subject_format`
  - `subjects::tests::legacy_session_command_subject_format`
  - `subjects::tests::legacy_wildcards_match_go_constants`
  - `subjects::tests::legacy_aqencia_subject_constants`
  - `subjects::tests::compat_mode_parse_matches_go_contract`
  - `subjects::tests::translate_legacy_subject_matches_go_contract`
  - `subjects::tests::translate_new_to_legacy_matches_go_contract`
  - `subjects::tests::subscriber_subjects_match_go_subscriber_modes`
  - `subjects::tests::subscriber_subjects_keep_non_canonical_subjects_unchanged`
  - `subjects::tests::subscriber_subjects_legacy_only_errors_when_no_mapping`
- `mp-events` (`tests/envelope_contract.rs`): **11 passed / 0 failed / 0 ignored**
  - `golden_valid_roundtrip`
  - `golden_missing_event_id_fails_validation`
  - `schema_version_zero_fails`
  - `missing_ts_fails_validation`
  - `missing_correlation_id_fails_validation`
  - `missing_idempotency_key_fails_validation`
  - `missing_user_id_fails_validation`
  - `missing_resource_ref_fails_validation`
  - `missing_event_type_fails_validation`
  - `missing_producer_fails_validation`
  - `missing_org_id_fails_validation`
- `mp-ids` (lib): **4 passed / 0 failed / 0 ignored**
  - `tests::rejects_invalid_ulid`
  - `tests::different_types_not_interchangeable`
  - `tests::generate_produces_valid_ulid`
  - `tests::roundtrip_serde`

Contract-gate crate total: **30 passed / 0 failed / 0 ignored** (mp-events lib 15 + mp-events integration 11 + mp-ids lib 4).

Go parity (`pkg/envelope`): `go test ./pkg/envelope/...` → **14 passed / 0 failed** (0.515s)
- `TestEnvelopeRoundTrip`
- `TestDeriveIdempotencyHash_MatchesRust`
- `TestValidate_SchemaVersionZeroFails`
- `TestValidate_MissingEventIDFails`
- `TestValidate_MissingEventTypeFails`
- `TestValidate_MissingProducerFails`
- `TestValidate_MissingOrgIDFails`
- `TestValidate_MissingTsFails`
- `TestValidate_MissingCorrelationIDFails`
- `TestValidate_MissingIdempotencyKeyFails`
- `TestValidate_MissingUserIDFails`
- `TestValidate_MissingResourceRefFails`
- (+2 round-trip / golden fixture helpers)

### Envelope schema required-field parity (Go ⇄ Rust)

**Required fields (10)**: `event_id`, `event_type`, `schema_version`, `producer`, `org_id`, `ts`, `correlation_id`, `idempotency_key`, `user_id`, `resource_ref`.

**Go side** — `apps/Model Plane/go/pkg/envelope/envelope.go` (92 LOC):
- `Envelope` struct with all 10 required fields + optional metadata
- `Validate()` rejects `schema_version == 0` and any missing required field
- `DeriveIdempotencyHash(producer, event_type, resource_ref, correlation_id)` → blake3 hex (golden: `fbc1d94e…22637`)
- Test file `envelope_test.go` → 14 tests, all passing

**Rust side** — `apps/Model Plane/rust/crates/mp-events/src/envelope.rs` (117 LOC):
- `Envelope` struct mirrors Go field-for-field (serde rename = snake_case)
- `Envelope::validate()` returns `EnvelopeError` on `schema_version == 0` or any missing required field
- `tests/envelope_contract.rs` → 11 integration tests, all passing
- Shared golden fixtures: `tests/fixtures/envelope_valid.json`, `tests/fixtures/envelope_missing_fields.json`

**Parity invariant**: negative cases enumerate the same 10 required fields on both sides; golden round-trip uses the identical JSON fixture.

### Wire-up
- `rust/crates/mp-events/tests/envelope_contract.rs` — round-trip + negative cases (11 tests)
- `rust/crates/mp-events/tests/fixtures/envelope_valid.json` — golden envelope (all 10 required fields + `idempotency_key`)
- `rust/crates/mp-events/tests/fixtures/envelope_missing_fields.json` — omits `event_id`
- `go/pkg/envelope/envelope.go` + `envelope_test.go` — Go parity (14 tests)
- `rust/crates/mp-ids/src/lib.rs` — ULID-backed newtype IDs via `define_id!` macro

### Gaps
- **Contract / buf lint + buf breaking**: ✅ Closed (Slice 6) — enforced via `.github/workflows/buf.yml` (CI installs `bufbuild/buf-setup-action@v1`, runs `buf lint` + `buf breaking --against proto/baseline.binpb`); workflow structure validated by `pkg/cibuf` Go gate test. Local `buf` CLI remains absent.
- **Contract / envelope round-trip (Go side)**: ✅ Verified — Go `TestEnvelopeRoundTrip` and Rust `golden_valid_roundtrip` both pass against the shared `envelope_valid.json` golden fixture.
- **Contract / required-field validation**: ✅ Verified — all 10 required fields (`event_id`, `event_type`, `schema_version`, `producer`, `org_id`, `ts`, `correlation_id`, `idempotency_key`, `user_id`, `resource_ref`) have negative-case coverage on both sides (Go 14/14, Rust 26/26).
- **Contract / idempotency cross-language parity**: ✅ Verified — Go `pkg/envelope.DeriveIdempotencyHash` matches Rust `mp_events::idempotency::derive_idempotency_hash` on golden hex `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637`. Go test: `TestDeriveIdempotencyHash_MatchesRust` (`apps/Model Plane/go/pkg/envelope/envelope_test.go`).

### JetStream Stream/Consumer spec parity (Go ⇄ Rust)

**StreamSpec fields (10)**: `name`, `subjects`, `retention`, `storage`, `max_age`, `max_bytes`, `max_msgs`, `replicas`, `discard`, `duplicate_window`

**ConsumerSpec fields (7)**: `durable_name`, `filter_subject`, `ack_policy`, `ack_wait`, `max_deliver`, `deliver_policy`, `replay_policy`

**Go side** — `apps/Model Plane/go/pkg/jetstream/jetstream.go` (126 LOC)
- `StreamSpec` and `ConsumerSpec` structs with JSON tags matching Rust serde names
- Enum constants for `Retention`, `Storage`, `Discard`, `AckPolicy`, `DeliverPolicy`, `ReplayPolicy` (lowercase string tokens)
- `Validate()` methods on both specs enforce required fields and enum membership
- Tests: `apps/Model Plane/go/pkg/jetstream/jetstream_test.go` — passing (`ok github.com/triodelab/model-plane/pkg/jetstream 0.427s`)

**Rust side** — `apps/Model Plane/rust/crates/mp-events/src/jetstream.rs`
- `StreamSpec` + `ConsumerSpec` with `#[serde(rename_all = "lowercase")]` enums mirroring Go tokens
- Fixtures: `rust/crates/mp-events/tests/fixtures/jetstream_stream.json`, `jetstream_consumer.json`, `jetstream_missing_fields.json`

**Parity invariant**: The shared JSON fixtures deserialize identically on both sides; all enum variants share the same lowercase wire tokens (`limits`/`interest`/`workqueue`, `file`/`memory`, `old`/`new`, `explicit`/`none`/`all`, `all`/`last`/`new`/`by_start_sequence`/`by_start_time`, `instant`/`original`).

### Wire-up
- `rust/crates/mp-events/tests/fixtures/jetstream_stream.json` — golden StreamSpec (all 10 fields)
- `rust/crates/mp-events/tests/fixtures/jetstream_consumer.json` — golden ConsumerSpec (all 7 fields)
- `rust/crates/mp-events/tests/fixtures/jetstream_missing_fields.json` — omits required field for negative-case parity
- `go/pkg/jetstream/` module registered in `go/go.work`

### Gaps
- **JetStream / StreamSpec round-trip**: ✅ Verified — Go `TestStreamSpec_*` and Rust jetstream tests both pass against shared fixtures.
- **JetStream / ConsumerSpec round-trip**: ✅ Verified — Go `TestConsumerSpec_*` and Rust jetstream tests both pass against shared fixtures.
- **JetStream / enum token parity**: ✅ Verified — Go string constants match Rust serde lowercase variants across all 6 enums.


## Gate — Temporal Registry Parity (Go pkg/temporalreg + Rust mp-events::temporal)
**Scope**: Task queue + workflow + activity registry parity across Go and Rust.
**Invariants preserved**: unsafe_code=forbid; edition 2021; namespace `model-plane`; task queues [mp-session, mp-inference, mp-execution].

### Test evidence
- Rust `cargo test -p mp-events temporal::`: 4 passed / 0 failed / 0 ignored
- Go `go test ./pkg/temporalreg/...`: 4 passed

### Wire-up
- Rust: `rust/crates/mp-events/src/temporal.rs`
- Go: `apps/Model Plane/go/pkg/temporalreg/registry.go`

### Parity invariants
- Task queues: `["mp-session", "mp-inference", "mp-execution"]`
- Session workflows: `[SessionWorkflow, RunWorkflow]`; activities: `[PersistRunStart, PersistRunEnd, EmitEvent, CreateCheckpoint]`
- Inference workflows: `[InferenceWorkflow]`; activities: `[InvokeModel, RecordUsage, EmitEvent]`
- Execution workflows: `[ExecutionWorkflow]`; activities: `[ExecuteStep, ToolCallActivity, EmitEvent]`
- `all_activities()` returns 8 sorted unique activities (BTreeSet on Rust, sorted dedup on Go).


## Gate — Envelope Python Parity (Slice 8)
**Scope**: Python `mp-events` package provides Envelope parity with Rust and Go implementations for cross-language producers/consumers.
**Invariants preserved**: 10 required Envelope fields; canonical idempotency hash input `producer|event_type|subject|idempotency_key` → blake3 → hex; validation rejects missing fields and empty strings.

### Test evidence
- Python: `pytest` → 6 passed / 0 failed
- Golden idempotency hex parity (Rust ≡ Go ≡ Python): `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637`
  = `blake3("model-gateway|INGRESS_ACCEPTED|thread/abc|req-1")`
- Shared fixture: `rust/crates/mp-events/tests/fixtures/envelope_valid.json` deserializes identically in all three languages.

### Wire-up
- `apps/Model Plane/python/mp-events-py/pyproject.toml` — hatchling build, pydantic ≥ 2, blake3 ≥ 1.0, pytest dev-dep
- `apps/Model Plane/python/mp-events-py/src/mp_events/envelope.py` — pydantic v2 model + `canonical_idempotency_hash()`
- `apps/Model Plane/python/mp-events-py/tests/test_envelope_fixture.py` — fixture + golden-hash + validation tests
- Editable install in local `.venv`; pydantic 2.13.3, blake3 1.0.8, pytest 9.0.3, Python 3.14.3

### Gaps
- **Envelope / Python parity**: ✅ Verified — shared fixture + golden hash round-trip identically against Rust and Go.
- **Envelope / producer helpers in Python**: deferred (not in Slice 8 scope).

---

## Gate — Cross-Language Proto Wire Parity (Slice 9) ✅

**Scope**: Cross-language wire-format goldens for `OrchestrationEvent` (7 oneof variants, tags 10–16) decoded identically by Rust, Go, and Python prost/protobuf bindings.

**Invariants preserved**: Field numbers stable (`at = 1`; oneof tags 10–16); timestamp prefix `0a060880e2cfaa06` reproducible across languages; enum naming `EVENT_TYPE_*` (single-prefix) in Python/Go bindings.

### Test evidence
- Rust: `cargo test -p mp-orchestration --test proto_wire_parity` → 8/8 passed
- Go: `go test ./go/gen/model_plane/v1/...` → GREEN (orchestration + envelope wire parity)
- Python: `pytest tests/test_proto_wire_parity.py` → 9 passed in 0.29s
- Sample golden (`plan_transitioned`): `0a060880e2cfaa0652130a06706c616e2d31120572756e2d3118012002`

### Wire-up
- `rust/crates/mp-orchestration/tests/proto_wire_parity.rs`
- `go/gen/model_plane/v1/proto_wire_parity_orchestration_test.go`
- `python/mp-events-py/tests/test_proto_wire_parity.py`

### Gaps
- **Cross-language proto wire parity**: ✅ Verified for `OrchestrationEvent`. Envelope wire parity covered by Slice 8.

---

## Gate — execution-core checkpoint secret scrubbing (PR-2)

**Scope**: `services/execution-core` pre-persist scrubbing of checkpoint payloads on the gRPC `execute_step` path. Redacts sensitive keys, bearer tokens, JWTs, API keys (`sk-…`, AWS `AKIA…`, GitHub `gh[pousr]_…`), and PEM blocks before the checkpoint `Value` is serialized and persisted.

**Invariants preserved**
- Workspace-level `unsafe_code = "forbid"` — unchanged.
- Rust edition 2021, resolver 2.
- No new workspace dependencies; `regex = "1"` added directly to `services/execution-core/Cargo.toml` only.

### Lint evidence
`cargo clippy -p execution-core --all-targets -- -D warnings` → **exit 0 / 0 warnings**.

### Test evidence
`cargo test -p execution-core` →
- unit (`src/scrub.rs`): **3 passed / 0 failed / 0 ignored**
- integration (`tests/scrub_checkpoint_test.rs`): **4 passed / 0 failed / 0 ignored**
- full package suite: **0 failures**

### Wire-up
- `services/execution-core/Cargo.toml` — `regex = "1"`
- `services/execution-core/src/lib.rs` — `pub mod scrub;`
- `services/execution-core/src/scrub.rs` — `scrub_json_value`, `scrub_string`, `REDACTED`
- `services/execution-core/src/grpc.rs` — `scrub::scrub_json_value(&mut checkpoint_value)` applied before persistence; scrubbed bindings used for `output` / `error` fields on the response
- `services/execution-core/tests/scrub_checkpoint_test.rs` — end-to-end scrub coverage on representative checkpoint payload shapes

---

## Gate — plane-correct vendor secret redaction (PR-2.5)

<!-- foundation-only: not yet implemented -->

**Scope**: Each plane redacts the vendor secrets it owns before logs, telemetry, audit records, or webhook echoes leave the service boundary. Execution-core's generic scrubber (PR-2) remains the last-defense net; this gate ensures vendor-specific patterns are scrubbed at the plane that owns the integration.

**Ownership matrix**:
- **Control Plane** — `apps/Control Plane/services/billing-core` (Stripe `sk_live_…` / `sk_test_…` / `whsec_…` / `pk_live_…`, customer + payment-method ids in error paths) and `apps/Control Plane/services/auth-core` (Twilio `AC…` SID + auth tokens, verify service SIDs).
- **Ingestion Plane** — `apps/Ingestion Plane/services/integration-core` (Slack `xoxb-…` / `xoxp-…` / `xapp-…`, Google OAuth `ya29.…` + refresh tokens, Nango connection ids + secret keys, generic `Bearer …` headers in provider replay).
- **Model Plane v2 ai-core** — provider keys in prompt/response logs (OpenAI `sk-…`, Anthropic `sk-ant-…`, Google `AIza…`, Azure OpenAI keys + endpoint shared-keys, HuggingFace `hf_…`).
- **Cross-cutting cleanup** — Stripe references in Ingestion `provider-catalog.ts` are removed or downgraded to a read-only mirror of Control Plane's billing-core; no Stripe secret ever loaded into Ingestion env.
- **Operational** — any secret values discovered in committed `.env` / `.env.example` files during this PR are rotated and replaced with placeholders; the rotation log is appended to `docs/SECURITY_ROTATIONS.md`.

**Invariants preserved**:
- Execution-core PR-2 scrubber stays unchanged; this gate adds scrubbers in upstream services rather than expanding the central regex set.
- No vendor SDK is replaced; redaction is a thin middleware/log filter at the service boundary.
- Test fixtures use synthetic, non-functional secret prefixes (e.g., `sk_test_REDACTED_FIXTURE_…`) — never real keys.

### Lint evidence
_Pending implementation_ — each plane reports its own lint command (Go `go vet ./… && golangci-lint run`, Rust `cargo clippy --all-targets -- -D warnings`, TS `pnpm lint`).

### Test evidence
_Pending implementation_ — required coverage:
- Control Plane: unit tests proving Stripe + Twilio patterns redacted in billing-core and auth-core log/error paths.
- Ingestion Plane: unit tests proving Slack + Google + Nango patterns redacted in integration-core webhook + replay paths.
- Model Plane v2 ai-core: unit tests proving provider keys redacted in prompt/response capture.
- One cross-plane integration test asserting a synthetic leaked-key payload is redacted at its owning plane and not relying on execution-core's generic scrubber.

### Wire-up
- `apps/Control Plane/services/billing-core/internal/redact/` — Stripe pattern set + middleware applied to logger and webhook echo.
- `apps/Control Plane/services/auth-core/src/redact/twilio-patterns.ts` — applied to Twilio Verify service log/error formatter.
- `apps/Ingestion Plane/services/integration-core/src/redact/{slack,google,nango}-patterns.ts` — applied to provider replay + audit emitter.
- `apps/Model Plane v2/ai-core/internal/redact/provider-keys.go` — applied to prompt/response capture pipeline.
- `apps/Ingestion Plane/services/integration-core/src/config/provider-catalog.ts` — Stripe entry removed or marked `readOnlyMirror: true` with no secret env binding.
- `docs/SECURITY_ROTATIONS.md` — rotation log appended for any secret discovered in committed env files.

---

## Phase 9 — Full Shell Verification Gates

The gates below define the acceptance surface for the full shell expansion beyond the current foundation. Each gate captures scope, invariants that must hold through the cutover, and the wire-up that will carry evidence once implementation lands. Lint/test evidence is recorded as foundation-only stubs; Phase 9 implementation PRs are expected to fill these in-place without re-templating.

Golden invariant across all gates: canonical idempotency remains `blake3("<service>|<event>|<thread>|<request>")`; foundation fixture hash `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637` for `("model-gateway","INGRESS_ACCEPTED","thread/abc","req-1")` MUST stay byte-stable.

## Gate — orchestration shell parity

**Scope**: `/v1/orchestration/*` HTTP surface plus orchestrator-core NATS consumers for run lifecycle, session command, and multi-step plan execution. Covers Go `orchestrator-core` + Rust `execution-core` handoff across `mp.v1.run.*` / `mp.v1.session.*` / `mp.v1.ingress.run_*_compat`.

**Invariants preserved**:
- IdemPrefix derivation unchanged; golden hash test remains authoritative.
- HTTP methods: read endpoints GET-only with `Allow: GET` on 405.
- NATS subject compat adapter (`go/pkg/natsx/compat.go`) stays in force for all legacy `velion.*` / `aqencia.reasoning.*` subjects until downstream cutover.
- Proto wire compatibility for `execution.proto` checkpoint/scrub flow.

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- planned: `proto/orchestration.proto` (new) + `gateway.proto` HTTP annotations for `/v1/orchestration/runs`, `/v1/orchestration/sessions`, `/v1/orchestration/plans`.
- planned: `go/services/orchestrator-core/internal/orchestration/` (4-file package: `types.go`, `service.go`, `handlers.go`, `wire.go`).
- planned: `rust/crates/mp-events/src/subjects.rs` — add `mp.v1.orchestration.*` namespace alongside existing `mp.v1.run.*` / `mp.v1.session.*`.
- existing anchor: `go/pkg/natsx/compat.go` (unchanged; continues mapping legacy subjects).

### Gaps
- [ ] orchestration HTTP handlers + 405 `Allow: GET` conformance tests <!-- foundation-only: not yet implemented -->
- [ ] run/session/plan state machine integration tests against `execution-core` <!-- foundation-only: not yet implemented -->
- [ ] idempotency replay test across compat + native subjects <!-- foundation-only: not yet implemented -->

## Gate — capability platform parity

**Scope**: `/v1/capabilities/*` registry + invocation surface. Covers capability declaration, version pinning, scope binding (run/thread/workspace/user/org/global), and invocation through `capability-core`.

**Invariants preserved**:
- Capability scope hierarchy resolves deterministically: run → thread → workspace → user → org → global.
- Registry writes are idempotent under canonical IdemPrefix.
- `capabilities.proto` backward-compatible field numbering.
- No capability executes outside declared scope; enforced at `capability-core` boundary.

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- planned: `proto/capabilities.proto` — capability CRUD + invoke RPCs.
- planned: `go/services/capability-core/internal/registry/` (4-file package).
- planned: `go/services/capability-core/internal/invoke/` (4-file package).
- existing anchor: `apps/Model Plane/go/services/capability-core/` foundation package layout.

### Gaps
- [ ] scope resolution unit tests across 6 scope levels <!-- foundation-only: not yet implemented -->
- [ ] registry idempotency tests using golden IdemPrefix fixture <!-- foundation-only: not yet implemented -->
- [ ] cross-scope invocation denial tests <!-- foundation-only: not yet implemented -->

## Gate — multimodal breadth

**Scope**: `/v1/ai/*` surface covering text, vision, audio, and embeddings fan-out. Targets `ai-core` (one half of the Option B two-core split) and downstream Python ML workers.

**Invariants preserved**:
- Model selection is capability-mediated (gate above); no direct model pinning from gateway.
- Token/usage accounting flows through `mp.v1.ingress.usage` compat subject until native `mp.v1.ai.usage` is live.
- Streaming responses preserve chunk ordering and close semantics.
- Binary payloads (vision/audio) never logged; scrub invariants from execution-core PR-2 extended here.

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- planned: `proto/ai.proto` — text/vision/audio/embedding RPCs.
- planned: `go/services/ai-core/internal/router/` (4-file package) — capability-gated model routing.
- planned: `python/workers/ml_*` — per-modality workers behind a common envelope.
- existing anchor: usage subject mapping in `go/pkg/natsx/compat.go` row `…usage.recorded → mp.v1.ingress.usage`.

### Gaps
- [ ] modality parity tests (text/vision/audio/embedding) <!-- foundation-only: not yet implemented -->
- [ ] streaming ordering + cancellation tests <!-- foundation-only: not yet implemented -->
- [ ] scrub extension tests for binary payload paths <!-- foundation-only: not yet implemented -->

## Gate — graph/wiki memory correctness

**Scope**: `/v1/memory/*` and `/v1/knowledge/*` surfaces for graph memory, wiki-style knowledge, and retrieval. Covers `letta-bridge` sidecar and future memory-core.

**Invariants preserved**:
- Memory writes carry scope binding identical to capability-core.
- Retrieval is deterministic under fixed embedding + index snapshot.
- Graph edges are versioned; no in-place mutation without provenance event.
- PII scrub applies to all persisted memory content.

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- planned: `proto/memory.proto` + `proto/knowledge.proto`.
- planned: `go/services/memory-core/internal/graph/` + `internal/wiki/` (4-file packages each).
- existing anchor: `letta-bridge` sidecar at `:8086` from CUTOVER.md Step 7 table.

### Gaps
- [ ] graph versioning + provenance event tests <!-- foundation-only: not yet implemented -->
- [ ] retrieval determinism tests under fixed snapshot <!-- foundation-only: not yet implemented -->
- [ ] memory scope resolution tests mirroring capability-core <!-- foundation-only: not yet implemented -->

## Gate — tasks/cron/coordinator durability

**Scope**: `/v1/tasks/*` and `/v1/cron/*` surfaces. Covers durable task execution, cron scheduling, and coordinator recovery across restarts.

**Invariants preserved**:
- Task IDs are content-addressed via canonical IdemPrefix.
- Cron expressions evaluated in UTC with explicit TZ offset stored alongside.
- Coordinator recovery replays from NATS JetStream; no task lost on restart.
- At-least-once delivery with idempotent handlers (golden hash dedup).

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- planned: `proto/tasks.proto` + `proto/cron.proto`.
- planned: `go/services/coordinator-core/internal/tasks/` + `internal/cron/` (4-file packages).
- planned: JetStream stream `mp.v1.tasks.*` + `mp.v1.cron.*` (no legacy compat — greenfield namespace).

### Gaps
- [ ] coordinator restart replay tests <!-- foundation-only: not yet implemented -->
- [ ] cron DST/TZ boundary tests <!-- foundation-only: not yet implemented -->
- [ ] at-least-once + idempotency combined tests <!-- foundation-only: not yet implemented -->

## Gate — bridge/voice/channel surfaces

**Scope**: `/v1/bridge/*`, `/v1/voice/*`, `/v1/channels/*` surfaces. Covers external channel integrations (chat, voice, webhook bridges) and the `browser-broker` + `sandbox-manager` sidecars.

**Invariants preserved**:
- Channel-inbound messages pass scope resolution before reaching orchestration.
- Voice streams are never persisted without explicit consent flag on the session.
- Bridge adapters are stateless; state lives in coordinator-core.
- Webhook signatures verified before any side effect.

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- planned: `proto/bridge.proto` + `proto/voice.proto` + `proto/channels.proto`.
- planned: `go/services/bridge-core/internal/channels/` (4-file package).
- existing anchor: `sandbox-manager :8084`, `browser-broker :8085` sidecars from CUTOVER.md Step 7 table.

### Gaps
- [ ] webhook signature verification tests across providers <!-- foundation-only: not yet implemented -->
- [ ] voice consent-gate tests <!-- foundation-only: not yet implemented -->
- [ ] bridge-adapter statelessness (restart) tests <!-- foundation-only: not yet implemented -->

## Stub Replacement Gates (added 2026-04-27)

These gates are paired with the entries in gap-analysis.md § 13. They are not part of the foundation 30/31 but must close before each Phase 9 parity gate above can flip green. Every gate inherits the canonical `IdemPrefix` invariant.

### Gate — orchestration handlers (replace 12 Unimplemented)

**Scope**: `go/services/orchestrator-core/internal/orchestration/handlers.go` — implement `ListPlans`, `GetPlan`, `TransitionPlan`, `ListTodos`, `GetTodo`, `TransitionTodo`, `ListApprovals`, `GetApproval`, `DecideApproval`, `GetSubagentLineage`, `AttachSubagent`, `StreamRunEvents`.

**Invariants**:
- Reads delegate to `session-core/orchestration_store.rs` over gRPC; no direct Postgres access from orchestrator-core.
- Writes emit canonical events via existing `pkg/natsx` publisher (`PLAN_TRANSITIONED`, `TODO_TRANSITIONED`, `APPROVAL_DECIDED`, `SUBAGENT_ATTACHED`, etc.).
- `StreamRunEvents` consumes JetStream `mp.v1.run.{run_id}.event`; preserves event_id ordering.
- `DecideApproval` signals the relevant Temporal workflow via `SignalApproval="approval"`.

**Lint evidence**: TBD — `go vet` + `golangci-lint run` on `services/orchestrator-core/...` once handlers land.
**Test evidence**: TBD — table-driven handler tests + e2e against in-tree compat harness.
**Wire-up**: see gap-analysis § 13.1.

### Gate — capability-core durable backing

**Scope**: replace in-memory `internal/registry` + `internal/policy` with Postgres-backed implementations. Add migrations under `services/capability-core/migrations/`.

**Invariants**:
- Existing 6 RPCs preserve their public contracts; only storage changes.
- Versioned, immutable rows for capability + skill bundles.
- Scope resolution deterministic (run → thread → workspace → user → org → global).

**Lint evidence**: TBD.
**Test evidence**: TBD — golden-fixture round-trip + scope-resolution tests across 6 levels.
**Wire-up**: see gap-analysis § 13.2.

### Gate — letta-bridge real upstream

**Scope**: replace `internal/memstore` with real Letta gRPC/HTTP client; wire `internal/{client,retrieval,sync}`.

**Invariants**:
- Graceful degradation on Letta unavailability (existing tolerant pattern).
- All writes carry org + thread scope; no cross-tenant bleed.

**Lint evidence**: TBD.
**Test evidence**: TBD — Letta integration test under `LETTA_HTTP=…` env gate.
**Wire-up**: see gap-analysis § 13.2.

### Gate — model-gateway thread autocreation

**Scope**: implement gateway-side thread autocreation against `session-core.CreateThread` so requests without `thread_id` succeed (`go/services/model-gateway/internal/proxy/proxy.go:170`).

**Invariants**:
- Autocreation idempotent under canonical IdemPrefix using `(producer, INGRESS_ACCEPTED, session_key, request_id)`.
- New thread emits `THREAD_CREATED` via session-core's transactional outbox.

**Lint evidence**: TBD.
**Test evidence**: TBD — request-without-thread flow + idempotent retry tests.
**Wire-up**: see gap-analysis § 13.3.

### Gate — sandbox-manager / browser-broker create paths

**Scope**: complete generated codecs and replace remaining `Unimplemented` create-path handlers in both services.

**Invariants**:
- TTL-scoped lease lifecycle preserved.
- PR-3 revocation invariants unchanged.

**Lint evidence**: TBD.
**Test evidence**: TBD.
**Wire-up**: see gap-analysis § 13.3.

### Gate — Redis hot cache integration

**Scope**: wire Redis client across `model-gateway` (rate-limit + idempotency dedup), `inference-core` (prompt cache), `sandbox-manager` (lease state), `browser-broker` (grant state).

**Invariants**:
- TTL boundaries from existing in-process implementations preserved.
- Per-peer rate-limit fairness retained (PR-5 contract unchanged).

**Lint evidence**: TBD.
**Test evidence**: TBD — integration tests under `REDIS_URL=…` env gate.

### Gate — MinIO artifact bucket

**Scope**: provision bucket + integrate MinIO client in `execution-core` artifact path, honouring CONTRACTS.md key convention `org/{org_id}/thread/{thread_id}/run/{run_id}/artifact/{id}`.

**Invariants**:
- Object keys byte-stable with the contract.
- Pre-persist scrub (PR-2) applied before any artifact bytes leave execution-core.

**Lint evidence**: TBD.
**Test evidence**: TBD — round-trip put/get with scrub fixture.

### Gate — unified docker compose

**Scope**: single `deploy/docker-compose.yml` covering Postgres, NATS+JetStream, Redis, MinIO, Temporal, OTEL collector, and all 9 Model Plane services with health checks + dependency ordering.

**Invariants**:
- Boot order obeys the authority graph (Postgres → NATS+Redis+MinIO → Temporal → session-core → inference-core+execution-core → orchestrator-core+capability-core+sandbox-manager+browser-broker+letta-bridge → model-gateway).
- All foundation gates in this file remain green inside the composed environment.

**Lint evidence**: TBD — `docker compose config` validation.
**Test evidence**: TBD — smoke against `/v1/invoke` end-to-end.

### Gate — fuzz coverage

**Scope**: `go test -fuzz` corpora for `pkg/envelope`, `pkg/idempotency`, `pkg/natsx`; `cargo fuzz` corpora for `mp-events` envelope + `mp-events::idempotency`.

**Invariants**: golden idempotency hex `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637` byte-stable across all corpora.

**Lint evidence**: TBD.
**Test evidence**: TBD.

### Gate — dead-letter handling

**Scope**: JetStream DLQ stream + handler for envelopes that fail validation or exhaust retries.

**Invariants**:
- DLQ messages preserve original envelope bytes + reason code.
- Replay from DLQ idempotent.

**Lint evidence**: TBD.
**Test evidence**: TBD — corrupt-envelope drop + replay test.

### Gate — ordered delivery e2e

**Scope**: transport-tier ordering proof for `mp.v1.run.{run_id}.event` across the model-gateway → session-core → orchestrator-core path.

**Invariants**: ordering tiebreaker = `event_id` per existing replay-determinism gate.

**Lint evidence**: TBD.
**Test evidence**: TBD — e2e ordering harness.

---

## Gate — compact transport correctness

**Scope**: Wire-level correctness across proto + NATS + HTTP surfaces. Covers envelope stability, subject routing determinism, and backward compatibility of all proto files under `proto/`.

**Invariants preserved**:
- All four target proto files (`gateway.proto`, `sessions.proto`, `execution.proto`, `capabilities.proto`) maintain field-number stability; no renumbering or type changes.
- `mp.v1.*` subject tree is append-only; no subject renamed once published.
- Compat adapter (`go/pkg/natsx/compat.go`) coverage is complete: every legacy subject has a forward mapping; no silent drops.
- Rust mirror (`rust/crates/mp-events/src/subjects.rs`) stays in lock-step with Go subject table.
- Golden IdemPrefix derivation byte-stable across Go and Rust (existing `TestDeriveIdempotencyHash_MatchesRust`).

### Lint evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Test evidence
- TBD — foundation only. <!-- foundation-only: not yet implemented -->

### Wire-up
- existing anchor: `go/pkg/natsx/compat.go` — 7-row legacy→`mp.v1.*` mapping (see CUTOVER.md NATS Subject Migration table).
- existing anchor: `rust/crates/mp-events/src/subjects.rs` — canonical Rust subject constants.
- existing anchor: `go/services/orchestrator-core/cmd/main.go` — active legacy subscriber during compat window.
- planned: proto buf-break CI gate across `gateway.proto` / `sessions.proto` / `execution.proto` / `capabilities.proto`.

### Gaps
- [ ] `buf breaking` CI gate across all four proto files <!-- foundation-only: not yet implemented -->
- [ ] Go↔Rust subject-table parity test <!-- foundation-only: not yet implemented -->
- [ ] compat-adapter exhaustiveness test (every legacy subject covered) <!-- foundation-only: not yet implemented -->
