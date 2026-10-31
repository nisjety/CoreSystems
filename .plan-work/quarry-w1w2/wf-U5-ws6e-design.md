# WS6.E design note: the grant-carried budget claim

Unit U5-ws6e, 2026-10-19, branch quarryv2-next. Written before coding.

## What the claim is

Five additive fields, the same numbers on every message that carries them (R-6: 40-44):
`budget_micro_usd=40`, `budget_max_steps=41`, `budget_max_runtime_s=42`, `budget_source=43`,
`budget_minted_at_unix=44` on `AcquireGrantRequest/Response`, `ValidateGrantResponse` and
`DeriveRunGrantRequest/Response`. Zero means "no bound on that dimension"; `budget_source` is
`request | org_default | entitlement` and is required exactly when some dimension is set.

The claim is an **upper bound that can only tighten**. Quarry computes
`effective = min(request-or-default, claim)` per dimension, after its own ceiling; a claim above what the
request or default already gives changes nothing, so a minter can never raise a run's budget, and nothing a
caller does with a grant can either. That is the property that makes it safe for a grant to carry numbers.

## Where it is minted and enforced

- **Minted** by model-gateway `mint_budget` (flag `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM`, default off): cost
  is `min(request.max_cost_usd, org-core per-run ceiling)`, steps and runtime come from the request;
  `source` says which side bound it. org-core failing is fail-open (Quarry's ceiling still applies). The
  claim rides `DeriveRunGrantRequest`, so it exists only on run grants derived for an AI run. A run grant a
  caller presents is not re-minted (the broker has no way to change an issued grant). Where the minter lives
  is ADR-1; only `mint_budget` moves if that changes.
- **Stored and returned verbatim** by the broker. `minted_at` is stamped by the broker's clock, never taken
  from a request (a non-zero request value is InvalidArgument). A derive may only tighten the parent: each
  dimension set on the parent must be set on the child and not above it, an omitted dimension inherits the
  parent's, a larger one is `ErrDeriveWidensBudget` (PermissionDenied, outcome `derive_widens_budget`).
  Approvals cannot carry a budget.
- **Enforced** by Quarry at `start_run` only, behind `QUARRY_EDGE__GRANT_BUDGET_CLAIM=ignore|enforce`
  (default `ignore`: the field is parsed and nothing changes). In `enforce` the claim is passed into
  `resolve_caps`; a tighter value is reported as `clamped[].by = "grant"`. The claim is snapshotted into
  the run's budget and its checkpoint, so per-action grant revalidation (which only checks usability and
  hosts) never reads it again, and a resume takes the tighter of the stored claim and the grant's.
  A grant of kind `run` classes the run as `agent` in `enforce` (R-8), so a user token holding a run grant
  is not an uncapped run.

## Who owns what

Broker: the grant row and the tighten rule (Model Plane). model-gateway: minting (Model Plane). Quarry:
enforcement and the effective-limit report (Ingestion). Control Plane: nothing here; org-core is read for
the existing per-run ceiling only. execution-core (mapping the 402 to a resource-limit outcome, the planner
token meter) is sibling (b)'s and is not touched.

## ZDR

The claim is three numbers and a label, no content, so it is allowed on a ZDR grant. ZDR grants live only in
the broker's memory (`z_` ids, X-2) and so does their claim. Quarry writes no checkpoint for a ZDR run, so no
claim is persisted there either. Nothing here logs a host, URL or title.

## Migration

`0003_browser_grants_budget.up.sql`: `ADD COLUMN IF NOT EXISTS` for the four plan columns plus
`budget_minted_at_unix BIGINT` (the plan lists four; the fifth proto field needs a place to live), each with
an inline non-negative CHECK, all nullable, no backfill. 0002 stays reserved for WS7. The entrypoint re-applies
every file at each start, so every statement is idempotent. Down drops the columns. An old broker returns
zeros and an old Quarry ignores the fields; the checkpoint field is optional (`#[serde(default)]`).

## Will not do

Deploy or rebuild any container; flip either flag; WS6.D entitlement minting (`source=entitlement` is
accepted and reserved); the execution-core side (sibling b); WS1.A2 service credential (X-1); regenerate the
Python or TypeScript proto trees (stale on purpose, in the stale-doc register); change the broker image,
entrypoint or CI (U1).
