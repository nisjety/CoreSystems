# Retiring `spaceMemberGrants` (WS04-S12)

`spaceMemberGrants` was Application's hand-kept list of a named room's people.
It was only ever an intent record: Control's `space_memberships` ledger is the
authority, and since WS04-S6 the gateway writes membership through Control's
source-tagged writer. The table, its mutations (`addSpaceMemberForGateway`,
`removeSpaceMemberForGateway`), its roster query (`roomMemberGrantsForGateway`,
`roomMembersForControl`) and the declarative sync (`syncRoomMembersForGateway`)
are removed. The gateway add and remove routes answer `404 feature_disabled`
when `SPACE_MEMBERSHIP_WRITER_API` is off; there is no legacy fallback.

## Preconditions

- At least one release has run with `SPACE_MEMBERSHIP_WRITER_API` on (M4).
- The previous release is still deployed when you run the check below.

## Migration check (run before deploying this change, per deployment)

A row is dangerous only if it is the sole record of a member: a grant whose
declaration to Control never succeeded (the gateway answered
`room_membership_unconfirmed`).

1. List the rows: `npx convex data spaceMemberGrants --limit 1000`
   (or the dashboard data view). Note each `(spaceRef, externalAuthId)`.
2. For each row, confirm the person has an active row in Control's roster for
   that room (the room's Members tab, or `GET /api/v1/spaces/{space_ref}/members`
   through the gateway). A row also present in Control is redundant.
3. For each row NOT in Control, add the person again through the Members tab
   (the writer path). Do not delete the row until Control shows the person.
4. When every row is confirmed, delete the rows (dashboard bulk delete, or
   `npx convex import --replace --table spaceMemberGrants` with an empty file).
   Deploy with the table still defined if any row remains: Convex rejects a
   schema push that no longer matches existing documents.
5. Deploy this change, then `docker restart convex-gateway` and wait for
   "Convex functions ready!".

## Rollback

Flag off is not a rollback: the legacy path is gone. Revert this commit to
restore the table, functions and the gateway legacy branch (no data is lost
while the table is empty).
