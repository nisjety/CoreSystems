# INTERNAL_API_KEY split — dry-run edge map & execution plan

**Status: DRY RUN — nothing deployed, no live `.env` touched, no secret rotated.**
Generated 2026-07-07 from a five-plane read-only code map. This supersedes the
detail level of `apps/Model Plane/docs/internal-api-key-split-proposal.md` (which
remains the "why"); this is the "exactly what and where."

The 16 generated per-core secrets live ONLY in a session staging file
(`scratchpad/internal_key_split_STAGING_SECRETS.env`) — not in the repo, not in
any `.env`. Regenerate at execution time; do not reuse dry-run values.

---

## 0. Three corrections to the original proposal (found during mapping)

1. **`INTEGRATION_COREV2_INTERNAL_KEY` is NOT an inbound override.** integration-corev2
   validates plain `INTERNAL_API_KEY` (`internal/config/config.go:178`). The name
   exists ONLY caller-side in exec-core (`integration_tools.rs:175`, compose `:483`)
   with a fallback to `INTERNAL_API_KEY`. It "works" today purely because both hold
   the identical shared value. So there is **no** pre-existing inbound scaffolding to
   build on for integration-corev2 — it's a from-scratch dual-accept change (and it's
   collision-locked; see §6).

2. **Every validator fails OPEN on an empty/unset key.** documents-api
   (`cmd/main.go:211-214`), retrieval-engine (gRPC `interceptor.rs:30`, HTTP
   `api/mod.rs:98` — `None => true`), and the App-plane Go cores skip auth when the
   configured key is empty. And every caller **silently omits** the header when its
   key is empty (`.filter(!is_empty)` in Rust, `if key != ""` in Go). Consequence:
   a misconfigured per-core key does not fail loud — it silently disables auth on
   that edge. The rollout MUST assert non-empty keys at boot (see §5 step 0).

3. **The Control Plane cores are already multi-key validators.** auth/user/org/
   billing/session-core accept `INTERNAL_API_KEY` OR `INTERNAL_SERVICE_SECRET` today
   (shared helper `internal/internalkey/assert.go` + per-core `configuredInternalKeys()`).
   Their dual-accept step is nearly free: add the per-core key to the configured list.
   audit-core is the exception (resolves ONE expected value at boot).

---

## 1. Validating cores (callees) — the real enforcement surface

16 cores actually `ConstantTimeCompare`/`compare_digest` the shared key on inbound.
Each needs its own inbound secret `INTERNAL_API_KEY_<CORE>`.

| # | Core | Plane | Inbound env today | Headers accepted | Multi-key today? | Dual-accept effort |
|---|------|-------|-------------------|------------------|------------------|--------------------|
| 1 | auth-core | Control | `INTERNAL_API_KEY \|\| INTERNAL_SERVICE_SECRET` | `x-internal-api-key`, `x-service-auth` (gRPC), `x-internal-service-secret` (oRPC), body `internalApiKey` | ✅ (`\|\|`) | **Low** — add key to the `\|\|` chain, per controller |
| 2 | user-core | Control | `INTERNAL_API_KEY` + `INTERNAL_SERVICE_SECRET` | `X-Internal-Api-Key` (HTTP + gRPC md) | ✅ `configuredInternalKeys()` | **Low** — append to list |
| 3 | org-core | Control | same | `X-Internal-Api-Key` | ✅ | **Low** |
| 4 | billing-core | Control | same | `X-Internal-Api-Key` | ✅ | **Low** |
| 5 | session-core | Control | same | `X-Internal-Api-Key` | ✅ | **Low** |
| 6 | audit-core | Control | `INTERNAL_API_KEY` → `INTERNAL_SERVICE_SECRET` (resolved once) | `X-Internal-Api-Key`, `X-Api-Key` | ❌ single | **Med** — make it a list; also kill `dev-super-secret` default |
| 7 | documents-api-go | Data | `INTERNAL_API_KEY` | `X-Internal-Api-Key`, `X-Internal-Key`, `X-Api-Key` | ❌ single, **fail-open** | **Med** |
| 8 | retrieval-engine-rs | Data | `INTERNAL_API_KEY` (or Bearer JWT alt) | `x-api-key`, `x-internal-api-key`, `x-internal-key` | ❌ single, **fail-open** | **Med** (gRPC interceptor + HTTP mw, two sites) |
| 9 | integration-corev2 | Ingestion | `INTERNAL_API_KEY` | `X-Internal-API-Key`, `x-internal-api-key` | ❌ single | **Med** — 🔒 COLLISION-LOCKED (§6) |
| 10 | imports-core | Ingestion (Py) | `INTERNAL_API_KEY` | `x-internal-api-key`, `x-service-auth` | ❌ single | **Med** |
| 11 | conversation-core-go | Application | `INTERNAL_API_KEY` | `x-internal-api-key` | ❌ single | **Med** |
| 12 | insight-core | Application | `INTERNAL_API_KEY` | `x-internal-api-key` | ❌ single | **Med** |
| 13 | notification-core | Application | `INTERNAL_API_KEY` | `x-internal-api-key` | ❌ single | **Med** |
| 14 | information-core | Application | `INTERNAL_API_KEY` | `x-internal-api-key` | ❌ single | **Med** |
| 15 | leads-core | Application | `INTERNAL_API_KEY` | `x-internal-api-key` | ❌ single | **Med** |
| 16 | social-core | Application | `INTERNAL_API_KEY` | `x-internal-api-key` | ❌ single | **Med** |

