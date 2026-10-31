# INNH-10 design: direct agent API for API-key callers (SECURITY-SENSITIVE)

Status: DESIGN, no code. Branch innh/lane-d (base quarryv2-next at 8c0884fa5), 2026-10-31.
Inputs: INNH-10-status.md (code map), and code read for this note: `crates/quarry-edge/src/auth.rs` (Claims :63, zdr_floor :127, validate_and_normalize_principal :159, require_service_route_scope :708), `agent_routes/lifecycle.rs` (bind_validated_grant :22-40, grant block :251-296, ZDR grant rule :337, run class :473-480), `agent_routes/admission.rs` (validate_run_grant :33-60, gate 10 sensitive approval :456), `agent_routes/run_registry.rs` (try_reserve_with :164, caps :200-260), `agent_routes/run_live.rs` (derive_initiator :84-95), `agent_routes/types.rs` (RunEntry :5, actor_id), `quarry-runtime/src/grant_validator.rs` (GrantValidation :25-79), `quarry-runtime/src/grpc/browser_broker.rs` (validation_from_response :315, vendored `proto/browser.proto`), `quarry-runtime/src/step_receipts.rs` (AgentIdentityBasis :299), `grant_boot.rs` (decide :95-118), `routes.rs` (service-scope layer :141 and :327); Model Plane read only: `go/services/browser-broker/internal/server/server.go` (AcquireGrant :53-157), `internal/authz/authz.go` (Authorize, OwnerFilter), `internal/server/config.go` (TTL 15 min, clamp 30 s to 1 h, MaxActivePerOwner 20), `rust/services/model-gateway/src/browser_grants.rs` (user-only door); Control Plane read only: `auth-core/src/auth/auth.ts:1111-1157` (Better Auth api-key plugin, user-keys and org-keys, header x-api-key), `auth-core/src/auth/plane-token.controller.ts:404-560` (RFC 8693 token-exchange: registered service only, `tokenExchange` registry opt-in, acting_user_id, subject zdr, 90 s TTL, jti, fail-closed durable audit).

One sentence: an API key never reaches Quarry or the broker; auth-core turns it into a short-lived token of a NEW principal type `api_key`, the broker issues an ordinary allow-list session grant stamped with the key's id, and Quarry admits a key-driven run only under such a grant, on a fenced route subset, with no human-only powers.

## 1. What a grant for an API key is

A **key grant** is an ordinary BrowserBroker session grant (kind `session`, policy mode `allowlist`) with five extra properties:

| Property | Rule |
|---|---|
| `key_id` | The id of the API key it was issued for (new grant column and `ValidateGrantResponse` field). Never the key itself. |
| `owner_id` | The user who owns the key (`acting_user_id` of the key token). Counts against that user's `MaxActivePerOwner` too. |
| Hosts | Mandatory, non-empty, at most the broker's `MaxSessionDomains`, and a subset of the key's host ceiling (OD-6). Broker `StaticPolicy` still applies (internal names, deny suffixes). Never `open_web`, never `human_browse`. |
| Budget | Mandatory claim (micro-USD, max steps, max runtime), each bounded by the key's ceiling (OD-10). Quarry always enforces it, whatever `QUARRY_EDGE__GRANT_BUDGET_CLAIM` says. |
| Lifetime | Own TTL (default 600 s, clamp 30-900 s), no renew, no derive, no sensitive-approval child (OD-9). |

ZDR: the broker floor (`req.zdr || principal.ZeroDataRetention`) applies unchanged; the key token's `zdr` is the org's standing posture raised by the key's own flag (section 5).

## 2. Who issues what (the chain)

```
API caller --x-api-key--> Verevon gateway (public ingress, registered service)
   gateway --service creds + key--> auth-core  POST /api/:audience/api-key-exchange   (ask CP-1)
           <-- two short-lived tokens: aud=browser-broker and aud=quarry, principal_type=api_key
   gateway --broker-aud key token--> model-gateway POST /v1/browser/key-grants        (ask MP-3)
           --> BrowserBroker AcquireGrant (api_key principal)                         (ask MP-1/MP-2)
           <-- grant_id (+ key_id, budget, expires_at)
   gateway --quarry-aud key token + grant_id--> Quarry POST /v1/agent/runs, /step ...  (Quarry U1-U4)
   Quarry  --its own service token--> BrowserBroker ValidateGrant on start and every step (unchanged)
   gateway --on run close / error--> RevokeGrant
```

