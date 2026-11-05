/**
 * WS10.S7a (ADR-8 part 2): `agentRuns.browserActivity` projection.
 *
 * convex-test is not a dependency of this package, so these run the exported
 * handlers against a small in-memory ctx (indexed equality reads, patch, auth
 * identity), in the convex-test style: seed rows, call the function, read the
 * row back. Run with:
 *   cd "apps/Application Plane/convex-core" && npx tsx --test convex/agentRuns.browserActivity.test.ts
 * (the package `test` script only globs test/*.test.cjs).
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  type BrowserActivityUpdate,
  decideBrowserActivityUpdate,
  getByRunIdHandler,
  listForOrgHandler,
  projectRunForViewer,
  upsertBrowserActivityHandler,
} from "./agentRuns";

type Row = Record<string, any> & { _id: string };

function fakeCtx(viewerExternalAuthId: string | null, tables: Record<string, Row[]>) {
  const patches: Array<[string, Record<string, unknown>]> = [];
  const db = {
    query(table: string) {
      return {
        withIndex(_index: string, build: (q: any) => any) {
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
            order: () => ({ take: async (limit: number) => rows.slice(0, limit) }),
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
  const auth = {
    getUserIdentity: async () =>
      viewerExternalAuthId === null
        ? null
        : { "properties.externalAuthId": viewerExternalAuthId },
  };
  return { ctx: { db, auth }, patches };
}

const ORG = "org-ext-1";

function seed(run: Partial<Row> = {}): Record<string, Row[]> {
  return {
    organizations: [
      { _id: "org-doc-1", externalOrgId: ORG, syncStatus: "active" },
      { _id: "org-doc-2", externalOrgId: "org-ext-2", syncStatus: "active" },
    ],
    users: [
      { _id: "u1", externalAuthId: "owner", orgId: "org-doc-1", syncStatus: "active" },
      { _id: "u2", externalAuthId: "colleague", orgId: "org-doc-1", syncStatus: "active" },
      { _id: "u3", externalAuthId: "outsider", orgId: "org-doc-2", syncStatus: "active" },
    ],
    agentRuns: [
      {
        _id: "run-doc-1",
        runId: "run-1",
        externalOrgId: ORG,
        externalUserId: "owner",
        status: "started",
        startedAt: 1,
        updatedAt: 1,
        ...run,
      },
    ],
  };
}

const update = (overrides: Partial<BrowserActivityUpdate> = {}): BrowserActivityUpdate => ({
  runId: "run-1",
  externalOrgId: ORG,
  producerSeq: 1,
  eventType: "BROWSER_NAVIGATED",
  eventAt: 1000,
  ...overrides,
});

test("the run owner sees browserActivity", async () => {
  const tables = seed();
  const writer = fakeCtx(null, tables);
  const result = await upsertBrowserActivityHandler(
    writer.ctx,
    update({ sessionState: "open", control: "agent", steps: 3 }),
  );
  assert.deepEqual(result, { applied: true });

  const { ctx } = fakeCtx("owner", tables);
  const row = await getByRunIdHandler(ctx, { runId: "run-1" });
  assert.deepEqual(row.browserActivity, {
    lastProducerSeq: 1,
    lastEventType: "BROWSER_NAVIGATED",
    lastEventAt: 1000,
    sessionState: "open",
    control: "agent",
    steps: 3,
  });
});

test("non_owner_org_member_sees_no_browser_activity", async () => {
  const tables = seed();
  await upsertBrowserActivityHandler(fakeCtx(null, tables).ctx, update());

  const { ctx } = fakeCtx("colleague", tables);
  const row = await getByRunIdHandler(ctx, { runId: "run-1" });
  assert.ok(row, "an org member still gets the run row");
  assert.equal(row.runId, "run-1");
  assert.equal(row.status, "started");
  assert.equal("browserActivity" in row, false);
  // The stored row is untouched; only the read narrows.
  assert.ok(tables.agentRuns[0].browserActivity);
});

test("a viewer outside the run's org is denied", async () => {
  const tables = seed();
  await upsertBrowserActivityHandler(fakeCtx(null, tables).ctx, update());
  const { ctx } = fakeCtx("outsider", tables);
  await assert.rejects(getByRunIdHandler(ctx, { runId: "run-1" }), /Unauthorized/);
});

test("both org list paths apply the same owner-only browser projection", async () => {
  const tables = seed();
  await upsertBrowserActivityHandler(fakeCtx(null, tables).ctx, update());
  for (const status of [undefined, "started"] as const) {
    const ownerRows = await listForOrgHandler(fakeCtx("owner", tables).ctx, { externalOrgId: ORG, status });
    const colleagueRows = await listForOrgHandler(fakeCtx("colleague", tables).ctx, { externalOrgId: ORG, status });
    assert.ok(ownerRows[0].browserActivity);
    assert.equal("browserActivity" in colleagueRows[0], false);
    await assert.rejects(listForOrgHandler(fakeCtx("outsider", tables).ctx, { externalOrgId: ORG, status }), /Unauthorized/);
  }
  assert.ok(tables.agentRuns[0].browserActivity);
});

test("a run with no externalUserId shows browserActivity to nobody", () => {
  const run = {
    runId: "run-1",
    externalOrgId: ORG,
    browserActivity: { lastProducerSeq: 1, lastEventType: "X", lastEventAt: 1 },
  };
  assert.equal("browserActivity" in projectRunForViewer(run, "owner"), false);
  assert.equal("browserActivity" in projectRunForViewer({ ...run, externalUserId: "" }, ""), false);
});

test("lastProducerSeq_never_moves_backwards: a stale or duplicate seq is ignored", async () => {
  const tables = seed();
  const { ctx, patches } = fakeCtx(null, tables);
  assert.deepEqual(
    await upsertBrowserActivityHandler(ctx, update({ producerSeq: 5, eventType: "STEP_DONE", steps: 5 })),
    { applied: true },
  );
  assert.deepEqual(
    await upsertBrowserActivityHandler(ctx, update({ producerSeq: 3, eventType: "OLD", steps: 2 })),
    { applied: false, reason: "stale_seq" },
  );
  assert.deepEqual(
    await upsertBrowserActivityHandler(ctx, update({ producerSeq: 5, eventType: "DUP" })),
    { applied: false, reason: "stale_seq" },
  );
  assert.equal(patches.length, 1);
  assert.equal(tables.agentRuns[0].browserActivity.lastProducerSeq, 5);
  assert.equal(tables.agentRuns[0].browserActivity.lastEventType, "STEP_DONE");
  assert.equal(tables.agentRuns[0].browserActivity.steps, 5);
});

test("an_org_mismatch_is_rejected: a wrong org writes nothing", async () => {
  const tables = seed();
  const { ctx, patches } = fakeCtx(null, tables);
  const result = await upsertBrowserActivityHandler(ctx, update({ externalOrgId: "org-ext-2" }));
  assert.deepEqual(result, { applied: false, reason: "org_mismatch" });
  assert.equal(patches.length, 0);
  assert.equal(tables.agentRuns[0].browserActivity, undefined);
});

test("a_trace_event_for_an_unknown_run_creates_nothing", async () => {
  const tables = seed();
  const { ctx, patches } = fakeCtx(null, tables);
  const result = await upsertBrowserActivityHandler(ctx, update({ runId: "run-404" }));
  assert.deepEqual(result, { applied: false, reason: "unknown_run" });
  assert.equal(patches.length, 0);
  assert.equal(tables.agentRuns.length, 1);
});

test("missing fields keep their previous value; counters are totals, not deltas", () => {
  const run = {
    externalOrgId: ORG,
    browserActivity: {
      lastProducerSeq: 2,
      lastEventType: "A",
      lastEventAt: 10,
      sessionState: "open",
      control: "agent",
      blockedEgress: 1,
      steps: 2,
      artifacts: 1,
    },
  };
  const decision = decideBrowserActivityUpdate(
    run,
    update({ producerSeq: 9, eventType: "BROWSER_SESSION_CLOSED", eventAt: 20, sessionState: "closed", steps: 7 }),
  );
  assert.deepEqual(decision, {
    kind: "apply",
    browserActivity: {
      lastProducerSeq: 9,
      lastEventType: "BROWSER_SESSION_CLOSED",
      lastEventAt: 20,
      sessionState: "closed",
      control: "agent",
      blockedEgress: 1,
      steps: 7,
      artifacts: 1,
    },
  });
});

test("projection_holds_no_payload_text: free text and bad numbers are refused", () => {
  const run = { externalOrgId: ORG };
  const bad: Array<Partial<BrowserActivityUpdate>> = [
    { eventType: "https://example.com/secret?q=1" },
    { eventType: "" },
    { sessionState: "user typed a password here" },
    { control: "x".repeat(65) },
    { steps: -1 },
    { artifacts: 1.5 },
    { blockedEgress: Number.NaN },
    { producerSeq: 0 },
    { producerSeq: 1.5 },
    { eventAt: Number.POSITIVE_INFINITY },
  ];
  for (const overrides of bad) {
    assert.deepEqual(
      decideBrowserActivityUpdate(run, update(overrides)),
      { kind: "skip", reason: "invalid" },
      JSON.stringify(overrides),
    );
  }
});
