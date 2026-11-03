# Model Plane asks from the Frontend and Quarry lanes (lane d, w23)

One section per unit. Each ask names the file, the exact change and why.

## W2-FE-L5a, WS15.L5 (admission contract re-vendor)

The Verevon v3 action registry changed, so the agent admission golden the SPA generates changed. The SPA copy is already regenerated and committed (`apps/Frontend Plane/verevonv3/src/shared/api/contracts/agent-action-admission.json`, LF sha256 `bcf3d6ffbf020946ceb76994bd9e6ef0cccb7e249d1ed2fa5c95318bd51c0482`). The repo-level drift check `scripts/tests/zdr-contract-sync.test.sh` fails until the Model Plane canonical copy and the three vendored copies match it.

- **Model Plane (contracts owner):** copy the SPA file byte for byte over `apps/Model Plane/api/contracts/agent-action-admission.json`; refresh the pin `apps/Model Plane/api/contracts/agent-action-admission.json.sha256` (`sha256sum`, keep the `<hash>  <name>` form).
- **Repo script:** in `scripts/tests/zdr-contract-sync.test.sh` `GOLDENS` (line 204) replace the digest `13f3b72a93c68868b6a4108e1584aec2adf4d0bf40becee878d6bf548d38eb3e` with `bcf3d6ffbf020946ceb76994bd9e6ef0cccb7e249d1ed2fa5c95318bd51c0482`.
- **Vendored copies (copy file and pin):** `apps/Model Plane/go/services/capability-core/internal/api/testdata/agent-action-admission.json`, `apps/Model Plane/rust/services/execution-core/tests/fixtures/agent-action-admission.json`, `apps/Model Plane/rust/services/model-gateway/tests/fixtures/agent-action-admission.json`. Run the three consumers' tests (the model-gateway test checks every Ask-loop read arm against the rows).
- **What changed in the rows:** `ingestions.run_schedule_action` is gone; added `ingestions.get_run_status` (side effect `read`), `pause_run`, `resume_run`, `cancel_run`, `pause_schedule`, `change_schedule_state` (all `access: none`, `exemptReason: no_owner_executor`, `external_write`); `ingestions.create_schedule` has a new schema hash. No row is eligible, so no consumer's executable set changes.

## W2-U39b, WS8.B / WS8.C (human-browse grant renewal from the Quarry run sweeper, D2-060)

Quarry's run sweeper now has a renew seam (`HumanGrantRenewer` in `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/run_sweeper.rs`) and calls it shortly before a human-browse grant expires, never past its absolute cap; a refusal closes the run as `grant_renewal_denied`. Today the edge plugs in `UnwiredHumanGrantRenewer` (always "unavailable", so nothing is closed on a renewal it could not try) because the broker side cannot serve it yet:

- **browser-broker (`apps/Model Plane/go/services/browser-broker/internal/authz/authz.go`, RenewGrant handler):** RenewGrant accepts user principals only (no service holds `browser:write`). Decide whether Quarry's service principal may renew a `human_browse` grant in the org of its token, for a grant it has validated (a scope such as `browser:renew`, kind `human_browse` only, never past `absolute_expires_at`). If the answer is no, say so: the edge then relies on the person's keep-alive through the gateway (WS8.D), and the sweeper only validates and closes.
- **Contract:** if yes, Quarry vendors `rpc RenewGrant(RenewGrantRequest) returns (RenewGrantResponse)` and the two messages (fields 1-2 each) into `crates/quarry-runtime/proto/browser.proto`, and `scripts/tests/browser-grant-contract.test.mjs` must cover the vendored subset. The edge would map `FAILED_PRECONDITION`, `PERMISSION_DENIED` and `NOT_FOUND` to denied (close the run) and `UNAVAILABLE` and `DEADLINE_EXCEEDED` to unavailable (keep the run, retry next pass). Confirm that mapping.

## W2-CP-WS7I, WS7.I (quarry token claims golden re-vendor, web_policy claim)

Auth Core now mints a `web_policy` claim beside `zdr` on `aud=quarry` and `aud=browser-broker` tokens (`apps/Control Plane/auth-core/src/auth/web-policy.ts`, `convex-token.service.ts`), so the quarry token claims golden changed. The Auth Core canonical copy and the Quarry edge copy are regenerated and committed on `w23/lane-d` (commit 752badfd5). The repo-level drift check `scripts/tests/zdr-contract-sync.test.sh` (VENDORED rows for the model-gateway copy) fails until the Model Plane copy matches.