- **Issuer of identity: auth-core only.** The key is verified once per exchange by Better Auth's api-key plugin; nothing downstream sees the key.
- **Issuer of the grant: BrowserBroker only** (unchanged rule: it is the only grant issuer). Quarry never issues grants and gains no `browser:write`. Quarry stays a consumer, as today.
- **Composer: the Verevon gateway** (recommended, OD-3): the caller sends one request with hosts, budget and task; the gateway exchanges, acquires, starts the run and revokes on close. The caller never needs a grant id. Mirrors the human path (gateway `browser.rs:4470-4740` issues on session create, revokes on close).

## 3. How an API key becomes a principal (D1 resolved: a distinct principal type)

Recommended (OD-1): a **new principal type `api_key`**, not a user-shaped token (D1a as written) and not a service acting for the user (D1b).

Why not user-shaped: every plane would read it as a person. In Quarry alone 24 `is_service()` branches treat "not a service" as a person (initiator `Human`, profile owner stamping, human_browse admission, attach and takeover, interactive run class). A key token that looks like a user inherits every human-only power by default: fail-open.
Why not service-on-behalf-of: it reuses token-exchange, but Quarry today does not bind a service to the grant's owner (pinned gap `start_run_grants.rs:397`), and the gateway's service id would own the run, so audit and run scoping would not name the key.
A new type fails closed wherever it is not taught: Quarry's `validate_and_normalize_principal` returns `Err` for an unknown `principal_type` (auth.rs:181), and Go `authctx` callers that switch on `PrincipalType == "user"` fall into the scoped-service branch (`authz.go` Authorize, OwnerFilter) where the key holds no `browser:write`.

**Key token shape** (minted by auth-core, ask CP-1):

| Claim | Value |
|---|---|
| `sub` | `apikey:<key_id>` |
| `principal_type` | `api_key` |
| `key_id` | Better Auth key id (bounded printable, at most 128 bytes) |
| `acting_user_id` | the key's owning user (same claim name the token-exchange route already mints) |
| `user_id`, `service_id` | absent |
| `org_id` | the org the key is bound to at creation (key metadata); the exchange refuses another `orgId` and refuses if the user is no longer an active member |
| `aud` | `quarry` or `browser-broker` (one token per audience) |
| `scopes` | `browser:agent` only, and only if the key holds that permission (OD-5) |
| `zdr` | org posture OR key flag (raise only) |
| `web_policy` | the org web-policy claim, as for a user token |
| `key_hosts` (broker aud only) | the key's host ceiling, for the broker's subset check |
| `exp` | at most 120 s after issue (OD-9); fresh `jti` |

**Quarry normalization** (U1): for `api_key`, require `key_id` non-empty and bounded, `sub == "apikey:" + key_id`, `acting_user_id` non-empty, `user_id` empty, `service_id` absent; then set `user_id = sub` (exactly the service pattern). So `actor_id()` is `apikey:<key_id>`: runs started by a key are scoped to that key (`RunEntry.actor_id`), never to the owning person; a person's session cannot step a key's run, nor a key a person's run. `key_owner_user_id()` returns `acting_user_id` and is used only for the grant owner check and attribution.

## 4. Scope, routes and allowed hosts

