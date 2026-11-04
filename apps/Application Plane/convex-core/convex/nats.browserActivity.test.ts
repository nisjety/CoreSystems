/**
 * WS10.S7b (ADR-8 part 2): the `mp.v1.trace.run.*` consumer
 * (`nats.onBrowserTraceEvent`) with fake messages.
 *
 * Same harness as `agentRuns.browserActivity.test.ts`: convex-test is not a
 * dependency, so the exported handler runs against a small in-memory ctx
 * (indexed equality reads and patch). Run with:
 *   cd "apps/Application Plane/convex-core" && npx tsx --test convex/nats.browserActivity.test.ts
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  browserActivityUpdateFromTrace,
  browserTraceConsumerEnabled,
  onBrowserTraceEventHandler,
  parseBrowserTraceEvent,
  runIdFromTraceSubject,
} from "./nats";

type Row = Record<string, any> & { _id: string };

const FLAG = "BROWSER_TRACE_CONSUMER_ENABLED";
const ORG = "org-ext-1";
const SUBJECT = "mp.v1.trace.run.run-1";

function fakeCtx(tables: Record<string, Row[]>) {
  const patches: Array<[string, Record<string, unknown>]> = [];
  let reads = 0;
  const db = {
    query(table: string) {
      return {
        withIndex(_index: string, build: (q: any) => any) {
          reads += 1;
          const conditions: Array<[string, unknown]> = [];
          const q = {
            eq(field: string, value: unknown) {
              conditions.push([field, value]);
              return q;
            },
          };
          build(q);
          const rows = (tables[table] ?? []).filter((row) =>
            conditions.every(([field, value]) => row[field] === value),
          );
          return {
            unique: async () => rows[0] ?? null,
            collect: async () => rows,
          };
        },
      };
    },
    async patch(id: string, fields: Record<string, unknown>) {
      patches.push([id, fields]);
      for (const rows of Object.values(tables)) {
        const row = rows.find((candidate) => candidate._id === id);
        if (row) Object.assign(row, fields);
      }
    },
  };
  return { ctx: { db }, patches, reads: () => reads };
}

function seed(): Record<string, Row[]> {
  return {
    agentRuns: [
      {
        _id: "run-doc-1",
        runId: "run-1",
        externalOrgId: ORG,
        externalUserId: "owner",
        status: "started",
        startedAt: 1,
        updatedAt: 1,
      },
      {
        _id: "run-doc-2",
        runId: "run-2",
        externalOrgId: "org-ext-2",
        externalUserId: "someone",
        status: "started",
        startedAt: 1,
        updatedAt: 1,
      },
    ],
  };
}

/** A fake JSON twin of `model_plane.v1.Event` as Quarry's mapper writes it. */
function traceEvent(
  eventType: string,
  producerSeq: number | string,
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    event_id: `01J0000000000000000000000${producerSeq}`,
    event_type: eventType,
    schema_version: 1,
    ts: "2026-11-03T10:00:00Z",
    producer: "quarry-edge",
    correlation_id: "run-1",
    idempotency_key: `quarry:br-1:${producerSeq}`,
    org_id: ORG,
    resource_ref: "browser_run/br-1",
    run_id: "run-1",
    producer_seq: producerSeq,
    zdr: false,
    payload: {},
    ...overrides,
  };
}

function withFlag(value: string | undefined, body: () => Promise<void>): () => Promise<void> {
  return async () => {
    const before = process.env[FLAG];
    if (value === undefined) delete process.env[FLAG];
    else process.env[FLAG] = value;
    try {
      await body();
    } finally {
      if (before === undefined) delete process.env[FLAG];
      else process.env[FLAG] = before;
    }
  };
}

const on = (body: () => Promise<void>) => withFlag("1", body);

async function deliver(ctx: any, event: unknown, subject: string = SUBJECT) {
  return onBrowserTraceEventHandler(ctx, { subject, event });
}

test(
  "the consumer is off by default: nothing is read or written",
  withFlag(undefined, async () => {
    assert.equal(browserTraceConsumerEnabled(), false);
    const tables = seed();
    const { ctx, patches, reads } = fakeCtx(tables);
    const result = await deliver(ctx, traceEvent("BROWSER_SESSION_OPENED", 1));
    assert.deepEqual(result, { status: "disabled" });
    assert.equal(reads(), 0);
    assert.equal(patches.length, 0);
    assert.equal(tables.agentRuns[0].browserActivity, undefined);
  }),
);

