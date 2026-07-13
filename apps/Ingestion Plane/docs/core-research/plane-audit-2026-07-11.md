# Ingestion Plane Audit

Date: 2026-07-02

Scope: `apps/Ingestion Plane`

This is a plane-local audit report. It focuses on Quarry-v2, import/connectors, and cross-plane ingestion boundaries without editing service code.

## Current Shape

Ingestion Plane owns evidence capture and acquisition. `Quarry-v2` is the active web/search ingestion target for Velion v3. Legacy `Quarry/` references still exist in tooling/docs and should not be used for new Velion v3 work.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `cargo test --workspace` in `Quarry-v2` | Fail | Compile failure from stale `DataPlaneIngestRequest` constructors. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` in `Quarry-v2` | Fail | Formatting drift in `quarry-runtime` CAS, scheduler, ingest client, lib, and page-image modules. |
| `cargo clippy --workspace --all-targets -- -D warnings` in `Quarry-v2` | Fail | Same stale `DataPlaneIngestRequest` constructors plus `clippy::unnecessary_to_owned` in `pipeline.rs`. |
| `gofmt -l services pkg` | Fail | Formatting drift in quarry-control, quarry-orchestrator, and quarrycontracts files. |
| Go vet over Quarry services/packages | Pass | `quarry-control`, `quarry-orchestrator`, `quarrycontracts`, and `quarryotel` pass vet. |
| `staticcheck` over Quarry Go services/packages | Partial/blocking | Reports unused `store.mu` and unused `scheduleName`; analyzer also hits Go 1.26 vs Go 1.25 toolchain skew. |

## Live Validation Addendum

Additional validations run against the local Ingestion runtime:

| Probe | Result | Notes |
|---|---|---|
| Updated `smoke-test-integration-api.sh` | Fail on real boundary | Current health, provider catalog, auth rejection, internal key, connect-session, and not-found checks pass; GitHub webhook signature checks fail open. |
| GitHub webhook without signature | Fail | `/api/v1/webhooks/github` returned 200 accepted with no signature. |
| GitHub webhook with invalid signature | Fail | `/api/v1/webhooks/github` returned 200 accepted with `sha256=invalid`. |
| Quarry control job registry | Open risk | Host `/v1/jobs` on control returns 200 under rollout mode. |
| Quarry edge job registry | Protected/unproven | Host `/v1/jobs` on edge returns 401 without edge auth/HMAC context. Velion onboarding still calls control directly in source. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P0 | Velion v3 onboarding still has a documented direct `quarry-control` path that bypasses `quarry-edge`. | `Quarry-v2/docs/ARCHITECTURE.md` says onboarding crawl handlers post to control `/v1/jobs/` directly and work only while HMAC rollout mode trusts private-network calls. | Migrate onboarding crawl handlers to `quarry-edge` and block direct cross-plane control calls. |
| P0 | Integration API accepts GitHub webhooks with missing or invalid signatures in the live environment. | Updated smoke script fails because `/api/v1/webhooks/github` returns 200 accepted for both no signature and invalid signature. | Require configured provider webhook secrets or reject unsigned provider webhooks by default. |
| P1 | Quarry-v2 tests fail to compile after Data Plane ingest contract expansion. | `crates/quarry-core/tests/contracts.rs:231` and `crates/quarry-runtime/src/ingest_client.rs:380` construct `DataPlaneIngestRequest` without `initiator_user_id` and `visibility`. | Update constructors and contract tests with explicit initiator/visibility behavior. |
| P1 | Top-level Ingestion Makefile still targets legacy `Quarry`. | `apps/Ingestion Plane/Makefile` uses `cd Quarry` for setup/dev/test and docs output. | Update targets to Quarry-v2 or explicitly label legacy commands. |
| P2 | Quarry-v2 has Rust and Go formatting drift. | `cargo fmt --all -- --check` and `gofmt -l services pkg` both fail. | Run mechanical formatters after coordinating with active branches. |
| P2 | HMAC rollout mode and trust-the-network behavior need deployment verification. | Quarry-v2 architecture docs describe `quarry-control` as HMAC-internal and note degraded trust when secret is unset. | Verify production env requires HMAC and that gateway callers target edge. |
| P2 | Quarry-control resource/schedule surfaces still contain partial behavior. | Existing core research notes identify stubbed schedule/source/backfill behavior in control resources. | Convert partial resource families into tracked service issues with endpoint-level tests. |
| P2 | Runtime durability still has in-memory compatibility paths. | Existing core research notes identify in-memory store/queue paths in control/runtime. | Document allowed dev-only use and add production config guards. |
| P3 | Staticcheck found likely unused Go symbols before hitting toolchain skew. | `services/quarry-control/internal/store/store.go:219` has unused `mu`; `services/quarry-control/internal/temporal/client.go:113` has unused `scheduleName`. | Confirm these are not future hooks, then remove or wire them once staticcheck is upgraded. |
| P3 | Rust clippy has one production cleanup beyond the contract compile failure. | `crates/quarry-runtime/src/pipeline.rs:582` reports unnecessary `to_string()`. | Use the suggested borrowed value after the contract compile fix. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Endpoint health | This pass did not start Docker Compose or run `make test-endpoints`. | Run after compile failures are fixed and local services are available. |
| imports/integration/finspo current gates | No Python/Go tests were rerun in this pass. | Run service-local tests for imports-core, integration-corev2, finspo-core, autocomplete-core, and support-worker. |
| Data Plane ingest semantics | `initiator_user_id` and `visibility` need product/security decisions. | Confirm with Data and Control owners before patching defaults. |

## Quality Gate

- Quarry-v2 Rust workspace tests: fail at compile time.
- Quarry-v2 Rust format/clippy: fail.
- Quarry Go format: fail.
- Quarry Go vet: pass.
- Staticcheck: partial findings, then blocked by analyzer/toolchain mismatch.
- Endpoint smoke tests: not run.
- Import/connectors tests: not run in this pass.

## Recommended Remediation Order

1. Patch Quarry-v2 `DataPlaneIngestRequest` constructors and rerun `cargo test --workspace`.
2. Move Velion onboarding crawl jobs through `quarry-edge`.
3. Update top-level Ingestion Makefile/help output away from legacy `Quarry`.
4. Verify HMAC-required deployment behavior.
5. Run endpoint and connector/import service tests.
