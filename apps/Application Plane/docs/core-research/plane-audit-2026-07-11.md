# Application Plane Audit

Date: 2026-07-02

Scope: `apps/Application Plane`

This is a plane-local audit report. It focuses on Application Plane projection/runtime services and does not rework cross-plane shared docs.

## Current Shape

Application Plane owns collaborative/realtime workspace projections and notification/application-facing services. It must not become the authority for identity, billing, durable knowledge, ingestion, or reasoning. Its current service set includes Convex projection/runtime assets, conversation services, information/insight/leads/notification/social services, and Zammad foundation assets.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `go test ./...` in `conversation-core/conversation-core-go` | Pass | Checked service tests passed. |
| `go test ./...` in `information-core` | Pass | Checked service tests passed. |
| `go test ./...` in `insight-core` | Pass | Checked service tests passed. |
| `go test ./...` in `leads-core` | Pass | Checked service tests passed. |
| `go test ./...` in `notification-core` | Pass | Checked service tests passed. |
| `go test ./...` in `social-core` | Pass | Checked service tests passed. |
| `pnpm typecheck` in `convex-core` | Blocked | pnpm exits before the script because `esbuild@0.27.0` has ignored build scripts pending approval. |
| `pnpm lint` in `convex-core` | Blocked | Same pnpm ignored-build policy block. |
| `./node_modules/.bin/tsc --noEmit` in `convex-core` | Pass | Direct TypeScript check passed. |
| `./node_modules/.bin/eslint "convex/**/*.ts"` in `convex-core` | Pass | Direct ESLint check passed. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `gofmt -l conversation-core/conversation-core-go information-core insight-core leads-core notification-core social-core` | Fail | Formatting drift in conversation, information, and notification Go files. |
| Go vet over checked Go services | Pass | conversation, information, insight, leads, notification, and social pass vet. |
| `go test ./...` over checked Go services | Pass | conversation, information, insight, leads, notification, and social tests pass. |
| `staticcheck` over checked Go services | Blocked | Local staticcheck was built with Go 1.25 and cannot analyze Go 1.26 source. |
| `npx -y knip --no-progress` in `convex-core` | Fail | Reports unused `convex.config.ts`, `nats-subscriber.js`, unused dependency `nats`, and a package entry-file mismatch for `index.js`. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P2 | Convex-core pnpm script gates are blocked by package build-script policy. | `pnpm typecheck` and `pnpm lint` fail with `ERR_PNPM_IGNORED_BUILDS` for `esbuild@0.27.0`. | Approve or document pnpm build-script policy, then rerun scripts through pnpm. |
| P2 | Application Go services have formatting drift despite tests and vet passing. | `gofmt -l` reports drift in conversation-core, information-core, and notification-core files. | Run `gofmt` as a mechanical cleanup and keep the gate in CI. |
| P2 | Convex-core has small but concrete dead-code/package-entry scan findings. | Knip reports unused `convex.config.ts`, `nats-subscriber.js`, unused dependency `nats`, and missing package entry `index.js`. | Confirm whether `nats-subscriber.js` is operational tooling; fix package entry metadata or remove stale files/deps. |
| P2 | Application Plane lacks a current top-level orientation doc outside service-local notes. | No `apps/Application Plane/README.md` or `APPLICATION_PLANE_ARCHITECTURE.md` was found; core-research README is service-local. | Add or designate a top-level Application Plane orientation doc if this plane is actively onboarded by agents. |
| P2 | Application projection authority needs explicit guardrails. | Cross-plane rules say Application may project/mirror state but does not own lower-plane authorities. | Add docs/tests for projected state vs canonical authority in inbox, social, conversation, and notification surfaces. |
| P2 | Application gateway naming is confusing beside the current Velion v3 Frontend gateway. | Existing core research includes `velion-gateway-rs`; current Velion v3 also has `apps/Frontend Plane/velionv3/apps/gateway`. | Clarify whether Application `velion-gateway-rs` is transitional, legacy, or still live for onboarding. |
| P3 | Go service tests are green for checked services, but Convex runtime/integration was not exercised. | Commands above. | Add Convex runtime tests or smoke checks after pnpm policy is settled. |
| P3 | Go static analysis is blocked by local analyzer/toolchain skew. | Staticcheck reports Go 1.26 source requiring a newer analyzer than the installed Go 1.25-built binary. | Upgrade/reinstall staticcheck with the active Go toolchain and rerun. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Convex generated/runtime behavior | Direct `tsc` and ESLint pass, but pnpm script wrappers are blocked. | Resolve build-script approval and run official scripts plus Convex dev/codegen checks. |
| Realtime integration coverage | Go service tests passed, but NATS/Convex realtime integration was not run. | Run compose or integration smoke tests with shared bus dependencies. |
| Zammad foundation status | Not tested in this pass. | Run its package scripts or bootstrap smoke tests if it remains active. |

## Quality Gate

- Checked Go services: pass.
- Checked Go service vet: pass.
- Checked Go service format: fail.
- Convex direct typecheck/lint: pass.
- Convex pnpm scripts: blocked by ignored-build approval.
- Convex Knip: fail.
- Staticcheck: blocked by analyzer/toolchain mismatch.
- Runtime/integration stack: not run.

## Recommended Remediation Order

1. Resolve Convex pnpm ignored-build approval and rerun official scripts.
2. Clarify Application `velion-gateway-rs` vs Frontend Velion v3 gateway ownership.
3. Add or designate a top-level Application Plane orientation doc.
4. Add projection authority tests/docs for realtime/application surfaces.
5. Run Convex/runtime integration smoke tests.
