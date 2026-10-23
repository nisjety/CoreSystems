# WS1.C design note: the Postgres grant store of browser-broker

Unit R3-ws1c, 2026-10-18, branch quarryv2-next. Written before coding.

## Schema (`migrations/0001_browser_grants.up.sql`, every statement IF NOT EXISTS)

`browser_grants(id TEXT PK, kind TEXT CHECK IN ('session','run','sensitive_approval'), org_id TEXT <> '',
owner_id, session_key, scope_url, allowed_domains TEXT[] CHECK cardinality 1..32, allowed_actions /
frame_ids / dialog_ids / artifact_ids TEXT[] DEFAULT '{}', parent_grant_id TEXT REFERENCES browser_grants(id)
ON DELETE CASCADE, created_at, expires_at, absolute_expires_at (= expires_at today; human_browse's 4 h
backstop later, CHECK expires_at <= absolute_expires_at), revoked_at, revoked_reason)`.
Named CHECKs: `(kind = 'session') = (parent_grant_id IS NULL)` and `id NOT LIKE 'z\_%'`. No `zdr` column.
Indexes: `browser_grants_owner_active_idx (org_id, owner_id, expires_at) WHERE kind='session' AND revoked_at IS
NULL`, `browser_grants_parent_idx (parent_grant_id)`, unique `browser_grants_run_idempotency_idx
(parent_grant_id, session_key) WHERE kind='run'`, `browser_grants_expiry_idx (expires_at)`. Down: DROP TABLE IF
EXISTS.

## Clock

All expiry arithmetic happens in Go with the store's `now` (truncated to microseconds) passed as a
parameter, exactly like MemoryRepository. One replica, so no cross-host skew; and the same contract
tests (with a fake clock) run against both stores.

## Derive: one READ COMMITTED transaction

1. `SELECT ... WHERE id=$parent FOR SHARE`; scope (org, owner filter) -> NotFound; revoked -> Revoked; expired
   -> Expired. Kind/actions -> NotDerivable; PrincipalZdr -> ZdrPostureChanged (a Postgres parent is never ZDR).
2. Host derivation and the policy re-check (same functions as memory).
3. `pg_advisory_xact_lock(<ns:run>, hashtext(parent_id))`: serialises derives under one parent, so the
   count-then-insert below is exact.
4. Existing (parent, key) row -> replay rules (revoked / expired / different hosts / same grant unchanged).
5. Expiry = min(now+ttl, parent expiry), refuse < 30 s; count live runs under the parent, refuse >= cap.
6. `INSERT ... ON CONFLICT (parent_grant_id, session_key) WHERE kind='run' DO NOTHING RETURNING`; no row ->
   re-read the winner and apply the replay rules (belt and braces; the advisory lock already serialises).

Approvals: read parent plain, lock the chain top-down (`FOR SHARE` on the root session, then the parent),
so a concurrent revoke of the root cannot interleave (no lock-order cycle with revoke).

## Revoke subtree, and why it is two statements in one transaction

`SELECT ... WHERE id=$1 FOR UPDATE` (scope check -> NotFound) waits for every in-flight derive/approval
holding FOR SHARE on that row. THEN the single `WITH RECURSIVE subtree ... UPDATE` runs in a fresh statement
snapshot, so it sees any child those transactions committed. Doing it as one statement would compute the
subtree from a snapshot taken before the lock wait and could miss a just-committed child (live run grant
under a revoked session). Reverse order: a derive arriving after the revoke's lock waits on FOR SHARE and
then reads `revoked_at` (EvalPlanQual) -> ErrGrantRevoked.

## Caps: per-owner sessions, per-parent runs (reconciling the plan)

The plan text says "advisory lock for an exact per-owner cap" with `hashtext(org||':'||owner)`. The WS1.B
review (73ad8767) decided run grants do NOT count toward the per-owner cap (20 sessions) and are bounded per
parent session (8 live). Both are kept and both are exact under concurrency: CreateSession takes
`pg_advisory_xact_lock(<ns:owner>, hashtext(org||':'||owner))`, Derive the per-parent lock above.
`TestPg_PerOwnerCapIsExactUnderConcurrency` checks both.

## Routing (`router.go`)

`Routed{persistent, ephemeral}`: CreateSession routes by `p.Zdr`; approvals, Derive, GetScoped, RevokeScoped by
the `z_` prefix of the (parent) id. Subtrees never span stores: a ZDR parent's children are `z_`; a
non-ZDR parent refuses a ZDR caller (Derive) and PgStore refuses a ZDR-raised approval
(ErrZdrPostureChanged, fail closed: no `zdr` column to record it, and a memory child of a Postgres
parent would escape the cascade). PgStore also refuses `p.Zdr` creates and answers NotFound for a `z_` id
without querying (defence in depth).

## Restart semantics

| Event | Non-ZDR grant | ZDR grant |
|---|---|---|
| Broker restart | survives (row in Postgres) | lost; next /step 403 inactive |
| Broker down | 5xx/503, no fail-open | same |
| Postgres down | store errors -> gRPC Unavailable; /readyz 503 | unaffected (memory) |
| Migration re-applied on start | no-op (IF NOT EXISTS) | n/a |

Purge: every 10 min, rows with `expires_at < now - 24h` (children cascade); memory store purges too.

## Will not do

Compose, CI workflow, Control Plane edits (compose lines go to wf-R3-ws1c-compose.md); a dedicated database
role; renewal / absolute_expires_at semantics (WS8); rebuilding or restarting the live broker; the
deployment-evidence run (needs a rebuilt broker in the live stack).