**Special scheme (handle separately, NOT `INTERNAL_API_KEY`):**
- **convex-core** validates header `X-Service-Key` against
  `CONVEX_INTERNAL_SERVICE_KEY || INTERNAL_API_KEY || "change-me-internal-service-secret"`
  (`convex/ingest.ts:16`, `controlSessions.ts:27`, `authz.ts:5`, `nats.ts:17`, `ai.ts:12`).
  It already has its own key name; the split just means (a) drop the `|| INTERNAL_API_KEY`
  fallback and (b) **remove the hardcoded `"change-me-internal-service-secret"` default**
  (real security bug — a leaked image with no env set authenticates with a public string).

**NOT on the shared-key surface (no change needed):**
- Model Plane: model-gateway (JWT/JWKS), inference-core, session-core (MP), execution-core,
  orchestrator, capability, sandbox-manager, browser-broker (strips inbound internal md),
  bridge-core (own JWT), letta-bridge (own token). **cost-core validates NOTHING** yet is
  called with the shared key — flagged below as a latent gap, not fixed by this split.
- Data: embedding-engine, graph-index, index-engine, quickwit-adapter, wiki-store,
  data-orchestrator, data-quality (no inbound validation).
- Ingestion: finspo-core validates `FINSPO_API_KEY` (external, not shared); Quarry uses
  its own JWT + `QUARRY_INTERNAL_SECRET` HMAC + `QUARRY_CONTROL_API_KEY`; shipping-core
  (carrier creds only).
