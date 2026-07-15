# Velion v3 Plane Audit

> **2026-07-13 correction:** this is retained as the 2026-07-11 historical
> audit. Plain chat's no-tool behavior is now treated as intentional policy, not
> an unconditional defect. Explicit Browse/actions/Plan remain the intended UX;
> writes must use the governed agentic approval path. Exact target-audience
> token issuance and forwarding are fixed in source. The current blocker is the
> absent live Model Gateway/inference gRPC path plus divergent capability state,
> approval/browser caller gaps, and no selectable shipping action. See
> [MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md](../MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md).

Baseline date: 2026-07-02
Verification dates: 2026-07-10 (live) and 2026-07-11 (full SPA+gateway source re-verification, this pass)

Scope: `apps/Frontend Plane/velionv3` (SolidJS SPA at `src/` + Rust BFF gateway at `apps/gateway`)

See also `../../FRONTEND_PLANE_STATUS.md` and `../../FRONTEND_PLANE_ROADMAP.md`.

## 2026-07-15 Data Plane and GraphRAG re-verification

The historical Data findings below are superseded in source. The gateway now
mints a session-bound `aud=data-plane` bearer through Auth Core for every
interactive documents, retrieval, wiki, source, graph, navbar-search,
ingestion-source, and Operating Map path. Canonical session membership supplies
the org; conflicting browser tenant input cannot widen it. Delegation failure is
a sanitized 503, with no shared-key user fallback.

The Knowledge workspace now consumes the actual Data envelopes: documents and
wiki payloads are normalized, retrieval `candidates`/`sources` are mapped to the
SPA result contract, source cards derive from visible documents, all document
pages are loaded within a bounded 20-page cap with honest truncation metadata,
and chunk expansion uses singular `document_id`. Navbar search targets the
implemented `/v1/knowledge/search`. The onboarding graph-preview IDOR is closed;
Knowledge and onboarding graph reads use the same verified graph boundary.

Sanitized source evidence dated 2026-07-15:

- `cargo test --all-targets` in `apps/gateway`: **272 passed**;
- `pnpm test -- --run`: **68 files / 360 tests passed**;
- `pnpm typecheck` and `pnpm build`: passed;
- `pnpm lint`: zero errors, one pre-existing Solid reactivity warning.

An isolated Data checkpoint passed **28/28** HTTP auth/tenant checks,
including authorized retrieval/graph reads and spoofed-tenant denial. That run
did not include the Velion gateway/browser and did not replace the shared live
frontend/Data containers, whose revisions remain older than this dirty
worktree. It also predates the final Documents signed-ZDR/source-object guards.
The frontend remains implemented/tested/built rather than a full deployed/
effective claim.

## 2026-07-11 re-verification — executive summary

Source-only this pass: the velionv3 SPA (:5173) and velion-gateway-rs (:3185) containers are **down** (collateral of the fleet-wide Docker containerd corruption), so no live SPA/gateway curl was possible. Findings graded `[source-only]` / `[inspect]`.

### Headline: velionv3 is REAL and fully wired — it is NOT mock-backed

The single most important correction: **`docs/core-research/mock-backed-surfaces.md` is completely obsolete and every claim in it is now false.** `src/shared/mocks/` no longer exists, `velion-operating-model.ts` is gone with zero references (even in tests), `src/shared/graphrest/`'s hardcoded `src_website/topic_returns/agent_support` graph is gone, and `src/shared/api` has **56 real client files** (the doc says it's empty). All 20 `src/features/*` areas import real API clients and fetch real `/api/v1/*` endpoints via `createResource` / TanStack `createQuery` / `requestJson`. Per-area verdict: **18 fully REAL**, **2 MIXED** (both honest, not deceptive):
- **agents**: `AgentRunConsole` + chatbot runtime status are real; `AgentsPage`/`ChatbotStudio`/`WorkflowBuilder` render static *blueprint* content behind an explicit `DesignPreviewBadge` ("Blueprint — not yet configured for this org", disabled controls, neutral-placeholder metrics) — intentional honesty contract, not fake data.
- **settings**: members/audit/MCP/trust/integrations/admin-users are live; the fabricated status grids were emptied in the Phase-4 de-fake sweep — but two small unlabeled hardcoded rows survived (see finding below).

The gateway BFF (~50 domain proxies) is likewise **real throughout** — every sampled domain reads a `*_URL` from config and forwards a minted credential (internal API key / HMAC service-delegation / per-audience bearer); no domain serves canned business data or a dead 501. So "features that should work don't" is **not** the frontend faking data — it's (a) the chat tool-surfacing gap below, (b) backends down/degraded from the Docker + Postgres-volume corruption, (c) specific bugs like the MCP transport trap (fixed) and the new IDOR below.

### New findings (this pass)

