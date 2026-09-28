# Design 2 — Token Exchange (RFC 8693‑style) for the two identity gaps

Read-only design, grounded in the current code at `C:/dev/CoresSystem` (no files changed). Builds directly on the "Identity Flow Map" supplied in the task and on the three scratchpad reports (`security.md`, `schedule.md`, `mail-task.md`). All line numbers were re-read from the live files during this pass.

## 0. The one new primitive, used twice

Everywhere in this codebase that already binds an end user across a service boundary does it by **re-verifying a signature the same authority already produced**, never by trusting a caller's bare claim:

- Auth-core's RS256 keypair (single keypair, JWKS at `/api/convex-auth/jwks`, loaded in `ConvexTokenService.loadKeyPair()`, `apps/Control Plane/auth-core/src/auth/convex-token.service.ts:743-815`) signs every audience-scoped user/service token.
- Control's Ed25519 keypair (`CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64` in user-core / `CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON` in session-core, `apps/Control Plane/user-core/internal/spaces/decision.go:12-55`, `apps/Model Plane/rust/services/session-core/src/grpc.rs:1397-1441`) signs "Control decision" tokens already consumed by six different claim families (`Decision`, `RunActionDecision`, `OwnerGrantDecision`, `ModelActionView`, the scheduled-run/personal-thread decisions).

**Design 2 adds no new key material to either gap.** Gap A is a real RFC 8693 token exchange against auth-core's *existing* keypair (a new endpoint that re-verifies a subject token auth-core itself issued, and mints a new, narrower one). Gap B is a new claim family signed with Control's *existing* Ed25519 keypair, verified with the verification function session-core already has wired to that key set.

---

## 1. Gap A — execution-core → integration-corev2 (personal-connection reads)

### 1.1 What's actually missing

`integration-corev2`'s new owner check is precise, not blunt: writes already carry a real, cryptographically-bound acting user via the provider-write attestation (`apps/Ingestion Plane/integration-corev2/internal/api/connection_access.go:46-52`, `attestation.rs`'s `actor_id` — execution-core's `execute_action` already signs this at `integration_tools.rs:365-380`). The gap is the **read-only paths**: `list_provider_actions`, `provider_key_for_connection`, `read_action_json`, and the read branch of `execute_action` (no `approval_ref`) — all four call `mint_ingestion_token` (`integration_tools.rs:503-539`), whose request body is `{orgId, scopes, reason}` only (confirmed again at `plane-token.controller.ts:90-94`'s `PlaneInternalTokenBody`). `clients.go`'s `VerifyToken` (`apps/Ingestion Plane/integration-corev2/internal/controlplane/clients.go:167-183`) already reads a defensive `acting_user_id` claim for `principal_type=="service"` tokens — **it needs no change**. That's the whole gap: nothing ever puts that claim on the wire.

### 1.2 New token: the **delegated ingestion token**

Minted by a new auth-core endpoint, `POST /api/:audience/token-exchange`, sibling to the existing `POST /api/:audience/internal-token` (`plane-token.controller.ts:230-333`), added for `audience=ingestion` first.

**Request** (RFC 8693 shapes, service-principal-authenticated the same way `/internal-token` is — `x-service-id`/`x-service-api-key` against `PLANE_SERVICE_PRINCIPALS_JSON`/`_FILE`):

```
POST /api/ingestion/token-exchange
x-service-id: model-execution
x-service-api-key: <existing INGESTION_SERVICE_API_KEY>
{
  "orgId": "org_...",
  "scopes": ["integration:read"],
  "reason": "execution-core integration:read",
  "subjectToken": "<the caller's own already-verified aud=session-core bearer>",
  "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt"
}
```

**`subjectToken` is execution-core's own `x-session-authorization` delegated bearer** — the one it already holds per RunAgentRequest/ExecuteStepRequest as `DelegatedSessionBearer` (`apps/Model Plane/rust/services/execution-core/src/auth.rs:147-163, 238-252`), already independently re-verified at ingress against the SAME caller's `org_id`/`user_id`/`zdr` by every sibling `authenticate_delegated_*` (pattern at `auth.rs:421-520`), and already forwarded all the way down into the tool-dispatch layer — `execute_provider_action`'s existing `session_bearer: Option<&str>` parameter (`apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs:1606-1614`, and threaded from the dispatcher at `runtime_loop/mod.rs:640-715`). Using this token as the RFC 8693 subject token means auth-core is re-verifying a signature **it itself produced minutes earlier for this exact run**, not trusting execution-core's bare say-so.