- Application: conversation-ingest-rs has **NO** inbound validation on
  `/internal/ingest/email` (fail-open — relevant to the email-inbound build, item #6).

---

## 2. Caller → callee edge table (who sends the shared key)

Each caller must, post-split, hold the CALLEE's key under a per-target name
(`<CALLEE>_INTERNAL_KEY`) instead of the shared `INTERNAL_API_KEY`. `*` marks a
silent fallback to `INTERNAL_API_KEY` that must be REMOVED after cutover so a
missing per-target key fails loud.

| Caller | → Callee | Key env today | Header | Site |
|--------|----------|---------------|--------|------|
| **velion-gateway-rs** | auth,user,org,billing,session,audit,documents-api,retrieval,integration-corev2,imports,conversation,insight,notification,information,leads,social (ALL via one field) | `INTERNAL_API_KEY` (single `state.internal_api_key`) | `x-internal-api-key` | `upstream.rs:162` (`proxy_json`) + 11 direct `.header(...)` sites |
| auth-core | user-core (HTTP+gRPC) | `INTERNAL_API_KEY \|\| INTERNAL_SERVICE_SECRET`* | `X-Internal-Api-Key` / md | convex-auth `:115`, plane-token `:271`, model-plane-token `:214`, grpc-client `:32` |
| user-core | org-core | `INTERNAL_API_KEY \|\| INTERNAL_SERVICE_SECRET`* | `X-Internal-Api-Key` | handlers `:93`, `:141` |
| user-core | auth-core | `INTERNAL_API_KEY \|\| INTERNAL_SERVICE_SECRET`* | body `internalApiKey` | authcore_oauth_client `:93/176` |
| billing-core | org-core | `INTERNAL_API_KEY \|\| INTERNAL_SERVICE_SECRET`* | `X-Internal-Api-Key` | billing/service `:112` |
| session-core | user-core | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | user_client `:91` |
| session-core | billing-core | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | billing_client `:49` |
| documents-api-go | user-core | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | userauthz/client `:61` |
| retrieval-engine-rs | user-core | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | authz/visibility `:133` |
| retrieval/embedding/graph-index | inference-core | `INTERNAL_API_KEY` | `x-api-key` | embed `:251`, provider `:224`, extractor `:161` — **no-op today** (inference-core doesn't validate); leave as-is or wire to `DATAPLANE_*` |
| integration-corev2 | documents-api | `DATA_PLANE_INTERNAL_API_KEY`* | `X-Internal-Api-Key` | handoff/dataplane `:75` |
| integration-corev2 | auth/org/billing/audit | `AUTH_CORE_INTERNAL_API_KEY`* | `X-Internal-Api-Key` | controlplane/clients `:116/154/207/242` |
| integration-corev2 | self (sync-jobs) | `INTERNAL_API_KEY` | `X-Internal-API-Key` | handoff/integration `:104` |
| finspo-core | documents-api | `DATA_PLANE_INTERNAL_API_KEY` | `X-Internal-Api-Key` | dataplane/documents `:107` |
| finspo-core | integration-corev2 | `INTERNAL_API_KEY` | `X-Internal-API-Key` | sharepoint/http_token_provider `:46` |
| imports-core | documents-api | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | service.py `:342` |
| imports-core | integration-corev2 | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | actions_gateway.py `:43` |
| quarry-edge | documents-api | `DATA_PLANE_API_KEY`* (compose) | `authorization: Bearer` + `x-internal-api-key` | ingest_client.rs `:88-89` |
| quarry-edge | retrieval-engine | `DATA_PLANE_API_KEY`* | `x-internal-key` | vector_index.rs `:137` |
| quarry-control | notification-core | `NOTIFICATION_CORE_INTERNAL_KEY` | `x-internal-api-key` | notify/sink.go `:62` |
| conversation-core-go | integration-corev2 | `INTEGRATION_INTERNAL_API_KEY`* | `x-internal-api-key` | integration/client `:196` |
| conversation-ingest-rs | conversation-core-go | `INTERNAL_API_KEY` | `x-internal-api-key` | lib.rs `:145` |
| insight-core | social-core | `INTERNAL_API_KEY` | `x-internal-api-key` | socialmetrics/client `:73` |
| insight-core | notification-core | `INTERNAL_API_KEY` | `x-internal-api-key` | briefs/client `:62` |
| leads-core | integration-corev2 | `INTERNAL_API_KEY` | `X-Internal-API-Key` | integration/client `:147` |
| social-core | integration-corev2 | `INTERNAL_API_KEY` | `X-Internal-API-Key` | integration/client `:171` |
| support-worker | notification-core | `INTERNAL_API_KEY` | `X-Internal-Api-Key` | notify-agent.ts `:33` |
| exec-core (MP) | integration-corev2 | `INTEGRATION_COREV2_INTERNAL_KEY` → `INTERNAL_API_KEY`* | `x-internal-api-key` | integration_tools.rs `:175` |
| exec-core (MP) | information-core | `INFORMATION_CORE_INTERNAL_KEY` | `x-internal-api-key` | info_tools.rs `:75` |

**The gateway is the dominant edge.** It reaches 16 of the 16 validators through one
key. Its refactor is the bulk of the work: `proxy_json(url, …)` must map `url →
per-callee key` (or take a key arg), and the 11 direct sites each get the specific
callee's key. Everything else is ≤2 edges per service.

---

## 3. Naming scheme

- **Inbound (callee owns):** `INTERNAL_API_KEY_<CORE>` — e.g. `INTERNAL_API_KEY_ORG_CORE`.
  16 secrets, generated (staging file only for the dry run).
- **Outbound (caller holds callee's key):** `<CALLEE>_INTERNAL_KEY` — e.g. a caller of
  org-core sets `ORG_CORE_INTERNAL_KEY=<value of INTERNAL_API_KEY_ORG_CORE>`. This
  extends the pattern that already exists caller-side (`AUTH_CORE_INTERNAL_API_KEY`,
  `INTEGRATION_COREV2_INTERNAL_KEY`, `INFORMATION_CORE_INTERNAL_KEY`,
  `NOTIFICATION_CORE_INTERNAL_KEY`, `DATA_PLANE_INTERNAL_API_KEY`) — reconcile those
  existing names to the scheme rather than adding parallel ones.

---

## 4. Per-core dual-accept change (grouped by effort)

**Group A — Control Plane multi-key cores (auth/user/org/billing/session): trivial.**
They already accept a list. Add the per-core key to `configuredInternalKeys()` /
the `||` chain. Callers on the legacy shared key AND callers on the new per-core key
both pass during cutover. No new acceptance machinery.

**Group B — single-key Go/Python validators (audit, documents-api, imports, all App
cores): make the compare a list.** Change each from "compare against one expected
value" to "accept if the provided key equals ANY of {new per-core key, legacy shared
key}", both from env, both non-empty-guarded, constant-time each. Add a one-line log
tagging which key matched (drives the drain decision in §5 step 4). Pattern to copy:
the CP `configuredInternalKeys()` list + compare loop.

**Group C — Rust validators (retrieval-engine): two sites.** gRPC
`grpc/interceptor.rs` and HTTP `api/mod.rs` both compare `provided == expected` — turn
`expected: Option<String>` into `expected: Vec<String>` (new + legacy) and match-any.
Keep the Bearer-JWT alternate path untouched.

**Group D — integration-corev2: same as B but COLLISION-LOCKED (§6).**

**Group E — convex-core: drop fallbacks.** Remove `|| INTERNAL_API_KEY` and the
`"change-me-internal-service-secret"` literal from all five sites; require
`CONVEX_INTERNAL_SERVICE_KEY`. Not part of the shared-key dual-accept, but do it in
the same pass since it's the same class of bug.

**Boot assertion (all cores):** because empty = fail-open, each core must refuse to
start if its inbound key is empty. CP already has `internalkey.AssertFromEnv`; extend
it to the per-core name and add equivalents to the Data/Ingestion/App validators that
lack it (documents-api only logs a warning today; retrieval-engine silently bypasses).

---

## 5. Rollout order (no mid-rollout auth break)

Invariant: at every step, every in-flight caller presents a key the callee accepts.

0. **Generate** 16 non-empty CSPRNG secrets → secret store / plane `.env`. **Add the
   boot-time non-empty assertion first** (deploy that alone) so a later empty-key
   misconfig fails loud instead of silently disabling auth.
1. **Callee dual-accept:** deploy every validator accepting {new per-core key, legacy
   shared key} + the "which key matched" log. No caller changed → nothing breaks.
2. **Rotate callers, one callee at a time:** point each caller at the callee's new key
   via `<CALLEE>_INTERNAL_KEY`. Start with the **gateway** (biggest surface) per
   callee, then the core-to-core callers. Because the callee dual-accepts, old-key and
   new-key callers both succeed. Verify each edge (§ below) before the next callee.
3. **Drain check:** when the callee's log shows 0 legacy-key hits for a full traffic
   cycle, proceed.
4. **Drop legacy acceptance** per callee; remove the caller-side `* → INTERNAL_API_KEY`
   fallbacks (exec-core `integration_tools.rs:176`, conversation-core
   `config.go:45`, integration-corev2 `config.go:181/191`, the compose `${…:-${INTERNAL_API_KEY}}`
   chains) so a missing per-target key now fails loud.
5. **Decommission** the shared `INTERNAL_API_KEY` value; scrub from `.env`/compose/secret
   store; rotate per the secret-leak remediation process.

Rollback: until step 4 for a given callee, both keys work — revert a single deploy safely.

## 5b. Verification per edge
- **Positive:** caller on NEW key → representative call returns 2xx (gateway→org-core
  list; exec-core→integration-corev2 `/connections`; session-core→billing entitlements).
  Existing `smoke-*` / `make test-endpoints` with the new env exercise this.
- **Negative (the whole point):** present core A's key to core B → **401**. Add one
  focused test per validator asserting cross-key rejection.
- **No silent fallback:** grep callers for `unwrap_or`/`||`/`envOr(…, INTERNAL_API_KEY)`
  after cutover — must be gone (list in §2, `*` rows).

---

## 6. integration-corev2 collision protocol

integration-corev2 is under the active collision protocol (2h quiescence check; never
touch `internal/actions/service.go` or OAuth files). Its dual-accept change lives in
`internal/auth/middleware.go` + `internal/config/config.go` — NOT the locked files —
but must still be quiescence-checked and kept to a small, clearly-labeled diff. Because
it is BOTH a heavily-called callee (gateway, conversation, leads, social, imports,
finspo, exec-core all call it) AND a caller (documents-api, CP, self), sequence it late
in step 2 so its many callers are already dual-accept-safe.

---

## 7. Gotchas that will bite (read before executing)

1. **Fail-open empties** (§0.2) — the #1 risk. Assert non-empty at boot BEFORE anything.
2. **cost-core validates nothing** but is called with the shared key by both the gateway
   and model-gateway. The split doesn't fix this; either add validation (new work) or
   explicitly accept it and document that cost-core trusts the network.
3. **Header spelling variants** — `X-Internal-Api-Key` / `x-internal-api-key` /
   `X-Internal-API-Key` / `x-internal-key` / `x-api-key` / `x-service-auth` all appear;
   HTTP-case-insensitive so they interoperate, but any constant touched must treat all
   as the same logical header. quarry→retrieval uses `x-internal-key` (the odd one).
4. **convex `change-me` default** — pre-existing security bug; fix in this pass.
5. **Caller-name/callee-name mismatch already exists** — exec-core sends
   `INTEGRATION_COREV2_INTERNAL_KEY` but integration-corev2 reads `INTERNAL_API_KEY`;
   works only because both = shared value. The split makes this correct by construction.
6. **audit-core `dev-super-secret-internal-api-key`** compose default and **imports-core
   key comes from `env_file` not compose** — inventory both when writing env.
7. **The three inference-core hops** (retrieval/embedding/graph → `x-api-key`) are no-ops
   today (inference-core doesn't validate). Don't waste a per-core key on them unless you
   also add validation to inference-core; leave them on the DataPlane `x-api-key` path.

---

## 8. Effort estimate

- Group A (5 CP cores): ~0.5 day (append to existing list + env).
- Group B/C/D (11 single-key validators incl. Rust 2-site + collision core): ~2 days.
- Group E (convex): ~0.5 day.
- **Gateway url→key mapping** (the big one): ~1 day (proxy_json signature + 11 sites + config).
- Core-to-core caller env + fallback removal: ~1 day.
- Boot assertions + per-validator negative tests: ~1 day.
- Rollout + per-edge verification (operator-driven, stacks up): ~1 day elapsed.

Total ~6–7 engineering-days + a supervised rollout window. Executable as one focused
stream for the code (dual-accept + tests, all reversible) followed by an operator-run
cutover. Do NOT do code + cutover in one unsupervised pass.
