# integration-corev2 Research Dive

Generated: 2026-07-11 (supersedes the 2026-06-07 dive)

Scope: `apps/Ingestion Plane/integration-corev2`
> **2026-07-12 source update:** the shared-key tenant bypass is disabled outside triple-gated isolated E2E, ingestion-audience RS256 tokens are verified locally with cached JWKS, provider writes require a service principal with `integration:write` plus approval, and Model execution mints scoped tokens. Full Go test/vet is green. Server-side action idempotency and independent approval lookup remain open; no Visma provider is claimed.
Container: `integration-api` (:3026) + workers `integration-email-worker`, `integration-finspo-worker` + `integration-webhook-normalizer` (:3036, Rust `services/webhook-normalizer-rs`).

Evidence grades used below: **[live-curl]** verified by host HTTP; **[source-only]** read from disk; **[logs]** from container logs.

> Environment caveat: Docker's containerd content store is corrupted (blob I/O errors). `docker exec` and `docker logs` fail for these containers ("input/output error"), which is why every app container reports `(unhealthy)` — the healthcheck EXEC cannot run even though the process serves traffic. `docker build`/redeploy is blocked, so the uncommitted source below is almost certainly NOT in the running image. Live checks are host `curl`/urllib to published ports only; DB/env/schema facts are read from source.

---

## Snapshot / verdict

`integration-corev2` is the Ingestion Plane connector broker: provider catalog, OAuth/token brokerage, discovery, action execution, webhook hot-path, and worker handoff (Finspo, email). It is **real and live**: `/health` and `/health/detailed` respond, auth is enforced, the webhook path fails closed, and `go build` / `go vet` / the touched unit tests are all clean. The standing theme from the last dive still holds — **provider catalog breadth (20) outruns per-provider action/OAuth parity** — but the security posture has materially improved since 2026-06-07 (fail-closed webhooks, enforced connection auth, and an in-flight move off shared internal keys toward a scoped Data-Plane service token).

Headline answers for this audit:
- **Visma is still ABSENT** from the 20-provider catalog. "Test the Visma MCP" cannot be satisfied through integration-corev2 today — there is no Visma provider, discovery, OAuth, or action path here. **[live-curl + source-only]**
- **Shipping is catalog-visible only; it is NOT an action-executing provider here.** The Model-Plane path to "shipping time Oslo→Trondheim" does not run through integration-corev2 — it is a direct call to `shipping-core` (:3156). The Bring delivery-time parsing defect lives in shipping-core, not in this service. **[source-only]**

---

## Live verification (host curl / urllib, 2026-07-11)

| Check | Result | Evidence |
|---|---|---|
| `GET :3026/health` | `200 {"service":"integration-corev2","status":"ok"}` | [live-curl] |
| `GET :3026/health/detailed` | `200` — capabilities `actions/discovery/oauth/tokenBroker/webhookHotPath` all `true`; `environment:"dev"`; `natsEnabled:true`; `providers:20`; `storage:"ready"` | [live-curl] |
| `GET :3026/api/v1/connections` (no auth) | `401 {"error":{"code":"unauthorized","message":"Authentication required. Provide an Authorization header or x-internal-api-key header."}}` — auth enforced | [live-curl] |
| `GET :3026/api/v1/providers` (no auth) | `200` public catalog, 20 providers | [live-curl] |
| Visma in catalog | **not present** (0/20) | [live-curl] |

`/health/detailed` `storage:"ready"` indicates the Postgres repository is active (not the in-memory fallback) in the running container.

---

## Provider catalog (20; Visma absent)

Live `/api/v1/providers` **[live-curl]**, cross-checked against `internal/providers/catalog.go` **[source-only]**:

`microsoft, google, slack, discord, github, notion, shopify*, stripe*, linkedin, x, meta, instagram, facebook, whatsapp, meta-ads, tiktok, snapchat*, shipping†, okta*, scim*`

`*` = `missing_config` (shopify, stripe, snapchat, okta, scim). `†` = `shipping` reports `manual_or_admin_config` (aggregator admin config; no per-user OAuth). All others report `ready` with `directOAuthReady:true`.

