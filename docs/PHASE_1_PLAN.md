# Verevon — Phase 1 "Honest Core Loop" Execution Plan

> **Status:** Approved 2026-06-19 (recon-grounded + hardened by a 6-persona honesty-first review council). **Gated on Phase 0 (`docs/PHASE_0_PLAN.md`) being merged.**
> **Goal:** Wire already-built backends through the Phase-0-clean gateway so the **monitor → brief → approve** spine is genuinely LIVE — without shipping a single new fabrication. Source audit: `Verevon-ai-first.md`.
>
> ## THE ABSOLUTE RULE — no new fakeness
> A reviewer must be able to point at **any rendered number, label, or status** and trace it to a real upstream response for a real org *today* — OR it is **explicitly labeled** setup / preview / empty / unavailable. No zeros-as-data, no placeholder-as-live, no per-connection claim the events can't support, no "execute" copy where nothing executes, no "complete export" where it's partial, no "monitoring" affordance where nothing runs. Replacing one fabrication with a quieter one is failure.
>
> ## Hard invariants (every PR)
> - Gated on **Phase 0 merged**: `rg 'fn org_id_from_headers' "apps/Frontend Plane/verevonv3/apps/gateway/src"` = 0; `x-verevon-org-id` in `STRIPPED_HEADERS`. **Every new domain resolves org/identity via `upstream::authorized_org_id` / validated `AuthenticatedUser.user_id` only — never a client header/query/body**, proven by a negative test (spoofed org ignored).
> - Each PR is a focused, revertible commit on its own branch (branch off `main`; never reset/clean the user's tree).
> - **Never change the live DB password hex** (`apps/Control Plane/.env`); use it where a Control-Plane container is recreated (the documented drift is a live landmine).
> - Quality gates green per PR: gateway `cargo fmt --check` + `clippy -- -D warnings` (gateway-crate-scoped, no `--workspace`) + `cargo test`; Go `go test -race ./...`; SPA `pnpm typecheck && pnpm lint && pnpm test && pnpm build`; live smoke through the running gateway.

Paths: gateway = `apps/Frontend Plane/verevonv3/apps/gateway`; SPA = `apps/Frontend Plane/verevonv3/src` (quote the space).

---

## Scope — IN vs DEFERRED

**IN (Phase 1):**
- **A — insight-core:** DEPLOY (Dockerfile + Application Plane compose) + gateway `insights.rs` proxy + SPA renders the **real connector registry**; metrics shown as an explicit **"not yet reporting"** empty-state. *No live metrics dashboard.*
- **B — AI-action HITL:** backend `ListAIActions` + `ReviewAIAction` 404-hardening, then gateway list/approve/reject + a `ConversationPanel` review panel. *The "after human approval" spine.*
- **C — Quarry change-monitoring:** rebuild quarry-edge with `postgres-queue`, then gateway `monitoring.rs` + an on-demand "check this URL" Monitoring tab that **replaces the fabricated** `social.rs` competitor-watch.
- **D — GDPR self-service:** gateway `privacy.rs` (self-scoped) export + erase + an AccountSettings "Privacy & data" section, under 4 GDPR conditions.
- **E — "Used by AI?" audit:** SPA-only — surface `zdr` + a **workspace / per-data-category** AI-activity view. *No per-connection claim.*
- **F — cross-cutting:** PR-0 compose-env fix, wiring-recipe docs, action-registry entries.

**DEFERRED to Phase 2 (do NOT build now; do NOT ship an affordance for them):**
- A-FULL: insight-core Postgres persistence + first metric producer (paired unit — the gate that makes metrics live).
- B-FULL: post-approval **executor** (nothing consumes `ai_action.reviewed` today) + richer agent/model HITL producer beyond `ticket.classification`.
- C-FULL: Temporal-driven scheduled at-scale monitoring + sources registry (migration 005).
- D-FULL: cross-plane DSAR aggregation (Model/Data plane) + admin erasure surface.
- E5: backend — real human-readable tool name (replace opaque call-id) + source/connection attribution. **Prerequisite for any per-connection "Used by AI?" label.**

---

## PR sequence (PR-0 first; then 5 parallelizable enabler tracks)

After PR-0, the enablers **A1, B1, C1, D0, E1** are mutually independent and parallelizable. Each proxy/UI PR depends strictly on its own enabler. The three deploy/infra enablers (A1, B1, C1) are the long poles — land them early.

### `PR-0` — gateway compose-env fix *(tiny, first, alone)* — depends: Phase 0
- Add `AUDIT_CORE_URL` + `INSIGHT_CORE_URL` to the gateway `docker-compose.yml` env block (config.rs has defaults but compose omits them → silent misroute).
- **Reconcile the audit-core port** (config.rs default `http://audit-core:8187` vs the port audit-core actually binds in the gateway's stack — verify).
- **DoD:** both keys present with correct ports; gateway boots resolving both; `/api/v1/audit` still 200.

### Track A — insight-core (Market Intelligence, registry-only)
- **`A1` — deploy** *(depends: PR-0)*: new Dockerfile (mirror `social-core`, **drop** its DATABASE_URL + NATS env — insight-core has neither); Application Plane compose service **pinned `replicas: 1`** (in-memory per-process repo); `INTERNAL_API_KEY` = the gateway's forwarded key (config.go hard-fails if empty); set `INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE` explicitly. **DoD:** container builds + healthy single replica; `/api/v1/insights/connectors` returns the real registry behind the internal key (not 401/404/502).
- **`A2` — gateway + SPA** *(depends: A1 + PR-0)*: `insights.rs` (mirror `audit.rs`: `proxy_json` + `authorized_org_id`, empty-org → empty success envelope) with a **private `normalize_connector`** fn (insight-core `ConnectorSlot[]` → SPA `{id,kind,label,status}`) + `#[cfg(test)]` shape test; add `insight_core_url` to AppState/`config.rs`/**`test_state`** (or tests won't compile); merge in `build_router`. SPA: **drop `x-verevon-org-id`**; rewrite `insights-client.test.ts` to assert the header is absent + no internal-key leak; metrics render an explicit **"not yet reporting / connect a source"** empty-state; `insightCore.state` never `"live"` for metrics. **DoD:** InsightsPage stops 404ing; registry renders; no zeros-as-data; gates green.

### Track B — AI-action HITL (the centerpiece)
- **`B1` — cc-go backend** *(depends: Phase 0)*: add `ListAIActions` across Repository/PGRepository/Service/Handler + routes `GET /ai-actions` (+ gated `/internal/ai-actions`), mirroring `ListTickets` (org-scoped `WHERE org_id=$1`, `status` default `suggested`, `conversationId` filter, limit clamp 1..100). **Harden `ReviewAIAction`** (`repository.go:374`): capture the UPDATE `CommandTag`; if `RowsAffected()==0` roll back + return `conversation.ErrNotFound` (→404) and **do NOT write the phantom `conversation_ai_reviews` row**; validate `Decision ∈ {approved,rejected}` (→400). **DoD:** `go test -race ./...` green incl. new table-driven tests (org-isolation; 404 on missing/foreign-org with no phantom row; decision-enum).
- **`B2` — gateway proxy** *(depends: B1)*: GET list + POST approve/reject via the `inbox.rs` safe pattern (`proxy_json` + `authorized_org_id`, `actor_for`, server-set `x-org-id`); 403 on empty-org for mutations; `inbox-client` methods. **DoD:** end-to-end 200; foreign-org id → 404; gates green.
- **`B3` — SPA panel** *(depends: B2)*: review panel in `ConversationPanel.tsx` with an explicit `<Show>` empty-state; approve/reject routed through the action-registry (`review_ai_action`, risk medium); outcome copy **"Decision recorded"** — never "executed"/"applied". **DoD:** real items + empty-state render; no execution claim; gates green.

### Track C — Quarry change-monitoring
- **`C1` — edge rebuild** *(depends: Phase 0)*: `Dockerfile.edge` → `--features grpc,http3,postgres-queue`; provision a **dedicated `quarry_edge` Postgres DB**; set **`QUARRY_EDGE__DATABASE_URL`** (NOT plain `DATABASE_URL` — config uses prefix `QUARRY_EDGE` separator `__`); confirm the "PostgresBaselineStore wired" log. **Post-deploy smoke (the `.ok()` migration swallows errors):** connect to the edge DB and assert `quarry_baselines` + `quarry_change_diffs` exist + a `0004` `_sqlx_migrations` row; a live `POST /v1/change/check` returns **200, not 501**; existing `/v1/search`, `/v1/agent/*`, scrape unchanged. **DoD:** all of the above.
- **`C2` — gateway monitoring.rs** *(depends: C1 verified 200)*: mirror `schedules.rs` (bearer `quarry_token`, **NO `x-org-id`**, `normalize_target` SSRF-guard) for `/v1/change/{check,latest,history}`; `ingestions-client` additions. **DoD:** proxy 200 on a public URL; SSRF-guard + bearer-path tests pass.
- **`C3` — SPA Monitoring tab** *(depends: C2)*: `createResource` + `<For>/<Show>`; check-now + last-checked + history; **NO scheduling control**; delete the fabricated `social.rs` competitor-watch path. **DoD:** on-demand check works; no scheduling affordance; fabrication removed. *(If C1 slips, cut C2/C3 and keep the honestly-labeled placeholder — documented.)*

### Track D — GDPR self-service
- **`D0` — AUTH_DATABASE_URL invariant** *(depends: Phase 0)*: change user-core (`main.go:303`) so an unset `AUTH_DATABASE_URL` makes the erase route **unavailable / explicit 503** (today it boots with erase disabled, logs only to stdout, then returns an opaque 500 — an Art.17 trap); set `AUTH_DATABASE_URL` with the **real hex DB password** in deployed user-core; verify the connected-pool log. **DoD:** set → route live + log; unset → 503/refused, never a silent 500. Keep `gdpr_test.go` green.
- **`D1` — gateway privacy.rs** *(depends: D0)*: mirror `settings/preferences.rs`, **self-scoped to the authenticated `user.user_id`** (reject any client-supplied id); `GET /api/v1/privacy/export` + `DELETE /api/v1/privacy/erase`; gateway returns **422 `confirmation_required`** when `confirm!=true` *before* proxying; rate-limit the DELETE; `privacy-client.ts`. **DoD:** export returns CP data; erase requires `confirm:true` + rate-limited; spoofed-id negative test passes.
- **`D2` — SPA Privacy & data** *(depends: D1)*: section in `AccountSettingsPage.tsx`; export view **enumerates contents AND renders the user-core `gdpr.go` Control-Plane-only disclosure VERBATIM** (SPA test asserts the string); erase gated by **typed-confirm + step-up re-auth** (reuse Better Auth credential re-verification) + `confirm:true`; sign-out + "data erased" fire **only on a confirmed 2xx**; failure path stays logged in with "erasure did not complete — contact support" (test both paths); `privacy.erase_account` registry entry (risk high, reversible false, approval reauth). **DoD:** disclosure rendered verbatim (tested); re-auth required; sign-out only on 2xx.

### Track E — "Used by AI?" audit view
- **`E1` — SPA only** *(depends: PR-0)*: surface the already-present-but-dropped `zdr` flag per event; render a **workspace-level per-data-category** "AI activity" rollup from the Phase-0-clean `/api/v1/audit`; **hide the per-connection column** (or label its No state "Attribution unavailable") until E5. Honesty-invariant test: a no-source event contributes to the workspace rollup but **never** a per-connection bucket. **DoD:** `zdr` surfaced; per-data-category view renders; no asserted per-connection Yes/No; invariant test green.

### `F1` — recipes + registry *(depends: B3 + C2 + D1 — routes exist)*
- Write `add-gateway-domain` (via `audit.rs`) + `add-SPA-feature` recipe docs with the auth-model doc-comment convention and a one-line **honesty checklist** ("does this render real data for a real org today? if no, ship labeled setup/preview").
- Confirm `review_ai_action` / `privacy.erase_account` / `monitoring.check_url` registry descriptors carry correct risk/approval/reversible flags; add to `LIVE_ACTIONS` **only after** their gateway routes exist; cover in `action-registry.test` + `agent-tools.test`.

---

## Resolved questions
- **Q1 scope:** IN/DEFERRED as above (unanimous).
- **Q2 insight-core honesty:** registry live; metrics = explicit empty-state; `state` never "live" for metrics. No zeros-as-findings.
- **Q3 Quarry:** YES rebuild edge (`postgres-queue` + `QUARRY_EDGE__DATABASE_URL` + verify migrations); on-demand check is the honest MVP; no scheduling affordance.
- **Q4 GDPR:** self-service only; disclose CP-only DSAR verbatim; `AUTH_DATABASE_URL` is a fail-fast gate; erase = confirm + step-up re-auth + rate-limit; sign-out only on 2xx.
- **Q5 HITL:** approving `ticket.classification` is an honest MVP **without** an executor — copy is "Decision recorded", never "executed"; list endpoint lands first; route via the action registry.
- **Q6 sequencing:** PR-0 first; then A1/B1/C1/D0/E1 parallel; proxy/UI strictly after their enabler.
- **Q7 Used-by-AI:** ship workspace/per-data-category now; per-connection label requires E5 (deferred).

## Risk register
- **Live-but-empty fabrication (A)** → registry only; explicit metrics empty-state; `state` ≠ "live" for metrics.
- **Irreversible erasure (D)** → `AUTH_DATABASE_URL` fail-fast invariant; confirm + step-up re-auth + rate-limit; sign-out only on 2xx.
- **Incomplete-DSAR disclosure (D)** → render the `gdpr.go` Control-Plane-only Notes verbatim; SPA regression test asserts the string.
- **Quarry-edge 501 enabler (C)** → C1 mandatory before C2/C3; correct `QUARRY_EDGE__DATABASE_URL` prefix.
- **Silent edge migration failure** → dedicated `quarry_edge` DB (avoid `_sqlx_migrations` checksum collisions); post-deploy table+migration-row assertion.
- **HITL silent cross-tenant write (B)** → harden `ReviewAIAction` (RowsAffected → 404, no phantom row); `ListAIActions` first.
- **Per-connection AI over-claim (E)** → workspace granularity only until E5.
- **HITL false-execution implication** → "Decision recorded" copy; registry capability flag encodes "no executor yet".
- **insight-core boot/auth (A)** → `INTERNAL_API_KEY` must match the gateway; `replicas:1`.
- **Stack-safety / config drift** → reconcile audit-core port in PR-0; real hex DB password on any CP recreate; regression bring-up of Application + Ingestion + gateway after infra PRs.

---

## Tooling & skills kit for the executor
Use the right specialist tool per task — don't hand-roll:
- **Structural lookup:** `codegraph` MCP (`codegraph_context` first, then `codegraph_explore`) for "what calls X / where is X". `context-mode` (`ctx_batch_execute`, `ctx_execute_file`) to scan/grep large files without burning context. Always re-confirm call sites with `rg` before editing.
- **Library/API docs:** `context7` MCP (or the `ctx7` CLI) for axum, sqlx, gin, SolidJS, Better Auth, Docker — fetch current docs, don't guess.
- **Go (B1, A1 deploy, D0):** skills `golang-pro`, `golang-patterns`, `golang-testing`; agents `go-reviewer`, `go-build-resolver`.
- **Rust (A2, C1, C2, gateway):** skills `rust-patterns`, `rust-testing`; agents `rust-reviewer`, `rust-build-resolver`.
- **SolidJS SPA (A2, B3, C3, D2, E1):** skills `solidjs-vite-typescript`, `solid-*`, `vite` (NO Tailwind — semantic CSS in `global.css`; `<For>/<Show>`; `props.x`); agents `typescript-reviewer`, `build-error-resolver`.
- **Auth/GDPR (D):** skills `better-auth-best-practices`, `better-auth-security-best-practices`, `security-review`; agent `security-reviewer`.
- **DB/migrations (C1):** skills `database-migrations`, `postgres-patterns`; agent `database-reviewer`.
- **Deploy (A1, C1):** skills `docker-patterns`, `deployment-patterns`.
- **Method:** skills `tdd` / `tdd-workflow` (RED-first for the hardening + honesty-invariant tests), `code-review` / `verification-loop`; agents `code-reviewer`, `tdd-guide`. **Browser smoke:** `playwright` / `chrome-devtools` MCP to verify each SPA surface renders real data (or an honest empty-state).
- **PRs:** `github` MCP / `gh` CLI. Each PR: conventional-commit title, the per-PR DoD as the test plan.

## Global Definition of Done
Phase 0 verified merged; PR-0 merged first; A–F meet their per-PR DoD; the **global no-new-fakeness gate** holds (every rendered value traces to real data for a real org, or is explicitly labeled); all quality gates green; a clean-checkout stack-safety bring-up of Application Plane + Ingestion Plane + gateway leaves all previously-running services healthy with no new failing healthchecks; no secrets introduced.
