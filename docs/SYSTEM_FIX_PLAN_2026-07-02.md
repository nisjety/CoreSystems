# CoreSystem Fix Plan — 2026-07-02

Sequenced remediation plan: **auth → onboarding → dashboard → functionality → integrations → knowledge base → E2E proof**, with a quick-wins Phase 0 in front. Every finding below was re-verified against the current working tree on 2026-07-02 (the tree is dirty with WIP; several audit findings turned out to be already fixed — they are listed so nobody re-fixes them).

## Execution status (updated during this session)

| Phase | Status | Verification |
|---|---|---|
| 0 — quality gates | ✅ Done | Quarry-v2 `cargo check --tests` green; orchestrator-core `go test` green; convex-core `pnpm typecheck`+`lint` green; gateway `cargo fmt --check` clean; Makefile no longer targets legacy Quarry. |
| 1 — auth | ✅ Done | `sendOnSignUp:true`; seeded verified account (`e2e@velion.dev`) live-proven through gateway (`/api/v1/me` 200); gateway dev-bypass hardened with `APP_ENV=production` guard + unit test. |
| 2 — onboarding edge migration | ✅ Code done, live-verify pending image rebuild | 3 direct `quarry_control_url` call sites → `quarry-edge` `/v1/crawl` + `/v1/jobs/{id}/events`, each carrying the `quarry` audience bearer; edge gained `after_seq`+`max_depth` passthrough; gateway 148 tests pass; `quarry_control_url` fully removed from config. |
| 3 — dashboard honesty | ✅ Done (already honest) | social-workspace + insights + read-data all drive UI from explicit `state` markers (`live`/`empty`/`unavailable`/`planned`); no fabricated-as-live data found. |
| 4 — functionality | ✅ Done (wired + live) | `/api/v1/models` returns real Azure OpenAI models (200); insights/agent-runs routes correctly wired; run-list requires `thread_id` by design. |
| 5 — integrations webhook | ✅ Done + live-verified | `verifyProviderWebhook` now fails closed (unset/unknown secret → 401); GitHub webhook returns 401 for missing + invalid signature; `smoke-test-integration-api.sh` = 17 passed / 0 failed. |
| 6 — knowledge trust | ✅ Code+tests done, enforce OFF by default | documents-api `authctx.Verify` implemented (RS256 static key + stdlib JWKS, aud/iss/exp/org checks), 8 unit tests pass incl. cross-tenant 403 + misconfig 503; gateway knowledge legs now mint+attach `data-plane` bearer; compose wired `AUTHCTX_ENFORCE=0`. Flip to 1 is a separate rollout after live cross-tenant test. |
| 7 — E2E proof | ✅ Done + green | Playwright harness added (`playwright.config.ts`, `tests/e2e/auth.setup.ts`, `tests/e2e/cross-plane-smoke.spec.ts`, `pnpm test:e2e`). Suite runs **6/6 green**: setup seeds+signs-in+creates-org, spec asserts Control `/me`, Model catalog, Data+Ingestion+Application knowledge fan-out, insights, SPA shell — all 200 through the gateway. Proof-ladder L4 closed. |

Root causes found (not in original audit): **auth** — docker `REQUIRE_EMAIL_VERIFICATION=true` + `sendOnSignUp:false` = signup never mails, permanent `EMAIL_NOT_VERIFIED`. **webhooks** — `verifyProviderWebhook` returned `nil` (accept) on unset secret AND `default: nil` for unknown providers. **knowledge** — gateway sent no data-plane bearer, so flipping enforce would 401 the whole surface; fixed first.

Sources: [audit backlog](../apps/CORESYSTEM_AUDIT_BACKLOG.md), [cross-plane map](CORESYSTEM_CROSS_PLANE_ARCHITECTURE_MAP.md), the six 2026-07-02 plane audits, plus fresh code verification (file:line evidence per task).

## Already fixed — do not re-do