Parity is uneven by design (honest "not implemented" guards, not fake data):
- **Action execution** (`internal/actions/service.go` dispatch switch): implemented for `microsoft, slack, google, github, notion, shopify, stripe, linkedin, meta/facebook/instagram/whatsapp/meta-ads, snapchat, okta`. **No case for `x`, `discord`, `scim`, `shipping`** → falls through to `"actions are not implemented for provider %s"` (service.go:78). **[source-only]**
- **OAuth**: catalog presence does not guarantee direct OAuth — `internal/oauth/service.go:120` returns `"...registered in the catalog but direct OAuth is not implemented yet"`; `internal/oauth/provider_client.go:176` returns `"profile discovery is not implemented for %s"`. **[source-only]**

---

## Auth posture

`internal/auth/middleware.go` **[source-only]**:
- `InternalOrBearer` (used by `/api/v1/connections` etc.): accepts either a `Bearer` token (verified via auth-core `TokenVerifier`, requires non-empty `UserID`+`OrganizationID`) OR a plain internal API key (`X-Internal-API-Key` / `x-internal-api-key`), compared with `crypto/subtle.ConstantTimeCompare`. Missing/empty credentials → 401 (confirmed live).
- `InternalOnly` guards internal routes (e.g. `/internal/webhooks/events/:id`) with the same constant-time key check.
- `AssertOrgAccess` enforces `principal.OrganizationID == targetOrgID` for non-internal callers (tenant isolation on org-scoped routes).
- `RequirePlan` gates plan-restricted features via org-core; internal calls bypass.

Minor coverage gap: `internal/auth/` has **no `*_test.go`** for this security-critical middleware (`go test` reports "no test files"); it is exercised indirectly by `internal/api/server_test.go`. **[source-only]**

---

## Webhook signature verification — fail-closed CONFIRMED

`internal/api/server.go` `POST /api/v1/webhooks/:provider` → `verifyProviderWebhook` (server.go:1736–1774) runs BEFORE normalize/store/publish and returns 401 `invalid_webhook_signature` on failure. **[source-only]**

- **GitHub** (`verifyGitHubWebhookSignature`, server.go:1896): requires `X-Hub-Signature-256`, validates `sha256=` prefix, computes HMAC-SHA256 over the raw body, compares with `hmac.Equal` (constant-time). Missing/incomplete/mismatched → error → 401. **Confirmed fail-closed.**
- Per-provider schemes also verified: `shopify` (X-Shopify-Hmac-Sha256), `slack` (v0 signature + 300s replay window), `stripe` (t/v1 scheme), `meta/facebook/instagram/whatsapp/meta-ads` (X-Hub-Signature-256 with meta/fb/ig secret).
- **Unset secret** for a scheme → `unverifiedWebhookError`, which **rejects** unless `ALLOW_UNVERIFIED_WEBHOOKS=true` (documented dev-only escape hatch).
- **Unknown provider** (`default` case) → reject, not trust.
- Meta GET handshake (`verifyProviderWebhookChallenge`) requires `META_WEBHOOK_VERIFY_TOKEN` and validates `hub.mode`/`hub.verify_token`/`hub.challenge`.

Note: `internal/hotpath/webhook.go` only *hashes* signature headers into `signatureHash` for dedup/replay keys — it is not the verification site. Verification is the `server.go` gate above (and, when configured, the Rust `integration-webhook-normalizer` normalizes post-verification). Per instruction, the live webhook endpoint was NOT POSTed to.

---

## Uncommitted WIP review

`git diff` touches 6 files (37 insertions / 45 deletions), all in the **non-locked** area (config + finspo-worker + handoff, plus `.env.example`). None touch `internal/actions/service.go` or OAuth files (collision protocol respected). **[source-only]**

