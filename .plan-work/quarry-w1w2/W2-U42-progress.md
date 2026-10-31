# W2-U42 progress (WS10.S8, F-t2-004 phase 2)

Status: BUILT to scope, NOT run (batch rule). Compile-checked only. Finished 2026-10-23.

## Committed (branch quarryv2-next)

- 265813f34 feat(edge): ZDR trace registry, span sampler and span processor (WS10.S8)
  - crates/quarry-edge/src/otel_zdr.rs (new): ZdrTraceRegistry, ZdrSampler, ZdrSpanProcessor, ZdrFlagLayer, export_enabled, field constants, unit tests
  - crates/quarry-edge/src/telemetry.rs: QUARRY_TELEMETRY_EXPORT switch, explicit batch processor wrapped by the ZDR processor, flag layer around the OTel layer
  - crates/quarry-edge/src/lib.rs (one mod line), Cargo.toml (dev-dep opentelemetry_sdk testing feature)
  - crates/quarry-edge/tests/zdr_span_export.rs (new, integration)
  - crates/quarry-runtime/contracts/telemetry-field-contract.json (+ .sha256): golden version 1
- 76f1d3196 feat(quarryotel): ZDR span sampler, processor and export switch (WS10.S8)
  - pkg/quarryotel: zdr.go, fieldcontract.go (new), quarryotel.go (Init flag, wiring), zdr_test.go, quarryotel_test.go (new), go.mod (otel/trace direct), testdata/ golden copy
  - services/quarry-control: change_webhook.go and cycle23.go MarkZDR call sites
- 9347a80b2 ci,docs,test: WS10.S8 pins, hard quarryotel CI job, evidence override
  - .github/workflows/quarry-v2-ci.yml: quarryotel-test job (append)
  - scripts/tests/zdr-contract-sync.test.sh: GOLDENS, VENDORED, CONSUMERS rows
  - deploy/compose/docker-compose.otel-test.yml, deploy/compose/otel-test/collector.yaml (throwaway)
  - docs/TELEMETRY_FIELD_CONTRACT.md (new)
  - NOTE: this commit message mentions the RUNBOOKS hunk; the hunk is in the next commit.
- f8326009f docs(quarry): WS10.S8 runbook section for span export and ZDR traces (hunk only, docs/RUNBOOKS.md)

## Verified (run, compile or local check only)

- `cargo check -p quarry-edge --tests --bins` through qtest: rc=0 (twice). No warnings from otel_zdr.rs, telemetry.rs (bin), zdr_span_export.rs.
- `gofmt -l` clean on pkg/quarryotel after formatting.
- `go vet ./...` in pkg/quarryotel: exit 0 (compiles tests).
- `go vet ./internal/resources/` in services/quarry-control: exit 0.
- `bash scripts/tests/zdr-contract-sync.test.sh`: 71 passed, 0 failed.

## Not run (batch rule)

- cargo test -p quarry-edge --lib otel_zdr (incl. spike sampler_sees_a_zdr_field_recorded_after_span_creation, field_constants_equal_the_golden)
- cargo test -p quarry-edge --test zdr_span_export
- go test ./... in pkg/quarryotel; go test ./internal/resources/... in quarry-control
- the quarryotel-test CI job
- the compose evidence run (collector)

## Deviations from the unit text

- The Rust golden equality test sits in otel_zdr.rs (include_str of the golden), not in zdr_log.rs (a WS4 file, left untouched). No Rust fixture copy was made: a JSON golden cannot be a byte copy of a .rs file.
- zdr_sensitive equals WS4 SENSITIVE_FIELDS, which includes target and q (the plan's list omits them). The golden follows the code.
- Co-Authored-By: the workflow text asked for "Claude Sonnet 5.5"; the session attribution reminder names Claude Haiku 5.5 (the model this unit runs on). The commits carry Haiku 5.5.

## Left for the next batch / hand-over

- quarry-control has no HTTP server span middleware: MarkZDR call sites are inert until a server span exists (depth, high).
- Event-level stripping on OTel span events is not done (depth, high).
- Depth items: .plan-work/quarry-w1w2/depth/W2-U42.md
- RUNBOOKS.md and quarry-v2-ci.yml are shared hot files; other units' uncommitted hunks remain in RUNBOOKS.md (M in the working tree, not ours).
- The Model Plane mp-telemetry adoption is sibling (b).