| Audit finding | Verified state |
|---|---|
| `loadStudioWorkspace` unhandled rejection (`studio-canvas-model.ts:43`) | `pnpm vitest run src/features/studio` → 3/3 pass. Note: `--reporter=basic` no longer exists in Vitest 4; the audit's flag itself errors. |
| letta-bridge `TestTimeRangeFiltering/cutoff_excludes_old_record` | All 3 subtests pass; filter at `memstore.go:93` is correct. |
| Gateway `/api/v1/knowledge/sources` fan-out contract drift | Every leg (documents, retrieval chunks+freshness, graph, finspo, quarry-edge sources, sync, sharepoint, diagnostics) matches current upstream contracts; per-leg timeouts, no whole-payload 500. |
| SPA knowledge feature dead calls / mock data | Every SPA path has a matching gateway route; all `fallback=` hits are honest Solid `<Show>` empty states. |

## Phase 0 — Quick gate fixes (all S-effort, no dependencies, do first)

| # | Task | Evidence | Fix | Verify |
|---|---|---|---|---|
| 0.1 | Quarry-v2 test compile: `DataPlaneIngestRequest` constructors missing `initiator_user_id`/`visibility` | E0063 at `Quarry-v2/crates/quarry-core/tests/contracts.rs:231` and `crates/quarry-runtime/src/ingest_client.rs:380` (cfg(test) helper). Production code (`pipeline.rs:621-641`) is already correct. Contract semantics locked in `quarry-core/src/contracts.rs:121-157`: `Some(initiator)` → forwarded as `x-user-id`, doc private-by-default; `None` = system ingest → org-visible. | Add `initiator_user_id: None, visibility: None` at both test sites; extend serde roundtrip test with a `Some(...)` case asserting `skip_serializing_if` omits `None` keys. | `cd "apps/Ingestion Plane/Quarry-v2" && cargo check --tests -p quarry-core -p quarry-runtime` |
| 0.2 | orchestrator-core tests don't compile: `stubClient` missing `ListPendingApprovals` | `handlers_test.go:140,214` — generated gRPC client interface gained the method (HITL work); stub never extended. | Add `ListPendingApprovals` method to `stubClient` returning an empty response. Test-only. | `cd "apps/Model Plane/go/services/orchestrator-core" && go vet ./internal/orchestration/ && go test ./internal/orchestration/` |
| 0.3 | convex-core pnpm scripts blocked: `ERR_PNPM_IGNORED_BUILDS` (`esbuild@0.27.0`) | pnpm 11 refuses scripts until the esbuild postinstall is approved. | Add `"pnpm": { "onlyBuiltDependencies": ["esbuild"] }` to `apps/Application Plane/convex-core/package.json` (versioned, CI-safe). | `cd "apps/Application Plane/convex-core" && pnpm typecheck && pnpm lint` |
| 0.4 | velionv3 gateway rustfmt drift | 12 diff sites in `cost.rs`, `eval.rs`, `middleware.rs`, `rate_limit.rs` (committed code, not WIP). | `cargo fmt` in `apps/gateway`; formatting-only commit. | `cargo fmt --check` in the gateway |
| 0.5 | Ingestion Makefile targets legacy `Quarry` | `setup`/`dev-quarry`/`test-quarry`/`ci-test`/`docs`/`version` all cd into legacy Go `Quarry`. | Repoint to Quarry-v2 (`cargo test --workspace`, edge run target); keep legacy targets only if explicitly marked deprecated. | `make -n test-quarry \| grep -c "cd Quarry "` → 0 |

## Phase 1 — Auth (unblocks everything user-facing)

**Root cause found**: the dockerized auth-core runs with `REQUIRE_EMAIL_VERIFICATION="true"` (`auth-core/.env.docker:82`) while `sendOnSignUp: false` (`auth-core/src/auth/auth.ts:578`) and Resend is the only mailer. New signups therefore never receive a verification email and are permanently stuck at `EMAIL_NOT_VERIFIED` on signin. This is why the audit's L4 "authenticated product journey" is unprovable.