**What changed and why — Data-Plane handoff hardening (security improvement):**
- `internal/handoff/dataplane.go` + `config.go` replace the old shared internal-key scheme
  - OLD: `DATA_PLANE_INTERNAL_API_KEY` (falling back to shared `INTERNAL_API_KEY`) sent as `X-Internal-Api-Key`, plus a **caller-selected** `X-Org-ID: input.OrgID` tenant header.
  - NEW: a single `DATA_PLANE_SERVICE_TOKEN` sent as `Authorization: Bearer …` — a short-lived, org-constrained Control-issued service JWT (`aud=data-plane`, `documents:write`). Data Plane verifies the token and pins the tenant from it; **caller-selected identity headers are deliberately no longer sent.**
  - The rewritten test (`dataplane_test.go` `TestDataPlaneCreateDocumentForwardsOnlyScopedServiceBearer`) asserts `X-Internal-Api-Key`, `X-Api-Key`, `X-Org-ID`, `X-User-ID` are all **absent**.
- This directly closes the Data-Plane Phase-2 finding that `documents-api-go` trusted self-supplied tenant headers behind a shared internal key. It is a genuine posture upgrade and is newer than the 2026-07-07 key-split dry-run doc (which still records the old `DATA_PLANE_INTERNAL_API_KEY`/`X-Internal-Api-Key` edge at `handoff/dataplane:75`).
- `.env.example`, `config.go`, `config_test.go`, `cmd/finspo-worker/main.go` updated consistently (comments + env name). `DATA_PLANE_SERVICE_TOKEN` default is **empty** in `.env.example`.

**Caveat — the hardened client is DORMANT.** `NewDataPlaneDocumentsClientFromConfig` is constructed by `cmd/finspo-worker/main.go`, but `internal/workers/finspo.go` **never calls `CreateDocument`** — `logSkippedDataPlaneForward` (finspo.go:162) only logs. Per the field doc (finspo.go:46–66), finspo-core's Graph sync captures metadata only (no document body), and DP v2 requires non-empty `content`, so forwarding is intentionally not wired to avoid poisoning Data Plane with fabricated text. Net: the token change hardens a client that is not yet on any live write path. When `DATA_PLANE_SERVICE_TOKEN` is unset, `Configured()` is false and forwarding is silently skipped regardless.

Assessment: **safe, well-scoped, test-backed hardening; not a regression.** It should be committed. It will not take effect until (a) the image is rebuilt and (b) finspo-core gains content capture — neither blocked by this diff.

---

## INTERNAL_API_KEY validation (plain, collision-locked) — CONFIRMED

Per `docs/INTERNAL_API_KEY_SPLIT_DRYRUN.md` §0.1 and row 9, and confirmed in source: **integration-corev2 validates the plain `INTERNAL_API_KEY`** (`internal/config/config.go:187`, required at config.go:331/356/385; enforced by the middleware constant-time compare). `INTEGRATION_COREV2_INTERNAL_KEY` does **not** exist as an inbound override here — it is **caller-side only** in exec-core (Model Plane) with a fallback to `INTERNAL_API_KEY`. integration-corev2 is 🔒 **collision-locked** (§6): never edit `internal/actions/service.go` or OAuth files. This audit only read source and wrote this doc — no service source was modified. **[source-only]**

---

## Shipping provider boundary (user headline)

`internal/providers/catalog.go:1150–1199` documents `shipping` as "Verevon's OWN freight aggregator (shipping-core)… no per-user OAuth: carrier credentials are org/admin-level configured on shipping-core itself, so this provider is catalog-visible with an admin-setup status rather than a connect popup." It exposes capabilities `shipping.quotes.read / carriers.read / tracking.read` for the connect UI. **[source-only]**

Crucially, `shipping` has **no action dispatch case** in `internal/actions/service.go` — integration-corev2 does **not** proxy shipping quotes/tracking. So:
- The Bring "delivery-time parsing" defect (transit_days:0 / 0001-01-01) is a **shipping-core** concern (`internal/carrier/bring/wire.go`+`bring.go`), out of scope for this doc; see the shipping-core dive.
- A Model-Plane "shipping time Oslo→Trondheim" tool must target **shipping-core :3156 directly** (health path `/healthz`, not `/health`), not integration-corev2. integration-corev2's shipping entry is metadata/onboarding surface only.

