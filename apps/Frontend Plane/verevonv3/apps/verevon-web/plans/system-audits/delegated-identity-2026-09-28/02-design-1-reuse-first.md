# Design: closing the two identity gaps (execution‑core → integration‑corev2 reads, and mail‑task → session‑core thread ownership)

Both designs below are **reuse‑first**: no new cryptographic primitive, no new secret *type*, no new signing key is introduced anywhere. Each gap is closed by widening a delegation mechanism that already exists, is already deployed, and is already verified on the receiving side, to cover a case it doesn't cover yet.

| Gap | Mechanism reused | Why this one |
|---|---|---|
| (A) execution-core reads have no end‑user identity | **(f) Ed25519 provider‑write attestation** (`execution-core/src/attestation.rs` ↔ `integration-corev2/internal/attestation/verifier.go`) | It is the *only* mechanism in the table that already binds `{org_id, connection_id, actor_id}` per‑call, is already wired end‑to‑end for writes on this exact boundary, and already has an unused, already‑verified `human_intent` authorization kind sitting right next to the one that's used — the verifier already accepts a no‑approval‑id claim shape. |
| (B) `PrepareUserOwnedThread` has no user‑bound authority | **(e) Control‑issued Ed25519 Space decision** (`user-core/internal/spaces/decision.go` ↔ `session-core`'s `configured_control_space_decision_keys`/`verify_scheduled_run_decision_with_keys`) | It is the mechanism this codebase already uses for the *sibling* RPC (`PrepareScheduledRunThread`) to bind a service‑named `user_id` cryptographically instead of trusting an allow‑list; mail‑task's alias owner already has a personal Space, so the existing personal‑Space evidence/issuance path applies almost unmodified. |

Both gaps also need a small amount of mechanism **(b)** (service‑principal plane token) plumbing — execution-core already has it for gap (A)'s existing token; model-gateway needs a first instance of it for gap (B) — but in neither case does (b) carry the actual user binding; it only gets the caller in the door. The user binding is cryptographic in both final designs, not "trust the caller's own claim."

---

## Part A — execution-core → integration-corev2 reads

### A.1 What's being extended

`integration-corev2`'s write path already has a working answer to "who is this action really for": `executeConnectionAction` (`apps/Ingestion Plane/integration-corev2/internal/api/server.go:3001-3194`) requires a `writeAttestation` JWS whose `actor_id` — cryptographically bound to `{org_id, connection_id, provider_key, operation, payload_sha256, idempotency_key}` — is what `assertPersonalConnectionOwner` checks (line 3061), *not* the bearer principal. Reads on the very same route explicitly opt out of this (line 3016-3029, comment: *"A read has no write attestation to carry a cryptographically verified acting user"*) and fall back to the bearer's `EffectiveUserID`, which for execution-core's `mint_ingestion_token`-issued bearer (`integration_tools.rs:503-539`) is always empty (§1/§2b of the identity map — confirmed by reading `PlaneInternalTokenBody`, `plane-token.controller.ts:90-94`, and `EffectiveUserID`, `middleware.go:90-101`).

The fix: sign the same kind of attestation for reads too, using a new `authorization_kind` that already fits the verifier's existing shape validation.

### A.2 Claims / binding / lifetime

Reuse `attestation.Claims` (`verifier.go:51-71`) / `attestation::Claims` (`attestation.rs:73-95`) byte‑for‑byte — same header (`alg:"EdDSA"`, `typ:"verevon.provider-write-attestation+jwt"`), same field set, same `v2`‑envelope‑free compact‑JWS wire format. New constant only:

```rust
// execution-core/src/attestation.rs
pub const AUTHORIZATION_READ_ACCESS: &str = "read_access";
```
```go
// integration-corev2/internal/attestation/verifier.go
AuthorizationReadAccess = "read_access"
```

Validation shape is identical to the existing, currently‑unused‑in‑this‑direction `human_intent` kind: no `approval_id`, `action_id == authorization_id`. Concretely, one line changes in each language:

- Rust `validate()` (`attestation.rs:293-309`): `AUTHORIZATION_HUMAN_INTENT => {...}` becomes `AUTHORIZATION_HUMAN_INTENT | AUTHORIZATION_READ_ACCESS => {...}` (same body).
- Go `validateClaims()` (`verifier.go:272-283`): `case AuthorizationHumanIntent:` becomes `case AuthorizationHumanIntent, AuthorizationReadAccess:` (same body).

Binding:
- `org_id` — the run's org (unchanged input).
- `connection_id` / `provider_key` / `operation` — the real values for a connection‑scoped read (`GET /connections/:id`, and the existing `/connections/:id/actions` route used by `read_action_json`); a sentinel `"*"`/`"list"` triple for the org‑level `list_provider_actions` call, which has no single connection to bind (see A.4).
- `actor_id` — **the run's real, already‑verified owning user** — this is the load‑bearing new binding. It is already threaded to the write path (`runtime_loop/mod.rs:1606-1678`, `execute_provider_action(... user_id: &str ...)` → `client.execute_action(..., user_id, ...)`), just not to the read‑only entry points.
- `payload_sha256` — computed exactly as today via `attestation::payload_sha256`/`attestation.PayloadSHA256`, over `{org_id, connection_id, provider_key, operation, params, body}` (params/body = `null` for the sentinel list/lookup cases).
- `idempotency_key` — reuse the run's own `run_id` (already in scope at every call site, see A.3), which is a free run‑binding: the same JTI/actor/run triple can't be silently reused across an unrelated run without the caller changing the claim.
- `exp - iat` — unchanged, ≤60s, enforced by the *existing* `validateClaims` lifetime check (`verifier.go:284-290`) — no new expiry logic. `Signer`'s existing `DEFAULT_TTL_SECONDS = 30` (`attestation.rs:61`) is reused as‑is.

No approval, no idempotency‑key DB row (reads have no `ActionReceipt` table) — replay defense is the same ≤60s window that already is the documented primary defense for this whole mechanism (identity map §2, row f: *"30s TTL is the primary defense"*). This is a like‑for‑like posture with what's already shipped for writes; going further (a JTI replay cache) is optional hardening, not required to close the gap (see A.7).

### A.3 Issuance — execution-core changes

1. `integration_tools.rs` (`IntegrationActionsClient`):
   - `list_provider_actions(&self, org_id: &str)` → `list_provider_actions(&self, org_id: &str, actor_id: &str, run_id: &str)`. Before the `GET /connections` call, build and sign a sentinel attestation (`connection_id:"*"`, `provider_key:"*"`, `operation:"list"`) via the existing `self.write_attestor`, attach as header `x-access-attestation`.
   - `provider_key_for_connection` (line 269-307) gains an `actor_id: &str` parameter (already available at its one call site inside `execute_action`, line 354-356) and signs the same shape bound to the real `connection_id`.
   - `read_action_json` (line 456-501) gains `actor_id: &str`; its two callers in `postcondition.rs` (`verify_provider_action` line 797, `verify_unreceipted_mutation` line 846) already run inside the write path where `actor_id` is available one frame up — thread it down.
   - All three fail closed exactly like `execute_action` already does when `self.write_attestor` is `None` (attestation.rs:365-367's existing pattern) — **no new env var**: `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY`/`_KEY_ID` already must be set in every deployment for writes to work at all, so signing reads costs zero new configuration.
2. `runtime_loop/mod.rs`:
   - `execute_list_provider_actions(org_id: &str)` (line 1567) → add `user_id: &str, run_id: &str` params (the tool‑dispatch call site already has both — `execute_provider_action` right below it, line 1606, receives the identical parameters from the same dispatcher).
3. `attestation.rs`: add the one constant + one match arm shown in A.2. No struct/field changes.

Effort: **S** (execution-core). All plumbing is "thread two already‑verified strings one layer deeper," not new authority.

### A.4 Verification — integration-corev2 changes

1. `internal/attestation/verifier.go`: one constant, one match‑arm merge (A.2). No change to `Verify`'s signature, key‑lookup, or signature‑check code path.
2. `internal/api/connection_access.go`: new helper mirroring `assertPersonalConnectionOwner`'s existing split —
   ```go
   func assertConnectionAccessOrAttestedRead(c *fiber.Ctx, cfg ServerConfig, connection store.Connection) error
   ```
   Tries the existing bearer‑based `assertConnectionAccess` path first (line 53‑63, unchanged — every non‑execution‑core caller, e.g. the SPA's own session bearer, capability-core, conversation-core, keeps working with **zero code change and zero behavior change**). Only when the connection is personal (`providers.IsPersonalProvider`) *and* the bearer's `EffectiveUserID` is empty (today's fail‑closed case) does it look for an `X-Access-Attestation` header, verify it via `cfg.WriteAttestations.Verify(...)` with `Binding{PresenterService: principal.UserID /* == "service:model-execution"-style subject already on the bearer */, OrganizationID: connection.OrganizationID, ConnectionID: connection.ID, ProviderKey: connection.ProviderKey, Operation: "get", PayloadSHA256: <fixed digest of the null/null params/body pair>, IdempotencyKey: <x-run-id header>}`, and — only on success — calls `assertPersonalConnectionOwner(connection, verified.ActorID, true)`. Any verification failure falls through to today's existing 403 (`personal_connection_no_delegated_user`) — **strictly additive, never weaker** than current behavior.
3. `GET /api/v1/connections/:id` (`server.go:337-346`) and `GET /api/v1/connections/:id/status` (`server.go:348-362`, used transitively by execution-core's write path via `provider_key_for_connection`) switch from `assertConnectionAccess` to the new helper.
4. `GET /api/v1/connections` list handler (`server.go:311-335`): `filterPersonalConnectionsForCaller` (`connection_access.go:100-114`) gains an attestation‑aware variant that, for a `list`‑scope attestation (connection_id `"*"`), resolves one `effectiveUserID` from it exactly like the single‑connection case, before doing the same per‑row filter it does today.
5. `POST /api/v1/connections/:id/actions`, read branch (`server.go:3014-3029`): the existing `writeAction` `if`/`else` already isolates this from the write path completely. Add one call to the new helper right where the comment says a read has no attestation — it now might.
6. `actionBody` (`server.go:1519`/`1528`, wherever the JSON struct is declared): add `ReadAttestation string \`json:"readAttestation,omitempty"\`` — a **new, additive** field, distinct from `WriteAttestation`, so nothing about the write path's wire contract changes and no existing test/fixture needs updating.

Effort: **M** (integration-corev2) — mechanically small per‑route, but touches five route handlers plus the Go verifier, and needs the table‑driven test coverage this codebase's own review process (security.md) already insists on for every route class.

### A.5 Key management

Zero new keys. Reuses:
- execution-core's existing `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY`/`_KEY_ID` (already required for writes).
- integration-corev2's existing `ProviderWriteAttestationKeysJSON` → `cfg.WriteAttestations` (`server.go:54,88-90`), already trusting issuer `"model-execution"` (`verifier.go:36-39`) — that allow‑list needs **no change**, since the same issuer now also signs `read_access` claims with the same key.

### A.6 Failure modes / ZDR / owner ruling

- No attestation, personal connection, no bearer identity → **unchanged** 403 `personal_connection_no_delegated_user` (today's exact behavior; this design only ever *adds* a path to success, never removes the existing fail‑closed default).
- Invalid/expired attestation → treated as "no attestation" (fail closed), never surfaced as a distinguishable error to avoid giving an attacker an oracle on why verification failed — matches `verifyWriteAttestation`'s existing `errActionAttestationInvalid` opacity (`server.go:3302`).
- Org‑wide connectors (Slack, GitHub, …) → `IsPersonalProvider` gate means this entire code path is never even reached — zero behavior change, exactly as today.
- ZDR/content: the attestation carries no message content or provider payload beyond a hash — same "content and secrets never enter this token" property the module doc already states (`attestation.rs:1-33`) for writes.
- Owner ruling 5 ("personal connections usable only by the connecting user") is *strengthened*, not relaxed: today execution-core can't reach personal connections at all (correctly fail‑closed but functionally broken for the "check my inbox" use case); after this change it can reach them **only** as the run's real, cryptographically‑attested owning human, never as any other user, never as the org, never via a forged claim (Ed25519‑signed, ≤60s TTL, org+connection+actor bound).

### A.7 Migration / rollout / tests

- No flag needed. Every consumer that doesn't send `X-Access-Attestation`/`readAttestation` gets today's exact behavior (org‑wide unaffected; personal connections still fail‑closed for callers with no identity). This is purely capability‑additive.
- Rollout order: ship integration-corev2's verifier + route changes first (inert until a caller sends the new header/field), then execution-core's signer + plumbing. No coordinated cutover required.
- Optional hardening (not required to close the gap, worth a follow‑up): an in‑memory JTI replay cache on integration-corev2's side, same shape as `conversation-ingest-rs`'s `ReplayCache` (`lib.rs:32-36,452-458`), since reads have no DB idempotency row to lean on the way writes do.
- Tests (mirroring this codebase's existing conventions):
  - `execution-core/src/attestation.rs`: new `sign_produces_a_verifiable_read_access_token` (mirrors `sign_produces_a_verifiable_three_segment_jws`, line 517), `sign_rejects_read_access_with_an_approval_id` (mirrors line 596).
  - `execution-core/src/integration_tools.rs`: extend `list_provider_actions`/`read_action_json`/`provider_key_for_connection` tests to assert the outbound request carries `x-access-attestation`.
  - `integration-corev2/internal/attestation/verifier_test.go`: `Verify_accepts_read_access_kind_with_no_approval_id`, `Verify_rejects_read_access_with_an_approval_id_present` (mutation‑style, matching this file's existing pattern).
  - `integration-corev2/internal/api/connection_access_test.go`: new table‑driven cases for the attested‑read path, mirroring the 11 subtests already there for the bearer path (security.md, `TestSyncJobRouteClassesEnforcePersonalConnectionOwnership`'s style) — cross‑user attestation rejected, correct‑user attestation accepted, expired attestation rejected, org‑wide connector unaffected by any of this.

Effort total for gap A: **S (execution-core) + M (integration-corev2)**, no other service touched, no auth-core change at all.

---

## Part B — mail‑task → session‑core `PrepareUserOwnedThread`

### B.1 What's being extended

Two things are missing simultaneously (schedule.md's own review already flagged risk #1 for the second one):

1. Model‑gateway has **no** credential to call session-core except a forwarded *live user's* bearer (`grpc_auth.rs:51-61,403-426` — `VerifiedIdentity.session_bearer` only ever comes from `verify_delegated_bearer` on an inbound user session). An inbound email has no live session.
2. Even once it has *some* credential, `prepare_user_owned_thread` (`session-core/src/grpc.rs:4435-4487`) authorizes purely by caller identity — `caller.is_service() && USER_OWNED_THREAD_CALLERS.contains(&caller.principal_id())` (line 4441, allow‑list is exactly `["service:model-gateway"]`, line 2146) plus `caller.authorize_org(&req.org_id)` — with **nothing** verifying that the caller is entitled to name *this specific* `req.user_id` as the thread's owner. Compare `prepare_scheduled_run_thread` (line 4346-4429), which additionally requires `verify_scheduled_run_decision(&req)` — a Control‑issued, Ed25519‑signed, user‑bound decision.

Since `MailTaskPipeline` is `None` in every production path today (mail-task.md: *"stays None/503 in production, unchanged from batch1"*), **there is no live caller of this RPC anywhere**. That means the stronger authorization can ship as a hard requirement from day one — no flag, no backward‑compat shim, because nothing currently depends on the weaker behavior.

### B.2 Claims / binding / lifetime — new Control decision

Reuse `spaces.Decision` / `SignDecision` / `VerifyDecision` (`user-core/internal/spaces/decision.go`) and the Rust `ControlSpaceDecisionClaims` / envelope parser (`session-core/src/grpc.rs:615-647,736-804`) byte‑for‑byte — same `v2.key_id.payload.sig` wire format, same signing/verification code paths, same key‑rotation machinery (`configured_control_space_decision_keys`, `grpc.rs:1397-1456`, reading `CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON`/`_KEY_ID`/`_PUBLIC_KEY_BASE64` — **no new env var**, and Go's issuer side reuses `LoadSigningKeyFromEnv` off `CONTROL_SPACE_DECISION_KEY_ID`/`CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64`, also **already configured**).

New constants only (`user-core/internal/spaces`, new file `mail_task_thread_decision.go`):
```go
const (
    mailTaskThreadPrepareAction   = "model.mail_task.thread_prepare"
    mailTaskThreadPrepareAudience = "model-plane-model-gateway"
    mailTaskThreadPrepareSchema   = "sha256:mail-task-thread-prepare-v1"
)
```
Mirrored in Rust (`session-core/src/grpc.rs`, next to lines 85-87):
```rust
const CONTROL_MAIL_TASK_THREAD_PREPARE_AUDIENCE: &str = "model-plane-model-gateway";
const CONTROL_MAIL_TASK_THREAD_PREPARE_ACTION: &str = "model.mail_task.thread_prepare";
const CONTROL_MAIL_TASK_THREAD_PREPARE_SCHEMA: &str = "sha256:mail-task-thread-prepare-v1";
```

**Evidence / authority anchor**: `PersonalThreadDecisionEvidence` with `Membership.Kind == KindPersonal` — the alias owner's own personal Space, resolved by the exact same lookup `own_scheduled_thread_read_decision.go` already does (schedule.md, Phase B). New evidence validator, mirroring `ValidateForOwnScheduledThreadRead` (`personal_thread_decision.go:230-244`) and `ValidateForAgentAction`'s ZDR guard (line 292-294):
```go
func (e PersonalThreadDecisionEvidence) ValidateForMailTaskThreadPrepare() error {
    if err := e.Membership.Validate(); err != nil { return err }
    if e.Membership.Kind != KindPersonal {
        return fmt.Errorf("mail-task thread preparation requires the alias owner's personal Space")
    }
    if !matchesOneOf(e.Membership.Role, "owner") {
        return fmt.Errorf("only the Space's own owner may receive a mail-task thread")
    }
    if !e.ThreadCreateEntitled {
        return fmt.Errorf("thread creation entitlement is not active")
    }
    if e.Privacy.ZeroDataRetention {
        return fmt.Errorf("zero data retention forbids a durable mail-task thread")
    }
    return e.Privacy.Validate()
}
```
(Reuses the existing `ThreadCreateEntitled` flag rather than inventing a new entitlement — same one `IssuePersonalThreadCreateDecision` already gates on.)

**Payload digest** — new function `mailTaskThreadPreparePayloadDigest`, structurally identical to `personalThreadCreatePayloadDigest` (`decision.go:400-431`) but over exactly the fields `PrepareUserOwnedThreadRequest` carries (confirmed at `grpc.rs:4446-4471`: `org_id`, `user_id`, `session_key`, `origin` — **no** `space_id`/`recipient_audience_*`/`privacy_policy_ref`/`resource_authorization_ref` on the wire, since this RPC deliberately never sets a Space): `{org_id, user_id, origin, session_key, action_schema_hash, idempotency_key}`.

**Issuer**:
```go
func IssueMailTaskThreadPrepareDecision(evidence PersonalThreadDecisionEvidence, req MailTaskThreadPrepareRequest, decisionRef, nonce string, now time.Time) (Decision, error)
```
where `MailTaskThreadPrepareRequest{OrgID, UserID, Origin /* fixed "mail_task" */, SessionKey, IdempotencyKey /* = provider_message_id */, Nonce}` — same shape/validation style as `PersonalThreadDecisionRequest` (`decision.go:301-306,382-393`).

**Verifier** — new sibling of `verify_scheduled_run_decision_with_keys` (`grpc.rs:736-804`), `verify_user_owned_thread_decision_with_keys`, reusing the identical envelope‑parsing/signature‑check code (parts.len()==4, key lookup, Ed25519 verify) and matching only the fields this RPC actually has: `claims.org_id == req.org_id`, `claims.subject_id == req.user_id`, `claims.service_audience == CONTROL_MAIL_TASK_THREAD_PREPARE_AUDIENCE`, `claims.action_id == CONTROL_MAIL_TASK_THREAD_PREPARE_ACTION`, `claims.action_schema_hash == CONTROL_MAIL_TASK_THREAD_PREPARE_SCHEMA`, `claims.idempotency_key == req.idempotency_key`, `claims.payload_digest == mail_task_thread_prepare_payload_digest(req, &claims)`, `claims.permissions.contains("mail_task:thread_prepare")`, `!claims.zero_data_retention`, plus the same `nonce`/`expires_at`/`issued_at` checks (lines 794-802) — **deliberately does not compare `claims.space_ref`**, since the request has no `space_id` to compare it to; `space_ref` still rides in the envelope only as required evidence metadata (`Decision.Validate()` still requires it non‑empty, so Control still had to resolve a real personal‑Space membership to sign this at all).

**Lifetime**: reuses `personalDecisionLifetime = 2 * time.Minute` (`personal_thread_decision.go:21`) — no new constant.

### B.3 Where the decision is minted — reuse the existing alias‑resolve endpoint

`GET /api/v1/internal/mail-task/resolve` (`user-core/internal/http/mail_task_handlers.go`, scope `mail-task:resolve`, service‑principal‑only per mail-task.md's report) already resolves an inbound alias to `{userId, orgId}` for integration-corev2's `HTTPAliasResolver` (`integration-corev2/internal/mailtask/resolver.go`). Extend its response with one new optional field, `controlDecisionToken`, populated by calling `spaces.IssueMailTaskThreadPrepareDecision` + `spaces.SignDecision` right there (the handler already has an authenticated org/user context from the resolve itself) when — and only when — the resolved user has an active personal Space with `ThreadCreateEntitled` and non‑ZDR posture; otherwise the field is simply omitted and the pipeline degrades to its current `503` (fail closed, not fail open).

`integration-corev2`'s `resolver.go` reads this one new field and threads it, **opaquely** (it never needs to inspect or trust its contents — that's session-core's job), into the payload it already forwards to the gateway over the existing HMAC v2 delegation (`internal/mailtask/gateway_client.go`, same `mailTaskIngestDelegationHeaders` shape `conversation-ingest-rs`/`emailsync.go` already use). **No change to the HMAC canonical string or replay cache** — the new field is just one more JSON body field, already covered end‑to‑end by the existing `x-delegation-body-sha256`.

### B.4 Where model-gateway gets *a* credential at all — new, minimal (b)‑mechanism instance

Gateway BFF (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mail_task.rs`) gains a small client function mirroring execution-core's `mint_ingestion_token` (`integration_tools.rs:503-539`) almost line for line:
```rust
async fn mint_session_gateway_token(org_id: &str) -> Result<String, String> {
    // POST {auth_core_url}/api/session-core/internal-token
    // headers: x-service-id: model-gateway, x-service-api-key: <MODEL_GATEWAY_SESSION_CORE_SERVICE_API_KEY>
    // body: { orgId, scopes: ["session:user-owned-thread-prepare"], reason }
}
```
This is the coarse "may model-gateway call this RPC at all" gate — unchanged mechanism, new registrant. Auth-core needs one new registry row in the existing `PLANE_SERVICE_PRINCIPALS_JSON`/`_FILE` (`plane-service-principal.ts:13-21,147-183`) — a **deploy‑config change, not new code** on auth-core:
```json
"model-gateway": {
  "credential": "<...>",
  "audiences": ["session-core"],
  "orgIds": [...],
  "scopes": ["session:user-owned-thread-prepare"]
}
```
This matches the scope string `prepare_user_owned_thread` already checks via `authorize_operation(&caller, "session:user-owned-thread-prepare")` (`grpc.rs:4440`, and `authorize_operation`'s scope enforcement at `session-core/src/auth.rs:198-209`) — that check is unmodified; it's satisfied for the first time once this registry row exists.

Model-gateway also needs the actual gRPC (or an internal HTTP‑to‑gRPC bridge, since this crate has no `tonic`/`mp_contracts` dependency today, per mail-task.md's own deferred‑item note) call to session-core's `PrepareUserOwnedThread`, attaching the minted token as the primary bearer and the Control decision token (B.3) as a new proto field.

### B.5 Proto / session-core changes

`apps/Model Plane/proto/model_plane/v1/sessions.proto`: add `string control_decision_token = N;` to `PrepareUserOwnedThreadRequest` — additive only, same pattern as every other decision‑bearing request already in this file.

`session-core/src/grpc.rs`, inside `prepare_user_owned_thread` (line 4435-4487), insert one call after the existing origin/user_id/org validation (after line 4471) and before `create_thread_inner_preverified`:
```rust
verify_user_owned_thread_decision(&req)?;
```
No other line in this function changes. `USER_OWNED_THREAD_CALLERS`'s allow‑list check (line 4441) stays — it's still the right coarse gate; the decision check is layered on top, exactly mirroring how `prepare_scheduled_run_thread` layers `verify_scheduled_run_decision` on top of its own allow‑list check.

**Both session-core and model-gateway need a rebuild for this** (unlike the earlier `THREAD_ORIGINS` widening, which mail-task.md correctly noted needed no model-gateway rebuild because it was an existing opaque string field — this is a genuine new proto field on a request model-gateway itself builds, so per the project's own noted precedent (`correct_memory PATCH needed model-gateway rebuild too`) both sides of the wire contract must be rebuilt together).

### B.6 Failure modes / ZDR / replay / idempotency

- Missing/invalid decision → `Status::permission_denied`, surfaces through `MailTaskPipeline`'s existing error path exactly like any other session-core error already does (mail-task.md: fails closed to a retryable `503`, never a false `200`) — no new failure *shape*, just a new possible cause.
- ZDR: `ValidateForMailTaskThreadPrepare` refuses to sign at all when the owner's personal Space is ZDR — mirrors `ValidateForAgentAction`'s existing rule, so a ZDR org can never get a durable mail‑task thread spun up from an inbound email, consistent with "ZDR forbids durable owner actions" everywhere else in this file.
- Owner‑only ruling: the decision's `subject_id` is resolved server‑side from the alias table (`ResolveAlias`), never from anything integration-corev2, the gateway, or the inbound email itself asserts — so a forged `From:`/alias claim cannot name a different owner; this is strictly the alias‑ownership model mail-task.md's own webhook‑auth control already establishes (`AuthResult==pass && From==alias-owner's-verified-email`), now carried forward cryptographically instead of trusted at each hop.
- Replay: a captured decision token is bounded by the existing 2‑minute `personalDecisionLifetime`, and even within that window replaying it against `prepare_user_owned_thread` is **harmless by construction** — `create_thread_inner_preverified`'s advisory‑lock‑guarded idempotency on `(org_id, user_id, session_key)` (`grpc.rs:2038-2069`, confirmed by reading it directly) means a second call returns the *same* thread_id, never a duplicate or a different owner's thread. This is a stronger replay posture than the write‑attestation side gets (A.2) — no additional nonce cache is needed here.

### B.7 Migration / rollout / tests

- **No flag.** `PrepareUserOwnedThread` has zero production callers today; ship the stronger check in the same change that gives it its first caller, so the weaker (allow‑list‑only) posture is never live.
- Rollout order: user-core issuer + registry row → session-core verifier + proto field (rebuild) → model-gateway credential + RPC bridge (rebuild) → flip `MailTaskPipeline` from `None` to the real implementation last, once all four legs exist.
- Tests:
  - `user-core/internal/spaces/mail_task_thread_decision_test.go` (new): mirrors `own_scheduled_thread_read_decision_test.go`'s 8‑case structure — happy path, wrong Space kind, non‑owner role, ZDR org rejected, entitlement missing, mismatched evidence/request org or user.
  - `session-core/src/grpc.rs` test module (~line 7392+, alongside the existing `verify_scheduled_run_decision_with_keys` fixed‑vector tests): `prepare_user_owned_thread_rejects_a_missing_decision_token`, `..._rejects_a_decision_for_a_different_user`, `..._accepts_a_valid_decision_and_is_idempotent_on_session_key` (extending the existing `prepare_user_owned_thread_*` suite at lines 9955‑10087).
  - `apps/gateway`: new unit tests for `mint_session_gateway_token` (wiremock‑based, matching mail-task.md's own review pattern of adding wiremock coverage where a file had none).
  - Cross‑language fixed‑vector test for the payload digest (matching `attestation.rs`'s `FIXTURE_PAYLOAD_SHA256`‑style pinning), so a Go/Rust digest drift is caught immediately rather than at runtime.

Effort: **S (user-core, one new file mirroring an existing one) + M (session-core, new proto field + rebuild + one verifier function + tests) + M (model-gateway, first-ever outbound plane-token mint + first-ever gRPC/bridge call to session-core, no existing scaffolding to lean on) + config-only (auth-core registry row)**.

---

## What this does *not* solve (explicitly out of scope, flagged for the owner)

- Model-gateway's HTTP‑to‑gRPC bridge to session-core is new surface area regardless of this design — B.4/B.5 specify its authorization, not its transport implementation (`tonic`/`mp_contracts` dependency decision stays a separate call, as mail-task.md already flagged).
- Gap (A) covers execution-core's own reads; it does not touch `execute_action`'s existing write‑attestation path at all (already correctly user‑bound) except to note that `provider_key_for_connection`'s preliminary `GET` was silently blocking writes against personal connections too (A.4 point 3) — worth calling out to the owner as a second, free fix riding along with A.
- Neither design changes `PLANE_SERVICE_PRINCIPALS_JSON`'s trust model (mechanism b) itself — a compromised `INGESTION_SERVICE_API_KEY` or the new `MODEL_GATEWAY_SESSION_CORE_SERVICE_API_KEY` still lets an attacker mint an org‑scoped service token; what's new is that such a token *alone* is no longer sufficient to read a personal connection or name a thread owner — it also needs a per‑call, short‑lived, cryptographically distinct binding (the read‑access attestation or the Control decision) that a stolen static API key doesn't give you.

## Files read to ground this design (no files modified)

- `apps/Model Plane/rust/services/execution-core/src/{integration_tools.rs, attestation.rs, runtime_loop/mod.rs, postcondition.rs}`
- `apps/Ingestion Plane/integration-corev2/internal/{auth/middleware.go, controlplane/clients.go, api/connection_access.go, api/server.go, attestation/verifier.go}`
- `apps/Control Plane/auth-core/src/auth/{plane-token.controller.ts, plane-service-principal.ts}`
- `apps/Control Plane/user-core/internal/spaces/{decision.go, decision_test.go, scheduled_run_decision.go, personal_thread_decision.go, shared_thread_read_decision.go, schedule_create_decision.go}`
- `apps/Model Plane/rust/services/session-core/src/{grpc.rs, auth.rs}`
- `apps/Model Plane/rust/services/model-gateway/src/grpc_auth.rs`
- `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs`
- Scratchpad reports: `batch2/security.md`, `batch2/schedule.md`, `batch1/mail-task.md`