- Token scope: `browser:agent` (new). Not `browser:execute`, which is the Model Plane service scope read by `require_service_route_scope`.
- **Route fence** (U1), a middleware beside `require_service_route_scope` on both layers (routes.rs :141, :327). For a key principal: flag off -> 403 `api_key_agent_runs_disabled` on EVERY route; no `browser:agent` -> 403 `api_key_scope_missing`; route not on the allow-list -> 403 `api_key_route_refused`. Allow-list (method + path, nothing else):
  - `POST /v1/agent/runs`
  - `POST /v1/agent/runs/:run_id/step`
  - `GET  /v1/agent/runs/:run_id/receipts`, `GET .../egress-receipts`
  - `GET, POST /v1/agent/runs/:run_id/tabs`, `POST .../tabs/:tab_id/select`, `DELETE .../tabs/:tab_id`
  - `GET  /v1/agent/runs/:run_id/events/stream` (closed at the token's `exp`, U3)
  - `DELETE /v1/agent/runs/:run_id`
  - Refused by omission: `/interact`, `browser-session*` (control, takeover, timeline), `attach*`, `live-view`, `frame`, `frames/*`, `devtools`, `procedure*`, `tabs/:id/control`, `/v1/profiles*`, `/v1/artifacts*` (OD-13), and every scrape, crawl, search, extract, job, schedule, change and team route.
- Hosts: the broker is the only host authority. For a key run the grant's canonical hosts ALWAYS replace the body's `allowed_domains`, in every environment (today only when grants are required or the grant carries hosts, lifecycle.rs:36). Grant hosts must be a subset of the key's host ceiling (broker, MP-2). The egress broker and web policy stay on the path unchanged (SSRF guard, private targets).
- Sensitive actions: refused for keys (403 `api_key_sensitive_action_refused` at admission gate 10, before any broker call), see section 10.
- Profiles and resume: `profile_id`, `persist_profile` and `resume_run_id` are refused for keys in v1 (403 `api_key_profile_refused`, `api_key_resume_refused`, OD-14). A key run always gets a fresh, run-scoped profile.

## 5. ZDR propagation

| Hop | Rule |
|---|---|
| auth-core | `zdr = org standing posture OR key.metadata.zdr`. A key can raise, never lower. |
| broker | Floor unchanged: `req.zdr OR principal.ZeroDataRetention`; `z_` grant id; hosts omitted from logs and from the door's response under ZDR (as `browser_grants.rs` does). |
| Quarry | `zdr_floor()` counts for `api_key` (it carries the org's posture like a user token, unlike a service's registry row). A ZDR grant cannot back a non-ZDR run (lifecycle.rs:337, unchanged). Under ZDR: no checkpoint, no resume, no persistent profile, metadata-only receipts, URL-bearing log events dropped (zdr_log). |
| models | Quarry sends nothing to a model on these routes (the caller drives each step). Unchanged. |
| ZDR matrix | New row `agent_run_api_key` in `crates/quarry-core/src/zdr_matrix.rs` (U4): same persistence as `agent_run`, plus "no resume, no persistent profile, no artifacts route, in every posture". |

Recommended (OD-12): ZDR orgs may use keys; key runs in a ZDR org are ephemeral.

## 6. Audit trail

Nothing logs the key, a token, or (under ZDR) a host.

| Event | Where | Fields |
|---|---|---|
| `api_key_token_exchanged` / `_denied` | auth-core, durable NATS audit, fail-closed like token-exchange (:538-555) | key_id, org_id, acting_user_id, audience, scopes, zdr, outcome, deny reason, calling service |
| `api_key_revoked` | auth-core, durable | key_id, org_id, actor, reason (delete, disable, expire, owner left org) |
| grant issued / denied / revoked / expired | broker `logGrantIssued` + telemetry, plus key_id | grant_id, key_id, owner_id, org_id, zdr, budget, hosts (omitted under ZDR), revoke reason (`key_revoked`, `run_closed`) |
| `agent.run.started` / refused | Quarry structured log (U2) | run_id, org_id, auth_method=`api_key`, key_id, grant_id, zdr, class, refusal reason. No hosts under ZDR. |
| step receipts | Quarry receipts (U3) | initiator `agent`, new `AgentIdentityBasis::ApiKey`, principal_id `apikey:<key_id>` |

Quarry has no durable audit publisher today; a durable Quarry audit stream is a depth finding, not part of this item. The durable trail is auth-core (who got a token) plus broker (who got a grant); Quarry's receipts name the key per step.

## 7. Revocation (key revoked => grants die)

Three independent kills, each bounded:
1. **Token kill (no new steps):** the exchange refuses a revoked, disabled or expired key. Every Quarry request needs a token at most 120 s old, so a revoked key's steps stop within the token TTL even if nothing else works. The gateway may cache an exchanged token only up to its `exp`, keyed by a hash of the key.
2. **Grant kill (broker):** on `api_key_revoked` the broker revokes every live grant with that `key_id` (reason `key_revoked`, ask MP-2). Quarry revalidates on start, every step (admission.rs:33) and new tabs, so the run fails its next step with `browser grant is inactive or expired`.
3. **TTL kill:** a key grant lives at most 900 s and never renews; a key run's `max_runtime_s` is capped by the grant's budget claim.

A key run that stops stepping is reclaimed by the existing idle sweeper. Quarry also checks on every step that the validated grant's `key_id` still equals the run's (U3), so a grant swapped under a run is refused.

## 8. Rate and concurrency limits

| Limit | Owner | Recommended default (OD-11) |
|---|---|---|
| Exchange rate per key | auth-core (Better Auth key rate limit; a separate counter for browser:agent) | 600 per hour |
| Live key grants per key | broker `MaxActivePerKey` (new) | 2 |
| Live key runs per key | Quarry `QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY` (new, U3), 429 `per_key_run_cap` with Retry-After | 2 |
| Org caps, plan caps, agent allowance | Quarry, unchanged: a key run is ALWAYS class `Agent` (`org_run_cap`, `org_plan_cap`, 402 `org_allowance`) | as configured |
| Spend per run | grant budget claim, always enforced for keys | key ceiling |
| Ingress rate per key | gateway (ask GW-1) | gateway's choice |

## 9. What a leaked key can do, and how it is bounded

With everything above in place, a holder of a leaked `browser:agent` key can, until the key is revoked: start at most 2 concurrent runs in the key's org, on hosts inside the key's host ceiling only, each within the key's budget ceiling and at most 15 minutes; read what the browser sees on those hosts (observations in step responses); and perform non-sensitive actions there (navigate, click, type, submit an ordinary form). Its spend is charged to the org's agent allowance.

It cannot: reach any other host or the open web; reach internal or metadata hosts (broker policy + egress guard); run a sensitive action; touch a person's runs, sessions, profiles or cookies; attach, take over or live-view; read org artifacts by id; resume or persist; renew a grant; act in another org; mint tokens without the gateway's service credentials (the exchange requires both); call any non-agent Quarry route.

Threat model (each row gets a negative test in U1-U3, or a broker/auth-core test in the asks):

| Threat | Bound | Test |
|---|---|---|
| Leaked key used from elsewhere | host ceiling, budget, 2 runs, revoke kills within 120 s; IP allow-list later (depth) | U2 hosts replaced; U3 per_key_run_cap |
| Leaked key token (120 s) | aud-bound, short exp, jti | existing expired-token 401 |
| Stolen grant id used by another key or a person | grant `key_id` and `owner_id` must match the caller; a person cannot present a key grant (owner rule + key_id) | U2 `api_key_grant_owner_mismatch`, both directions |
| Cross-org grant id | `validate_for_org` with the run's org (existing) + broker org rule | existing `start_run_grants.rs:507` + U2 key variant |
| Key rotated or revoked mid-run | token kill + broker kill + per-step key_id check | U3 revoked on next step |
| Grant outlives key | broker revokes on event; TTL at most 900 s | MP-2 test |
| Key escalates to human powers | distinct principal type, route fence, initiator Agent | U1 fence table; U3 initiator |
| Key steps a person's run (same owner) | actor_id is `apikey:<key_id>` | U3 404 both ways |
| SSRF via allow-list | broker policy at issue, egress guard per request (existing) | existing browser_egress_boundary + MP-2 |
| open_web / human_browse grant presented by a key | refused | U2 `api_key_open_web_refused` |
| Sensitive action without a human | refused before the broker | U3 `api_key_sensitive_action_refused` |
| Budget-less grant | refused | U2 `api_key_grant_budget_required` |
| Flag off but a key token arrives | every route 403 | U1 |
| Non-production edge with no broker (Noop validator) | flag on refuses boot without a real validator | U2 grant_boot test |
| Long-lived SSE outlives token | stream closed at token exp | U3 |
| Free-tier abuse | exchange rate, plan caps, allowance | CP-1, existing caps |

## 10. How this differs from the human browser grant (SC1 posture)

| | Human grant (SC1, live-proven 2026-10-29) | Key grant |
|---|---|---|
| Principal | user session (`user_id`), browser bearer delegated by the gateway | `api_key` principal, `actor_id = apikey:<key_id>` |
| Issuer door | model-gateway `/v1/browser/grants`, user only | model-gateway `/v1/browser/key-grants`, api_key only (MP-3) |
| Enforcement | strict in production (gateway `BROWSER_GRANT_ENFORCEMENT=strict`, Quarry `REQUIRE_BROWSER_GRANTS` implied); optional outside | **always strict for keys**, every environment, independent of `REQUIRE_BROWSER_GRANTS` |
| TTL | broker default 15 min (gate stack 45 s); renew allowed (WS8.C) | 10 min default, 15 max, no renew |
| Hosts | user's request, broker policy | request within the key's host ceiling, broker policy |
| Kinds | session, run (derived), sensitive_approval, human_browse | session only |
| Sensitive actions | allowed with a single-use sensitive_approval grant AND a signed ask the person approved (execution-core `verify_ask`, v2 claims: policy_source, tool, user) | **refused**: there is no person to sign an ask |
| Budget claim | optional (`QUARRY_EDGE__GRANT_BUDGET_CLAIM` ignore or enforce) | mandatory, always enforced |
| Profiles, resume, attach, takeover, live view | allowed per existing guards | refused |
| Revocation | session close, TTL | key revoke event, token TTL, grant TTL, run close |
| Initiator in receipts | Human, or Agent by hint | always Agent, basis ApiKey |

## 11. Quarry-side build (now, behind `QUARRY_EDGE__API_KEY_AGENT_RUNS`, default 0)

Flag: `QUARRY_EDGE__API_KEY_AGENT_RUNS` = `0|1|true|false`, default 0, an invalid value fails boot. Read once through a `OnceLock` in `run_limits.rs` (`parse_api_key_agent_runs`, `api_key_agent_runs_enabled()`), the `human_browse_enabled()` pattern. Not set in any compose file. Second knob `QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY` (default 2, minimum 1) in `RunLimits::from_lookup`.
All tests through `qtest-lane.sh d '... cargo test -p quarry-edge --features browser-agent ...'` (never cargo on the host). Integration tests that need the flag on follow the env-before-first-read pattern of `tests/human_browse.rs` (own test binary). Each unit is one commit `feat(quarry-edge): ... (INNH-10 Un)`.

### U1: principal and route fence (`crates/quarry-edge/src/auth.rs`, `run_limits.rs`, `routes.rs`)
1. `Claims`: add `#[serde(default, skip_serializing_if = "Option::is_none")] key_id: Option<String>` and `acting_user_id: Option<String>`. Legacy tokens deserialise unchanged.
2. `validate_and_normalize_principal` (:159): new arm `Some("api_key")` with the rules of section 3; set `user_id = sub`.
3. Methods: `is_api_key()`, `key_id() -> Option<&str>`, `key_owner_user_id() -> Option<&str>`, `is_person()` (not service and not api_key). `zdr_floor()` (:127): counts api_key as it does a user (`!is_service()` already does; add a doc line and a test that pins it).
4. Constant `SCOPE_BROWSER_AGENT = "browser:agent"`. Middleware `require_api_key_route_fence` (allow-list of section 4, matched on method + path segments, anything unknown fails closed), layered next to `require_service_route_scope` at routes.rs :141 and :327. Refusal body: the typed error envelope with `error.details.reason` = `api_key_agent_runs_disabled`, `api_key_scope_missing` or `api_key_route_refused` (helper `api_key_refusal(reason)` in auth.rs).
5. `require_service_route_scope` unchanged (a key is not a service).

Tests: auth.rs unit tests (api_key valid; missing key_id; sub mismatch; user_id present; service_id present; missing acting_user_id; key_id over 128 bytes; legacy user and service tokens unchanged; actor_id is `apikey:<id>`; zdr_floor true for a ZDR key token). New `tests/api_key_fence.rs`: flag off -> a table of 10+ routes all 403 `api_key_agent_runs_disabled`; flag on without scope -> 403 `api_key_scope_missing`; flag on with scope -> every refused route of section 4 is 403 `api_key_route_refused`, allow-listed routes pass the fence (reach the handler; 404 for an unknown run is fine); user and service tokens are unaffected by the fence on the same routes.

### U2: start-run binding (`agent_routes/lifecycle.rs`, `quarry-runtime/src/grant_validator.rs`, `quarry-runtime/src/grpc/browser_broker.rs`, `grant_boot.rs`, `agent_routes/types.rs`)
1. `GrantValidation`: add `#[serde(default)] key_id: String`. The HTTP/JSON validator reads it now. gRPC: `validation_from_response` maps it once the vendored `proto/browser.proto` carries the field (MP-2 picks the field number; vendor it then). Until then the gRPC path yields an empty `key_id`, so every key run is refused (`api_key_grant_owner_mismatch`): fail closed, and stated in RUNBOOKS.
2. New `bind_key_grant(grant, claims) -> Result<Vec<String>, &'static str>` beside `bind_validated_grant` (:22): kind must be `""` or `session` (else `api_key_grant_kind_refused`); `kind == human_browse` or `policy_mode == open_web` -> `api_key_open_web_refused`; `grant.key_id == claims.key_id()` AND `grant.owner_id == claims.key_owner_user_id()` (else `api_key_grant_owner_mismatch`); hosts non-empty and canonical (`canonical_broker_domains`), always returned so they replace the body's; budget claim set (else `api_key_grant_budget_required`).
3. In `start_run`, for `claims.is_api_key()`, ahead of the grant block (:251): refuse `resume_run_id` (`api_key_resume_refused`), `profile_id` or `persist_profile` (`api_key_profile_refused`); require a grant_id regardless of `require_browser_grants` (`api_key_grant_required`); use `bind_key_grant`; never enter the human_browse branch; treat the budget claim as enforced regardless of `current_claim_mode()`; classify the run `Agent` (:477: `claims.is_service() || claims.is_api_key() || ...`). All refusals 403 with `details.reason`.
4. `RunEntry` (types.rs:5) gains `key_id: Option<String>`, set at start; not written to the durable checkpoint (resume is refused for keys).
5. `grant_boot::decide`: new input `api_key_runs`; flag on with `ValidatorChoice::Noop` -> new refusal `ApiKeyRunsWithoutValidator` (the edge refuses to boot).
6. Log `agent.run.started` with `auth_method`, `key_id`, `grant_id`, `zdr`, `class` (no hosts under ZDR; the existing zdr_log filter applies).

Tests: new `tests/api_key_agent_runs.rs` (support/agent_harness.rs and the FakeBroker of start_run_grants.rs): no grant_id with `REQUIRE_BROWSER_GRANTS=0` -> 403 `api_key_grant_required`; grant of another key (same owner) -> mismatch; grant of the owner without key_id -> mismatch; human_browse grant -> `api_key_open_web_refused`; open_web policy -> same; run-kind grant -> `api_key_grant_kind_refused`; no budget -> `api_key_grant_budget_required`; body hosts wider than the grant -> run uses grant hosts; profile_id, persist_profile, resume -> refused; ZDR key token with a non-ZDR grant -> run is ZDR; ZDR grant with a non-ZDR request from a non-ZDR key -> 403 (existing rule); run class Agent (the org agent cap counts it); user and service starts unchanged (rerun start_run_grants.rs, browser_grants_prod.rs, grant_budget_claim.rs, human_browse.rs green). `grant_boot.rs` unit tests: new refusal plus every existing case unchanged.

### U3: per-step and per-key rules (`agent_routes/admission.rs`, `run_registry.rs`, `run_limits.rs`, `run_live.rs`, `quarry-runtime/src/step_receipts.rs`, the events stream handler)
1. `validate_run_grant` (admission.rs:33): when the run has a `key_id`, also require `grant.key_id == entry.key_id` (`SecurityBlocked`, "browser grant no longer belongs to this key"). Key runs always compare the broker policy (as `require_browser_grants` does today).
2. Gate 10 (admission.rs:456): an api_key caller for whom `sensitive_approval_for_action` returns Some -> 403 `api_key_sensitive_action_refused` before any broker call.
3. Run scoping: confirm every allow-listed handler compares `entry.actor_id` with `claims.actor_id()`; where one checks only the org, add the actor check for api_key callers (404, the existing not-found shape).
4. `derive_initiator` (run_live.rs:84): api_key -> `StepInitiator::Agent`; `AgentIdentityBasis::ApiKey` (new variant, serde name `api_key`) with `principal_id = apikey:<key_id>`. Check receipt and timeline consumers tolerate the new variant (grep `AgentIdentityBasis`).
5. Per-key cap: `RunLimits.max_runs_per_key`; `try_reserve_with` gets `key_id: Option<&str>` and counts live plus pending runs with that key_id (a pending map like `pending_org`); 429 `per_key_run_cap`, scope `key`, Retry-After from `cap_retry_after_s`.
6. Events stream: for an api_key caller, end the stream at the token's `exp`.

Tests (`tests/api_key_agent_runs.rs` and run_registry unit tests): broker revokes after start -> next step 403; validator returns another key_id -> step refused; sensitive dialog response -> 403 reason; a person token of the key's owner cannot step the key's run and the key cannot step the owner's run (404 both); receipts carry basis `api_key`; a third concurrent run of one key -> 429 `per_key_run_cap`, another key of the same org unaffected; closing a run frees the slot; stream ends at exp (short-exp token).

### U4: docs and ZDR matrix
- `docs/openapi.yaml` (:467-524): security note on `startAgentRun` for the `api_key` principal; the new 403 reasons and the 429 `per_key_run_cap`; a note on the step routes.
- `docs/CONTRACTS.md`: new section "API-key agent runs (INNH-10)": consumed token shape (section 3), consumed grant fields (`key_id`, `owner_id`, `budget`, `policy_mode`), route allow-list, refusal reasons, what is refused and why, flag.
- `docs/ERROR_REASONS.md`: the 11 reasons (`api_key_agent_runs_disabled`, `api_key_scope_missing`, `api_key_route_refused`, `api_key_grant_required`, `api_key_grant_kind_refused`, `api_key_open_web_refused`, `api_key_grant_owner_mismatch`, `api_key_grant_budget_required`, `api_key_profile_refused`, `api_key_resume_refused`, `api_key_sensitive_action_refused`) plus `per_key_run_cap`.
- `docs/RUNBOOKS.md`: enablement order (auth-core CP-1 flag, broker MP flag, then the Quarry flag), the boot refusal, kill switches (revoke the key; set the Quarry flag to 0), the gRPC `key_id` fail-closed note.
- ZDR matrix row `agent_run_api_key` in `crates/quarry-core/src/zdr_matrix.rs`; regenerate `crates/quarry-runtime/contracts/zdr-feature-matrix.json` + `.sha256` + `docs/ZDR_MATRIX.md` with the existing update mechanism, then rerun the check without it. Run `node --test scripts/tests/*.test.mjs` for the openapi parity tests.

Order: U1, U2, U3, U4 (each builds on the previous). Estimate 2.5-3 days for Quarry. The whole path stays dark until CP-1, MP-1..3 and GW-1 exist and the owner turns every flag on.

## 12. Asks to other owners (precise)

**Control Plane, auth-core**
- CP-1 Route `POST /api/:audience/api-key-exchange`, audiences `quarry` and `browser-broker` only. The caller must present service credentials (`x-service-id`, `x-service-api-key`) of a registry principal whose new field `apiKeyExchange` lists the audience (only the Verevon gateway). Body `{ apiKey, orgId, scopes[] }`. Verify with Better Auth (`verifyApiKey`: enabled, not expired, rate limit consumed), configId `user-keys` only (OD-4), key's `org_id` metadata == `orgId`, owner still an active member, requested scopes a subset of the key's permissions. Mint the token of section 3 (TTL at most 120 s, `jti`, zdr = org OR key flag, `web_policy`, `key_hosts` for the broker audience). Durable audit `api_key_token_exchanged` / `_denied`, fail-closed (as `issueTokenExchange`). Never log the key. Flag `API_KEY_AGENT_EXCHANGE_ENABLED` default false (on top of `API_KEY_ENABLED`). Tests: org mismatch, missing permission, org-key refused, disabled flag, expired or disabled key, TTL cap, audit failure -> 503, zdr raise-only.
- CP-2 Key creation and update: `org_id` binding (required for browser keys), permission `browser:agent` (default none; who may grant it, OD-5), host ceiling (at most 32 hosts, broker-normalized), budget ceiling, `zdr` flag, expiry at most 90 days (OD-15).
- CP-3 Durable event `verevon.audit.v2.control.auth-core.api_key_revoked` `{key_id, org_id, reason}` on delete, disable, expiry and the owner's removal from the org.
- CP-4 Teach `issuePlaneToken` the `api_key` principal type (a new `principalType`) so no other code path can mint it.

**Model Plane, BrowserBroker + model-gateway**
- MP-1 `pkg/authctx` accepts `principal_type=api_key` with `key_id` and `acting_user_id`; `ActorID` = `acting_user_id` for owner accounting, `KeyID` exposed. `authz.Authorize`: api_key may call `AcquireGrant` (grant_kind empty, allowlist mode, no sensitive scopes) and `RevokeGrant` (its own key's grants only, filtered by key_id); refuse `RenewGrant`, `DeriveRunGrant`, human_browse and sensitive approvals; scope `browser:agent` required.
- MP-2 Grant row `key_id` (migration); `ValidateGrantResponse.key_id` (new optional field; tell Quarry the number so it can vendor `browser.proto`); for key grants: hosts non-empty and a subset of the token's `key_hosts`, budget mandatory and within the ceiling, TTL `BROWSER_BROKER_KEY_GRANT_TTL_SECONDS` (default 600, clamp 30-900), `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_KEY` (default 2), a consumer of CP-3 that revokes every live grant of the key (reason `key_revoked`); `logGrantIssued` carries key_id. Flag `BROWSER_BROKER_KEY_GRANTS_ENABLED` default off. Table tests plus a Postgres integration test.
- MP-3 model-gateway door `POST /v1/browser/key-grants` and `DELETE /v1/browser/key-grants/:grant_id`: accepts an api_key-principal browser-broker bearer, same bounds as `/v1/browser/grants`, no `kind`, no renew; hosts omitted from the response under ZDR. Response `{ grant_id, expires_at, allowed_domains?, budget, key_id }`.

**Frontend Plane, Verevon gateway (Innhenting session)**
- GW-1 Public ingress for `x-api-key` on a narrow route set (the path is the Innhenting session's choice): exchange per request (cache at most to `exp`, keyed by a hash of the key, never the key), acquire the key grant, start and step the Quarry run, revoke on close or failure; strip any caller `Authorization`, `x-browser-authorization` or scoping header; per-key ingress rate limit; never forward the raw key anywhere but auth-core.
- GW-2 (optional) key-management UI for `browser:agent`, host ceiling, budget ceiling.

Proposed caller-facing shape for GW-1 (the gateway owns it; Quarry's contract stays `POST /v1/agent/runs` + `grant_id`): request `{ allowed_domains[], budget {micro_usd, max_steps, max_runtime_s}, zdr?, start_url?, initiated_by: "agent" }` with header `x-api-key`; response `{ data: { run_id, expires_at, allowed_domains? (omitted under ZDR), budget } }`; then `POST .../runs/{run_id}/step` and `DELETE .../runs/{run_id}`; refusals `{ error: { code, message, details: { reason } } }` with the reasons of U4.

## 13. Owner decisions

1. **OD-1 Principal model:** new `api_key` principal type (recommended) vs user-shaped token (D1a) vs service on behalf of the user (D1b; would first need the `start_run_grants.rs:397` gap closed).
2. **OD-2 Who may exchange a key:** only the Verevon gateway with its service credentials (recommended) vs the key holder directly at auth-core.
3. **OD-3 Grant handling:** the gateway composes (acquire on start, revoke on close, the caller never sees a grant; recommended) vs a public key-grant endpoint the caller manages.
4. **OD-4 Key kinds:** user-keys only in v1 (recommended); org-keys later, only with a named owning user.
5. **OD-5 Permission:** explicit `browser:agent` on the key, default off (recommended); who may grant it (recommended: org admin or owner only).
6. **OD-6 Host ceiling:** mandatory per key (recommended), at most 32 hosts; subdomain wildcards (recommended: no).
7. **OD-7 Open web for keys:** never (recommended).
8. **OD-8 Sensitive actions for keys:** refused in v1 (recommended); an out-of-band owner approval flow would be a separate design.
9. **OD-9 Lifetimes:** key grant 600 s default, 900 s max, no renew; exchanged token at most 120 s (recommended).
10. **OD-10 Budget ceiling per key:** mandatory; default numbers (proposal: 1 USD, 50 steps, 600 s per run) and who sets them (key creator, capped by the org plan).
11. **OD-11 Rates and concurrency:** 2 live runs and 2 live grants per key, 600 exchanges per hour per key (proposal).
12. **OD-12 ZDR orgs:** may use keys, runs ephemeral, the key may raise ZDR (recommended).
13. **OD-13 Artifacts route for keys:** excluded in v1 (recommended) until an artifact-to-run binding lets Quarry check the run's key.
14. **OD-14 Resume and persistent profiles for keys:** refused in v1 (recommended).
15. **OD-15 Key lifetime and network bounds:** maximum key expiry (proposal 90 days); IP allow-list per key (depth, not v1).
16. **OD-16 Revocation:** event-driven broker revoke on `api_key_revoked` plus the 120 s token bound (recommended) vs the broker asking auth-core on every validate.
17. **OD-17 Billing attribution per key:** whether usage events carry key_id for per-key cost (Control Plane metering; depth unless wanted now).

Next step after owner approval: one Opus security review of this note, then U1-U4 by a Sonnet builder.
