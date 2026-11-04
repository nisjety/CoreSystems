# W2-AP-S7b status: WS10.S7b Application Plane consumer for browser run trace events (lane c, branch w23/lane-c)

## Built: partly
Code only, not compiled, no tests run (owner order 2026-11-03).

Checked first: S7a's `agentRuns.upsertBrowserActivity` (and its exported handler) existed; no trace consumer existed anywhere in convex-core (`mp.v1.trace.run` appeared only in comments). Built the gap in this unit's two files:

- `convex/nats.ts`: `onBrowserTraceEvent` (internalMutation, args `{ subject, event }`) with exported pure helpers `runIdFromTraceSubject`, `parseBrowserTraceEvent`, `browserActivityUpdateFromTrace`, the handler `onBrowserTraceEventHandler`, and `browserTraceConsumerEnabled`.
  - Flag off: returns `{status:"disabled"}`, reads and writes nothing.
  - Run id from the subject `mp.v1.trace.run.{runId}` (one token, `[A-Za-z0-9_-]{1,128}`); a payload `run_id` that differs drops the event.
  - Org: the `agentRuns` row's `externalOrgId` for the subject's run (the verified lifecycle mirror), never the payload; a payload `org_id` that differs drops the event; an unknown run creates nothing.
  - ZDR: anything but an explicit `zdr: false` drops the event before the run row is read.
  - Only the seven browser types (150-156; `EVENT_TYPE_` prefix accepted) are consumed; others drop as `not_browser`.
  - Idempotent by `producer_seq` through `upsertBrowserActivityHandler` (same transaction): duplicate or older seq is `skipped/stale_seq`.
  - Reduction: SESSION_OPENED/CLOSED -> `sessionState` open/closed; CONTROL_CHANGED -> `control` = `payload.mode` only if `^[a-z][a-z0-9_]{0,31}$`; STEP_RECEIPT_RECORDED -> `steps` +1; EGRESS_DECIDED with `payload.decision` block/blocked/deny/denied -> `blockedEgress` +1; ARTIFACT_RECORDED -> `artifacts` +1; TAB_CHANGED -> seq/type/time only. Counters approximate (+1 on the stored total).
  - Never throws for a bad event: returns `{status: disabled | dropped(reason) | applied | skipped(reason)}`.
- `convex/nats.browserActivity.test.ts` (NEW): node:test + tsx over a fake ctx (same harness as S7a): off by default (no read, no write), off word, ordering builds the projection, duplicate seq skipped and not counted, older-after-newer dropped (approximate counters), ZDR drop (true, missing key, string) with no DB read, org from the subject's run not the payload (mismatch drop, no-org applies with the row's org, cannot redirect to another run), unknown run creates nothing, bad subjects / other types / bad seqs / non-objects dropped, no payload text lands, pure helper golden. Run: `cd "apps/Application Plane/convex-core" && npx tsx --test convex/nats.browserActivity.test.ts`.

Why partly: the NATS subscription in `nats-subscriber.js` and the `onBrowserTraceEvent` route in `convex/http.ts` are outside this unit's file list and were not built, so nothing calls the consumer yet (depth row, high). The plan's 2 s debounce reducer (in `nats-subscriber.js`) is not built either.

## Commits (lane c)
- fe2ea8579 feat(convex-core): browser trace consumer for agentRuns.browserActivity, off by default (WS10.S7b)
- 6041007ac test(convex-core): browser trace consumer ordering, duplicate, ZDR drop and org from subject (WS10.S7b)
- docs(plan-work) commit with this file and the depth rows

## Code only, not compiled
No pnpm, tsc, vitest or tsx was run. Expected checks for the integration step: `tsc6 -p tsconfig.json` (or the package typecheck) and the tsx command above.

## Flag and default
Convex env `BROWSER_TRACE_CONSUMER_ENABLED`, default off (unset, empty or an off word = off; read with `envFlagOn` at handler time). The subscriber-side subscription, when wired, should have its own env defaulting off too.

## Assumed decisions
- "Org from the verified subject mapping": the subject carries only the run id; the NATS grant (publish-only `quarry-trace-producer` on `mp.v1.trace.run.*`) is the verification, and the org is the one the run lifecycle mirror recorded for that run. The payload `org_id` is only cross-checked (mismatch drops), never used.
- ZDR fail closed: the producer always writes `"zdr": false` (plan: `a_zdr_false_key_is_always_written`), so a missing or non-boolean key is treated as ZDR.
- Subject token and event `run_id` = Model run id = `agentRuns.runId`.
- The consumer runs in Convex per event (one mutation: read row, reduce, `upsertBrowserActivityHandler`) rather than an in-memory reducer in the subscriber; counters are +1 on the stored total, so an out-of-order event is lost, never double counted.
- `producer_seq` accepted as a number or a proto3-JSON digit string; `ts` as RFC 3339 or epoch ms, else the consumer's clock.
- `sessionState` labels `open` / `closed`; `payload.reason` is not stored.

## MP asks
None new. The `nats.conf` subscribe grant for `application-convex-model` (`"mp.v1.trace.run.*"`) is already in MP-ASKS.md under W2-AP-S7a. The producer side is Quarry (Ingestion), and its contract assumptions are in the depth rows.

## Not verified
- Not compiled, not type-checked, tests not run.
- That `handler: onBrowserTraceEventHandler` (ctx `any`, args `{subject; event: unknown}`) type-checks against `internalMutation` with `v.any()` on this Convex version (same pattern as S7a's handler).
- The Quarry producer does not exist yet, so the event shape is from the plan only.
- No live evidence.

## Depth
W2-AP-S7b rows in W23-depth-lane-c.md (subscriber/route gap high, `listForOrg` leak still open high).
