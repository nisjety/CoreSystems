# WS10-model-entitlements - Org and role model entitlements

Item: F-t3-023 (priority, audit effort L, this design sizes it XL). Wave W1. Written against HEAD afb83928 (worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b`) and the LIVE tree `C:/dev/CoresSystem`. Abbreviations: `MP` = `apps/Model Plane`, `MPR` = `apps/Model Plane/rust`, `CP` = `apps/Control Plane`, `FE` = `apps/Frontend Plane/verevonv3`. Line numbers are HEAD unless marked `[live]`.

## 1. Scope, items resolved, owner

**Item.** F-t3-023: give the refused `OrgPolicy.allowed_models` a real owner, filter the model catalogue per caller, and refuse a disallowed model at admission with a typed reason, failing closed.

**Owner.** Control Plane decides (org-core stores the policy, resolves it per member, explains it). Model Plane enforces: inference-core is the policy enforcement point (PEP) and the only Model Plane service that pulls the resolved policy; model-gateway asks it for early admission. Frontend Plane owns the admin UI and the BFF route. Needs ADR-0005 (S0).

**IN**
- Org default policy plus per-role overrides (role = `organization_members.role`: owner, admin, member, viewer, or a custom role name). Allow list or allow-all, deny list (deny wins), maximum effort, minimum privacy tier, default model and effort per surface (chat, do, research).
- Admin read/write API in org-core, member self-explain and admin member-explain, machine resolve endpoint behind a new narrow scope.
- Chat-modality enforcement in inference-core (infer, infer_stream, auto-routing, ladder, provider default), list filtering, early admission in model-gateway, effort clamp.
- Typed refusals (`model_not_entitled`, `model_entitlement_unavailable`), golden contract, SPA strings.
- Admin UI section, action contract `org.set_model_entitlement`, composer reconciliation.

**OUT (explicit)**
- User groups: no user-group model exists. `org_group` in auth-core `migrations/028_org_group_grants.sql` is an account grouping for data access and billing, not a member group. Schema leaves `scope_kind` extensible.
- Plan layer (plan-to-model ceilings): needs a commercial mapping nobody has written. Deferred (D7).
- Non-chat modalities (speech, translation, vision, document, image, video, realtime, embeddings): not enforced and not filtered in v1 (D5). Chat-only avoids a catalogue that hides what is still callable.
- Service principals (no `user_id`): exempt in v1 and counted (D6).
- Per-Space model ACL (Space session), router-policy write hardening, bring-your-own-key custody (F-t3-065), model record v2 (F-t3-042).
- NATS push invalidation (`org.model_entitlement.changed`): TTL only in v1.

**Slice split**

| Slice | Tag | One line |
|---|---|---|
| S0 | [BUILD] docs | ADR-0005, ownership rows, ledger entry, supersede CR-06 direction |
| S1 | [FINISH] | Type the refusal that exists today (`ALLOWED_MODELS` 400 string) and ship the refusal catalogue |
| S2 | [BUILD] | org-core policy store, resolve, explain, admin and machine API, scope |
| S3 | [BUILD] | mp-contracts matcher, `GetModelAccess` RPC, inference-core PEP and puller, goldens |
| S4 | [BUILD] / [FINISH] | model-gateway admission, list meta, effort clamp, `OrgPolicy` read-back, execution-core typed surface |
| S5 | [BUILD] | BFF routes, action contract, ZDR interplay |
| S6 | [BUILD] | SPA admin section, composer reconcile, error strings, e2e |
| S7 | [BUILD] | Shadow to enforce rollout and live proof |

## 2. Verified current state

**Admission today is one deployment env list that few paths read.**
- `ALLOWED_MODELS` is read by `load_allowed_models` (`MP.../model-gateway/src/normalize.rs:84-93`); unset or empty means all models. The gate (`normalize.rs:363-378`) returns an untyped 400 `{"error": "model '..' is not in the allowed list: [..]"}`; the helper that can carry a code (`normalize.rs:120-126`) is not used by it.
- `normalize` has exactly three callers: unary invoke (`http_routes.rs:8821`) and the stream route (`sse.rs:688`, `sse.rs:927`). The gRPC `invoke` (`grpc.rs:683`), `ai_chat` (`http_routes.rs:2306`), deep research, tool-loop rounds and execution-core never call it. So even the env list is not a universal gate. This is why inference-core is the PEP.
- `ALLOWED_MODELS: ""` in `MP/deploy/docker-compose.override.yml:12` (dev) is a no-op. It is not declared in `MP/deploy/docker-compose.yml`.
- `OrgPolicy.allowed_models = 4` (`MP/proto/model_plane/v1/gateway.proto:1343-1355`) is refused by name: `unowned_policy_fields` (`model-gateway/src/grpc.rs:148-160`), refusal at `grpc.rs:2220-2226` (`Status::unimplemented`); `get_policy` (`grpc.rs:2167-2200`) returns it zero-valued.

**Listing has no identity at the gateway, and inference-core is where identity exists.**
- `list_models` binds `Extension(_claims)` and sends `ListModelsRequest::default()` (`http_routes.rs:7490-7502`). `ai_models` (`http_routes.rs:3577-3594`), `ai_realtime_models` (4849) and `ai_video_models` (5002) proxy the same RPC. The proxy carries the user's own `aud=inference-core` bearer, so inference-core sees the real `org_id` and `user_id`.
- inference-core `ListModels` (`inference-core/src/grpc.rs:388-459`) reads only `modality` and `provider`, then merges eight chains and de-duplicates by id. `AuthenticatedPrincipal` has `org_id`, `user_id: Option<String>`, `scopes` (`inference-core/src/auth.rs:82-90`); `to_internal_request` already copies `org_id` and `user_id` onto the provider `InferRequest` (`grpc.rs:1107-1108`; struct `provider/mod.rs:120-176`).
- Roles are not in any Model Plane claim: model-gateway `Claims` (`model-gateway/src/auth.rs:42-80`) and inference-core claims (`inference-core/src/auth.rs:60-77`) have no role field.

**The chain can pick a model nobody admitted.**
- `resolve_intent` rewrites `verevon-*` to a concrete model from a routing-policy cell (`provider/fallback.rs:1011-1041`, `provider/intent.rs:198-270`); `tool_ladder` adds fallback rungs (`fallback.rs:1105-1127`); an empty or `default` model resolves to the provider default inside `infer_one_model` (`fallback.rs:236-244`, `1261-1317`). The gateway tool loop can also swap the answer model after admission (`sse.rs:2530-2537`). Any per-request allowlist at the gateway misses all of these; a per-candidate gate in `infer_one_model` / `stream_one_model` (`fallback.rs:1280`, `1616`, next to `provider_meets_privacy_tier` at `1293`) does not.
- The response cache key is scoped by `org_id` and `user_id` (`inference-core/src/cache.rs:52-70`), so no cross-user cache leak, but a user whose role just lost a model would still hit their own cached answer unless admission runs before `self.cache.get` (`fallback.rs:1160`).

**Control Plane already has the pieces to reuse, and one that does not fit.**
- Precedent for the exact route shape: `GET /internal/orgs/:orgId/retention-posture` behind its own narrow scope `org:retention:read:any` (`CP/org-core/internal/http/server.go:168`, `service_auth.go:30-51`, `348-375`, `retention_posture_handler.go:15-67`), read from the database, `Cache-Control: no-store`, tested by a contract that reads the Control compose registry (`internal/http/retention_posture_contract_test.go:133-175`).
- Precedent for the write path: BFF `require_org_admin` (`FE/apps/gateway/src/domains/orgs/shared.rs:35-50`) then org-core `PUT .../quotas/:key` behind `guard` (`server.go:97`, `membership_guard.go:39-75`) with the gateway principal's `org:settings:write:self` and a signed v3 delegation (`service_auth.go:425-488`, `556-611`). Owner/admin check in org-core: `CallerRole` (`org/service_enhanced.go:963-979`, used at `http/gdpr_handlers.go:91-100`).
- Member role: `organization_members.role` (migration `005_organization_members.up.sql`), one role per member, point query pattern `IsActiveMember` (`org/repository.go:1166-1187`). RLS: `WithOrgScope` (`internal/database/database.go:97-125`), policy template `019_interactive_retention_outbox.up.sql:16-26`.
- `org_entitlements` (`org/repository.go:813-868`, `types.go:28-32`, `266-271`) is key plus boolean feature flags. It cannot carry allow or deny lists. The card's "reuse org_entitlements" holds for the GET and cache plumbing only; the policy needs its own table. `org_role_mappings` and the capability catalogue (`rbac/catalog.go`, `rbac/effective_capabilities.go:17-77`) enumerate fixed capability strings and cannot enumerate deployment-specific model ids; rejected as storage.
- capability-core `routing_policies` (`MP/go/services/capability-core/migrations/0003_capabilities_registry.up.sql:262-286`) is CRUD only (`internal/api/registry_apis.go:1431-1598`); no Rust or Go service reads it at runtime (grep of `MP` rust, go, ts). It models a routing strategy (`round_robin`), not authorization. `GATEWAY_DEDUPLICATION_PLAN.md:388` names it as the allowlist home; ADR-0005 reverses that explicitly.
- Credential: `model-gateway` already holds an org-core principal with `org:read:any` (`CP/docker-compose.yml:540`, token required by `:?`); `MP/deploy/docker-compose.yml:653-673` wires `ORG_CORE_URL` and `MODEL_GATEWAY_ORG_CORE_SERVICE_TOKEN` (default empty). inference-core has no org-core principal and no org-core URL. The sweep's "no service principal can read entitlements" is stale for deployment, true for scope: `org:read:any` covers only `GET /api/v1/organizations/:id/entitlements` and `quotas` (`service_auth.go:290`), which return feature flags.
- Staleness precedent: auth-core serves the last posture when org-core is down, 24 h if it was ZDR, 15 min if not, else 503 (`CP/CONTROL_PLANE_OWNERSHIP.md:356`). The audit's "60-second claim" is the service-principal token cap, not the user path.

**Quota fail-open is a different decision.** `org_quota::fetch_org_limits` fails open by design (`org_quota.rs:97-157`, call sites `sse.rs:1084-1094`, `http_routes.rs:8964`, `grpc.rs:2177`). WS10 does not reverse it: a spend guard is accounted again by cost-core, a deny list has no second line. The ledger entry (S0) states the asymmetry.

**Frontend.**
- Pattern to copy: action `org.set_quota` (`FE/src/shared/actions/action-registry.ts:978-983`, `2739-2748`), dispatcher `dispatch_org_set_quota` (`FE/apps/gateway/src/domains/actions/dispatchers.rs:4335-4364`) reusing the real handler `quotas::set_quota` (`domains/orgs/quotas.rs:72-156`), route registration `domains/orgs.rs:47-52`, 1:1 registry-to-handler gate (`apps/gateway/scripts/action-surface-contract.test.mjs:21-45`), client `src/shared/api/org-quota-client.ts`, section `src/features/settings/components/OrgQuotasSection.tsx`, mounted `WorkspaceSettingsPage.tsx` (`Match` blocks `364-387`; `quotas` section id `settings-sections.ts:18,147`).
- The catalogue is fetched through `GET /api/v1/models` (`FE/apps/gateway/src/domains/chat/json_handlers.rs:453-478`) and `listModels()` (`src/shared/api/chat-client.ts:2443-2473`, `ModelInfo` `849-869`). The composer holds a persisted `selectedModel` and a stale-connection notice pattern (`DashboardComposer.tsx:1115-1180`).
- The org ZDR switch gate reads `/v1/models` as the acting admin and treats the catalogue as deployment-level (`FE/apps/gateway/src/domains/orgs/settings.rs:68-118`). After S3 the catalogue is per caller, so the gate can say "no attested provider" because of the admin's own entitlement. S5 fixes it.
- Error strings are keyed by code in `streamFailurePresentation` (`src/features/chat/components/chat-normalizers.ts:380-470`).
- Router policy is a global singleton (`FE/apps/gateway/src/domains/router_policy.rs:1-78`) whose admin gate is the SPA plus a forwarded role header (its doc, lines 9-10). Out of scope, flagged in section 8.

**Live-only or live-modified (must land first, or rebase on).** All verified in `.plan-work/live-tree-status.txt`.

| File | State | Why it matters |
|---|---|---|
| `MPR/crates/mp-contracts/src/zdr_refusal.rs` (live path `C:/dev/CoresSystem/apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`) | ?? | Envelope precedent `{code, message, retryable, action}`; WS10 builds a sibling catalogue, does not import it |
| `MPR/crates/mp-contracts/src/lib.rs` | M (+2) | S1 and S3 add a `pub mod` line; rebase |
| `model-gateway/src/{normalize,grpc,http_routes,sse,state,auth,grpc_auth}.rs` | M | S1 and S4 edit regions; line numbers move (+7 normalize gate, +68 sse org_limits, +113 http_routes list_models) |
| `inference-core/src/{grpc,auth}.rs` | M | S3 hot spot (ZDR waiver work) |
| `FE/src/features/chat/components/chat-normalizers.ts`, `DashboardComposer.tsx`, `src/shared/api/http.ts`, `src/shared/i18n/errors.ts`, `src/app/App.tsx` | M | S1 and S6 SPA edits |
| `scripts/tests/zdr-contract-sync.test.sh`, `zdr-contract-schema.mjs` | M | WS10 does not edit them; it adds its own sync script |

Precondition: the ZDR working-tree wave is committed (WS01 S0) before S1, S3, S4, S6 start. S0 and S2 do not depend on it.

**Audit text that is stale or incomplete**
- Cited lines drift (normalize 360-384 is HEAD 363-378; `list_models` 7590-7640 is HEAD 7490-7529; inference-core `ListModels` 394-424 is HEAD 388-459; `sse.rs:1154` is HEAD 1087).
- "ALLOWED_MODELS is the only live allowlist": true, but it gates only the two normalize routes.
- "model-gateway pulls": the design below moves the pull to inference-core (D3).
- "Roles, groups, plan": roles yes; groups do not exist; plan deferred.
- "Reuse org_entitlements": plumbing only; table is new.

## 3. Decisions and open questions

| ID | Question | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | Entitlement ownership ADR; reverse `GATEWAY_DEDUPLICATION_PLAN.md:388` (allowlist to capability-core `routing_policies`) | (a) org-core stores, inference-core enforces. (b) capability-core `routing_policies`. (c) Convex mirror (`schema.ts:21-22`) | (a). Matches `CLAUDE.md:23`, role resolution lives in org-core, `routing_policies` is a routing strategy with no runtime reader, Convex may only narrow (matrix rule 10). Record as ADR-0005 plus a ledger entry (proposed). Explicit reversal of the CR-06 fix direction | S2 to S7. S1 proceeds without it |
| D2 | Delivery to Model Plane | (a) service-principal pull with TTL and last-known-good. (b) auth-core mints a signed claim. (c) gateway attaches the resolved policy to every `InferRequest` | (a). (b) needs role-rule-sized claims or a second fetch, and token TTL becomes revocation latency; rejected for v1. (c) misses ~12 gateway build sites plus execution-core, session-core and scheduled callers | S3 |
| D3 | Which Model Plane service pulls | (a) both gateway and inference-core pull (the card). (b) inference-core only; gateway asks via new RPC `GetModelAccess`. | (b): one cache, one fail-closed implementation, one new principal, no admit/refuse disagreement window. Cost: one RPC and 7 mock impls to extend. Fallback to (a) if the mock churn blocks (swap S4 to embed the client) | S3, S4 |
| D4 | Fail-closed semantics | Fresh answer within TTL; else stale-if-error bounded; else refuse | TTL 30 s. Stale bound asymmetric like auth-core (`CONTROL_PLANE_OWNERSHIP.md:356`): 900 s if last answer was restrictive, 120 s if it was unrestricted. Beyond the bound, or never seen: refuse `model_entitlement_unavailable` (503, retryable). Mode flag `off`, `shadow`, `enforce`; unknown value means `enforce` (deliberate deviation from the safe-fallback-to-off vocabulary K, because a typo must not open access) | S3, S7 |
| D5 | Modalities in v1 | chat only; or all | Chat only. Policy applies to `modality == "chat"` entries and chat calls; others pass unfiltered | S3 |
| D6 | Service principals (`user_id` None) | exempt in v1; or org default applies | Exempt, with metric `decision="service_exempt"`. Platform jobs (dreaming, learning review, intent probes) must not break on an org list. Consequence: unattended or scheduled runs must carry the owner's user principal or they escape the policy; interface to WS20 (D10 identity) | S3 |
| D7 | Plan and group layers | defer; or build | Defer. No group model; plan-to-model needs product pricing. Schema key `scope_kind` accepts `org`, `role` today | S2 |
| D8 | Effort cap semantics | clamp; or refuse | Clamp at the gateway where the effort string becomes a budget (`sse.rs:2539-2545`), surface the effective value in explain and `entitlement.max_effort`. Refuse only for models. Rung ids come from WS11; interim `quick < standard < deep` | S4 |
| D9 | Role override precedence | replace org default; or field-wise merge | Replace: a role override is a complete policy, the explain output names exactly one layer. Within a policy deny always wins | S2 |
| D10 | ZDR interplay | allow admin to strip every attested model; or block | Block at write time (BFF, S5: 409 `model_policy_leaves_no_zdr_model` when org ZDR is on) and refuse at runtime with the typed code. Never substitute | S5 |
| D11 | Refusal wire | keep 400 string; or typed | Typed: HTTP 403 `{"error":{code,message,retryable,action,details}}`, gRPC `PERMISSION_DENIED` plus `x-mp-provider-error` trailer, SSE `error` frame with the same code. Client-visible change from 400, documented | S1 |
| D12 | `ALLOWED_MODELS` fate | remove; keep as operator ceiling; move to inference-core | Keep as an operator ceiling at the two normalize routes, typed refusal reason `deployment_allowlist`, documented as non-universal; deprecate once org policies are normal | S1 |
| D13 | Who may write | owner only; owner and admin | Owner and admin, checked at the BFF (`require_org_admin`) and again in org-core (`CallerRole`), because the forwarded role header is not authority | S2, S5 |
| D14 | Convex mirror `allowedModels` | delete; mark deprecated | Comment-only deprecation; never mirror policy into Convex (rule 10). `ai.ts:81` reads `defaultModel` for the legacy AI Core path and is out of scope | S0 |

Slices that need no decision: S0 itself, S1 (needs D11 only), S2 (needs D1, D9, D13, which S0 records).

## 4. Slices

### S0 [BUILD] ADR, ownership rows, ledger
**Goal.** Make ownership explicit before code.
**Files**
- NEW `apps/MODEL_ENTITLEMENTS_ADR_2026-10-15.md` (Series A, apps root; number ADR-0005 inferred from 0001 to 0004, integrator confirms). Sections: Context, Decision, Alternatives (capability-core, Convex, claim, gateway-attach), Consequences, release gates.
- `apps/master-ownership-matrix.md` - add rule 19 after the last rule in section 3 (rules `157-179`): "Model entitlements: Control decides, Model enforces, never widens, outage never widens (rule 10)". Add a note on the inference-core row (`127`).
- `CP/CONTROL_PLANE_OWNERSHIP.md` - row beside `:340` and bullet beside `:355-356`.
- `MP/docs/capability-ownership-matrix.md` - clarify beside the registry rows that model entitlement is not a registry.
- `MP/docs/decisions/ledger.md` - new newest-first entry above the 2026-10-06 entry: state `proposed`; names the quota fail-open asymmetry.
- `MP/docs/GATEWAY_DEDUPLICATION_PLAN.md:225-241, 388` - annotate CR-06 "superseded by ADR-0005".
- `apps/Application Plane/convex-core/convex/schema.ts:21-22` - comment-only deprecation of `allowedModels`.
**Flag / ZDR / tests.** None. ZDR clause: states that entitlement metadata holds model ids only and no content. Tests: `node --test scripts/tests/ownership-docs-contract.test.mjs` (exists).
**DoD.** ADR accepted by owner; ledger entry present. **Evidence.** Review record. **Effort** S, 1-2 d. **Deps** none. **Risks** owner picks capability-core: re-plan S2 and S3 (org-core stays the role resolver).

### S1 [FINISH] Typed refusal for the gate that exists, and the catalogue
**Goal.** The one refusal in production today becomes a stable code the SPA can render; the refusal vocabulary for S3 to S6 exists.
**Files**
- NEW `MPR/crates/mp-contracts/src/model_entitlement.rs` - `code::{MODEL_NOT_ENTITLED = "model_not_entitled", ENTITLEMENT_UNAVAILABLE = "model_entitlement_unavailable"}`, `Reason` (`not_in_allow_list`, `explicitly_denied`, `deployment_allowlist`, `no_entitled_route`, `member_not_found`), `Action`, `Entry` table, builders for JSON body, gRPC status trailer, SSE pair. Same envelope as the live ZDR catalogue; separate module (no import of the live-only file).
- `MPR/crates/mp-contracts/src/lib.rs` - `pub mod model_entitlement;` (file is M live; rebase).
- `MPR/services/model-gateway/src/normalize.rs:84-93, 363-378` - make the gate a pure `fn deployment_allowlist_refusal(model, allowed: Option<&[String]>)`; the env read stays in `load_allowed_models`. Refusal: 403, nested typed body, reason `deployment_allowlist`.
- `MPR/services/model-gateway/src/sse.rs:932-945` - read both shapes (`error` string plus top-level `code`, or nested `error.code`): tiny helper, no behaviour change for existing refusals.
- `MP/api/contracts/model-entitlement-refusals.json` NEW (+ `.sha256`) authored catalogue; `MP/api/contracts/README.md` row.
- NEW `scripts/tests/model-entitlement-contract-sync.test.sh` and `scripts/tests/model-entitlement-contract-schema.mjs` (+ `.test.mjs`): pins and vendored copies, modelled on the ZDR pair without editing them (those are M live). `.github/workflows/contract-goldens.yml` - add the path.
- `FE/src/features/chat/components/chat-normalizers.ts:380-470` - cases `model_not_entitled` (action choose another model, not unavailable) and `model_entitlement_unavailable` (retry), Norwegian first. File is M live.
- NEW `FE/src/shared/api/model-entitlement-refusals.ts` plus test: code list checked against the golden.
**Contracts.** New error codes only. No proto. **Flag.** None; rollback is revert. **ZDR.** Refusal bodies carry model id and reason only, never content; golden row `r_refusal_is_content_free`.
**Tests.** `cd "apps/Model Plane/rust" && cargo test -q -p mp-contracts --all-targets && cargo test -q -p model-gateway --lib normalize`. New unit tests in `normalize.rs`: `allowlist_refusal_is_typed_403`, `allowlist_refusal_names_deployment_allowlist_reason`, `empty_allowlist_admits_everything` (pins today's behaviour), `pseudo_and_empty_models_skip_the_gate`. New `MPR/crates/mp-contracts/tests/model_entitlement_catalogue.rs`: catalogue equals golden. `bash scripts/tests/model-entitlement-contract-sync.test.sh`. SPA: `pnpm test -- chat-normalizers`.
**DoD.** Setting `ALLOWED_MODELS=a` and sending model `b` on `/v1/invoke` and `/v1/invoke/stream` gives the typed 403 and an SSE `error` frame with `code: model_not_entitled`; SPA shows the Norwegian text; goldens green.
**Evidence.** Rebuild model-gateway (`bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build model-gateway`), set the env on the stack, capture one curl transcript (unary) and one SSE capture; restore env. **Effort** S/M, 2-3 d. **Deps** none (D11). **Risks** clients matching on 400; only the SPA exists, grep for status checks in `http.ts` (M live).

### S2 [BUILD] org-core policy store, resolve, explain, admin and machine API
**Goal.** Control Plane owns, stores, resolves and explains model policy. Dark: nothing calls the machine route yet.
**Data model.** Migration `<next>_org_model_entitlements.up.sql` (+ `.down.sql`; org-core numbering is next free 021, the integrator assigns):
- `org_model_policies(org_id TEXT REFERENCES organizations(id) ON DELETE CASCADE, scope_kind TEXT CHECK (scope_kind IN ('org','role')), scope_value TEXT NOT NULL DEFAULT '', allow_mode TEXT CHECK (allow_mode IN ('all','listed')), allow TEXT[] NOT NULL DEFAULT '{}', deny TEXT[] NOT NULL DEFAULT '{}', max_effort TEXT, min_privacy_tier TEXT, defaults JSONB NOT NULL DEFAULT '{}', PRIMARY KEY (org_id, scope_kind, scope_value))`.
- `org_model_policy_revisions(org_id PK, revision BIGINT, updated_by TEXT, updated_at TIMESTAMPTZ)` and append-only `org_model_policy_history(id, org_id, revision, actor_id, before JSONB, after JSONB, created_at)`.
- GRANT to `org_core_app`, RLS `app.current_org` policy per table (template `019:16-26`), erasure: FK cascade plus a postgres test proving `gdpr_hard_delete_organization` (`003:4-48`, redefined in `018`, not opened) leaves no rows.
- Entries are exact model ids (case-insensitive) or `provider:<id>`; no globs (an allow glob would widen silently). Limits: 500 entries, 128 chars each, 20 roles, 64 KB body.
**Resolution (pure, table-tested).** Role override for the member's role replaces the org default; absent override uses org default; absent policy is `state: none` (explicit, unrestricted). Non-member user: allow-listed to nothing (`member_not_found`). `user_id` absent: org default only. Validation: default model must pass the same policy; default effort not above max effort; `max_effort` must be a WS11 ladder rung.
**API (all new)**
- `GET /api/v1/organizations/:id/model-entitlement` (scope `org:read:self`, delegation): `{organization_id, revision, org_default, roles, updated_at, updated_by}`.
- `PUT /api/v1/organizations/:id/model-entitlement` (guard plus `org:settings:write:self`; handler `CallerRole` owner or admin): body `{expected_revision, org_default, roles}`, 409 on revision mismatch, writes history in the same transaction, invalidates nothing (reads bypass caches, like retention posture).
- `GET /api/v1/organizations/:id/model-entitlement/explain[?user_id=]` (`org:read:self`): effective policy, matched layer, role, revision; admin only when `user_id` names someone else.
- `GET /internal/orgs/:orgId/model-entitlement?user_id=` (new scope `org:model-entitlement:read:any`, `Cache-Control: no-store`): `{state, revision, role, can_manage, policy, sources[]}`.
**Files**
- NEW `CP/org-core/internal/modelentitlement/{types,validate,resolve,repository,service}.go` (package pattern of `internal/rbac/`).
- NEW `CP/org-core/internal/http/model_entitlement_handlers.go`; `internal/http/server.go:78-180` routes (v1, `/orgs/:id/...` proxy variants, `/internal`); setter `SetModelEntitlements` beside `retentionPosture` (`server.go:52-56`) so `NewServer` keeps its signature; `cmd/server/main.go:73,167` wiring.
- `internal/http/service_auth.go` - scope allowlist `30-51`, `organizationAPIScopes`/`organizationProxyScopes` (`282-346`), `internalOrganizationScopes` (`348-375`).
- `CP/docker-compose.yml:539-540` registry: add the scope to `model-gateway` (kept for gateway-side explain) and add principal `inference-core` (`INFERENCE_ORG_CORE_SERVICE_TOKEN`, scope `org:model-entitlement:read:any` only). `CP/scripts/run-control-plane.sh:425-457` add the token to the minted list.
- NEW `CP/org-core/internal/http/testdata/inference-core-org-core-model-entitlement.contract.json` and `model_entitlement_contract_test.go` (reads the compose block, as `retention_posture_contract_test.go:133-175`).
- Golden `MP/api/contracts/model-entitlement-matrix.json` (NEW, authored; resolution rows including ZDR-tagged rows) vendored into `internal/modelentitlement/testdata/`.
**Flag.** None needed; no caller until S3. Rollback: remove the scope from the registry. **ZDR.** Stores ids and actor ids only; history is org configuration, erased with the org; rows `z_policy_has_no_content`, `z_policy_survives_zdr_toggle`.
**Tests** (`cd "apps/Control Plane/org-core" && go test -p 1 ./...`, Postgres env as in `.github/workflows/org-service.yml:96-103`): NEW `internal/modelentitlement/resolve_test.go` (matrix rows: role replaces default, deny wins, non-member, service principal, `none` state), `validate_test.go`, `repository_postgres_test.go` (RLS cross-org read refused, revision conflict, history row, erase), `internal/http/model_entitlement_handlers_test.go` (member cannot PUT, owner can, explain other member needs admin, stale revision 409), `service_auth_test.go` additions (new scope accepted only on the internal path, `:any` refused for the gateway principal per `service_auth.go:141-146`).
**DoD.** All above green; compose registry parses with the new principal.
**Evidence.** Rebuild org-core (`bash "apps/Control Plane/scripts/run-control-plane.sh" up -d --no-deps --build org-core`); with the new token curl the internal route for an org with no policy (`state: none`), after a PUT through org-core with a signed delegation (use the contract test vector), and a member of each role. Capture bodies and the history row. **Effort** L, 5-7 d. **Deps** S0. **Risks** role projection lag from auth-core (explain shows `role` and `projection_revision` if available); custom role names.

### S3 [BUILD] mp-contracts matcher, `GetModelAccess`, inference-core PEP
**Goal.** inference-core resolves the caller's policy, enforces it on every chat call and in the catalogue, fails closed, and tells model-gateway what it needs.
**Proto** (`MP/proto/model_plane/v1/inference.proto`, additive; `buf breaking` clean). No `InferRequest` tag is consumed (tags 17 to 20 are reserved for WS11, WS02, WS08).
- `rpc GetModelAccess(GetModelAccessRequest) returns (GetModelAccessResponse)` in `InferenceCore` (`:11-75`).
- `message ModelPolicyDoc { string allow_mode = 1; repeated string allow = 2; repeated string deny = 3; string max_effort = 4; PrivacyTier min_privacy_tier = 5; map<string, SurfaceDefault> defaults = 6; }`, `message SurfaceDefault { string model = 1; string effort = 2; }`.
- `message GetModelAccessRequest { string request_id = 1; string org_id = 2; string model = 3; string provider_hint = 4; string surface = 5; ModelPolicyDoc draft = 6; bool include_catalogue = 7; }` (`draft` and `include_catalogue` need `can_manage`).
- `message GetModelAccessResponse { string request_id = 1; string state = 2; bool enforced = 3; int64 revision = 4; string role = 5; bool restricted = 6; bool model_allowed = 7; string reason = 8; string default_model = 9; string default_effort = 10; string max_effort = 11; PrivacyTier min_privacy_tier = 12; bool can_manage = 13; repeated ModelAccessEntry catalogue = 14; }` and `message ModelAccessEntry { string id = 1; string provider = 2; bool entitled = 3; string reason = 4; }`.
- `ListModelsResponse` (`:523-526`, field 1 used) `+ ModelAccessMeta access = 2` (`state`, `enforced`, `revision`, `restricted`, `hidden_count`, `default_model`, `default_effort`, `max_effort`).
- Every new field, message and enum value carries a doc comment (buf config wants them; `ledger.md:3764-3773` records adding zero lint violations as the bar). Regenerate Go only, scoped: `buf generate --template buf.gen.go-only.yaml --path model_plane/v1/inference.proto` (`ledger.md:3769-3771`), to avoid the unrelated-file churn of a full run; Rust is generated by `mp-contracts/build.rs` (fingerprint caveat: drop the Docker cache mount or touch `build.rs` if the image misses the proto change).
**Files**
- `MPR/crates/mp-contracts/src/model_entitlement.rs` (from S1) - add `Policy`, `ResolvedAccess::{permits(model, provider), deny_reason, clamp_effort}`, wire-doc parse, `Decision`. Pure. Golden vendored as `MPR/crates/mp-contracts/tests/fixtures/model-entitlement-matrix.json`.
- NEW `MPR/services/inference-core/src/entitlement.rs` - client for `GET /internal/orgs/:orgId/model-entitlement` (pattern: `BudgetClient`, `provider/intent.rs:276-340`: 1 s total timeout, 600 s idle pool, keepalive), single-flight per `(org_id, user_id)`, LRU 10 000 entries, TTL and asymmetric stale bound (D4), mode `off|shadow|enforce`, metrics `model_entitlement_total{decision,mode}`.
- `inference-core/src/config.rs:226-258` - `model_entitlement_mode`, `org_core_url`, token, service id, TTLs.
- `inference-core/src/grpc.rs` - `authorize` unchanged; `infer` (`276-300`) and `infer_stream` (`307-330`) resolve access before `to_internal_request` (`1062-1115`, gains an `access` argument, test at `~1539` updated); `list_models` (`388-459`) filters chat entries after de-dup and fills `access`; new `get_model_access`; `TenantRequest` entry for the new request (`96-152`); new `ProviderError` mapping at `1467-1509`; new trailer kind constants at `1340-1377`.
- `inference-core/src/provider/mod.rs:120-176` - `InferRequest.access: Option<Arc<ResolvedAccess>>`; `ProviderError::{ModelNotEntitled, EntitlementUnavailable}` (`:940`).
- `inference-core/src/provider/fallback.rs` - (1) pinned-model pre-check before the cache (`1144-1160`, and in `infer_stream` at `1537`); (2) per-candidate permit in `infer_one_model` after `request_for` resolves the provider default (`1317`) and in `stream_one_model` (`1616`), skip like the tier gate; (3) `tool_ladder` (`1112`) prunes denied rungs; (4) exhaustion error picks `ModelNotEntitled` when `total_attempts == 0` and a denial happened (`1222`, `1586`); (5) min privacy tier is `max(request tier, policy tier)` through the existing gate (`1293`); (6) `list_models` unchanged.
- `inference-core/src/metered.rs:401-440` - `get_model_access` arm (a new RPC cannot compile unmetered, `ledger.md:146-170`).
- `MP/api/contracts/infer-statuses.json` - regenerate (`MP_UPDATE_GOLDEN=1`, writer at `grpc.rs:1904-1935`), then update the pin in the sync script that owns it (M live: coordinate with WS01).
- Mock `InferenceCore` impls that must add the method: `model-gateway/src/grpc.rs:2920,3279`, `http_routes.rs:11049`, `model-gateway/tests/e2e_invoke_chain_test.rs:263,752,898`, `execution-core/src/runtime_loop/agent.rs:5671`.
- Deploy: `MP/deploy/docker-compose.yml` inference-core block (`808-1004`) gets `ORG_CORE_URL`, `INFERENCE_ORG_CORE_SERVICE_TOKEN`, `MODEL_ENTITLEMENT_MODE: ${MODEL_ENTITLEMENT_MODE:-off}`; `MP/deploy/.env.example`; `MP/scripts/release-artifact.sh:50-100` secret key list (credential-shaped names fail artifact construction otherwise, `728-745`; WS01 is sole editor, send the one-line change to it).
**Flag.** `MODEL_ENTITLEMENT_MODE` default `off`; `shadow` logs and counts would-refuse and never refuses or filters; `enforce` fails closed. Restart required. Rollback: set `off` and restart inference-core.
**ZDR.** In-memory cache only; keys are ids, values are model-id lists; no persistence. Under `enforce` an unresolved entitlement refuses, never falls back, ZDR included. A policy that leaves no attested model for a ZDR token yields `model_not_entitled` (entitlement named first; the ZDR refusal for the unentitled-but-attested remainder is unchanged). Golden rows: `z_zdr_token_pinned_denied`, `z_zdr_org_policy_strips_all_attested`, `z_entitlement_unavailable_under_zdr_refuses`.
**Tests** (`cd "apps/Model Plane/rust" && cargo test -q -p inference-core --all-targets`; `-p mp-contracts`): `mp-contracts/tests/model_entitlement_matrix.rs` (every golden row); inference-core NEW `tests/model_entitlement_chain_test.rs` (wiremock org-core): pinned denied refused before provider call and before cache, auto-route skips a denied cell and uses an allowed rung, all rungs denied gives `no_entitled_route`, provider default denied tries next provider, deny wins over allow, `provider:` selector, listing hides denied chat models and keeps non-chat, role change visible after TTL, org-core down within stale bound serves last answer, beyond bound refuses `model_entitlement_unavailable`, `shadow` never refuses, `off` makes no HTTP call, service principal exempt and counted; `grpc.rs` tests for trailers; metrics series `model_entitlement_total` and `inference_requests_total{outcome="model_not_entitled"}`. Golden regeneration test must show only the two new kinds added.
**DoD.** Matrix green in Go (S2), mp-contracts and inference-core; `buf lint`, `buf breaking` pass; mocks compile.
**Evidence.** Rebuild inference-core with `MODEL_ENTITLEMENT_MODE=shadow`; with an org policy denying model B, send B as member: request succeeds, `model_entitlement_total{decision="would_refuse"}` increments, log line names org, revision, reason (no content). Switch to `enforce`: typed refusal. Capture scrape and gRPC trailers (grpcurl). **Effort** L, 7-10 d. **Deps** S1, S2 (contract file), D3, WS01 S5 landed first on `inference-core/src/{config,auth,grpc}.rs`. **Risks** `grpc.rs` and `auth.rs` live hunks (ZDR waiver); latency (cold path, copy the BudgetClient lesson); 7 mock impls; typed trailer collides with `FailedPrecondition` handling (use `PERMISSION_DENIED` and `UNAVAILABLE`, never `FAILED_PRECONDITION`, so `sse.rs:8146-8176` is not hit).

### S4 [BUILD]+[FINISH] model-gateway admission, list meta, effort clamp, `OrgPolicy` read-back, execution-core typed surface
**Goal.** Refuse before durable state is created, show defaults, clamp effort, finish the CR-06 decomposition.
**Files**
- NEW `MPR/services/model-gateway/src/model_access.rs` - `admit(state, inference_bearer, org_id, model, provider_hint, surface) -> Result<Admission, Refusal>` calling `GetModelAccess` through `state.inference_client` and `authenticated_inference_request` (`http_routes.rs:668`, `sse.rs:82`); `Admission` carries `default_model`, `max_effort`, `min_privacy_tier`, `enforced`. RPC error under any mode: typed `model_entitlement_unavailable`.
- Call sites, after normalize and before any session-core call: unary `http_routes.rs:8821` region; stream `sse.rs:927-947` (before the durable run is prepared); gRPC `invoke` (`grpc.rs:683`). `min_privacy_tier` raised to the policy floor (`normalize.rs:43-50` `min_privacy_tier_wire`). Empty model on a surface with a default: use the default (starting model, not a substitution; response says so).
- Effort clamp at `sse.rs:2539-2545` and the unary equivalent, via `mp_contracts::model_entitlement::clamp_effort` and `thinking::budget_tokens` (`thinking.rs:31-53`); WS11 swaps the rung table.
- `http_routes.rs:7490-7529` `list_models`: map `access` into `ListModelsHttpResponse` as sibling field `entitlement` (additive; `models` unchanged). `grpc_status_to_http` (`5760-5790`) and the stream error arm at `sse.rs:8146-8176` map the new trailer kinds to the typed body and frame before the `FailedPrecondition` arms.
- `[FINISH]` `grpc.rs:148-160, 2167-2226`: `get_policy` fills `allowed_models` from `GetModelAccess` for the caller's org default (read back what is applied); `set_policy` keeps refusing by name with a message that names the owner API (`PUT /api/v1/orgs/{id}/model-entitlement`). Update test `policy_fields_with_no_owning_service_are_named_not_silently_banked` (`grpc.rs:2760` area).
- execution-core: map inference trailer kinds to a typed run failure (precedent `execution-core/src/subscription_refusal.rs:65,155`), so a Do run that loses a model mid-run ends with `model_not_entitled`, not a generic failure. New `execution-core/tests/model_entitlement_refusal_contract.rs` (pattern `subscription_refusal_contract.rs`).
**Flag.** None new; behaviour follows `GetModelAccess.enforced`. **ZDR.** Refusal happens before `StartRun`, so a refused turn creates no thread, message or run under any posture; test asserts zero session-core calls.
**Tests** (`cargo test -q -p model-gateway --all-targets`, `-p execution-core --all-targets`): NEW `model-gateway/tests/model_entitlement_admission.rs` (mock inference; unary, stream, gRPC; no session-core call on refusal; effort clamp; default injection; `ALLOWED_MODELS` intersection; mode `off` passes through), update `e2e_invoke_chain_test.rs` mocks, `grpc.rs` read-back test.
**DoD.** Member with a restricted role is refused on all three invoke transports with the same code; owner is not.
**Evidence.** Rebuild model-gateway, inference-core, execution-core; run the S7 flow through the BFF for refusal text and listing. **Effort** M/L, 5-7 d. **Deps** S1, S3, WS11 ladder (interim table), WS01 S6 merges first on `sse.rs`. **Risks** `sse.rs`/`http_routes.rs` live hunks; tool loop swapping models (covered by inference-core).

### S5 [BUILD] BFF routes, action contract, ZDR interplay
**Goal.** Admin and member API through the gateway with the repo's action-contract discipline.
**Files**
- NEW `FE/apps/gateway/src/domains/orgs/model_entitlement.rs`; `domains/orgs.rs:47-52` routes `GET|PUT /api/v1/orgs/{id}/model-entitlement`, `GET .../model-entitlement/explain`; admin gate `require_org_admin` (`shared.rs:35-50`) on writes and member-explain; `proxy_json` to org-core with the delegation as in `quotas.rs:72-156`. Validation mirrors org-core limits and rejects unknown fields (the `ENFORCED_QUOTA_KEYS` lesson: nothing settable that nothing reads).
- Write guard (D10): when the org's interactive ZDR is on, call model-gateway `GetModelAccess` with `draft` plus `include_catalogue` (as the acting admin) and refuse 409 `model_policy_leaves_no_zdr_model` if no attested chat model remains; fail closed 503 if the catalogue cannot be read (same shape as `zdr_enable_refusal`, `orgs/settings.rs:134-163`).
- `orgs/settings.rs:68-118` `zdr_switch_gate`: read `entitlement.hidden_count` from the catalogue meta; when the admin sees no attested model but models are hidden, answer a distinct code (`zdr_models_not_entitled`) instead of "no provider attested".
- Capability off-switch: gateway config `MODEL_ENTITLEMENT_ADMIN_ENABLED` (typed field in `apps/gateway/src/config.rs`, default false in production compose until S7); GET returns `{enabled:false}` so the SPA hides the section.
- `FE/src/shared/actions/action-registry.ts` - `org.set_model_entitlement` (ownerPlane `control`, risk `high`, `requiresApproval: true`, reversible), input schema next to `orgSetQuotaInput` (`978-983`); `apps/gateway/src/domains/actions/handlers.rs:304` and `dispatchers.rs` (`dispatch_org_set_model_entitlement`, reusing the real handler). Not added to `MODEL_EXECUTABLE_ACTION_IDS` (`src/shared/actions/model-eligibility.ts:126-135`): a model must not edit its own model access.
- NEW `FE/src/shared/api/model-entitlement-client.ts`; `FE/docs/endpoint-map.md` row.
**Flag.** `MODEL_ENTITLEMENT_ADMIN_ENABLED` as above. **ZDR.** Policy writes carry ids only; the guard reads the catalogue, no content.
**Tests.** `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (new unit tests in `orgs/model_entitlement.rs`: member refused, owner allowed, unknown key refused, ZDR guard, wiremock for org-core and model-gateway); `pnpm test` runs `action-surface-contract.test.mjs` (registry and handler 1:1) and `action-registry.test.ts`.
**DoD.** Admin PUT through the BFF round-trips through org-core and shows in `GET`. **Evidence.** Rebuild gateway image (`verevon-gateway-rs`), curl as owner and as member with real session cookies. **Effort** M, 3-4 d. **Deps** S2, S3. **Risks** two admin gates must agree (BFF membership role vs org-core `CallerRole`); test the disagreement.

### S6 [BUILD] SPA admin section, composer reconciliation, e2e
**Files**
- NEW `FE/src/features/settings/components/OrgModelAccessSection.tsx` (+ test) cloned from `OrgQuotasSection.tsx`: org default editor (all or listed, deny list, picked from `listModels()` entries plus manual id), per-role overrides, max effort, minimum privacy tier, defaults per surface, optimistic revision, "test member access" calling explain; banner when `entitlement.enforced` is false: "saved, not enforced in this environment".
- `FE/src/features/settings/lib/settings-sections.ts:18,147` add section `model-access`; `WorkspaceSettingsPage.tsx` `sectionStatusCards` (`200-238`) and a `Match` (`~492`); `src/features/core/lib/sidebar-navigation.ts`.
- `FE/src/shared/api/chat-client.ts:2443-2473` - `listModelCatalogue()` returning `{models, entitlement}`; `listModels()` keeps its signature. `AiModelPicker.tsx:28-60` and `DashboardComposer.tsx:1039-1068, 1115-1180`: when a persisted selection is not in the filtered catalogue, reset to the entitlement default and show a one-line notice (pattern `staleSubscriptionNotice`); response-effort selector (`3528-3556`) capped at `max_effort`.
- `FE/src/shared/i18n/errors.ts` Norwegian and English strings for the two codes (M live).
- NEW `FE/tests/e2e/model-entitlement.spec.ts`; `FE/src/features/settings/components/OrgModelAccessSection.test.tsx`; composer reconcile test next to `DashboardComposer.subscription.test.tsx`.
**ZDR.** UI shows ids only. **Flag.** Section visibility follows the BFF `enabled` capability, never a `VITE_` flag.
**Tests.** `cd "apps/Frontend Plane/verevonv3" && pnpm lint && pnpm typecheck && pnpm test && pnpm build`; baseline failures in this tree are pre-existing, diff the failing set against HEAD. Playwright `pnpm test:e2e` against a running dev server (`E2E_BASE_URL`, second server on 5174 per `verevonv3-worktree-verification` memory).
**DoD.** Admin edits a role policy; the member's picker loses the model on next catalogue load; denied pinned send shows the Norwegian refusal; stale selection resets with notice.
**Evidence.** Playwright trace plus screenshots, rebuilt frontend and gateway images. **Effort** L, 6-8 d. **Deps** S4, S5, ZDR SPA wave committed, WS12 gates. **Risks** `DashboardComposer.tsx` is large and M live; the Space composer (`SpaceRoomComposer.tsx`) shares `listModels`.

### S7 [BUILD] Rollout and live proof
**Procedure.** Land S0 to S6 with the mode `off`. (1) org-core and registry first (principal and token minted by `run-control-plane.sh`, org-core image restarted: it refuses a registry its image does not know). (2) inference-core `shadow` for at least one working day; read `model_entitlement_total` for `would_refuse` and `service_exempt` against real traffic; fix surprises. (3) BFF admin enabled on the ZDR-TEST org and one staging org. (4) `enforce` on inference-core; keep `off` as the one-restart rollback. (5) Flip `MODEL_ENTITLEMENT_ADMIN_ENABLED` for production orgs.
**Live flow (the evidence the repo counts).** On rebuilt images with a running stack: (a) owner sets role `member` allow-listed to one chat model; (b) member `GET /api/v1/models` shows only that model plus the Verevon modes; (c) member pins another model over `/api/v1/chat` stream: typed `model_not_entitled`, zero new thread or run rows; (d) member on Verevon Auto resolves inside the allowed set or gets `no_entitled_route`; (e) owner unaffected; (f) edit takes effect within the TTL; (g) stop org-core: member calls still served within the stale bound, then `model_entitlement_unavailable` 503; restart, recovers; (h) ZDR-TEST org: attempt a policy with no attested model, refused 409; (i) screenshots of the admin section and the refusal. Capture: curl and SSE transcripts, `docker compose images` digests of org-core, inference-core, model-gateway, execution-core, gateway, frontend, the `model_entitlement_total` and `inference_requests_total` series, history rows, Playwright trace. Record in `MP/MODEL_PLANE_STATUS.md` (append-only row), flip the ledger entry from proposed to implemented.
**Effort** S/M, 2-3 d active plus the shadow day. **Deps** all slices, WS01 deploy harness and ZDR-TEST org. **Risks** operators set a policy while enforcement is `off` (banner and the BFF flag prevent it).

## 5. Interfaces

**Provides**
| To | Contract |
|---|---|
| WS11 effort ladder | `ResolvedAccess::clamp_effort` and `ModelAccessMeta.max_effort`; org-core validates `max_effort` against WS11's ladder golden. WS11 takes `InferRequest` tag 17; WS10 uses none |
| WS27 cost meter / pre-run estimate | `GetModelAccess` verdict and filtered catalogue: estimates price only entitled models; refusal codes |
| WS20 scheduled runs | Entitlement applies only to user principals (D6). Scheduled or unattended steps must run under the owner's user principal, or WS20 and WS10 add an "acts for user" contract. A scheduled run on a service token escapes the policy until then |
| WS06 audit | `org_model_policy_history` rows (actor, before, after, revision); WS06 decides whether to ship them as audit events |
| WS25 org policy registry | Model lists stay in org-core. WS25 must reference `revision`, not copy lists |
| WS08 attachments | Catalogue `features` untouched; vision-capable filtering composes with the entitlement filter |
| Space session (a) | A Space's model choice must be a subset of the entitlement; Space validates at write time through org-core explain and Model Plane re-checks at run (intersection) |

**Consumes**
| From | Contract |
|---|---|
| WS01 | ZDR wave committed; `inference-core/src/{config,auth,grpc}.rs` hunk (S5) merges first; deploy harness; ZDR-TEST org; `release-artifact.sh` sole editor (secret key line); WS01 S6 on `sse.rs` first |
| WS11 | Canonical rung ids and rank |
| WS12 | UI gates must pass on the new SPA files |
| Existing | org-core service-principal registry and delegation, auth-core membership projection, inference-core privacy-tier gate |

**Conflict hot-spots and edit order**
| File | Others | Rule |
|---|---|---|
| `inference-core/src/{grpc,auth,config}.rs`, `provider/fallback.rs`, `provider/mod.rs` | WS01, WS11, WS23, WS02 | WS01 first; WS10 hunks are additive and local; proto tags per the integrator table in `WS02` D8 |
| `inference.proto` | WS11 (17), WS02 (18-19), WS08 (20) | WS10 appends new messages and an RPC only |
| `model-gateway/src/{sse,http_routes,normalize,grpc}.rs` | WS02, WS03, WS04, WS27 | S1 is small and goes first; S4 after WS01 S6 |
| `execution-core/src/runtime_loop/agent.rs` (mock only) | WS02, WS03, WS05 | add the mock method as a standalone commit that rebases trivially |
| `MP/deploy/docker-compose.yml`, `.env.example`, `release-artifact.sh` | all | follow `${VAR:-default}`; secret key line via WS01 |
| `FE/.../action-registry.ts`, `handlers.rs`, `dispatchers.rs` | WS13, WS14, WS24 | add-only blocks; registry and handler must land in one PR |
| `chat-normalizers.ts`, `DashboardComposer.tsx`, `errors.ts` | ZDR wave, WS11 | land after the ZDR wave; small case blocks |

## 6. Cross-session interfaces

- **Space session (a).** Shared piece: per-Space model default and any Space-level allow list. Owner: Space session stores the Space binding (Application/Control ACL rule 10: projections only narrow); WS10 owns the org and role policy. Assumption: the Space session does not store `allowed_models` itself, consumes org-core explain for write-time validation, and maps the typed codes (`overlap-space.md:45`). `F-t1-006` phase 2 and `F-t1-011` wait for S2 (overlap-space `:76`).
- **Quarry / browser session (b).** Quarry's planner and invoke grant call model-gateway with service credentials (overlap-quarry row 9): exempt under D6. Assumption: (b) either delivers the invoke grant with the acting user's identity or accepts the exemption in writing. No Quarry code changes in WS10.

## 7. Regression guards

| Audit section 8 strength | Touch | Gate |
|---|---|---|
| ZDR by construction | Typed refusals, never substitute, no persistence, golden rows | `model-entitlement-contract-sync`, `mp-contracts/tests/model_entitlement_matrix.rs`; existing `scripts/tests/zdr-contract-sync.test.sh` and refusal-inventory tests must stay green (untouched) |
| Intent-based routing with provenance | Candidate set constrained, cell and ladder unchanged when no policy | `provider/intent.rs` tests unchanged; new chain test "no policy is byte-identical" |
| Honest outcomes | Refusal is explicit, served-by line unchanged, no silent downgrade | admission test asserts no `StartRun`; typed code in `infer-statuses.json` |
| Typed ChatEvent taxonomy | No new variant; `error` frame reused | `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (HEAD already fails it, live passes; do not regress) |
| Two loops separate | Enforcement sits below both loops in inference-core | no change to `dispatch_tool` or `execute_step_inner`; `cross_service_loop_contract.rs` unchanged |
| Credential isolation | New narrow scope and secret partition; no end-user token reaches org-core | `service_auth_test.go`, contract test, `release-contract-test.sh` |
| Multi-tenant runtime | RLS on new tables, tenant pinned by token org | postgres RLS test, `authorize_org` |

## 8. Effort and risks

**Total** XL: 31-44 dev-days. Critical path S0, S2, S3, S4, S6, S7 about 26-37 d; S1 and the contract/golden work parallelise with S2; with three engineers (org-core, Model Plane, frontend) about 4-5 elapsed weeks plus the shadow day.

| Risk | Mitigation |
|---|---|
| Stale cache lets a revoked model through | TTL 30 s, shadow period, explain shows revision, NATS invalidation as follow-up; restrictive answers stay longer in the stale window on purpose |
| Fail-closed outage blocks chat | Mirrors auth-core's mint behaviour; stale bound; one-restart `off` rollback; shadow first |
| Live ZDR hunks in `grpc.rs`, `auth.rs`, `sse.rs`, `http_routes.rs` | Hard precondition: ZDR wave committed first; additive local hunks; WS01 edits first |
| Policy escapes via service principals or scheduled runs | D6 stated, metric `service_exempt`, WS20 interface; revisit with an acts-for-user contract |
| Admin writes while enforcement is off, or strips every ZDR model | BFF off-switch, UI banner, write guard 409, runtime typed refusal |
| Out-of-scope but adjacent: router-policy PUT has no server-side admin gate (`router_policy.rs:9-10`) | Not fixed here; entitlement makes the data path safe, the write hole needs its own item |
