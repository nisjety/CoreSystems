# Model Plane Audit

Date: 2026-07-02

Scope: `apps/Model Plane`

This is a plane-local audit report. It separates Rust runtime status from Go control-service status because the gates currently differ.

## Current Shape

Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants, bridge services, and cost. Rust services cover the hot reasoning/runtime path. Go services cover Temporal orchestration, registries, sandboxes, browser grants, memory bridge, cost, and bridge boundaries.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `cargo test --workspace` in `rust` | Pass | Full Rust workspace passed; live-DB/browser/doc examples are intentionally ignored. |
| `go test ./...` across Go core packages and `services/orchestrator-core` | Fail | Core packages passed; `orchestrator-core/internal/orchestration` test stubs miss `ListPendingApprovals`. |
| `go test ./...` across remaining Go services | Fail | capability/browser/sandbox/cost/bridge passed; `letta-bridge/internal/memstore` failed time-range filtering. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` in `rust` | Fail | Formatting drift in execution-core and model-gateway modules. |
| `cargo clippy --workspace --all-targets -- -D warnings` in `rust` | Fail | `execution-core/src/promote_on_use.rs:22` hits `clippy::doc_markdown` for `web_fetch`. |
| `gofmt -l .` in `go` | Fail | Formatting drift in `pkg/natsx`, `pkg/quarry`, bridge, capability, orchestrator, and test files. |
| Go vet over Model Go modules | Fail | `orchestrator-core/internal/orchestration` test stub does not implement `ListPendingApprovals`. |
| `go test ./...` over actual Go services/packages | Fail | `pkg/natsx`, `pkg/quarry`, bridge, browser, capability, cost, sandbox pass; `orchestrator-core` and `letta-bridge` fail. |
| `staticcheck` over Go services/packages | Blocked | Local staticcheck was built with Go 1.25 and cannot analyze Go 1.26 source. |

## Live Validation Addendum

Additional validation run against the local Model Plane scripts:

| Probe | Result | Notes |
|---|---|---|
| `bash scripts/verify-durable-layer.sh` | Pass | Script now waits for successful SQL readiness against `session_core` before migrations; durable-layer assertions pass against throwaway Postgres. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P1 | `orchestrator-core/internal/orchestration` tests no longer compile against the generated orchestration client interface. | `handlers_test.go` stubs do not implement `ListPendingApprovals`. | Update fake clients or generate compliant mocks, then rerun `go test ./...` in `services/orchestrator-core`. |
| P1 | `letta-bridge/internal/memstore` time-range filtering includes an old record. | `TestTimeRangeFiltering/cutoff_excludes_old_record` got 2 hits and wanted 1. | Clarify cutoff inclusivity/time source behavior and fix the filter or fixture. |
| P1 | Memory consolidation and skill promotion activities are still documented as placeholders. | `apps/Model Plane/README.md` lists orchestrator memory consolidation and skill promotion activities as placeholder implementations. | Replace placeholders or gate dependent product features. |
| P1 | Rust runtime is green, but Model Plane as a whole is not green because Go gates fail. | Rust workspace passed; Go failures above. | Treat Rust and Go status separately in release checks. |
| P2 | Rust formatting and clippy gates are not clean despite tests passing. | `cargo fmt --all -- --check` fails; clippy blocks on a doc markdown warning in `promote_on_use.rs`. | Apply mechanical formatting and patch the doc comment before using Rust test green as release evidence. |
| P2 | Model Go formatting drift spans packages and services. | `gofmt -l .` reports drift in `pkg/natsx`, `pkg/quarry`, bridge, capability, and orchestrator files. | Run `gofmt` as a mechanical cleanup. |
| P2 | Full live cross-service validation remains incomplete. | Model README still lists `model-gateway -> session-core -> inference-core -> execution-core` validation as remaining work. | Run the verification plan with live service dependencies. |
| P2 | Auth/security hardening remains a documented follow-up. | Model README lists issuer/audience/key rotation/claim tests and malformed token coverage. | Convert security hardening list into automated tests where not already covered. |
| P3 | Go static analysis is blocked by local analyzer/toolchain skew. | Staticcheck reports Go 1.26 source requiring a newer analyzer than the installed Go 1.25-built binary. | Upgrade/reinstall staticcheck with the active Go toolchain and rerun. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Live Temporal workflows | Unit tests passed/fail as above, but live Temporal stack was not started. | Run compose and workflow smoke tests. |
| Browser broker with Quarry | Rust browser-agent e2e test is intentionally ignored without live quarry-edge/inference-core stack. | Run the live e2e once Ingestion Plane compile gates pass. |
| Cost and capability service release readiness | Service-local tests passed, but integration contracts were not exercised here. | Run cross-service contract tests and NATS/Temporal smoke tests. |

## Quality Gate

- Rust workspace tests: pass.
- Durable-layer integration script: pass.
- Rust format/clippy: fail.
- Go package/service tests: fail in orchestrator-core and letta-bridge; other checked packages/services pass.
- Go vet: fail due the same orchestrator-core stub interface drift.
- Go format: fail.
- Staticcheck: blocked by analyzer/toolchain mismatch.
- Live stack verification: not run.

## Recommended Remediation Order

1. Update orchestrator-core test stubs for `ListPendingApprovals`.
2. Fix letta-bridge memstore cutoff filtering.
3. Decide whether placeholder memory consolidation/skill promotion paths should be implemented or feature-gated.
4. Run Model Plane Go tests end to end.
5. Run live cross-service verification from `docs/VERIFICATION.md`.
