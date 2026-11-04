# W2-AP-S7a status: WS10.S7a Application Plane browserActivity schema and owner-only projection (lane c, branch w23/lane-c)

## Built
All of the scope. Code only, not compiled, no tests run (owner order 2026-11-03).

Checked first: nothing of it existed (no `browserActivity` anywhere in convex-core).

- `convex/schema/agents.ts` (the `agentRuns` table lives here since the P-CX1 split; `convex/schema.ts` only composes the groups and was not touched): optional `browserActivity: { lastProducerSeq, lastEventType, lastEventAt, sessionState?, control?, blockedEgress?, steps?, artifacts? }` (numbers and strings; counters approximate).
- `convex/agentRuns.ts`:
  - `upsertBrowserActivity` (internalMutation; args `runId, externalOrgId, producerSeq, eventType, eventAt` plus the optional fields): patches an existing run row only (an unknown run creates nothing), refuses an `externalOrgId` different from the row's, ignores a `producerSeq` <= the stored `lastProducerSeq`, refuses non-label strings and non-integer or negative counters (no payload text can land). Omitted optional fields keep their previous value. Returns `{ applied: true }` or `{ applied: false, reason: unknown_run | org_mismatch | stale_seq | invalid }`; never throws for these. Bumps `updatedAt` on an applied write.
  - `getByRunId`: same membership check as before; returns `browserActivity` only when the viewer's `externalAuthId` equals the run's `externalUserId`; other org members get the row without it; a run with no `externalUserId` shows it to nobody.
  - Exported for tests: `decideBrowserActivityUpdate`, `projectRunForViewer`, `upsertBrowserActivityHandler`, `getByRunIdHandler`, types `BrowserActivity`, `BrowserActivityUpdate`.
- `convex/agentRuns.browserActivity.test.ts` (NEW): node:test via tsx against an in-memory ctx (convex-test style; convex-test is not a dependency): owner sees it, `non_owner_org_member_sees_no_browser_activity`, outsider denied, a run with no owner hides it, `lastProducerSeq_never_moves_backwards` (stale and duplicate), `an_org_mismatch_is_rejected`, `a_trace_event_for_an_unknown_run_creates_nothing`, a merge keeps previous fields, `projection_holds_no_payload_text`. Run: `cd "apps/Application Plane/convex-core" && npx tsx --test convex/agentRuns.browserActivity.test.ts`.

## Commits (lane c)
- e29c504ee feat(convex-core): agentRuns browserActivity projection, owner-only read (WS10.S7a)
- 14ac4ecb6 test(convex-core): browserActivity owner-only, stale seq and wrong org (WS10.S7a)
- docs(plan-work) commit with this file, the MP ask and the depth rows

## Flag and default
No flag (as the queue says): inert without producers. Nothing calls `upsertBrowserActivity` until W2-AP-S7b's consumer (subscription off by default) and a Model Plane trace producer exist; until then no row has `browserActivity` and `getByRunId` returns exactly what it did before.

## Assumed decisions
- `sessionState` and `control` are label strings, not literal unions: the producer vocabulary (WS10.S2 types 150-159) is not frozen here. Labels are bounded to `[A-Za-z0-9_.:-]{1,64}`.
- Counters are totals from the consumer's reducer (latest wins), not deltas.
- Wrong org, unknown run, stale seq and invalid input return a reason instead of throwing, so the consumer does not retry them.
- Owner = `agentRuns.externalUserId === viewer externalAuthId` (both the Control Plane user id); no owner id means nobody sees it (fail closed).
- Test kept at the queue's path `convex/agentRuns.browserActivity.test.ts`, written with node:test + tsx and exported handlers over a fake ctx, since convex-test is not installed and no install was allowed.

## MP asks
- `apps/Model Plane/deploy/nats.conf`, user `application-convex-model`: add `"mp.v1.trace.run.*"` to `subscribe` (appended to MP-ASKS.md, "W2-AP-S7a").

## Not verified
- Not compiled, not type-checked, not linted, tests not run (no node_modules in the lane).
- That Convex skips `*.test.ts` in `convex/` when bundling on this version, and that `delete` on the generic spread in `projectRunForViewer` type-checks.
- `listForOrg` (and the row `rate` returns) still give full rows, with `browserActivity`, to any org member: depth row (high), must be closed before S7b is switched on.
- No live evidence (no Convex restart, no browser run).

## Depth
W2-AP-S7a rows in W23-depth-lane-c.md.
