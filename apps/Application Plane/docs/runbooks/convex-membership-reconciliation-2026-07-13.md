# Convex membership projection reconciliation — 2026-07-13

Purpose: remove excess access and demote over-privileged roles in one organization after the hardened Convex schema/functions/subscriber are deployed. The tool is dry-run by default and cannot create memberships or promote roles.

As of 2026-07-13 this procedure has **not** been executed against the live projection. Do not apply it until Control publishes authoritative signed/revisioned membership events, an executing backend test proves revocation, and an operator has backed up Convex.

## Safety properties

- one explicit organization per invocation;
- Control Plane is read as authority;
- dedicated `CONVEX_RECONCILIATION_KEY`, separate from generic internal/NATS keys;
- HMAC-SHA256 covers timestamp, method, path, organization, nonce, and exact body;
- five-minute timestamp window and server-side nonce replay claim;
- apply requires `--apply --confirm-org <exact-id>`;
- removal/demotion only; missing authoritative members and promotions are reported, never granted;
- matching mirrored Control Sessions are removed when access is revoked;
- audit output contains counts/IDs needed for review, not secrets.

## Prerequisites

1. Record the Convex backend/function/subscriber revisions and immutable image IDs.
2. Back up the Convex volume and prove the backup can be opened in an isolated restore test.
3. Confirm the Control membership endpoint returns only the selected tenant and defines active status/role semantics.
4. Load `CONTROL_PLANE_INTERNAL_KEY` and `CONVEX_RECONCILIATION_KEY` from the approved secret store without printing them.
5. Set the two internal URLs from the operator network:

```bash
export CONTROL_PLANE_ORG_CORE_URL='http://org-core:8080'
export CONVEX_HTTP_ACTIONS_URL='http://convex-backend:3211'
test -n "$CONTROL_PLANE_INTERNAL_KEY"
test -n "$CONVEX_RECONCILIATION_KEY"
```

## Dry-run

From `apps/Application Plane/convex-core`:

```bash
export TARGET_ORG_ID='<approved-organization-id>'
node scripts/reconcile-memberships.cjs --org "$TARGET_ORG_ID" \
  >"/secure/operator/path/convex-membership-${TARGET_ORG_ID}-dry-run.json"
```

Review the report with the Control owner and tenant owner:

- removals are projection rows absent from Control active membership;
- demotions match authoritative roles;
- `missing` and unsafe promotions are investigation items and remain unapplied;
- unexpected duplicates, unknown roles, future timestamps, or excessive scope are abort conditions.

Run the same dry-run twice. The second plan must be identical before apply. A dry-run changes only the short-lived nonce claim used for replay protection; it does not change membership.

## Apply

Apply only after a signed change approval names the exact organization and backup:

```bash
node scripts/reconcile-memberships.cjs \
  --org "$TARGET_ORG_ID" \
  --apply \
  --confirm-org "$TARGET_ORG_ID" \
  >"/secure/operator/path/convex-membership-${TARGET_ORG_ID}-apply.json"
```

Immediately verify with approved seeded identities:

1. removed member cannot read org-scoped planner documents, knowledge Q&A, projects, agent runs, or Control Session mirrors;
2. removed member cannot mutate any org-scoped projection;
3. retained member still has only the authoritative role;
4. Control-authoritative membership is unchanged;
5. audit and subscriber/DLQ telemetry contain the reconciliation correlation without email/token content;
6. a fresh dry-run reports no remaining removal/demotion for the organization.

## Failure and compensation

Stop on any partial failure. Do not widen access to compensate. The reconciliation is idempotent for removal/demotion, so retry the same tenant only after identifying the failure and confirming the authority response is unchanged.

If a legitimate member was removed because Control returned bad authority data, restore access from a corrected, revisioned Control event or an owner-approved Control change—never by editing Convex directly. Preserve the audit record and dry-run/apply artifacts. If the deployed functions/schema are faulty, roll back the Convex release using the recorded image/function revision and restore the volume only under the Convex owner’s tested restore procedure.
