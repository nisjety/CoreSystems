# Velion — Phase 0 "Survival Sprint" Execution Plan

> **Status:** Approved 2026-06-19 (hardened by a 5-persona review council). **Execute in order: PR-0 → PR-1 → PR-2 → PR-3.**
> **Source audit:** `Velion-ai-first.md` (repo root). **Goal of Phase 0:** make Velion v3 *honest and tenant-safe* so a second tenant / pilot can exist. Ships nothing net-new.
> **Hard invariants (apply to every PR below):**
> - **Zero schema/data migrations.** Each PR is a single-commit, cleanly revertible change on its own branch.
> - **Never change the live DB password hex** (`apps/Control Plane/.env:8`, value `5b5a3bbc…`); auth-core depends on it inline.
> - **PR ordering is strict** — PR-0 must merge before PR-1/PR-2 so they are CI-gated; the fabrication-lint (PR-3) can only turn on after PR-2 removes violations.
> - After every change: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (gateway-crate-scoped, **no `--workspace`**), `cargo test`, and `pnpm verify` must be green.

Paths: gateway = `apps/Frontend Plane/velionv3/apps/gateway`; SPA = `apps/Frontend Plane/velionv3/src`. (Note the space in "Frontend Plane" — quote paths.)

---

## Ground truth (verified by code recon — re-verify with `rg`, don't trust blindly)

**The IDOR:** 6 gateway domains read the org id from the **client-controlled `x-velion-org-id` header** instead of the validated session, so any authenticated user can forge a victim org's id and read/write its data. There are **6 `org_id_from_headers` definitions** and **~58 call sites** across 6 domains.

**The safe pattern (already in-repo):** `upstream::authorized_org_id(&state, &user).await -> String` (`upstream.rs:115`) derives org from the validated Better-Auth session (`scope_org_id(user)` = `user.active_org_id`) or the cached user-core session-context — **never a header**. It returns `""` when no org; `proxy_json` only sets `x-org-id` when non-empty (`upstream.rs:173`), so **empty-string is equivalent to today's `None`** — safe.

**Domains already safe (leave unchanged):** billing, audit, inbox, tickets, agents_runs, chat (history/shared/streams), ingestions (shared.rs), settings/api_keys, social, studio, orgs — all use `authorized_org_id`/`resolve_session_context`.

---

## PR-0 — CI scaffold (gates PR-1/PR-2)

**Goal:** a PR gate exists before the security-critical changes land. Mirror `apps/Data Plane v2/.github/workflows/ci.yml` (the gold reference).

**Steps:**
1. Create `/.github/workflows/velionv3-ci.yml`, triggered on `pull_request` + `push` to `main`, **path-scoped** to `apps/Frontend Plane/velionv3/**` and the workflow file itself.
2. **`gateway-rust` job** — `working-directory: apps/Frontend Plane/velionv3/apps/gateway`. Steps: `checkout@v4`; `dtolnay/rust-toolchain@stable` (components: clippy); `Swatinem/rust-cache@v2`; `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`. **Scope to the gateway crate only — no `--workspace`** (avoids unrelated inference-core lints).
3. **`spa-verify` job** — `working-directory: apps/Frontend Plane/velionv3`. Steps: `checkout@v4`; `pnpm/action-setup@v4`; `actions/setup-node@v4` (node 20, pnpm cache, `cache-dependency-path: apps/Frontend Plane/velionv3/pnpm-lock.yaml`); `pnpm install --frozen-lockfile`; `pnpm verify` (= lint→typecheck→test→build).
4. **No fabrication-lint yet** (WI2 violations still exist on main; it would fail red).

**DoD:** both jobs green on a no-op PR; clippy `-D warnings` clean on the gateway crate.

---

## PR-1 — Cross-tenant IDOR fix (the gating security change)

**Delete all 6 `org_id_from_headers` definitions:** `domains/actions/shared.rs:5`, `domains/integrations/shared.rs:14`, `domains/knowledge/shared.rs:12`, `domains/router_policy.rs:35`, `domains/search.rs:623`, `domains/finetune.rs:43`. *(Verified safe: no other helper in those files calls them; remove the now-dangling `org_id_from_headers` from `use super::shared::{…}` imports in integrations/connections.rs, profile.rs, connect_sessions.rs, sync_jobs.rs, providers.rs — clippy `-D warnings` will catch leftovers.)*

**Swap ~58 call sites to `crate::upstream::authorized_org_id(&state, &user).await`** — every handler has `State(state)` + `Extension(user)` in scope. **Handle the 4 shapes correctly (these are real compile traps):**