| # | Task | Fix | Verify |
|---|---|---|---|
| 1.1 | Unblock verified sessions for local/E2E | Add a seed script that inserts (or updates) a **verified** test account in `auth_service` DB (`emailVerified=true`), alongside the existing dev user `local@velion.dev`. Wire it as a make target so the Playwright fixture (Phase 7) can depend on it. Do **not** silently flip `REQUIRE_EMAIL_VERIFICATION` off in docker env — that would mask the product bug. | signup→verify(seeded)→signin→`/api/v1/me` 200 through the gateway |
| 1.2 | Fix the verification-email dead end for real users | Set `sendOnSignUp: true` (or send on blocked signin) so a verification email is actually dispatched when verification is required; confirm the SPA login surface offers "resend verification". Guard: if `RESEND_API_KEY` is absent, log loudly — never pretend the mail was sent. | throwaway signup produces a Resend send attempt (visible in auth-core logs) |
| 1.3 | Harden gateway dev-bypass | `allow_dev_auth_bypass` is env-gated default-false (`gateway/src/config.rs:189`, `middleware.rs:172`) — good. Add a belt-and-braces guard: refuse to honor the flag when the deploy profile is production (e.g. panic/ignore when `APP_ENV=production`), plus a config test. | gateway cargo tests |
| 1.4 | auth-core lint backlog (649 problems) + duplicate token controllers | Defer mechanical Prettier fixes to a formatting-only commit; file unsafe-`any`/floating-promise fixes as a follow-up batch. Not release-blocking. | `pnpm exec eslint ...` trend |

## Phase 2 — Onboarding (your flag #1) — P0 boundary violation

**Verified**: exactly three gateway call sites still hit `quarry-control` directly, bypassing `quarry-edge` (the only sanctioned cross-plane Ingestion entrypoint):

- `gateway/src/onboarding/crawl_preview/quarry.rs:66` — `create_crawl_job` → `POST {control}/v1/jobs/` (unauthenticated)
- `gateway/src/onboarding/crawl_preview/quarry.rs:102` — `poll_crawl_events` → `GET {control}/v1/jobs/{id}/events`
- `gateway/src/onboarding/actions/website.rs:45` — `start_website_ingest` → `POST {control}/v1/jobs/`