test(
  "an explicit off word keeps it off",
  withFlag("false", async () => {
    const { ctx, patches } = fakeCtx(seed());
    assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_SESSION_OPENED", 1)), {
      status: "disabled",
    });
    assert.equal(patches.length, 0);
  }),
);

test(
  "events in order build the projection (ordering)",
  on(async () => {
    const tables = seed();
    const { ctx } = fakeCtx(tables);
    const sequence = [
      traceEvent("BROWSER_SESSION_OPENED", 1),
      traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 2),
      traceEvent("BROWSER_EGRESS_DECIDED", 3, { payload: { decision: "block" } }),
      traceEvent("BROWSER_EGRESS_DECIDED", 4, { payload: { decision: "allow" } }),
      traceEvent("BROWSER_ARTIFACT_RECORDED", 5),
      traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 6),
      traceEvent("BROWSER_CONTROL_CHANGED", 7, { payload: { mode: "human" } }),
      traceEvent("BROWSER_TAB_CHANGED", 8),
      traceEvent("EVENT_TYPE_BROWSER_SESSION_CLOSED", 9, { payload: { reason: "completed" } }),
    ];
    for (const event of sequence) {
      assert.deepEqual(await deliver(ctx, event), { status: "applied" });
    }
    assert.deepEqual(tables.agentRuns[0].browserActivity, {
      lastProducerSeq: 9,
      lastEventType: "BROWSER_SESSION_CLOSED",
      lastEventAt: Date.parse("2026-11-03T10:00:00Z"),
      sessionState: "closed",
      control: "human",
      blockedEgress: 1,
      steps: 2,
      artifacts: 1,
    });
    // The other org's run was never touched.
    assert.equal(tables.agentRuns[1].browserActivity, undefined);
  }),
);

test(
  "a duplicate producer_seq is skipped and counts nothing (duplicate)",
  on(async () => {
    const tables = seed();
    const { ctx, patches } = fakeCtx(tables);
    const step = traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 1);
    assert.deepEqual(await deliver(ctx, step), { status: "applied" });
    assert.deepEqual(await deliver(ctx, step), { status: "skipped", reason: "stale_seq" });
    assert.deepEqual(await deliver(ctx, { ...step }), { status: "skipped", reason: "stale_seq" });
    assert.equal(patches.length, 1);
    assert.equal(tables.agentRuns[0].browserActivity.steps, 1);
    assert.equal(tables.agentRuns[0].browserActivity.lastProducerSeq, 1);
  }),
);

test(
  "an older event after a newer one is dropped; counters stay approximate, never doubled",
  on(async () => {
    const tables = seed();
    const { ctx } = fakeCtx(tables);
    assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 3)), {
      status: "applied",
    });
    assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 2)), {
      status: "skipped",
      reason: "stale_seq",
    });
    assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_STEP_RECEIPT_RECORDED", "4")), {
      status: "applied",
    });
    const activity = tables.agentRuns[0].browserActivity;
    assert.equal(activity.lastProducerSeq, 4);
    assert.equal(activity.steps, 2);
  }),
);

test(
  "a ZDR run is dropped before the run row is read (ZDR drop)",
  on(async () => {
    const tables = seed();
    const { ctx, patches, reads } = fakeCtx(tables);
    assert.deepEqual(
      await deliver(ctx, traceEvent("BROWSER_SESSION_OPENED", 1, { zdr: true })),
      { status: "dropped", reason: "zdr" },
    );
    const noKey = traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 2);
    delete noKey.zdr;
    assert.deepEqual(await deliver(ctx, noKey), { status: "dropped", reason: "zdr" });
    assert.deepEqual(
      await deliver(ctx, traceEvent("BROWSER_STEP_RECEIPT_RECORDED", 3, { zdr: "false" })),
      { status: "dropped", reason: "zdr" },
    );
    assert.equal(reads(), 0);
    assert.equal(patches.length, 0);
    assert.equal(tables.agentRuns[0].browserActivity, undefined);
  }),
);

