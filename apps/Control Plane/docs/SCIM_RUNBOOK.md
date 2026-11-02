# SCIM runbook: provisioning and deprovision

Operator companion to build plan WS05 (`docs/plans/space-hybrid-build-plan-2026-10-14.md`, slices S8 and S9). Owner: Control Plane (`org-core`, `auth-core`, `user-core`) with the stateless edge in the Frontend gateway.

**Status.** The code is built and off by default. This file says what the code does and what to do with it. It does not say SCIM is switched on, or that any IdP has been tested against it, in any environment. Facts were checked against the source tree on 2026-11-02. Anything marked UNVERIFIED has not been run.

## 1. What is built

SCIM 2.0 `Users` only (`ServiceProviderConfig`, `ResourceTypes`, `Schemas`, `Users`). `Groups` answers 501.

| Hop | What it does |
|---|---|
| IdP | Sends `Authorization: Bearer <org SCIM token>` to `/api/v1/scim/v2/*` on the gateway. No session cookie. |
| Gateway edge (`domains/scim.rs`) | Stateless. Refuses with 501 unless `SCIM_EDGE_ENABLED` is on and its token is set. Forwards the bearer opaquely, at most 64 KiB, only `filter`, `startIndex`, `count`. Never forwards client `x-org-id` or `x-service-*` headers. |
| org-core | Under principal `verevon-scim-edge` (scope `org:scim:any`): token lookup by SHA-256 hash (revoked, expired or deleted org gives 401), verified-domain check per user, state in `organization_scim_users`, 300 requests per minute per token (429 with `Retry-After`). |
| auth-core | `POST /api/v1/internal/scim/memberships/{provision,deprovision}` through the existing membership mutation machinery (kind `scim_deprovision`, migration 030), so revision, outbox row and audit actor commit atomically. Deprovision also deletes the sessions whose active organization is this org. |

Rules that cannot be configured away:

- The role is always `member`. A `roles` or `groups` attribute is ignored, so SCIM can never grant admin.
- The last owner is never removed: 409 `mutability`, audited as `org.scim.deprovision.refused_last_owner`.
- Only `userName` (an e-mail whose domain is verified for the org), `externalId` and `active` are read. Other attributes are accepted and ignored so IdP retries never fail.
- Re-activation restores `role_at_deactivation`, except `owner`, which comes back as `admin`.
- `DELETE` answers 204 and keeps a tombstone.

## 2. Switches

All default to off.

| Variable | Read by | Effect |
|---|---|---|
| `ORG_SCIM_ENABLED` | org-core | `1` or `true` mounts the admin token routes and `/internal/scim/v2/*`. Otherwise 404. |
| `ORG_CORE_SCIM_EDGE_TOKEN` | org-core and gateway | Secret of principal `verevon-scim-edge`. The principal exists only while `ORG_SCIM_ENABLED` is on and this is set. |
| `SCIM_EDGE_ENABLED` | gateway | Must be on, with a secure token, or the edge answers 501. |

Kill switch: turn `SCIM_EDGE_ENABLED` off (the IdP gets 501 and nothing reaches org-core), or `ORG_SCIM_ENABLED` off. Neither removes data or reverses a deprovision already applied.

## 3. Setting up a tenant

1. In Settings, Identity: verify a domain (`organization_identity_domains`). Without one, creating a SCIM token answers 422 `no_verified_domain`.
2. Create a SCIM token. It is shown once. At most 3 active tokens per org (409 `scim_token_limit`). Only an owner or admin can create or revoke.
3. Configure the IdP with base URL `https://<host>/api/v1/scim/v2` and the token as bearer. Map `userName` to the user's e-mail and `active` to the IdP's assignment state.
4. Assign one test user and read the section 5 check list before assigning anyone else.

Rotating a token: create the new one, update the IdP, revoke the old one. A revoked token is refused on the next request.

## 4. What deprovision does, and how fast

`PATCH active=false`, `PUT active=false` and `DELETE` all take this path.

1. org-core derives an operation id from `(org_id, scim_id, target_active, revision)`, so a retried PATCH cannot apply twice.
2. auth-core removes the `member` row and deletes the user's sessions active in this org, in one transaction, and writes the outbox row.
3. The outbox publishes `auth.organization.member_removed` (and the `aqencia.controlplane.org.member_removed` form). Consumers:

| Consumer | Effect | Mechanism |
|---|---|---|
| auth-core (immediate) | membership gone, org sessions gone | same transaction as the removal |
| gateway | live membership check fails closed: 403 `organization_membership_required`; plane-token mint fails | live membership lookup per the WS05 design; UNVERIFIED in code |
| user-core | `user_org_memberships.status = 'removed'`; every Space decision resolver joins `status = 'active'`, so new decisions are refused | NATS subscriber (`handleOrganizationMemberRemoved`) |
| Convex | membership projection removed | NATS consumer |
| Session Core | revocation applied, in-flight runs end | `space.membership_changed` |

### Residual windows (stated honestly)

- **Event lag.** The `member_removed` event travels on plain NATS. A consumer that is down or slow keeps the old state until it catches up. The Space membership event (`controlplane.space.membership_changed`) is durable since WS04-S10 (outbox with PubAck and a 10-minute re-announce); the org-level `member_removed` event is not covered by that change. UNVERIFIED how long a restarted user-core takes to see a missed event.
- **Decisions already issued** stay valid until their expiry, at most 2 minutes. A removed person can finish an effect that was authorised before the removal.
- **In-flight runs** end when Session Core consumes the revocation, not at the moment of the PATCH.
- **No agent cascade.** Control holds no agent-to-owner link (ADR 4). Personal and room agents owned by the deprovisioned person are stopped only indirectly, when their owner's re-authorization fails. The agent half of this work is owned by WS17-S4 and is not built here. `org.scim.user.deprovisioned` and the member-removed event are what it will consume.
- **The IdP's own state.** Deprovision removes access to this org only. The user account itself, and memberships in other orgs, are untouched.

## 5. Verifying propagation: `scripts/verify-deprovision.sh`

Run it on a local or test stack, never against a real tenant's users. It provisions a test user, PATCHes `active=false`, polls each consumer for a 30 second budget, replays the PATCH and prints a table. `--apply` is required to send anything.

```
SCIM_TOKEN=... SCIM_ORG_ID=<org id> SCIM_USER_NAME=<test e-mail> \
  bash scripts/verify-deprovision.sh --apply
```

Required rows (the script exits 1 if any is not PASS): `auth-core`, `org-core`, `user-core`. Optional rows (`gateway`, `convex`, `space-decision`, `session-core`) run only when you give an executable in `PROBE_GATEWAY_CMD`, `PROBE_CONVEX_CMD`, `PROBE_SPACE_CMD` or `PROBE_SESSION_CORE_CMD`; without it the row says SKIPPED, which is not a pass. The script never prints the token.

Read the table as a measurement of propagation, not a guarantee. It does not measure the 2-minute decision expiry in section 4.

Per-issuer fail-closed coverage is a live Go test, `user-core/internal/spaces/decision_deprovision_live_test.go` (to be written; see the build ledger), which sets `user_org_memberships.status = 'removed'` and asserts that every effect-bound decision route refuses the user.

Record the first real run (IdP log, `organization_scim_users` row, `member` row gone, sessions gone, user-core status, Convex projection, a refused Space decision, timings) in the wave's live-proof file under `docs/plans/`. UNVERIFIED until run against a real IdP test tenant (Entra or Okta).

## 6. Failure modes

| Symptom | Likely cause | Action |
|---|---|---|
| IdP gets 501 | `SCIM_EDGE_ENABLED` off, or `ORG_CORE_SCIM_EDGE_TOKEN` missing or weak | set both, restart gateway |
| 404 on `/internal/scim/v2` or token routes | `ORG_SCIM_ENABLED` off | set it, restart org-core |
| 401 with `WWW-Authenticate` | token revoked, expired, wrong org, or mistyped | create a new token |
| 400 `invalidValue` on POST | `userName` domain not verified for this org | verify the domain first |
| 409 `uniqueness` | the `userName` already exists in this org | look it up with `filter=userName eq "..."` |
| 409 `mutability` on deprovision | the user is the last owner | transfer ownership first; the refusal is audited |
| 429 | over 300 requests per minute for the token | honour `Retry-After` |
| deprovision answered 200 but user-core still `active` | `member_removed` event not consumed | check user-core logs for the subscriber and NATS connectivity; the row corrects itself on the next event, it does not poll |
| Table shows `MISSED` for auth-core | auth-core transaction rolled back | the PATCH response and auth-core logs name the cause; replay is safe |

## 7. What not to do

- Do not widen the `verevon-scim-edge` scope or reuse its token for another service.
- Do not put a SCIM token, an operation id secret or an IdP secret in a ticket, a log or this file.
- Do not delete `organization_scim_users` rows by hand: the tombstone is what keeps retries idempotent.