---

## Build / vet / test (host, go 1.26.2) — CLEAN

- `go build ./...` → exit 0, no output. **[source-only]**
- `go vet ./...` → exit 0, no output. **[source-only]**
- `go test ./internal/config/... ./internal/handoff/... ./internal/hotpath/... ./internal/providers/...` → all `ok` (auth = no test files). Confirms the uncommitted config+handoff diff compiles and passes. **[source-only]**

TODO/stub scan (`internal`, `cmd`, excluding tests) surfaced only honest "not implemented" guards and one "placeholder name" comment — no fake data, no TODO/FIXME debt:
- `internal/oauth/service.go:120` — direct OAuth not implemented for some catalog providers.
- `internal/oauth/provider_client.go:176` — profile discovery not implemented for some providers; `:410` snapchat placeholder connection name.
- `internal/actions/service.go:78` — actions not implemented (default) for providers without a dispatch case.
- `internal/workers/finspo.go:59` — explicit "would silently poison Data Plane with fake document text" guard (why DP forward is not wired).

---

## Container / topology state

`docker ps` **[logs/inspect]**: `integration-api` (0.0.0.0:3026, `Up 2 days (unhealthy)`), `integration-webhook-normalizer` (0.0.0.0:3036, unhealthy), `integration-email-worker` (3026/tcp internal, no unhealthy flag), `integration-finspo-worker` (3026/tcp internal). `(unhealthy)` = corrupted-store EXEC healthcheck failure, not a process fault — integration-api answered curl. `docker logs integration-api` failed with `input/output error` (store corruption), so no log-grade evidence this pass.

Compose (`apps/Ingestion Plane/docker-compose.yml`) **[source-only]**: integration-api on `PORT 3026`; `INTEGRATION_WEBHOOK_HOTPATH_URL=http://integration-webhook-normalizer:3036`; workers `integration-finspo-worker`, `integration-email-worker`, `integration-webhook-normalizer` (built from `./integration-corev2/services/webhook-normalizer-rs`). A **separate legacy integration stack coexists** in the same compose — `integration-worker`, `integration-engine-go-api`, `integration-engine-go-worker`, and `connector-runtime-engine` (Nango, :3003/:3009) — distinct from integration-corev2; worth a boundary note but out of scope here. `autocomplete-core` is **not defined in the Ingestion Plane compose** (not deployed via this stack); its port could not be discovered here.

---

## Recommendations (unprioritized)

1. **Commit the Data-Plane service-token diff.** It is a clean, test-backed security hardening and closes a known Phase-2 tenant-header trust gap. It also drifts the key-split dry-run doc (edge `handoff/dataplane:75`), which should be updated to the Bearer/`DATA_PLANE_SERVICE_TOKEN` scheme after commit.
2. **Visma:** if "test the Visma MCP" is a real goal, Visma must be added as a provider (catalog + OAuth/token + actions), or reachable via a different service. It does not exist in integration-corev2 today. Confirm which service is intended to own Visma before building.
3. **Shipping as a Model-Plane tool:** wire the Model Plane to call shipping-core directly (:3156 `/healthz`), and fix the Bring delivery-time parse in shipping-core. Do not expect integration-corev2's `shipping` catalog entry to serve quotes.
4. **Add unit tests for `internal/auth/middleware.go`** — the constant-time key path, Bearer-verifier path, and `AssertOrgAccess` tenant check are security-critical and currently only covered transitively.
5. **`DATA_PLANE_SERVICE_TOKEN` issuance:** ensure a real Control-issued token is provisioned before finspo-core gains content capture, else DP forwarding will silently no-op (`Configured()==false`).

## Bottom line

integration-corev2 is a real, live, security-improved connector broker. Auth is enforced, webhooks fail closed, and the uncommitted work is a legitimate Data-Plane hardening (dormant until image rebuild + finspo content capture). The two user-facing headlines both resolve to boundaries outside this service: **Visma is absent from the catalog**, and **shipping quotes are not proxied here** (that is shipping-core, where the Bring parsing defect lives).