test(
  "the org comes from the run the subject names, never from the payload",
  on(async () => {
    const tables = seed();
    const { ctx, patches } = fakeCtx(tables);
    // A payload org_id naming another org cannot redirect or authorize the write.
    assert.deepEqual(
      await deliver(ctx, traceEvent("BROWSER_SESSION_OPENED", 1, { org_id: "org-ext-2" })),
      { status: "dropped", reason: "org_mismatch" },
    );
    // Subject run-1, payload claiming run-2's org and run id: dropped, run-2 untouched.
    assert.deepEqual(
      await deliver(
        ctx,
        traceEvent("BROWSER_SESSION_OPENED", 1, { org_id: "org-ext-2", run_id: "run-2" }),
      ),
      { status: "dropped", reason: "run_mismatch" },
    );
    assert.equal(patches.length, 0);
    // No org_id in the payload: the row's org is used.
    const noOrg = traceEvent("BROWSER_SESSION_OPENED", 1);
    delete noOrg.org_id;
    assert.deepEqual(await deliver(ctx, noOrg), { status: "applied" });
    assert.equal(tables.agentRuns[0].browserActivity.sessionState, "open");
    assert.equal(tables.agentRuns[1].browserActivity, undefined);
  }),
);

test(
  "a trace event for an unknown run creates nothing",
  on(async () => {
    const tables = seed();
    const { ctx, patches } = fakeCtx(tables);
    const event = traceEvent("BROWSER_SESSION_OPENED", 1, { run_id: "run-9" });
    assert.deepEqual(await deliver(ctx, event, "mp.v1.trace.run.run-9"), {
      status: "dropped",
      reason: "unknown_run",
    });
    assert.equal(patches.length, 0);
    assert.equal(tables.agentRuns.length, 2);
  }),
);

test(
  "bad subjects, other event types and malformed events are dropped",
  on(async () => {
    const tables = seed();
    const { ctx, patches } = fakeCtx(tables);
    for (const subject of [
      "mp.v1.trace.run.",
      "mp.v1.trace.run.*",
      "mp.v1.trace.run.run-1.extra",
      "mp.v1.run.run-1.event",
      "mp.v1.trace.run.run 1",
    ]) {
      assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_SESSION_OPENED", 1), subject), {
        status: "dropped",
        reason: "bad_subject",
      });
    }
    assert.deepEqual(await deliver(ctx, traceEvent("RUN_STARTED", 1)), {
      status: "dropped",
      reason: "not_browser",
    });
    assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_ACTION_DISPATCHED", 1)), {
      status: "dropped",
      reason: "not_browser",
    });
    for (const seq of [0, -1, 1.5, "01", "x", Number.MAX_SAFE_INTEGER + 2]) {
      assert.deepEqual(await deliver(ctx, traceEvent("BROWSER_SESSION_OPENED", seq as any)), {
        status: "dropped",
        reason: "invalid",
      });
    }
    assert.deepEqual(await deliver(ctx, null), { status: "dropped", reason: "invalid" });
    assert.deepEqual(await deliver(ctx, [traceEvent("BROWSER_SESSION_OPENED", 1)]), {
      status: "dropped",
      reason: "invalid",
    });
    assert.equal(patches.length, 0);
  }),
);

test(
  "no payload text lands: a free-text mode is not a control label",
  on(async () => {
    const tables = seed();
    const { ctx, patches } = fakeCtx(tables);
    assert.deepEqual(
      await deliver(
        ctx,
        traceEvent("BROWSER_CONTROL_CHANGED", 1, {
          payload: { mode: "https://example.com/account?token=abc", selector: "#password" },
        }),
      ),
      { status: "applied" },
    );
    const activity = tables.agentRuns[0].browserActivity;
    assert.equal(activity.control, undefined);
    assert.deepEqual(Object.keys(patches[0][1]).sort(), ["browserActivity", "updatedAt"]);
    const stored = JSON.stringify(activity);
    assert.equal(stored.includes("example.com"), false);
    assert.equal(stored.includes("password"), false);
  }),
);

test("pure helpers: subject mapping and the update use the row's org", () => {
  assert.equal(runIdFromTraceSubject("mp.v1.trace.run.run_A-1"), "run_A-1");
  assert.equal(runIdFromTraceSubject(42), null);

  const parsed = parseBrowserTraceEvent(
    SUBJECT,
    traceEvent("BROWSER_ARTIFACT_RECORDED", 7, { org_id: undefined, ts: undefined }),
  );
  assert.equal(parsed.kind, "ok");
  if (parsed.kind !== "ok") return;
  const next = browserActivityUpdateFromTrace(
    parsed.event,
    {
      externalOrgId: ORG,
      browserActivity: { lastProducerSeq: 6, lastEventType: "X", lastEventAt: 1, artifacts: 4 },
    },
    1234,
  );
  assert.deepEqual(next, {
    kind: "ok",
    update: {
      runId: "run-1",
      externalOrgId: ORG,
      producerSeq: 7,
      eventType: "BROWSER_ARTIFACT_RECORDED",
      eventAt: 1234,
      artifacts: 5,
    },
  });
});