**auth-core's new verification** (`ConvexTokenService`, new method `verifyPlaneSubjectToken(token, allowedAudiences)`): same signature/kid check as `verifyPlaneServiceToken` (`convex-token.service.ts:617-716`) but for a `principal_type=="user"` token whose `aud` is in a small allow-list (`["session-core", "execution-core"]` for the `ingestion` target) instead of one fixed value; requires `nbf<=now<exp` (i.e. **currently valid**, not just well-formed) and returns `{orgId, userId, zdr}`.

**Authorization gate (new, opt-in registry field)** — `ServicePrincipalConfig` in `plane-service-principal.ts:13-21` gets one new optional field:

```ts
tokenExchange?: Readonly<Record<string /* target audience */, readonly string[] /* allowed subject-token audiences */>>
```

`authorizePlaneServicePrincipal`'s existing bound-narrowing logic is reused verbatim for `scopes`/`orgId`; the exchange additionally requires `principal.tokenExchange?.[audience]?.includes(subjectAud)`. **Absent by default** — no currently-registered service principal (including `model-execution`'s existing entry) gains this capability until the deployment registry is edited to add it. This is the only registry change needed; `authorizePlaneServicePrincipal` itself (`plane-service-principal.ts:330-372`) needs one added branch, not a rewrite.

**Minted token claims** (`ConvexTokenService.issuePlaneToken`, `convex-token.service.ts:547-609` — one new optional field on `PlaneJwtClaims`, `actingUserId?: string`, emitted only when present):

```
{
  iss, aud: "ingestion", sub: "service:model-execution",
  org_id: <subjectToken.orgId>,      // must equal request orgId, checked
  service_id: "service:model-execution",
  principal_type: "service",          // UNCHANGED — clients.go needs no edit
  acting_user_id: <subjectToken.userId>,   // NEW — the only thing added to the wire
  scopes: ["integration:read"],
  zdr: <subjectToken.zdr>,            // inherited from the verified subject, NOT the registry default
  reason: "execution-core integration:read",
  exp: iat + 90   // new, short TTL — see 1.3
}
```

Because `principal_type` stays `"service"` and `sub === service_id === "service:model-execution"`, **`clients.go:145-196`'s existing verification requires zero code change** — this was already confirmed correct in `security.md`'s review ("integration-corev2's consuming code already exists and needs no further change").

### 1.3 Lifetimes, binding, replay

