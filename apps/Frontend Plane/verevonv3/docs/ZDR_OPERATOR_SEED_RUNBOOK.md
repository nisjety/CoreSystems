# ZDR operator seed runbook (one dedicated test org only)

Owner: Frontend Plane (the gateway gate) with Control Plane (org-core). Status: operator-only tool for the Z-60 live proof. This is not a product feature and it is not for any real organization.

## 1. What this is for, and why it exists

The org Zero Data Retention switch is gated on an attested provider. The gateway (`apps/gateway/src/domains/orgs/settings.rs`) reads the model catalogue and refuses to turn ZDR on unless at least one chat model's provider has a recorded zero-retention attestation:

| Answer | Meaning |
| --- | --- |
| `409 zdr_no_attested_provider` | the catalogue was read and no chat model advertises the `zdr` feature (or a `zdr_contractual` / `sovereign` privacy tier). Nothing is written. |
| `503 zdr_gate_unavailable` | the catalogue could not be read or minted for. Nothing is written. The gate fails closed on enable only. |
| turning ZDR off | never refused and never reads the catalogue. |

The same gate stands in front of every route that can write `interactiveRetention.zdr = true`: the settings page (`PATCH /api/v1/orgs/:id/settings`), the `org.update_zdr` agent action, and organization creation (the onboarding step carries the same switch). On creation the gate is stricter than the table says: a first-time creator has no active organization, auth-core mints them no catalogue credential, and the answer is always `503 zdr_gate_unavailable`, even once a provider is attested (section 8).

That is right for every real organization. With nothing attested, inference-core refuses every ZDR request, so an org with ZDR on could not chat at all. Today no provider is attested (owner default D-A: attestation is an operator and commercial step, built as data not code).

The live proof of the ZDR work (Z-60) needs ONE test org that has the switch on while nothing is attested yet. The gate lives in the gateway only, because the gateway is the only holder of org-core's `org:settings:write:self` and the only service that can read the Model Plane catalogue. org-core has no gate and cannot have one without a Control-to-Model dependency. So org-core itself is the operator-only way in: a signed `PATCH /api/v1/organizations/:id/settings` straight to org-core, bypassing the gateway.

## 2. Rules

1. A dedicated test org, created for this. Never an existing org, never a customer org. With nothing attested, switching ZDR on breaks that org's chat on purpose.
2. Its name starts with `ZDR-TEST` (for example `ZDR-TEST terra proof`). The script refuses any other org, and refuses unless the stored name equals the name you type.
3. The credential never goes on a command line, into chat, or into a log. It is read from the environment and never printed.
4. Switch it back off when the proof is done (section 5). An org left on is an org whose chat is refused.

## 3. Prerequisites

1. **The test org.** Create it through normal sign-up and onboarding, named `ZDR-TEST ...`. Leave ZDR off in onboarding (with nothing attested the onboarding step is refused with the same `zdr_no_attested_provider`). Put it on a plan that includes ZDR, through the normal plan path: org-core refuses ZDR on a plan without it (`402 plan_upgrade_required`), and this script does not bypass that.
2. **The org id and its owner's user id.** Read-only lookup on the Control Plane database (the compose default role is `coresystem`; there is no `postgres` role in this container):

   ```
   docker exec controlplane-postgres psql -U coresystem -d org_core -At -F '|' -c "SELECT o.id, o.name, o.plan, m.user_id, m.role FROM organizations o JOIN organization_members m ON m.org_id = o.id AND m.status = 'active' WHERE o.name ILIKE 'ZDR-TEST%' AND o.deleted_at IS NULL"
   ```

   The acting user must be an active member of the org: org-core's membership guard re-checks it before the write. Use the owner.