- **Copy byte for byte (both files, LF):** `apps/Control Plane/auth-core/src/auth/fixtures/quarry-token-claims.golden.json` over `apps/Model Plane/rust/services/model-gateway/tests/fixtures/auth-core/quarry-token-claims.golden.json`, and `apps/Control Plane/auth-core/src/auth/fixtures/quarry-token-claims.golden.sha256` over `apps/Model Plane/rust/services/model-gateway/tests/fixtures/auth-core/quarry-token-claims.golden.sha256`. New golden sha256 `76cc4af7f2a2a4f2730265d19f8ec4b26dd181b048e9135b66e8369b3ae8c266` (was `ec0ae10c182493fd61f369b7a63ee6dd3156039e5230918933e5413af8beeffa`); the pin file content is exactly `76cc4af7f2a2a4f2730265d19f8ec4b26dd181b048e9135b66e8369b3ae8c266  quarry-token-claims.golden.json` plus a newline.
- **What changed in the bytes:** each of the four existing cases gains `"web_policy": {"v": 1, "rev": "0"}` as the LAST claim, after `zdr`; no other claim moved or changed (`zdr` values identical). Four cases are appended: `user_in_org_with_web_policy` (org `org-web-policy-golden`, `zdr:false`, `web_policy {"v":1,"rev":"0123456789ab","caps":{"browser.act.upload":"disabled"},"robots_max":"respect"}`), `user_in_org_without_policy` (org `org-without-web-policy-golden`, `{"v":1,"rev":"0"}`), `service_execution_core_with_web_policy` and `service_model_gateway_with_web_policy` (both org `org-web-policy-golden`, `zdr:true`, the same policy claim as the first new case).
- **Consumer check:** model-gateway `src/auth.rs` (around line 3197, `include_str!` of the golden) verifies the user body of the real mint. If its claims struct or test rejects unknown claims (`deny_unknown_fields` or an exact key-set assertion), it must accept `web_policy` (ignore it, or read it as untyped `Option<serde_json::Value>`); model-gateway does not need to apply the policy. If the test iterates every case, the four new ones must pass (two are user cases with `zdr:false`, two are service cases with `zdr:true`). Run `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib auth` after the copy.
- **Browser broker (WS7.K, later):** user tokens for `aud=browser-broker` now also carry `web_policy` (`{"v":1,"rev":"0"}` while auth-core's `WEB_POLICY_CLAIM_ENABLED` is off, the default). The broker's token verifier must tolerate the extra claim; WS7.K reads it.

## W2-CP-INNH10B, INNH-10 CP-3 (api_key_revoked consumer for the browser broker, MP-2 part)

Auth Core now publishes `verevon.audit.v2.control.auth-core.api_key_revoked` on the LOCAL control-plane JetStream audit stream (same transport and envelope as every auth-core audit event: `occurred_at, event_id, org_id, plane:"control", producer:"auth-core", event:"api_key_revoked", resource_id:<key_id>, outcome:"ok"`). Producer: `apps/Control Plane/auth-core/src/auth/api-key-metadata.ts` (`buildApiKeyRevokedEvent`), commits a09d4054c..f4e46762b on `w23/lane-c`.

- **Consumer contract:** read `details.key_id`, `details.org_id`, `details.reason` (`deleted | disabled | expired | owner_removed`). On every event revoke all live grants whose `key_id` equals `details.key_id` with reason `key_revoked` (MP-2), within the same org.
- **Delivery:** at-least-once; `event_id` is `api_key:<key_id>:revoked:<reason>[:<epoch ms>]`, stable per revocation (JetStream msgID dedup). The consumer must be idempotent, and must tolerate an event for a key that still exists: delete/disable is published before the mutation, so if the mutation fails the grants just die early.
- **Latency:** delete/disable/browser:agent removal are immediate. Expiry and owner removal come from a sweep (default every 60 s, `API_KEY_REVOCATION_SWEEP_MS`). The 120 s token bound (CP-1) still applies.
- **Transport question:** the broker lives on the Model Plane bus; this event is on the control-plane audit stream. Say whether the broker subscribes there (needs a NATS user/permission row for that subject) or whether auth-core must also forward it to the shared bus (a separate Control Plane change, not built).
- **Key ceiling for MP-2:** the key's host ceiling and budget ceiling are in the key metadata (`key_hosts`, `budget_ceiling{micro_usd,max_steps,max_runtime_s}`); CP-1 is to put `key_hosts` into the broker-audience token. The broker never reads auth-core storage.

## W2-CP-INNH10A, INNH-10 CP-1 / CP-4 (api_key token shape for the browser broker and model-gateway, MP-1 / MP-3)

Auth Core now mints `principal_type: "api_key"` tokens through `POST /api/:audience/api-key-exchange` (audiences `quarry` and `browser-broker`; dark unless `API_KEY_ENABLED=true` and `API_KEY_AGENT_EXCHANGE_ENABLED=true`). Producer: `apps/Control Plane/auth-core/src/auth/convex-token.service.ts` (`issueApiKeyAgentToken`, the only minter) and `api-key-exchange.controller.ts`, commits 1cda680be..0b27467a3 on `w23/lane-b`.

- **Claims (both audiences):** `iss`, `aud`, `sub: "apikey:<key_id>"`, `iat`, `nbf = iat - 5`, `exp <= iat + 120`, `org_id`, `principal_type: "api_key"`, `key_id` (printable ASCII, 1..128), `acting_user_id` (the key's owning user), `jti` (fresh UUID per exchange), `scopes: ["browser:agent"]`, `zdr` (org posture OR key flag), `web_policy` (always present; `{"v":1,"rev":"0"}` while `WEB_POLICY_CLAIM_ENABLED` is off). Never `user_id`, never `service_id`.
- **Broker audience only:** `key_hosts`, the key's host ceiling: 1..32 broker-canonical hosts (lower case, punycode, no wildcard, no port), sorted, de-duplicated. A `quarry` token never carries it.
- **MP-1 ask (pkg/authctx, browser-broker):** accept `principal_type=api_key` with the claims above; fail closed on a missing `key_id`, `acting_user_id` or `jti`, or a present `user_id` / `service_id`. Use `acting_user_id` as the grant owner and expose `key_id`; check grant hosts are a subset of `key_hosts`. Same RS256 JWKS as every auth-core plane token.
- **MP-3 ask (model-gateway `/v1/browser/key-grants`):** the bearer is the `browser-broker`-audience api_key token above, presented by the Verevon gateway; do not route it through the user-only `/v1/browser/grants` door.
- **Replay:** the token is a 120 s bearer with a `jti`; the broker may refuse a reused `jti` within its lifetime (optional, recommended).

## W2-DOC-A (lane c, w23): ledger entries and one gateway.proto comment (2026-11-03)

Each block is the exact text for a Model Plane file. The Quarry lane did not edit `apps/Model Plane`. Nothing below is built or deployed. The unit's status is in `.plan-work/quarry-w1w2/W23-W2-DOC-A-status.md`.

### 1. `apps/Model Plane/docs/decisions/ledger.md`: WS7.A policy owner (new entry, above `## 2026-10-27 - WS04-S9`)

```markdown
## 2026-11-03 — Org web policy has one issuer: Auth Core signs the claim, org-core stores the policy, consumers only enforce (ADR-1 outcome C)

**State:** `proposed`. This is the recommended default that the W2 units build on. The owner ruling and the Control and Model Plane sign-offs are not recorded yet.

**The decision.** The policy of record for web and browser capability is org-core. An organisation's web policy (capability switches, robots and proxy ceilings, domain allow and deny lists) is stored and versioned by org-core and read by Auth Core alone, through `GET /internal/orgs/:orgId/web-policy` (scope `org:web-policy:read:any`). Auth Core is the only issuer: it signs `web_policy` beside `zdr` on user tokens and on tenant-row service tokens for the `quarry` and `browser-broker` audiences. Quarry, the browser broker and the gateway only enforce the claim. None of them mints, re-signs or widens it, and a grant's snapshot never overrides a tighter claim.

**Options (D7).** A: claims on user tokens only, plus grant fields for Model-driven runs (two resolution paths). B: capability-core owns the policy, grant fields only; it cannot reach `/v1/scrape`, `/v1/search` or `/v1/crawl`, and cannot store a proxy ceiling or robots mode. C: org-core owns the policy; the claim rides on user tokens and org-resolved service tokens; the grant carries a snapshot.

**Recommendation and limits.** C. Claim staleness is about 345 s by default (300 s audience TTL plus 45 s resolver cache), or 105 s with `WEB_POLICY_TOKEN_TTL_CAP_SECONDS=60`, which is not applied by default. Fail mode: the last good answer for 15 minutes, then refusal (`web_policy_unavailable`, no token). Fall back to A if Control declines the registry field `webPolicyByAudience`.

**Open point.** The design text says "fail-to-floor (last good for 15 min, then the static floor)". The built behaviour in auth-core (WS7.I) is a 503 with no token once the 15 minutes end. The owner must say which one stands before `WEB_POLICY_CLAIM_ENABLED` is turned on.

**Built state.** Code only, in the W2 wave: org-core storage, admin API and internal read (WS7.H); auth-core minting (WS7.I), flag off. No deployment claim.

**Decider.** The user, with Control Plane (registry field, token lifetime) and Model Plane (grant fields) sign-off. Not yet recorded.
```

### 2. `apps/Model Plane/docs/decisions/ledger.md`: WS16 DR-15.1 (new entry, above `## 2026-10-27 - WS04-S9`)

```markdown
## 2026-11-03 — Quarry reaches the Model Plane through one allowlisted, org-bound, metered machine lane (DR-15.1)

**State:** `proposed`. Only the registry rows are built. Trigger to flip to `implemented`: slice WS16.S6 tier E2 passes on the stack, and the owner has named the owner of the lane under X-7.

**The decision.** `service:quarry-edge` may call `POST /v1/ai/chat` on the Model Plane gateway, and no other model route. Its registry principal holds two audiences: `model-gateway` (`models:invoke`) and `inference-core` (`inference:invoke`, `models:invoke`), both at `tenant` retention, so the token's `zdr` is the organisation's own posture as Auth Core resolves it at each mint. The two manifest rows are `model-extract` (the user-ingress bearer of `POST /v1/ai/chat`) and `model-extract-inference` (the `inference-core` bearer, sent as `x-inference-authorization`). Quarry sends the run's ZDR flag explicitly on each request. The gateway must meter each non-ZDR call to cost-core under the organisation, and rate-limit services in their own bucket.

**Not chosen.** `/v1/invoke` or `/v1/invoke/stream` for services (the gateway refuses service principals there, and a grant alone does not change that). A gateway-held inference principal (ambient authority). Flipping `model-invoke`, `model-stream` or `model-audio` to open a path.

**Deliberately not decided.** AI-format pricing; whether ZDR extraction is ledgered; org `injection_defense` and PII on this lane.

**Evidence owed.** The `/v1/ai/chat` service route, its golden route table and the metering envelope (WS16.S1), and the live assertion in tier E2 (WS16.S6). Until then the registry rows are the only built part.
```

### 3. `apps/Model Plane/docs/decisions/ledger.md`: WS8.A row for U03 (new entry, above `## 2026-10-27 - WS04-S9`)

```markdown
## 2026-11-03 — Shared browser sessions: human_browse and attach grants, renewal by presence, per-tab control

**State:** `proposed`. Spike facts come from code reads; the live measurements are still owed.

- Records D8-1 to D8-3, D8-5 and D8-7 of `docs/plans/quarry-browser-build-plan-2026-10-14.md`.
- Proto allocation: browser.proto numbers 50-59 belong to WS8 (`policy_mode`, `denied_domains`, `absolute_expires_at`, `resource_id`, `RenewGrant`). Confirmed unused in the Model Plane proto on 2026-10-23.
- Attach uses a service principal. The OD13 delegated bearer (`aud=quarry`) is rejected for attach. The option-2 delegation claim (RFC 8693 `acting_user_id`) is deferred.
- Spike facts (code read at HEAD 2ac764f1; live measurements pending): new tabs activate on creation; actions reach only the active tab; egress policy is per session; page hygiene applies focus emulation to every page at attach.
- Open item recorded for WS8: the SPA sends `actor:'agent'` under the user's session (`KnowledgeComposer.tsx:1050`), and the gateway does not check the caller class for agent calls. This is not allowed as a pattern for attach.
- Measurements still owed: background throttle, two-tab Chromium RSS, the F3 domain-blocked count, broker validation latency at 20 actions/s, and the three pin tests as current behaviour (the U03 hand-over lists them).
```

### 4. `apps/Model Plane/proto/model_plane/v1/gateway.proto`: comment on `ExtractStructuredResponse.extracted_json` (comment only)

The three comment lines above `string extracted_json = 6;` (lines 493 to 495) say "Always a JSON object matching `schema_json`." That is not true of `ExtractStructured` today: the handler returns the model's content unchanged (`extract_structured` in `apps/Model Plane/rust/services/model-gateway/src/grpc.rs` sets `extracted_json: infer.content` with no check). Replace those three lines with:

```proto
  // The model's output as a JSON string, as the model returned it. The gateway
  // does NOT check it against `schema_json` on this path: no validator runs
  // before the reply, and the validating lane (the web_extraction work in the
  // build plan, WS16) is not built. Callers must validate it. Empty when the
  // model failed to produce output: see `error_message`.
```

Comment only. No field, type, name or number changes.

## W2-Q-WS7K, WS7.K (broker web-policy snapshot: the Model Plane half)

Quarry's half is built on `w23/lane-d` (code only, not compiled): the vendored `crates/quarry-runtime/proto/browser.proto` already has `message WebPolicy` and `WebPolicy web_policy = 20;` on `AcquireGrantResponse` and `ValidateGrantResponse`, `GrantValidation.web_policy` carries it, and the edge reads it per step as the grant layer. Until the broker sends field 20 the edge sees no snapshot, which is no grant layer (nothing changes). The repo L1 test `scripts/tests/browser-grant-contract.test.mjs` lists `WebPolicy`, `AcquireGrantResponse.20` and `ValidateGrantResponse.20` in `PENDING_UPSTREAM_*`; empty both sets in the same change that lands the upstream fields. Asks (plan D7 S10, `docs/plans/quarry-browser-build-plan-2026-10-14.md` "Slice WS7.K detail"):

- **Proto (`apps/Model Plane/proto/model_plane/v1/browser.proto`):** add, byte-compatible with the vendored copy:
  `message WebPolicy { uint32 v = 1; string rev = 2; map<string, string> caps = 3; string robots_max = 4; string proxy_max = 5; repeated string domains_allow = 6; repeated string domains_deny = 7; }`
  and `WebPolicy web_policy = 20;` on `AcquireGrantResponse`, `ValidateGrantResponse` and `DeriveRunGrantResponse`. No field on any request: the broker derives the snapshot from the verified principal only. Regenerate Go only (`docker run --rm -v "$PWD":/w -w /w bufbuild/buf generate --template buf.gen.go-only.yaml` from `apps/Model Plane/proto`). Update the upstream number lists in the L1 test ("grant messages keep their field numbers": `AcquireGrantResponse`, `ValidateGrantResponse`, `DeriveRunGrantResponse` gain 20) and the generated-code freshness check.
- **What the fields hold (Quarry reads exactly this):** the user's `web_policy` claim (`aud=browser-broker`, minted by auth-core since WS7.I) mapped 1:1: `v` = claim `v` (1), `rev` = claim `rev` (`"0"` or 1-12 lowercase hex), `caps` = claim `caps` (capability key to mode or ceiling name, e.g. `"browser.act.upload": "disabled"`), `robots_max`/`proxy_max` = the claim's ceilings or `""` for none, `domains_allow`/`domains_deny` = claim `domains.allow`/`domains.deny` (empty allow list = no allow rule). Quarry refuses (fail closed under enforce) a `v` other than 1, a bad `rev`, an unknown mode name, or a domain key inside `caps`; it ignores and counts unknown capability keys. Unset message = no claim on the token, no grant layer; send nothing rather than an empty `WebPolicy{}` (`v = 0` reads as unreadable).
- **`apps/Model Plane/go/pkg/authctx/authctx.go`:** `Principal.WebPolicy json.RawMessage`, parsed from the `web_policy` claim next to `ZDR` (around line 63, parse 155-157); optional, since the package also serves capability-core and cost-core. The verifier must tolerate the claim on every audience.
- **`apps/Model Plane/go/services/browser-broker/internal/grant/policy.go` [NEW]:** `ClaimPolicyResolver`: `Snapshot(principal)` builds the `WebPolicy` from `Principal.WebPolicy` (nil when absent); it narrows the requested `allowed_domains` by `domains_allow` (intersection on a dot boundary), removes `domains_deny` entries, and refuses a requested host the policy denies with `ErrDomainDenied` mapped to `browser_host_not_grantable`. A forged `web_policy` in gRPC metadata or the request body is ignored.
- **`grant.go`:** `Grant.WebPolicy`; `AcquireGrant` (server.go 42-108) stores the snapshot; `DeriveRunGrant` copies the parent's and may only tighten it (never replaces a tighter parent snapshot with a looser one); `ValidateGrant` returns it.
- **Migration `apps/Model Plane/go/services/browser-broker/migrations/0002_browser_grants_web_policy.up.sql` + `.down.sql` (R-3: 0002 is reserved for this):** `ALTER TABLE browser_grants ADD COLUMN IF NOT EXISTS web_policy JSONB;` / `ALTER TABLE browser_grants DROP COLUMN IF EXISTS web_policy;`. Idempotent (the entrypoint re-applies every file on start). ZDR grants are never persisted, so they keep the snapshot in memory only. Add 0002 to the Go test helper `newTestDB` (it applies 0001, 0003, 0004 today).
- **Tests (plan names):** `TestSnapshotComesFromVerifiedPrincipalNotRequest`, `TestRunGrantSnapshotOnlyTightens`, `TestDeniedDomainIsNotGrantable`, `TestAllowListIntersectsRequestedDomains`, `TestPolicyAbsentYieldsNoSnapshot`, `TestPostgresRoundTripKeepsSnapshot` (integration tag).
- **model-gateway `browser_run.rs`:** no logic change; `POST /v1/browser/grants` may add `policy_rev` for the UI.
- **Deploy order:** broker first; Quarry reads an absent snapshot as no grant layer, so either order is safe.

## W2-AP-S7a, WS10.S7a (Application Plane browserActivity: the nats.conf subscribe grant)

The Application Plane half is built on `w23/lane-c` (code only, not compiled): `agentRuns.browserActivity` in convex-core and the `upsertBrowserActivity` internal mutation; the consumer is W2-AP-S7b. Ask (plan "Slice WS10.S7 detail", ADR-8 part 2):

- **`apps/Model Plane/deploy/nats.conf`, user `application-convex-model`:** add `"mp.v1.trace.run.*"` to `permissions.subscribe` (a plain core-NATS subscription, next to `"mp.v1.run.*.event"`). Nothing else: no publish right, no JetStream API or ACK subject, no durable. The subject's last token is the run id; the publisher is `quarry-trace-producer` (X-4 (a), publish-only on `mp.v1.trace.run.*`).
- **When:** with or before W2-AP-S7b's consumer is switched on (its subscription is off by default). Until then the grant is unused; without it the consumer gets a permissions violation and the projection stays empty, which is safe.
- **No other Model Plane change.** The convex side writes nothing for an unknown run, an org mismatch, a stale `producer_seq` or any non-label text, so the grant does not widen what reaches Convex beyond labels and counters.

## W2-FE-GW1 (lane d, w23): INNH-10 MP-3, what the Verevon gateway sends to the key-grant door

Producer: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/api_key_agent.rs` (commit 853ff3403 on `w23/lane-d`), dark behind `API_KEY_AGENT_RUNS`. Built against the design's proposal; please confirm or say what differs.

- **Acquire:** `POST {model-gateway}/v1/browser/key-grants`, header `x-browser-authorization: Bearer <browser-broker-audience api_key token>` (the CP-1 token; NO `Authorization` header, because auth-core mints no model-gateway-audience key token), `content-type: application/json`, body `{ "allowed_domains": [1..32 hosts], "budget": { "micro_usd": u64, "max_steps": u32, "max_runtime_s": u32 }, "zdr": bool }`. No `kind`, no org or user header.
- **Answer read:** `{ grant_id, expires_at, allowed_domains?, budget?, key_id }`, bare or wrapped in `{ data }`. `grant_id` required (at most 256 bytes). A `z_` grant id or `zdr: true` makes the gateway start a ZDR run. `allowed_domains` absent (ZDR): the requested hosts go to Quarry, which replaces them with the grant's. `budget`, if present, is min-ed field-wise with the request.
- **Revoke:** `DELETE {model-gateway}/v1/browser/key-grants/{grant_id}` with the same header shape (a fresh or cached broker-audience token of the same key); 2xx or 404 is success. Called on run close, on any start failure after issuance, and when Quarry reports the run gone.
- **Refusals:** a 4xx keeps its status to the caller with code `api_key_grant_refused` and `details.reason` = the door's reason when it is a snake_case token at `error.details.reason`, `details.reason`, `reason`, `error.code` or `code`. Please return the design's reasons there (for example `api_key_grant_kind_refused`, hosts outside `key_hosts`, budget over ceiling, `MaxActivePerKey`).

## W3-E3.1-ab (lane d, w23): E3.1-b broker half, the `fill_otp` sensitive action

The Quarry half is built on `w23/lane-d` (code only, not compiled): `AgentAction::FillOtpRef { snapshot_id, generation, ref_id, approval_grant_id }` (wire tag `fill_otp_ref`, no value field) and a `fill_otp` arm in `quarry-edge/src/agent_routes/sensitive_approval.rs`, dark behind `QUARRY_EDGE__FILL_OTP` (default off, refused 403). Quarry validates the child grant with `authorizes_exact_sensitive_action("fill_otp", parent_grant_id, frame_id = Some(target frame), dialog_id = None, artifact_id = None)` and burns it once per live run. Ask (plan E3.1 scope, ADR-2a ratified 2026-10-23):

- **`apps/Model Plane/go/services/browser-broker/internal/grant/grant.go`, `allowedSensitiveActions` (line 103):** add `"fill_otp": {},` next to `respond_dialog`, `upload_ref`, `download_ref`.
- **Same file, the scope switch in the sensitive-scope normaliser (line 743, `switch canonicalActions[0]`):** add
  `case "fill_otp": if len(canonicalFrames) != 1 || len(canonicalDialogs) != 0 || len(canonicalArtifacts) != 0 { return sensitiveScopes{}, errors.New("fill_otp grant requires one frame and no dialog or artifact scopes") }`
  (the same shape as `download_ref`: one frame, nothing else).
- **Single use, child of the run grant:** issue a `fill_otp` grant only as a child of the live run grant (parent id = the run grant), lifetime as short as the other sensitive grants, never with a scope above "once" (plan B7 override). Quarry enforces once per live run; the broker should refuse a second issuance for the same approval decision.
- **Binding the approval to origin and mail connection (memo W3-E3.1-f section 4):** the grant should carry, or the approval record should bind, the top origin, the frame origin and the mail connection id (provider + label shown on the card, never the address). Quarry's validator today checks action, parent, frame, dialog and artifact only; if the broker adds origin/connection scopes, tell us the field names and Quarry will add the exact-match check (E3.1-h side).
- **Risk category:** reuse `RiskCategory::Login` (`execution-core/src/browser_agent/browser_risk.rs` line 27) for `fill_otp_ref` proposals, per the plan.
- **Tests on your side:** `allowedSensitiveActions` accepts `fill_otp`; the switch refuses a `fill_otp` grant with an artifact or dialog scope or with zero/two frames; `derive_test.go`-style child-of-run-grant case.
- **When:** before `QUARRY_EDGE__FILL_OTP` is switched on anywhere. Until then Quarry refuses the action before any broker call, so nothing breaks without it.

## W3-E3.2-f (lane d, w23): quarry token claims golden re-vendor (per-origin consent defaults in web_policy caps)

The Auth Core canonical golden and the Quarry edge copy are regenerated on `w23/lane-d` (code only, not run through jest). The repo drift check `scripts/tests/zdr-contract-sync.test.sh` now pins sha256 `6c3044d5004a36f2cab3187a31847d23a6d471e1902337e0c71c9e5f34770682` (was `76cc4af7f2a2a4f2730265d19f8ec4b26dd181b048e9135b66e8369b3ae8c266`), so its VENDORED rows for model-gateway fail until the Model Plane copy matches.

- **Copy byte for byte (both files, LF):** `apps/Control Plane/auth-core/src/auth/fixtures/quarry-token-claims.golden.json` over `apps/Model Plane/rust/services/model-gateway/tests/fixtures/auth-core/quarry-token-claims.golden.json`, and the sibling `.sha256` over model-gateway's `.sha256` (content exactly `6c3044d5004a36f2cab3187a31847d23a6d471e1902337e0c71c9e5f34770682  quarry-token-claims.golden.json` plus a newline). This supersedes the W2-CP-WS7I copy ask above if that one is not done yet: copy the newest bytes once.
- **What changed in the bytes:** one case appended, `user_in_org_with_origin_consent_defaults` (org `org-origin-consent-golden`, user, `zdr:false`, `web_policy {"v":1,"rev":"abcdef012345","caps":{"browser.consent.max_scope":"conversation"}}`). No existing case changed.
- **Consumer check:** model-gateway must not reject an unknown capability key inside `web_policy.caps` (it does not need to apply the policy). If its test iterates every case, the new one is a plain user case with `zdr:false`.
- **Browser broker (later, with WS7.K / E3.2-e1 broker half):** the claim caps may now carry `browser.consent.origin` (`disabled`|`allowed`) and `browser.consent.max_scope` (`turn`|`conversation`|`always`). When the broker issues an origin-extension grant from a person's browser_consent decision (user-core, E3.2-e3), it should refuse every extension when `browser.consent.origin` is `disabled` and treat a decision whose scope is wider than `browser.consent.max_scope` as the max scope (turn under ZDR stays turn). Absent keys state nothing.

## W3-E3.4-2-3 (lane b, w23): execution-core sends `resume_run_id` for a released Quarry run

The Quarry half is built on `w23/lane-b` (code only, not compiled), dark behind `QUARRY_EDGE__RUN_TTL` (default off): when the sweeper releases a non-ZDR run on its idle or absolute TTL, its checkpoint moves to `expired` (or `suspended` when the run persisted its profile), and `POST /v1/agent/runs` with `resume_run_id` continues it (same run id, same profile, stored constraints, step keys and trace seq continue). The edge already answers a step on a released run with `404 run_not_live` and `details.resumable: true` plus `details.resume_run_id` (WS2.D). Ask (plan E3.4 scope: "execution-core quarry_agent.rs 182-192 never sends resume_run_id"; the field belongs to the Quarry client contract, R-22):

- **`apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs`, `StartRunRequest` (line 183):** add `#[serde(skip_serializing_if = "Option::is_none")] pub resume_run_id: Option<String>,` and a `start_run` variant (or parameter) that sets it. Every existing caller sends `None`, which is byte-identical to today.
- **When to resume:** on a step (or tab call) answered `404` whose `error.details.reason` is `run_not_live` and `details.resumable == true`: start again with `resume_run_id = details.resume_run_id` (never an id the model proposed), once per step, then retry the step once. A plain `404 agent run not found` (not resumable, not owned, or ZDR) is not retried.
- **Grant:** send a currently valid broker grant for the run (`grant_id`); Quarry revalidates it at resume and refuses an inactive or expired one with 403 (a resumed run needs a fresh grant, plan WS1). If the run's grant has lapsed, mint a new run grant with the same hosts first: a resume whose broker hosts differ from the stored ones is refused 403 (`resumed run domain policy no longer matches its broker grant`).
- **Never for ZDR:** do not send `resume_run_id` for a ZDR run (`zdr: true`); Quarry refuses it with 403 and a `details.zdr_refusal { action: "resume" }`, and its 404 never carries the resumable hint.
- **Refusals to map:** `409 agent run is already closed` (explicitly closed, terminal) and `409 agent run is already live` (the id is live, or another request is resuming it) are not retried; `429 RATE_LIMITED` keeps its `Retry-After`.
- **Tests on your side:** `StartRunRequest` omits `resume_run_id` when `None`; one wiremock case: step 404 `run_not_live` resumable -> start with `resume_run_id` -> step retried once; a non-resumable 404 is not retried; a ZDR run never sends it.
- **When:** before `QUARRY_EDGE__RUN_TTL` is switched on anywhere. Until then a released run's checkpoint stays `active` and is resumable exactly as today, so nothing breaks without it.