| # | Severity | Finding |
|---|---|---|
| 1 | **HIGH (new IDOR)** | `GET /api/v1/onboarding/graph-preview` (`apps/gateway/src/onboarding/lookup/graph.rs`) trusts a **client query-param `org_id`** and reads any org's knowledge graph from graph-index with the shared internal key — no membership check. Any authenticated user can pass `?orgId=<victim>` and read that org's entire graph. Same bug class as the *fixed* `x-velion-org-id` header IDOR, but via a query param (header stripping doesn't help). The sibling `translate_recommendation` in the same crate derives org from the session correctly — `graph_preview` is the outlier. Fix: derive org via `authorized_org_id(&user)`, ignore the client param. |
| 2 | **MEDIUM (IDOR)** | Onboarding connector actions (`apps/gateway/src/onboarding/actions/connectors.rs`: `start_connect_session`/`discover_source`/source-cleanup) trust a **client body `org_id`** as tenant scope with no membership gate (unlike the billing path's `checkout_lifecycle_ready`). Exploitability depends on integration-corev2 independently enforcing actor-vs-org; if it doesn't, it's a cross-tenant connect/enumerate. |
| 3 | **HIGH (the chat-tools lever)** | Plain chat turns send **zero tools** — `buildChatWireBody` (`src/shared/api/chat-client.ts`) uses `DEFAULT_FEATURES=['usage','citations','reasoning','steps','artifacts']` (no `tools`/`agentic`) and only derives tools from browseWeb + composer-selected actions + explicitTools; `sendContent` passes neither, so a normal question reaches the model with `tools:[]`. The full registry→toolspec builders are **test-only**. This is THE reason a plain "shipping time…" question never calls a tool. Fix spec: default-advertise the registry's low-risk, no-approval tools on every turn (cheap; keeps writes out). |
| 4 | **HIGH (HITL bypass)** | If a user selects a **write/approval** action via the composer `/`-menu WITHOUT enabling Plan mode, `buildChatWireBody` emits it with `tools` but not `agentic` — and the direct tool loop never pauses for approval, so the write runs **un-gated**. Fix (ship with #3): force `features.add('agentic')` whenever any emitted tool `requiresApproval`. |
| 5 | **MEDIUM (residual fake)** | `settings/WorkspaceSettingsPage.tsx` has two unlabeled hardcoded arrays rendered as live workspace state: `businessHourRows` (fabricated support schedule) and `roleRows` (fake member counts "Owner 1 / Admin 1 / Agent 1 / Viewer 0") with active "Edit schedule"/"Create role" labels — missed by the Phase-4 de-fake sweep that emptied the adjacent grids. Especially inconsistent since `MembersSection` loads the REAL member list right below. |

### Prerequisite for chat-shipping (user's literal example)

`get_shipping_quotes` is **not in the SPA action registry at all** (`src/shared/actions/action-registry.ts`), so no feature-flag change surfaces it. Making shipping callable from chat needs a two-part change: (a) add shipping actions to the registry with proper zod I/O + risk, and (b) ensure model-gateway's server-side tool executor routes them to shipping-core. Shipping today is reachable only via gateway `shipping.rs` and the onboarding connect flow, never as a chat tool.

### Docs

`mock-backed-surfaces.md` → **delete** (fully obsolete; added to `STALE_DOC_DELETION_REGISTER.md`). Updated in place (2026-07-11): `README.md` (transport dirs now populated), `runtime-shell.md` (real route tree + CoreShell), `auth-boundary.md` (rewritten — auth is real, not "presentation-only"), `action-system.md` (~22 descriptors, not 4), `onboarding-gateway.md`. The MCP-add form validation fix (transport/URL scheme) was applied this pass in `src/features/settings/components/McpServersSection.tsx` (typecheck green).

---

## Prior passes (preserved below)

## Live Docker verification addendum — 2026-07-10

The source-mounted Vite frontend on `:5173` and Rust gateway on `:3185` were healthy. With the seeded local account, the existing `cross-plane-smoke.spec.ts` passed 6/6 in 7.7–10.9 seconds: session, model catalog, knowledge envelope, insights, and SPA shell.

The suite’s knowledge assertion is weaker than the product contract: the live payload reports `dataPlane.available=false`, zero documents/indexed documents, and unavailable finspo while still returning healthy diagnostics and fixture-style connected integrations. It proves envelope fan-out, not real knowledge grounding.

Normal chat returned HTTP 200 from live GPT-4o-mini with real token/cost/latency telemetry but zero tools and zero citations, reproducing the user’s complaint. Browse mode called live web search/fetch and emitted five citations, although Bring fetches were empty/mismatched and one search result was irrelevant. Knowledge search returned an honest empty result. After reopening the thread, citation/tool provenance was not persisted in the Sources tab.

The live `/api/v1/mcp/servers` catalog has one enabled misconfigured `visma mcp` record (`stdio` transport with an HTTPS URL). No MCP tool was executed. The checked-in E2E auth default was stale; the seeded local password supplied for this run was required. The default `:5199` target was not running; `:5173` is the active Dockerized v3 target.

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