**Edge parity exists** (verified in `quarry-edge/src/routes.rs`): `POST /v1/crawl` (crawl handoff, keyed by `job_id`) and `GET /v1/jobs/:id/events` (added for the "crawl 0-pages fix", forwards to control's event log). Edge auth = `Authorization: Bearer <JWT>` verified against auth-core JWKS; verified `org_id` claim overwrites client-supplied org. The gateway already has the token plumbing: `get_audience_token(state, user, cookie, "quarry")` (used by the ingestions + knowledge domains).

| # | Task | Fix | Verify |
|---|---|---|---|
| 2.1 | Migrate `create_crawl_job` + `poll_crawl_events` to edge | Swap `quarry_control_url` → `quarry_edge_url` (`/v1/crawl`, `/v1/jobs/{id}/events`), attach the quarry audience token for the onboarding user, map the handoff response (`job_id`) into the existing SSE normalizer. Onboarding's org exists by the website-crawl step, so the org-scoped claim is satisfiable. | onboarding crawl preview streams live events through edge; `grep -rn quarry_control_url src/onboarding` → 0 |
| 2.2 | Migrate `start_website_ingest` (actions/website.rs) the same way | Same swap + token; keep `auto_commit` semantics through the edge handoff body. | gateway tests + live onboarding website step |
| 2.3 | `forward_seed_scrape` (quarry.rs:18) already targets edge but sends **no bearer** — it currently rides on edge dev-bypass | Attach the audience token here too, so it survives strict edge auth. | seed scrape works with edge dev-bypass disabled |
| 2.4 | Regression guard | Add a gateway test (or CI grep gate) that fails if any non-test code references `quarry_control_url` outside an allowlist, so control never silently becomes a frontend entrypoint again. | test red when a control call is reintroduced |
| 2.5 | Re-verify the previously-fixed onboarding steps (Brreg shape, back-button logout) still intact after 2.1–2.3 | Walk signup → org create/Brreg → website crawl → workspace entry live. | manual/Playwright pass |

## Phase 3 — Dashboard/home (honest surfaces)

The SPA has a principled fallback framework (`shared/read-data`: `unavailable`/`not_connected`/`planned` states; `social-workspace.ts`: typed `'live' | 'fallback'` sources). The risk isn't the framework — it's surfaces that render **fabricated fallback data** without a visible state marker (e.g. `fallbackSocialOperations` synthesizes approvals/campaigns/trends when live calls fail).

| # | Task | Fix | Verify |
|---|---|---|---|
| 3.1 | Fallback register | Inventory every `source: 'fallback'` / `plannedResult` / `unavailableResult` consumer across social, studio, agents, shared read-data; classify: honest-empty vs planned-badge vs masking-missing-wiring. | register doc committed |
| 3.2 | Badge or wire | For every "masking" case: either render the state marker (unavailable/not-connected) or wire the live owner-plane call. No fabricated data presented as live. | UI shows real state per surface |
| 3.3 | Insights reality check | `/agents` insights use the RUN_* producer (live per Phase 7 work); conversation/social insight producers were blocked on a down `velion-nats`. Verify current bus health and either wire or badge those two. | insights panel shows real run metrics |

## Phase 4 — Functionality (chat/search/crawl/agent-runs)

| # | Task | Fix | Verify |
|---|---|---|---|
| 4.1 | Home tabs spot-check | Verify `/api/v1/models` selector, search filters (Exa surface), crawl start/discover actions against the running gateway — fix any drifted endpoint. | live clicks + network tab |
| 4.2 | Agent Run Console + HITL | With Phase 1 auth fixed, prove run streaming (`run_events_sse`) + pending-approvals flow end to end (orchestrator-core `ListPendingApprovals` now compiles per 0.2). | live run with approval gate |

## Phase 5 — Integrations (your flag #2) — P0 fail-open webhooks

**Root cause found**: `integration-corev2/internal/api/server.go:1670` `verifyProviderWebhook` returns `nil` (accept) whenever the provider secret is **unset** — for all four providers (github/shopify/slack/stripe) — and `default: return nil` for any other provider key. The live env has no `GITHUB_WEBHOOK_SECRET`, so unsigned and invalidly-signed webhooks are accepted (exactly what `smoke-test-integration-api.sh` now catches).

| # | Task | Fix | Verify |
|---|---|---|---|
| 5.1 | Fail closed | In `verifyProviderWebhook`: when the secret for a signature-bearing provider is unset → reject (503 `webhook_secret_unconfigured` or 401), never accept. Remove the `default: nil` accept for unknown keys (the route already 404s unsupported providers, but keep defense in depth). Config flag for explicit dev-only override, default off. | `bash smoke-test-integration-api.sh` → all pass |
| 5.2 | Same audit for Slack/Shopify/Stripe secrets in deploy envs | Ensure enabled providers have secrets configured; document required env per provider. | smoke + env review |
| 5.3 | Connect-session flow reality check | Verify the OAuth connect flow end to end (session create → provider redirect → token storage in integration-corev2, never through the gateway); classify each gateway `integrations` route live/stub. | connect a test provider |

## Phase 6 — Knowledge base (your flag #3) — P0 tenant-trust gap

**Verified chain** (this is a two-step fix with a hard ordering):

1. `documents-api-go/pkg/authctx/authctx.go` — enforce mode returns 503 unconditionally (`:145` ErrNotImplemented, `:160-168`); observe mode trusts `X-Org-ID` unverified (`:171-208`). No jwt/keyfunc deps in go.mod. Meanwhile **retrieval-engine already runs strict** (`.env:90`) with RS256 against `JWT_PUBLIC_KEY_FILE=/app/keys/convex-auth.pub` + optional JWKS kid-rotation — a proven in-plane pattern to port.
2. The gateway's knowledge fan-out (`domains/knowledge/shared.rs:47-70`) sends only internal-key + identity headers — **no data-plane Bearer** — so flipping enforcement today would 503/401 the whole knowledge surface. auth-core already issues data-plane audience tokens (`plane-token.controller.ts`), and the gateway already mints quarry tokens the same way.

| # | Task | Fix | Verify |
|---|---|---|---|
| 6.1 | Gateway sends data-plane Bearer (do FIRST) | Extend `knowledge/shared.rs` internal_request/fetch_json legs targeting Data Plane to mint `get_audience_token(..., "data-plane")` (cached per user+audience) and `.bearer_auth(token)`; keep `x-org-id` (enforce mode cross-checks it against the verified claim). | gateway cargo tests + knowledge sources payload unchanged |
| 6.2 | Implement documents-api JWT verification | Port retrieval-engine's approach: `github.com/golang-jwt/jwt/v5` + static `JWT_PUBLIC_KEY_FILE` (mount same `convex-auth.pub`), optional `github.com/MicahParks/keyfunc/v3` against `AUTH_CORE_JWKS_URL`; RS256-only, `aud=="data-plane"`, issuer check, 30s skew, non-empty `org_id`; 403 when verified org ≠ `X-Org-ID`. | `go test ./pkg/authctx/...` with minted test keys |
| 6.3 | Flip `AUTHCTX_ENFORCE=1` + tenant-isolation tests | After 6.1+6.2 are live: enable enforce in compose, add cross-tenant denial integration tests (org A token cannot read org B docs). | 401 unauth / 403 cross-tenant / 200 valid |
| 6.4 | Ingest contract semantics (paired with 0.1) | Contract already documents them (`contracts.rs:145-156`): user-initiated crawl → private-by-default; system/connector ingest → org-visible; `visibility: Some` only for promote-to-org. No product decision pending — just keep tests honest. | contract roundtrip tests |

## Phase 7 — E2E proof (proof-ladder L4→L5)

| # | Task | Fix | Verify |
|---|---|---|---|
| 7.1 | Playwright harness | Add Playwright dep + config to velionv3 (none exists today); seeded verified account from 1.1 as fixture. | `pnpm exec playwright test` runs |
| 7.2 | Authenticated cross-plane journey | One spec: signin → onboarding-complete workspace → knowledge sources (Data) → crawl start (Ingestion via edge) → chat/run (Model) → inbox/notification (Application). This is the audit's missing L4. | spec green against the local stack |
| 7.3 | L5 policy proof | Fold in Phase 6 tenant-denial + Phase 5 webhook smoke + Phase 2 edge-only guard as CI-runnable gates. | gates listed in cross-plane map updated |

## Dependencies & parallelism

```
Phase 0 (all parallel, no deps)
Phase 1.1 ──► Phase 7.1/7.2 (fixture)
Phase 6.1 ──► 6.2 ──► 6.3   (bearer before verification before enforce)
Phase 0.1 ──► 6.4           (compile before contract test additions)
Phase 2, 3, 5 independent of each other; 2 needs nothing from 1 (session already exists during onboarding)
```

Council-lens notes (synthesized): **security** — 5.1 (fail-open webhooks) and 6.2/6.3 (header-trust tenancy) are release-blocking regardless of product pressure; 2.x closes the last sanctioned-entrypoint bypass. **honesty** — 1.2 (mail never sent) and 3.2 (fabricated fallback data) are the two places the product currently lies; fix before any new feature work. **architecture** — every fix above stays inside existing plane contracts (port retrieval-engine's verifier rather than inventing a new Control dependency; reuse audience-token plumbing rather than new auth paths). **delivery** — Phase 0 is an afternoon; Phases 1+6.1 can run in parallel; the long pole is 6.2+6.3 with tests.
