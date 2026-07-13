# Control Plane Audit

Date: 2026-07-02

Scope: `apps/Control Plane`

This is a plane-local audit report. It focuses on Control Plane authority, checked gates, and cross-plane trust gaps without editing service code.

## Current Shape

Control Plane owns identity, users, organizations, billing, sessions, audit, quotas, and entitlements. Go services cover user/org/billing/session/audit authority. `auth-core` is a NestJS/TypeScript service with Better Auth-adjacent runtime, auth/session/token flows, gRPC/NATS surfaces, and plane-token issuance.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `go test ./...` in `audit-core` | Pass | Checked service tests passed. |
| `go test ./...` in `billing-core` | Pass | Checked service tests passed. |
| `go test ./...` in `org-core` | Pass | Checked service tests passed. |
| `go test ./...` in `session-core` | Pass | Checked service tests passed. |
| `go test ./...` in `user-core` | Pass | Checked service tests passed. |
| `pnpm exec jest --runInBand` in `auth-core` | Pass | 1 suite, 4 tests passed; Node emitted the recurring `--localstorage-file` warning. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `gofmt -l audit-core billing-core org-core session-core user-core` | Fail | Formatting drift in audit, billing, org, session, and user Go services. |
| Go vet over checked Go services | Pass | audit-core, billing-core, org-core, session-core, and user-core pass vet. |
| `staticcheck` over checked Go services | Blocked | Local staticcheck was built with Go 1.25 and cannot analyze Go 1.26 source. |
| `pnpm exec eslint "{src,apps,libs,test}/**/*.ts"` in `auth-core` | Fail | 649 problems: 617 errors and 32 warnings, including Prettier drift, unsafe `any`, floating promises, unused vars, and `require-await`. |
| `pnpm build` in `auth-core` | Pass | `nest build` completed. |
| `npx -y knip --no-progress` in `auth-core` | Fail | Reports 11 unused files, 12 unused dependencies, 8 unused devDependencies, 30 unused exports, and 75 unused exported types. |

## Live Validation Addendum

Additional validation run against the local Control Plane runtime:

| Probe | Result | Notes |
|---|---|---|
| `bash test-control-plane-integration.sh` | Pass | Updated script checks current auth session endpoint, user/org/billing/session/audit health, and `controlplane-postgres`; 9 passed, 0 failed. |
| audit-core health port | Confirmed | Native audit-core health is on host/container 8187; host 3016 is Lago, not audit-core. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P0 | Data Plane production readiness depends on Control Plane consultation. | Data Plane `docs/gap-data.md` states `X-Org-ID` trust needs auth-core/user-core/org-core/cost-core consultation. | Publish a small validation contract for org/user/session/quota checks consumed by Data Plane and gateway clients. |
| P1 | Active org/user scoping headers need one source of truth across planes. | Velion gateway strips forged headers; Data Plane still documents `X-Org-ID` trust risk. | Audit header names and trust boundaries across Control, gateway, Data, and Ingestion. |
| P1 | `auth-core` has a large lint backlog despite building successfully. | ESLint reports 649 total problems across auth, NATS, ORPC, security, service, and user modules. | Split mechanical Prettier fixes from unsafe-typing/promise-handling fixes, then rerun ESLint. |
| P2 | Control Go services have formatting drift. | `gofmt -l` reports drift across audit, billing, org, session, and user services. | Run `gofmt` as a mechanical cleanup and keep the gate in CI. |
| P2 | `auth-core` has sensitive fallback behavior that needs production hardening review. | Existing `auth-core` research notes identify mock email/SMS behavior, placeholder auth/admin paths, and development mock OAuth URLs. | Verify production config cannot enter dev/mock behavior and gate incomplete admin surfaces. |
| P2 | `auth-core` has duplication and source residue. | Existing research notes identify overlapping token controllers, monolithic `orpc-router.ts`, `.unused`, `.backup`, and `.DS_Store` residue. | Split cleanup into safe delete/refactor tickets after confirming public API compatibility. |
| P2 | Knip reports likely unused `auth-core` files, dependencies, and exports. | `npx -y knip --no-progress` reports unused scripts, generated proto/contracts, users module, smoke tests, dependencies, and exported symbols. | Triage scripts/generated files before deleting; verify public Nest/gRPC/NATS entrypoints. |
| P2 | Checked service tests are green, but coverage gates were not run. | Go/Jest commands above passed; `auth-core test:cov` and `user-core make test-coverage` were not run. | Run coverage gates before declaring release confidence. |
| P3 | Go static analysis is blocked by local analyzer/toolchain skew. | Staticcheck reports Go 1.26 source requiring a newer analyzer than the installed Go 1.25-built binary. | Upgrade/reinstall staticcheck with the active Go toolchain and rerun. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Auth-core full test surface | Only the available Jest suite was run directly. | Run `pnpm test`, `pnpm test:cov`, and e2e/smoke commands with expected env. |
| Billing/org/user integration behavior | Service-local tests passed, but Data Plane consultation contract is not implemented. | Add cross-plane contract tests once the validation API is defined. |
| Pre-existing local changes | The broader worktree had many Control Plane modifications before this docs pass. | Review/stabilize local changes before using this worktree as release evidence. |

## Quality Gate

- Go service tests: pass for checked services.
- Live Control smoke: pass.
- Go format: fail.
- Go vet: pass.
- Staticcheck: blocked by analyzer/toolchain mismatch.
- Auth-core Jest: pass for checked suite.
- Auth-core build: pass.
- Auth-core ESLint/Knip: fail.
- Coverage: not run.
- Cross-plane consultation: not implemented/verified.

## Recommended Remediation Order

1. Define Control validation contract for Data Plane and gateway callers.
2. Audit active org/user/session header trust boundaries.
3. Harden auth-core production/dev fallback behavior.
4. Clean inactive auth-core residue and duplicate token-controller logic.
5. Run coverage and e2e/smoke gates.
