# Velion v3 Plane Audit

Date: 2026-07-02

Scope: `apps/Frontend Plane/velionv3`

This is a plane-local audit report. It intentionally does not update the shared cross-plane map except where the separate system overview already points at this directory.

## Current Shape

Velion v3 currently has three runtime surfaces:

- SolidJS/Vite/TypeScript app at the plane root.
- Rust Axum same-origin gateway under `apps/gateway`.
- Nested Next.js app under `apps/velion-web`.

The root app should be treated as the current Velion workspace target. The gateway is now part of the Frontend Plane boundary and normalizes access to Control, Data, Ingestion, Model, and Application services. The nested Next app still needs project-specific ownership documentation.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `pnpm lint` | Pass | ESLint completed from the Velion v3 root. |
| `pnpm typecheck` | Pass | `tsc -b` completed. |
| `pnpm test` | Fail | 46 test files and 202 tests passed, but Vitest failed on 2 unhandled rejections from `loadStudioWorkspace`. |
| `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` | Pass | 146 Rust gateway tests passed. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `pnpm build` | Pass | Ran `pnpm typecheck && vite build`; main CSS emitted at 510.81 kB and Vite reported significant time in the Solid plugin. |
| `cargo fmt --manifest-path apps/gateway/Cargo.toml -- --check` | Fail | Formatting drift in `cost.rs`, `eval.rs`, `middleware.rs`, and `rate_limit.rs`. |
| `cargo clippy --manifest-path apps/gateway/Cargo.toml --all-targets -- -D warnings` | Pass | Rust gateway clippy gate is clean once formatting is ignored. |
| `npx -y knip --no-progress` | Fail | Reports 64 unused files, 3 unused dependencies, 172 unused exports, 131 unused exported types, and 2 duplicate exports; route/framework exports need false-positive triage. |

## Live Validation Addendum

Additional validations run against the local Velion v3 runtime:

| Probe | Result | Notes |
|---|---|---|
| Playwright harness discovery | Gap confirmed | No `playwright.config.*`, Playwright dependency, or e2e test tree was found under `velionv3`. |
| Gateway session without cookie | Expected unauthenticated | `/api/v1/auth/session` returns 200 `null`; `/api/v1/me` returns 401. |
| Throwaway signup/signin through gateway | Partial | Signup returns 200, but signin returns `EMAIL_NOT_VERIFIED`; protected `/api/v1/me` and session-context remain 401. |
| Gateway auth mode | Strict | `velion-gateway-rs` runs with `ALLOW_DEV_AUTH_BYPASS=false` and `CONTROL_PLANE_ENFORCEMENT=strict`. |
| Onboarding Quarry wiring | Gap confirmed | Website/crawl-preview onboarding code still posts jobs/events through `state.quarry_control_url`; general ingestion domains use `state.quarry_edge_url`. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P1 | Studio workspace loading can throw when session context lacks `orgs`. | `src/features/studio/lib/studio-canvas-model.ts:43` uses `ctx?.orgs[0]?.id`; Vitest reports unhandled rejections from `StudioPage.test.tsx`. | Use a safe optional access shape, add a regression test for context without `orgs`, rerun `pnpm test`. |
| P1 | No authenticated Velion Playwright/user journey exists yet. | No Playwright harness was found; live signup works but signin is blocked by email verification, so protected gateway routes remain unproven with a verified session. | Add a seeded verified test account or test-only verified session fixture, then add Playwright coverage for onboarding, knowledge/search, and at least one downstream plane action. |
| P1 | Runtime ownership docs are fragmented across Solid root, Rust gateway, and nested Next app. | Root README documents Solid/Vite; gateway tests prove Rust gateway is active; `apps/velion-web/README.md` is still generated Next template text. | Add an ownership section to the Velion v3 README or a focused frontend-plane runtime doc. |
| P2 | Rust gateway formatting drift blocks the format gate. | `cargo fmt --manifest-path apps/gateway/Cargo.toml -- --check` fails in four gateway files. | Run `cargo fmt` in the gateway once mechanical formatting changes are in scope. |
| P2 | Knip reports broad dead-code and export candidates across the root app and nested Next app. | `npx -y knip --no-progress` reports unused files/dependencies/exports and duplicate `SharedWithMePage`/`LeadsPage` exports. | Triage generated, route, and framework-entry false positives before deleting code. |
| P2 | The global stylesheet is a maintainability hotspot. | `src/styles/global.css` has 30,700 lines. | Split by tokens/layout/features or document generated/manual ownership before more UI work. |
| P2 | Preview/fallback-backed surfaces need an explicit live-wiring inventory. | Fallback paths exist in social workspace, shared read-data helpers, studio/agent comments, and design-preview tests. | Classify each fallback as honest empty state, planned state, or missing owner-plane integration. |
| P2 | Dev auth bypass and dev actor paths need config hardening review. | Gateway middleware/config comments and tests include dev-only bypass behavior. | Verify production profiles cannot enable dev bypass accidentally. |
| P3 | Production build passes but bundle/style size needs tracking. | Vite emits `index-Clvj3G9v.css` at 510.81 kB and reports high Solid plugin timing. | Track CSS split/build timing as frontend performance debt after correctness gates are green. |
| P3 | Action registry should become a durable inventory artifact. | `src/shared/actions/action-registry.ts` currently exposes 25 action IDs. | Add a generated action inventory test or doc that checks owner plane, risk, approval, and schema metadata. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Browser-to-gateway deployment shape | Vite dev proxy is clear; production routing should be verified against deploy config. | Inspect deployment manifests and run a production build/proxy smoke test. |
| Nested Next app role | Package exists, but current product ownership is unclear from generated README. | Decide whether `apps/velion-web` is marketing, product shell, or legacy experiment. |
| Fallback data acceptability | Some fallback paths are intentionally honest; others may mask missing live integrations. | Build a feature-by-feature fallback register. |

## Quality Gate

- Lint: pass.
- Typecheck: pass.
- Unit tests: fail due unhandled Studio workspace rejection.
- Gateway Rust tests: pass.
- Build: pass.
- Rust format gate: fail.
- Dead-code scan: fail with Knip candidates requiring triage.

## Recommended Remediation Order

1. Fix `loadStudioWorkspace` optional access and rerun `pnpm test`.
2. Replace `apps/velion-web/README.md` with project-specific ownership docs.
3. Add a fallback/live-wiring register for social, studio, agents, and shared read-data paths.
4. Split or document `src/styles/global.css` ownership.
5. Add an action registry completeness test.