3. **The credential.** org-core grants `org:settings:write:self` to exactly one principal, `verevon-gateway`, and only `:self` scopes (which need a signed delegation naming a real org member) are valid for that principal. So the operator credential is the gateway's org-core credential: `GATEWAY_ORG_CORE_SERVICE_TOKEN` in the Control Plane configuration, which the gateway container receives as `ORG_CORE_SERVICE_TOKEN`. Load it into your shell from the deployment secret store without echoing it, for example:

   ```
   # bash. Prints nothing. <secrets file> is the deployment's secret file, never a repo file.
   export ORG_CORE_SERVICE_TOKEN="$(grep -m1 '^GATEWAY_ORG_CORE_SERVICE_TOKEN=' <secrets file> | cut -d= -f2-)"
   ```

   ```
   # PowerShell. Prints nothing.
   $env:ORG_CORE_SERVICE_TOKEN = ((Get-Content <secrets file> | Where-Object { $_ -like 'GATEWAY_ORG_CORE_SERVICE_TOKEN=*' } | Select-Object -First 1) -split '=', 2)[1]
   ```

   Close the shell (or `unset ORG_CORE_SERVICE_TOKEN`) when you are done.
4. **org-core on loopback.** org-core publishes `127.0.0.1:18080` (`ORG_CORE_HTTP_HOST_PORT`). The script only talks to loopback, so the credential cannot leave the host. Run it on the host that runs the stack.

## 4. Run it

From `apps/Frontend Plane/verevonv3`. First the plan, which sends nothing at all:

```
node apps/gateway/scripts/zdr-seed-test-org.mjs --org-id <ORG_ID> --user-id <OWNER_USER_ID> --expect-name "ZDR-TEST terra proof"
```

It prints the org, the acting user, the two requests it would send and what it would switch. Then apply (`--zdr true` is the default):

```
node apps/gateway/scripts/zdr-seed-test-org.mjs --org-id <ORG_ID> --user-id <OWNER_USER_ID> --expect-name "ZDR-TEST terra proof" --apply
```

What `--apply` does, in order:

1. Signs and sends `GET /api/v1/organizations/<ORG_ID>` (scope `org:read:self`). It refuses, and sends nothing else, unless the org's id matches, its stored name equals `--expect-name` exactly, and the name starts with `ZDR-TEST`.
2. Signs and sends `PATCH /api/v1/organizations/<ORG_ID>/settings` with `{"zeroDataRetention":true}` (scope `org:settings:write:self`).
3. Prints the HTTP status and the org's stored posture. Never the credential, never a signature.

Pass: `ZDR is now ON for org <ORG_ID>` and exit code 0. Failure modes you can meet:

| Output | Meaning |
| --- | --- |
| `refused: org ... is named ...` | wrong org or wrong name. Nothing was changed. |
| `reading the org failed: HTTP 401/403` | the credential is wrong or lacks the scope, the acting user is not an active member, or the clock is more than 30 seconds off the stack. |
| `switching ZDR failed: HTTP 402 (plan_upgrade_required)` | the org's plan does not include ZDR. Fix the plan the normal way. |
| `HTTP 404` | the org id is not in org-core. |

Side effects are the same as turning it on in the UI: org-core records the intent, and a false-to-true change publishes the content-free cleanup event that purges that org's conversation-core drafts. org-core emits no audit event for this change (its only audit publisher is the GDPR erasure path; `SetInteractiveRetention` only stores `updatedBy` and `updatedAt` in the metadata), and `updatedBy` will name the owner you passed as `--user-id`, not you. The seed is a signed delegation as that owner. Keep your own note of who ran it, when, and for which org id, and put it in the Z-60 evidence.

## 5. Switch it back off

```
node apps/gateway/scripts/zdr-seed-test-org.mjs --org-id <ORG_ID> --user-id <OWNER_USER_ID> --expect-name "ZDR-TEST terra proof" --zdr false --apply
```

Turning ZDR off through the settings page also works and is never gated.

## 6. What you will see while it is on and nothing is attested