- TTL: 90s (shorter than every other plane token's 300s default — this is minted fresh per tool call, never cached, and the read it authorizes completes in one HTTP round trip).
- Binding: `(org_id, acting_user_id, zdr)` = exactly the subject token's own claims, which are themselves bound to `(org_id, user_id, zdr)` of the run at three independent points already (the identity map's §1: the RunAgentRequest ingress token, the RPC's own fields, session-core's durable run-ownership record). No `run_id` claim is needed on the exchanged token itself — the binding already happened upstream, twice.
- Replay: no nonce. A captured 90s-lived read-only token can be replayed only within its own tiny window and only ever grants `integration:read` for the one org/user it already named — the same replay surface every other short-lived plane token in this system already has (mechanisms a/b in the identity map's table). Not a new exposure.
- Revocation: none beyond TTL expiry — consistent with every other Auth Core JWT in this codebase today (no revocation list exists anywhere in `auth-core`).

### 1.4 Issuance/verification code locations (new)

| File | Change |
|---|---|
| `apps/Control Plane/auth-core/src/auth/plane-token.controller.ts` | new `@Post(':audience/token-exchange')` handler, mirrors `issueInternalToken` (230-333) incl. the fail-closed audit publish |
| `apps/Control Plane/auth-core/src/auth/plane-service-principal.ts` | `ServicePrincipalConfig.tokenExchange` field + one new check in `authorizePlaneServicePrincipal` (330-372) |
| `apps/Control Plane/auth-core/src/auth/convex-token.service.ts` | `verifyPlaneSubjectToken()` (new), `PlaneJwtClaims.actingUserId` (extends 170-179), `issuePlaneToken` payload line (extends 567-586) |
| `apps/Model Plane/rust/services/execution-core/src/integration_tools.rs` | new `mint_delegated_ingestion_token(&self, org_id, scope, session_bearer: &str)` beside `mint_ingestion_token` (503-539); called from `list_provider_actions` (234), `provider_key_for_connection` (269), `read_action_json` (456), and the read branch of `execute_action` (392-397) |
| `apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs` | thread `session_bearer: Option<&str>` into `execute_list_provider_actions` (currently `org_id`-only, line 1567); the call site at 705 already has `session_bearer` in scope |
| `apps/Model Plane/rust/services/execution-core/src/postcondition.rs` | thread `session_bearer` into `verify_provider_action`/`verify_unreceipted_mutation` (797, 846) so `read_action_json` (817, 856) gets it |
| `apps/Ingestion Plane/integration-corev2/...` | **none** |

### 1.5 Failure modes (all fail closed)

- No `session_bearer` at the call site (a run that lost its delegated session credential — the SAME condition that already produces `"delegation memory skipped: credential not forwardable"` elsewhere, e.g. `runtime_loop/agent.rs:2347`) → `mint_delegated_ingestion_token` returns `Err`, surfaced as an honest tool error, same shape as every other `mint_ingestion_token` failure today.
- auth-core registry has no `tokenExchange` entry for the service → 403, same `ForbiddenException` path as an unauthorized `/internal-token` call.
- `subjectToken` audience not allow-listed, expired, or org mismatch → 401/400, never silently downgraded to an org-only token.
- integration-corev2 side is unchanged and unaffected either way: no `acting_user_id` claim → today's fail-closed `personal_connection_no_delegated_user` (`connection_access.go:82-84`) continues to apply exactly as it does now for every org-wide (non-personal) provider, which is unaffected by any of this.

### 1.6 ZDR, residency, owner ruling

`zdr` on the exchanged token is the *subject token's own* `zdr`, never the service principal's registry default (`convex-token.service.ts:155-168`'s existing `issuedTokenZdr` logic is bypassed for this one field on purpose) — a ZDR-scoped user's run can never mint a persistent-posture ingestion credential through this path. The owner-only ruling (personal Microsoft/Google connections usable only by the connecting user) is *strengthened*, not loosened: today a personal connection is unreadable by the agent at all; after this change it becomes readable **only** for the exact user the RunAgentRequest was minted for, which is by construction the same user integration-corev2's `assertPersonalConnectionOwner` (`connection_access.go:73-90`) compares against.

### 1.7 Migration / rollout

Purely additive at every layer — new endpoint, new optional claim, new optional registry field, new optional function parameters with existing call sites passing `None`/omitting the exchange until wired. Rollout order: (1) auth-core endpoint + registry schema (dead code until a registry entry references it), (2) execution-core client change (still calls old `mint_ingestion_token` until explicitly switched), (3) flip the four read call sites, (4) add the `model-execution` principal's `tokenExchange: {ingestion: ["session-core"]}` entry to the deployment registry. Steps 1-3 can ship and be tested with zero behavior change; step 4 is the only step that activates it, and can be reverted by editing the registry file alone (no rebuild of auth-core needed — the registry is read at request time from `PLANE_SERVICE_PRINCIPALS_FILE`, not baked in).

### 1.8 Tests

- auth-core: subject-token verification (wrong audience, expired, wrong org) rejected; registry gate (absent `tokenExchange` → 403; present → succeeds and narrows scopes exactly like `/internal-token`); minted claims shape (`principal_type` stays `service`, `zdr` inherited not defaulted); audit-publish fail-closed path (mirror `issueInternalToken`'s existing test pattern).
- execution-core: `mint_delegated_ingestion_token` unit tests (missing bearer → error; success → correct headers/body); a `list_provider_actions`/`read_action_json` integration test against a fake auth-core/integration-corev2 confirming the read-only tool now succeeds for a personal connection when the run's own user owns it, and still fails for a different user's.
- integration-corev2: none needed (already covered by `security.md`'s `TestPrincipalEffectiveUserID` and `connection_access_test.go`), but worth one new fixture asserting a *real* execution-core-shaped token (service principal + `acting_user_id`) round-trips through `VerifyToken` end to end — closes the "inert until issued" note from `security.md`'s risk #6.

### 1.9 Effort

auth-core **M** (new endpoint, new verification path, registry schema, tests — no new key material). execution-core **S** (one new client method, mechanical parameter threading through 4 call sites). integration-corev2 **none**. Deployment **XS** (one registry-file edit).

---

## 2. Gap B — mail-task → session-core `PrepareUserOwnedThread`

### 2.1 What's actually missing, precisely

`PrepareUserOwnedThread` (`apps/Model Plane/rust/services/session-core/src/grpc.rs:4435-4487`) is built, tested, and *correctly* refuses a service-shaped `user_id` and a non-allow-listed caller — but its entire authorization is `caller.is_service() && USER_OWNED_THREAD_CALLERS.contains(&caller.principal_id())` (a one-entry allow-list, `["service:model-gateway"]`, `grpc.rs:2146`). Unlike its sibling `PrepareScheduledRunThread` (`grpc.rs:4346-4429`, gated by `verify_scheduled_run_decision`, `grpc.rs:728-804`), nothing independently attests that the specific `user_id` named in the request is who this specific call is actually supposed to act for — the RPC just trusts the caller's own field. Additionally, three non-identity pieces are still unbuilt (model-gateway's session-core credential, a BFF→model-gateway HTTP route, and `MailTaskPipeline`'s actual thread-creation wiring) — Design 2 below closes the identity/authorization piece and gives the minimum plumbing needed to *reach* it; it does not build the separate "post the synthesized turn, invoke execution-core" piece, which `mail-task.md`'s own deferred-item #1 already correctly scopes as distinctly larger, non-identity work.

### 2.2 New claim family: the **mail-task thread-prepare decision**

Issued by **user-core** (Control Plane) — the one service that already owns alias→user resolution (`internal/mailtask/{types,service,repository}.go`, `ResolveAlias`) — at the exact moment it resolves an inbound alias, i.e. inside the existing internal endpoint:

`GET /api/v1/internal/mail-task/resolve` (`apps/Control Plane/user-core/internal/http/mail_task_handlers.go:140-186`, already scoped to `mail-task:resolve` via `service_auth.go`, already the sole trust boundary the whole pipeline hangs off).

**New Go file** `apps/Control Plane/user-core/internal/spaces/mail_task_thread_decision.go`, modeled on `run_action_decision.go` (a *lean*, non-Space-shaped decision — reusing only `SigningKey`/`LoadSigningKeyFromEnv`/the `version.keyid.payload.signature` envelope machinery from `decision.go:12-169`, **not** the full `Decision` struct, whose `Validate()` mandates Space-only fields — `space_ref`, `recipient_audience_ref`, `privacy_policy_ref`, etc. — mail-task deliberately has none of these, per the module doc's own note that mail-task "avoids dragging in Space authority"):

```go
const mailTaskThreadDecisionVersion = "mail-task-v1"   // distinct tag: can never be
                                                         // accepted by the Space-decision
                                                         // verifier or vice versa
const mailTaskThreadPrepareAction   = "mail_task:thread_prepare"
const mailTaskThreadPrepareAudience = "model-plane-session-core"

type MailTaskThreadDecision struct {
    DecisionRef       string    `json:"decision_ref"`
    OrgID             string    `json:"org_id"`
    UserID            string    `json:"user_id"`
    AliasID           string    `json:"alias_id"`
    ProviderMessageID string    `json:"provider_message_id"`
    SessionKey        string    `json:"session_key"`       // "mail_task/"+sha256(org+providerMessageID)
    Origin            string    `json:"origin"`             // fixed "mail_task"
    ServiceAudience   string    `json:"service_audience"`   // fixed mailTaskThreadPrepareAudience
    ActionID          string    `json:"action_id"`          // fixed mailTaskThreadPrepareAction
    Nonce             string    `json:"nonce"`
    IssuedAt          time.Time `json:"issued_at"`
    ExpiresAt         time.Time `json:"expires_at"`
}

func IssueMailTaskThreadDecision(key SigningKey, orgID, userID, aliasID, providerMessageID string, now time.Time) (string, error)
```

`SessionKey` is computed with the *identical* formula the gateway BFF already uses (`IngestRequest::session_key()`, `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mail_task.rs:186-193`: `"mail_task/" + sha256(org_id + provider_message_id)`), so the decision and the eventual RPC request are checkable field-for-field, the same way `verify_scheduled_run_decision_with_keys` compares every field (`grpc.rs:773-793`).

**Response change** — `resolveMailTaskAlias` (`mail_task_handlers.go:181-185`) gains one field:

```go
c.JSON(http.StatusOK, gin.H{
    "org_id": address.OrgID, "user_id": address.UserID, "verified_email": user.Email,
    "thread_decision": decisionToken,   // NEW
})
```

### 2.3 Carrying the decision to session-core

Pure pass-through, three hops, all additive:

1. `ResolvedAlias` gains `ThreadDecision string` (`apps/Ingestion Plane/integration-corev2/internal/mailtask/resolver.go:16-20`, decoded at 87-98).
2. `GatewayIngestRequest` gains `ThreadDecision string \`json:"thread_decision"\`` (`apps/Ingestion Plane/integration-corev2/internal/mailtask/gateway_client.go:21-31`) — rides inside the same HMAC-delegated body integration-corev2 already sends the gateway BFF (`mailTaskIngestDelegationHeaders`, mechanism (c) in the identity map's table); the existing `body_sha256` covers it automatically, no HMAC canonical-string change needed.
3. `IngestRequest` (gateway BFF, `mail_task.rs:146-167`) gains `pub(crate) thread_decision: String`.

### 2.4 Reaching session-core: the two still-missing plumbing pieces

**(i) model-gateway (Model Plane)'s own session-core credential.** model-gateway *does* already have `tonic`/`mp-contracts` (confirmed: `apps/Model Plane/rust/services/model-gateway/Cargo.toml:10,18`) and already dials session-core via `grpc_auth.rs`'s forwarded `session_bearer` — what it lacks is a *self-service* credential for calls with no live user, i.e. an `mint_session_core_service_token()` mirroring execution-core's `mint_ingestion_token` (`integration_tools.rs:503-539`) almost exactly: `POST {auth_core_url}/api/session-core/internal-token` with `x-service-id: model-gateway`/`x-service-api-key`, using auth-core's **existing** `/internal-token` endpoint (`model-plane-token.controller.ts` doesn't cover `session-core` — that's the generic `plane-token.controller.ts:230-333`, which already supports every `PlaneAudience` including `session-core`). **No auth-core code change is needed for this piece at all** — only a deployment registry entry for `service:model-gateway` at `aud=session-core`, exactly mechanism (b) in the identity map's table, already fully built.

**(ii) A new internal HTTP route on model-gateway** (`POST /internal/mail-task/prepare-thread`), reachable over plain HTTP from the BFF (which has no `tonic`/`mp-contracts` at all — confirmed, `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml` has no such dependency). Authenticated via model-gateway's **existing** HTTP-level `Claims`/`service_id` check (`apps/Model Plane/rust/services/model-gateway/src/auth.rs:37,77`, the same axum middleware the rest of model-gateway's HTTP surface already uses), gated to one named `service_id` (e.g. `service:verevon-gateway`). The BFF reaches it by minting its **own** `aud=model-gateway` service-principal token — auth-core's existing `POST /api/model-plane/internal-token` (`model-plane-token.controller.ts:201-299`) already supports this; the BFF needs only a small reqwest-based minting helper mirroring its existing `audience_tokens.rs:87-117` pattern (no new crate — the BFF already does exactly this kind of call for user-cookie tokens).

Internally, this new model-gateway route:
```
verify caller is service:verevon-gateway (aud=model-gateway)
  -> mint/reuse cached service:model-gateway session-core token
  -> call session-core.PrepareUserOwnedThread {
         org_id, user_id, origin: "mail_task", session_key,
         control_decision_token: <forwarded from the BFF request body>
     }
  -> return {thread_id}
```

### 2.5 session-core: verifying the new decision

New proto field on `PrepareUserOwnedThreadRequest` (`apps/Model Plane/proto/model_plane/v1/sessions.proto:426-435`, field 5 is free):

```proto
message PrepareUserOwnedThreadRequest {
  string org_id = 1;
  string user_id = 2;
  string origin = 3;
  string session_key = 4;
  string control_decision_token = 5;   // NEW, additive
}
```

New verification function in `grpc.rs`, modeled directly on `verify_scheduled_run_decision_with_keys` (`grpc.rs:736-804`) — same `version.keyid.payload.signature` parsing, same `configured_control_space_decision_keys()` key set (`grpc.rs:1397-1441` — **the same Ed25519 public keys already loaded for scheduled-run decisions; no new key config**), but checking `parts[0] == "mail-task-v1"` and a leaner claims struct matching §2.2's Go type:

```rust
fn verify_mail_task_thread_decision_with_keys(
    req: &pb::PrepareUserOwnedThreadRequest, keys: &BTreeMap<String, VerifyingKey>, now: DateTime<Utc>,
) -> Result<(), Status> {
    // parse "mail-task-v1.<kid>.<payload>.<sig>", verify Ed25519 sig with keys[kid]
    // claims.org_id == req.org_id && claims.user_id == req.user_id
    //   && claims.session_key == req.session_key && claims.origin == req.origin
    //   && claims.service_audience == "model-plane-session-core"
    //   && claims.action_id == "mail_task:thread_prepare"
    //   && !claims.nonce.is_empty() && claims.expires_at > now
    //   && claims.issued_at <= now + Duration::minutes(1)
}
```

Wired into `prepare_user_owned_thread` (`grpc.rs:4435-4487`) as an **additional** check, gated on `origin == "mail_task"` specifically — every other current/future `USER_OWNED_THREAD_CALLERS` origin (today none besides `mail_task` is live in production; `schedule`'s own use of the sibling `PrepareScheduledRunThread` RPC is untouched) keeps the existing allow-list-only behavior unless and until it's given its own decision family. This is deliberately narrower than requiring it for *every* caller of this RPC, matching the task brief's framing that this is closing the specific mail-task gap, not redesigning the whole RPC's trust model in one pass:

```rust
if origin == "mail_task" {
    verify_mail_task_thread_decision(&req)?;   // NEW
}
```

### 2.6 Lifetimes, binding, replay

- TTL: 5 minutes from issuance (Postmark webhook redelivery + at-least-once processing needs headroom the 90s exchange token in Gap A doesn't; this isn't gating a live HTTP round trip, it's gating an asynchronous pipeline step).
- Binding: `(org_id, user_id, session_key, origin, action_id)` — every field the eventual RPC request carries is checked field-for-field against the signed claims, so a decision minted for one inbound message can never be pointed at a different `session_key`/user.
- Replay: `prepare_user_owned_thread`'s own thread creation is **already idempotent on `session_key`** (`create_thread_inner_preverified`, `grpc.rs:4472-4483` — same mechanism `PrepareScheduledRunThread` relies on). Replaying a captured decision within its 5-minute window therefore only ever re-resolves to the *same* thread, never a second one or a different user's thread — the same property that already makes Postmark's at-least-once redelivery safe end to end (`mail_task.rs`'s dedup-by-`provider_message_id` plus this). This is a materially safer replay profile than the scheduled-run decision's own (which the identity map's table already flags as having no nonce-reuse cache either).

### 2.7 Failure modes (all fail closed)

- Missing/malformed/expired/wrong-binding `control_decision_token` on a `mail_task`-origin request → `Status::permission_denied`, same shape as an invalid scheduled-run decision.
- BFF has no `aud=model-gateway` credential yet, or model-gateway has no `aud=session-core` credential yet (both are deploy-config gated) → the new route/RPC calls fail with a clear 5xx, and — critically — `MailTaskPipeline` stays wired to `None` exactly as it is today until an operator explicitly finishes the deploy-config steps, so the endpoint keeps answering its current, correct `503 mail_task_pipeline_not_configured` (`mail_task.rs`'s existing behavior) rather than ever silently swallowing an email.
- A non-`mail_task` origin calling `PrepareUserOwnedThread` is completely unaffected (no decision required, no behavior change) — this is the explicit backward-compatibility boundary.

### 2.8 ZDR, residency, owner ruling

`IssueMailTaskThreadDecision` should resolve the org's retention posture the same way interactive-token issuance already does (`resolveInteractiveRetentionPosture`, referenced from `plane-token.controller.ts:205-207`) and **refuse to issue** for a ZDR-scoped org — mirroring `issueScheduledRunDecision`'s existing rule, `if evidence.Privacy.ZeroDataRetention { return error }` (`scheduled_run_decision.go:126-128`). This is a real product question (does mail-task exist at all for ZDR orgs?) that this design surfaces rather than silently resolves; recommend refusing issuance until the owner decides, exactly matching the precedent already set for scheduled runs. The owner ruling ("personal Microsoft/Google connections usable only by the connecting user") is untouched by this gap and is automatically preserved downstream: once the mail-task pipeline eventually invokes an execution-core turn on the created thread, that turn's `RunAgentRequest.user_id` is the *same* alias-resolved user, so Gap A's fix (§1) binds provider reads to exactly that person — no separate wiring needed.

### 2.9 Migration / rollout

Additive at every hop; the proto field addition is the one step with a forced-ordering constraint noted in project memory ("a session-core proto change needs a model-gateway rebuild too") — so rollout order is: (1) proto field + session-core verification (dead until a caller sends `mail_task` origin with a token — today nothing does), (2) user-core issuance + resolver/gateway_client pass-through (dead until the BFF forwards it), (3) model-gateway credential + new route (dead until called), (4) BFF mints its model-gateway credential and forwards `thread_decision` (this is the step that activates everything, and it's still gated by `MailTaskPipeline` staying `None` until the separate, larger "post the synthesized turn" work lands — so this design closes the identity gap without prematurely turning the pipeline live).

### 2.10 Tests

- user-core: `IssueMailTaskThreadDecision` round-trips through a new `verify_mail_task_thread_decision_with_keys`-equivalent test vector (shared fixture keys, mirroring `run_action_decision_test.go:157`'s pattern); ZDR-org refusal.
- session-core: mirror `prepare_user_owned_thread`'s existing 6 tests (`grpc.rs`, referenced in `schedule.md`) plus new cases — missing/expired/wrong-user/wrong-session-key decision on a `mail_task` origin request all rejected; a non-`mail_task` origin request is unaffected by an absent decision (backward-compat pin).
- integration-corev2: `resolver_test.go` gains a case asserting `thread_decision` passes through `HTTPAliasResolver.Resolve` unmodified; `gateway_client.go`'s existing ingest tests get one asserting the field rides in the forwarded body.
- gateway BFF: `mail_task.rs`'s existing HMAC/dedup/rate-limit test suite (10 tests per `mail-task.md`) gains one asserting `thread_decision` deserializes and is forwarded verbatim to the new model-gateway call (still behind a fake `MailTaskPipeline`, per the existing test pattern).
- model-gateway: new route's auth gate (wrong/missing `service_id` → 403) and its session-core call (decision forwarded unmodified into `PrepareUserOwnedThreadRequest.control_decision_token`).

### 2.11 Effort

user-core **S** (one new lean decision file reusing existing signing primitives, one response-field addition). session-core **M** (new proto field + new verification function + wiring + tests, same shape as the existing scheduled-run verifier). model-gateway **M** (new credential-minting client + new authenticated HTTP route + session-core call). gateway BFF **S** (mint-and-forward, reusing the existing `audience_tokens.rs` pattern; `thread_decision` field is one line). integration-corev2 **XS** (two struct fields, pure pass-through). Deployment **XS** (two registry entries: `service:model-gateway`@`aud=session-core`, `service:verevon-gateway`@`aud=model-gateway`).

**Explicitly out of scope for this design** (unchanged from `mail-task.md`'s own honest accounting): posting the synthesized first chat turn and invoking exactly one execution-core turn on the prepared thread. This design makes `PrepareUserOwnedThread` reachable and properly Control-bound; it does not build the turn-invocation step, which has no unattended precedent anywhere in this codebase yet and is a distinctly larger piece of work.

---

## 3. Summary table

| Gap | Service | Change | Effort | New secret material |
|---|---|---|---|---|
| A | auth-core | `POST /:audience/token-exchange`, registry `tokenExchange` field, subject-token verify | M | none (reuses RS256 keypair) |
| A | execution-core | `mint_delegated_ingestion_token`, thread `session_bearer` into 4 call sites | S | none |
| A | integration-corev2 | none | — | — |
| B | user-core | `IssueMailTaskThreadDecision`, resolve-endpoint response field | S | none (reuses Ed25519 keypair) |
| B | session-core | proto field, `verify_mail_task_thread_decision_with_keys`, wiring | M | none (reuses same keys) |
| B | model-gateway | session-core credential mint, new internal HTTP route | M | none |
| B | gateway BFF | model-gateway credential mint, `thread_decision` forward | S | none |
| B | integration-corev2 | pass-through field ×2 | XS | none |

Both designs are strictly additive (new endpoints, new optional fields, new opt-in registry entries, new proto field) and require zero new signing keys — Gap A rides auth-core's existing RS256 keypair through a real token-exchange grant; Gap B rides Control's existing Ed25519 decision keypair through a new, narrowly-scoped claim family. Every failure mode preserves the current fail-closed behavior (`personal_connection_no_delegated_user` for Gap A short of full rollout; `503 mail_task_pipeline_not_configured` for Gap B short of full rollout) rather than ever widening access silently.