| Shape | Sites | Correct rewrite |
|---|---|---|
| **Inline `.as_deref()`** (the main trap) | finetune.rs `81,100,123,146,174`; router_policy.rs `63,82` | `authorized_org_id` returns `String`, not `Option`. **Hoist:** `let org_id = authorized_org_id(&state,&user).await;` then pass `Some(org_id.as_str())`. Writing `…await.as_deref()` inline fails **E0716/E0599**. |
| **`if let Some(org_id) = …`** | finetune.rs `208` | `let org_id = authorized_org_id(&state,&user).await; if !org_id.is_empty() { req = req.header("x-org-id", org_id); }` |
| **`let Some(org) … else { 400 }` guards** | knowledge/sync.rs `33,187` | `let org = authorized_org_id(&state,&user).await; if org.trim().is_empty() { return (BAD_REQUEST, error("no_active_org",…)).into_response(); }` |
| **`.unwrap_or_default()` / plain** | knowledge/* (documents/imports/operating_map/retrieval/wiki/workspace — 24 sites), integrations/* (9), actions/dispatchers.rs (6) | Drop-in: `let org_id = authorized_org_id(&state,&user).await;` (empty == no header == today's behavior). |
| **search.rs:542 user-id fallback** | search.rs `542` | **Keep the fallback as a documented conscious decision:** `let o = authorized_org_id(&state,&user).await; let org_id = if o.is_empty() { user.user_id.clone() } else { o };` — line 550 sets `x-org-id` unconditionally and autocomplete is per-user (not tenant data). Add a comment + test. |

**Defense-in-depth:** add `"x-velion-org-id"` to `STRIPPED_HEADERS` (`middleware.rs:12-32`) so a forged value is dropped at ingress before any handler. **Confirm the strip middleware runs as a request-layer before route extraction, matched by `HeaderName` (case-insensitive).**

**CORS: leave untouched** (keep `x-velion-org-id` in `config.rs:206-211` allow_headers — see Q1).

**Tests (RED-first — must FAIL on current code, PASS after the fix):**
- Add `[dev-dependencies]` to the gateway `Cargo.toml`: `tower = { features=["util"] }`, `http-body-util`, `wiremock` (pin versions compatible with axum 0.7 / Rust 1.86+).
- **Tier-1** (`middleware.rs` `#[cfg(test)]`): forge `x-velion-org-id` in a `HeaderMap`, run the strip set, assert it's gone.
- **Tier-2** (`main.rs` tests mod; reuse `build_router` + `test_state`, add an upstream-URL override so a target route points at a `wiremock` server): authenticate as org A (dev-bypass user), send `x-velion-org-id: org-B` to e.g. `/api/v1/router-policy`, assert the mock upstream **never** received `x-org-id: org-B` (resolves to A or empty). Add a negative-control across all recorded requests.

**DoD:** `rg 'fn org_id_from_headers' "apps/Frontend Plane/velionv3/apps/gateway/src"` returns **0**; `STRIPPED_HEADERS` contains `x-velion-org-id`; RED→GREEN proven for both tiers; `cargo check --all-targets` + `build` + `clippy -D warnings` + `test` green; CORS intact; deploy canary.

---

## PR-2 — De-fake → honest empty/error states

**MUST remove (these misrepresent connection/security state):**
1. **`src/features/inbox/lib/inbox-demo-data.ts`** (always-on fake Shopify/Stripe/calendar) — delete; rewire `InboxAside.tsx` (~38-44, 202-245) + `InboxPage.tsx:7` to live conversation-core data or an honest empty state; remove the ~160ms fake delay.
2. **`src/features/settings/components/WorkspaceSettingsPage.tsx`** — `SsoSection` (856-916) → honest **"Not configured"** panel (no `Configured`/`Verified`/`365 days`); remove fabricated `sectionStatusCards` sso entries (136-164), `ssoMappingRows`, `domainRows`. **Keep `OrgSecuritySection` as a host shell so the honest `RecentSecurityEvents` (~997) survives** — gut only the fabricated fields. CTA links to the SSO/OIDC *initiate* route only if confirmed wired, else a disabled/docs CTA.
3. **`gateway/src/domains/social.rs`** — remove `demo_accounts()` (2356-2433), `demo_posts()` (2435-2474), `seed_org_posts()` (110-115), and the `SocialStore` in-memory fallback (58-108). **Reads** on `CoreRead::Unavailable/Error` → empty list + `meta.source=unavailable` (HTTP 200). **Writes** (`create_post`/`schedule_post`/`publish_post`/`decide_approval`) → **HTTP 503 `social_core_unavailable`** (stop the silent success-swallow). Ensure `create_post`/`create_draft_from_inbox`/`create_studio_social_draft` (769/830/1065) no longer depend on `SocialStore`.
4. **`gateway/src/domains/actions/handlers.rs:72-84`** (`action_run_status` always returns `{"status":"queued"}`) — **remove the route + registration** (`actions.rs:16-17`); the SPA uses the existing `/api/v1/runs/{id}/events` SSE (`streamRunEvents`). Confirm no caller 404s.
5. **`src/shared/actions/action-client.ts`** — remove the synthetic else-branch (82-90 fabricates `runId`/`auditId`); every `executeAction` must hit the gateway. For actions the gateway can't yet run, throw a typed "not-available". Resolve `security.check_url_reputation`/`security.investigate_url` (listed LIVE but no gateway arm) — add real arms or de-advertise.

**Defer (COSMETIC-BACKLOG, not this PR):** social `derived_competitor_watch`/`derived_trends`/`derived_evergreen`; billing `default_billing_account` (dev-gated, labeled); studio "Launch canvas" seed. **Do NOT touch:** `TrustCenterSection` + `RecentSecurityEvents` (already honest/graceful).

**DoD:** `pnpm verify` green incl. new tests; zero fabrication matches; `RecentSecurityEvents` + `/api/v1/audit` still work; smoke with cores **up and down** (down must show honest empty/error, never fake success).

---

## PR-3 — Fabrication-lint ON + DB_PASSWORD reconcile

**Fabrication guard (turn on now that PR-2 cleared violations):**
- **FE:** ESLint `no-restricted-syntax`/`no-restricted-imports` in `eslint.config.js` forbidding `inbox-demo-data`, new `*demo-data*`/`*mock*` modules, and the action-client synthetic branch; scoped to `src`, excluding `tests`/`dist`.
- **Gateway:** a scoped `rg` CI step with an **exact zero-tolerance allowlist** (the draft's billing/router_policy/search allowlist was WRONG — it would re-open the IDOR): `fn org_id_from_headers` → **0** matches anywhere; `demo_accounts|demo_posts|SocialStore` → **0**; `Bearer dev-bypass` → only `src/shared/api/http.ts` + `src/shared/api/sse.ts`. Never grep generic `demo`/`mock`.

**DB_PASSWORD reconcile (drift is session-core only, latent — compose re-injects the live hex):**
- `apps/Control Plane/session-core/internal/config/config.go:111` — change the default from `"controlplane_pass"` to `""` (**keep the var name `DATABASE_PASSWORD`**; compose injects `${DB_PASSWORD}` at `docker-compose.yml:377`; user-core already defaults to `""`).
- `apps/Control Plane/session-core/.env.docker` — delete **only line 7** (`DATABASE_PASSWORD=controlplane_pass`); keep lines 4/6/8 (HOST/USER/NAME).
- Document the single-source rule in `apps/Control Plane/ENVIRONMENT_FILES.md` + `CONTROL_PLANE_OWNERSHIP.md`: DB_PASSWORD lives only in root `.env` (live hex), injected via compose with a fail-fast guard.
- **Never touch the live hex.**

**DoD:** guard is green AND fails on a reintroduced violation (test both FE + gateway); `config.go:111` default empty with var kept; `.env.docker:7` deleted; `docker compose config -q` resolves the live hex; rule documented; live hex unchanged.

---

## PR-4 (deferred, non-gating) — CORS + SPA cleanup
Remove `x-velion-org-id` from CORS `allow_headers` (`config.rs:210`) **and** strip the ~16–22 SPA clients that still send it, in the same PR (else the OPTIONS preflight breaks authed requests with `allow_credentials: true`). Pure hygiene; no security weight once PR-1 strips at ingress.

## Resolved decisions
- **Q1 CORS:** Option A — strip at ingress, keep the CORS entry; defer removal to PR-4.
- **Q2 social:** reads → empty + meta; writes → 503; no fabricated ids.
- **Q3 action_run_status:** remove the route; SPA uses run-events SSE.
- **Q4 SSO:** honest "Not configured" panel; no config-read binding this sprint.
- **Q5 lint:** FE eslint + gateway rg, zero-tolerance allowlist (above).
- **Q6 order:** PR-0 scaffold first, then PR-1 → PR-2 → PR-3.

## Risk register
- **IDOR under-scoped** → all 6 defs enumerated; `rg`=0 in DoD; `cargo build` (not just clippy) required.
- **Borrow-of-temporary on the swap (E0716/E0599)** → per-shape table above; mandatory `cargo check --all-targets`.
- **search.rs scope change** → explicit if-empty rewrite keeping the documented user_id fallback + test + Søk-tab smoke.
- **CORS preflight breakage** → Option A (keep entry); CI asserts it stays until PR-4.
- **De-fake coupling** → keep `OrgSecuritySection` as a host shell so `RecentSecurityEvents` survives.
- **Silent social mutation swallow** → replace with explicit 503.
- **DB fix wrong target** → keep var name, change only default, delete one line; `docker compose config -q` gate.
- **Residual (out of Phase 0, track it):** cores trust `x-org-id` via a shared `INTERNAL_API_KEY`; file a rule that cores must not publish host ports + a per-core-key/mTLS follow-up.