On the test org's Organization security page the switch shows on and a banner says no provider has a recorded attestation right now. The switch stays usable so the admin can turn it off. The composer's Temporary chat is not offered. Once the Control Plane posture work is deployed (Auth Core resolving the org's ZDR into the token), a user turn from that org is refused by inference-core, which fails a ZDR request closed. That is the state the Z-60 proof starts from.

The gateway still refuses the same request from the UI or the API:

```
PATCH /api/v1/orgs/<ORG_ID>/settings {"zeroDataRetention":true}  ->  409 zdr_no_attested_provider
```

and writes nothing. Do not toggle any other org to check this: run it against the test org only, or read the gateway's tests.

## 7. How this is tested

| Test | What it proves |
| --- | --- |
| `apps/gateway/src/domains/orgs/settings.rs` (`cargo test domains::orgs::settings`) | through the real router, session middleware, audience-token mint and upstream credential headers: 409 with zero org-core requests while nothing is attested, 503 on any catalogue failure, off is never gated, members are refused first, the agent action and organization creation are gated the same way. The catalogue double is `apps/gateway/tests/fixtures/zdr-model-catalogue-cases.json`, shaped like model-gateway's `ListModelsHttpResponse`. |
| `src/shared/api/chat-client.zdr-capability.test.ts` and `src/features/settings/components/WorkspaceSettingsPage.zdr-gate.test.tsx` | the composer, the settings toggle and the gateway read the one fixture and answer alike; the toggle is disabled with the reason, fails closed, and shows the lapse banner. |
| `apps/gateway/scripts/zdr-seed-test-org.test.mjs` (`node --test`) | the signer reproduces fixed vectors, the script's refusals (wrong name, wrong org, non-loopback, no credential), nothing secret is printed, and org-core's settings handler stays free of any catalogue or attestation reference. |
| `apps/Control Plane/org-core/internal/http/zdr_seed_delegation_vector_test.go` (`go test ./internal/http -run ZdrSeed`) | org-core's REAL verifier, middleware and membership guard accept the script's requests, refuse any tampering, replay or stale timestamp, and the settings route is the one the gateway proxies to. |

The two vector tests carry the same literals and each fails if they drift apart.

The catalogue fixture was built from model-gateway's wire struct (`ListModelsHttpResponse` / `ModelDescriptor` in `http_routes.rs`) and what inference-core emits, because a real body needs a signed-in session. Close that gap on the deployed stack, read-only:

1. `pnpm exec playwright test tests/e2e/zdr-org-switch-gate.spec.ts` (the first two tests write nothing). The second one fails if a live model entry lacks `id`, `modality`, `features` (a list) or `privacy_tier`.
2. Signed in, open `/api/v1/models` in the browser, save the JSON, and compare one entry's keys with case 1 of `apps/gateway/tests/fixtures/zdr-model-catalogue-cases.json`: `id`, `provider`, `modality`, `streaming`, `features`, `privacy_tier`, `residency`.

The third test of that spec sends the one refused PATCH and is opt-in (`ZDR_GATE_LIVE_PATCH=1`). It never sends it while a provider is attested, while the org already has ZDR on, or for any org that is neither a `ZDR-TEST` org nor the e2e fixture account's own org (a gateway build without the gate would switch that org on and queue the draft purge, which switching it off cannot undo).

## 8. Known limits and follow-ups

- There is no separate operator principal. org-core only lets `verevon-gateway` hold `:self` scopes and allows no `org:settings:write:any`, so the credential here is the gateway's own. A dedicated operator scope is a Control Plane change.
- `metadata.interactiveRetention` is stored by org-core exactly as sent on organization create and on re-mirror (`provisionOrganizationWithOwnerTx` merges the metadata object), with no plan check and no outbox event. The Auth Core projection reconcile (`ReconcileOrganizationProjection`) goes through the same merge, so whatever Better Auth holds under that key reaches it too. The gateway's organization-creation route is gated; any other writer of that key is not. The durable fix is org-core treating `interactiveRetention` as server-owned and writing it only through `SetInteractiveRetention`.
- Organization creation cannot switch ZDR on for a first-time creator, attested provider or not. The creator has no active organization, auth-core answers 400 "No active organisation on session" to both the model-plane and the inference-core token mints, and the gate fails closed with `503 zdr_gate_unavailable`. The gateway test `a_first_time_creator_cannot_switch_zdr_on_at_creation_even_when_a_provider_is_attested` pins this. The 503 copy tells the user to try again shortly, which does not help here; carrying on without it and switching it on in the settings does. The lasting fix is a pre-organization catalogue credential from the Control Plane (or an onboarding that applies the switch after the organization exists), and the onboarding toggle disabled up front.
- When a provider is attested (operator step Z-40), the settings gate opens by itself: it reads the catalogue, not a flag. Nothing here needs undoing.
