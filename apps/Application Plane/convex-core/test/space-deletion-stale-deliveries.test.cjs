const assert = require('node:assert/strict');
const test = require('node:test');

// The adapters read these at import time. The values are test fixtures.
process.env.MODEL_SPACE_DELETION_URL = 'http://model.invalid/api/v1/internal/space-deletion/threads';
process.env.CONTROL_SPACE_DELETION_MODEL_BEARER = 'model-bearer-aaaaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_SESSION_BEARER = 'session-bearer-aaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_CAPABILITY_BEARER = 'capability-bearer-aaaaaaaaaaaaaaaaaa';
process.env.DATA_SPACE_DELETION_URL = 'http://data.invalid/api/v1/internal/space-deletion/bindings';
process.env.CONTROL_SPACE_DELETION_DATA_BEARER = 'data-bearer-aaaaaaaaaaaaaaaaaaaaaaaaaa';
process.env.INGESTION_SPACE_DELETION_URL = 'http://ingestion.invalid/api/v1/internal/space-deletion/jobs';
process.env.CONTROL_SPACE_DELETION_INGESTION_BEARER = 'ingestion-bearer-aaaaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const modelAdapter = require('../convex/spaceDeletionModel.ts');
const dataAdapter = require('../convex/spaceDeletionData.ts');
const ingestionAdapter = require('../convex/spaceDeletionIngestion.ts');
const spaces = require('../convex/spaces.ts');
const { canClaimSpaceLifecycleDelivery } = require('../convex/spaceLifecycle.ts');
const fake = require('./helpers/fake-convex.cjs');

const NOW = 10_000;
const ORG = 'org_a';
const HOLD_CODE = 'erasure_blocked_legal_hold';
const OWNER_PLANES = ['application', 'control', 'data', 'ingestion', 'model', 'infra'];

// One entry per adapted owner: its claim and acknowledgement mutations, the adapter that
// re-drives it, and the request-body field that names the Space.
const OWNERS = {
  model: {
    adapter: modelAdapter,
    claim: 'claimNextModelDeletionOwnerDelivery',
    ack: 'acknowledgeModelDeletionOwnerDelivery',
    spaceField: 'space_id',
  },
  data: {
    adapter: dataAdapter,
    claim: 'claimNextDataDeletionOwnerDelivery',
    ack: 'acknowledgeDataDeletionOwnerDelivery',
    spaceField: 'space_ref',
  },
  ingestion: {
    adapter: ingestionAdapter,
    claim: 'claimNextIngestionDeletionOwnerDelivery',
    ack: 'acknowledgeIngestionDeletionOwnerDelivery',
    spaceField: 'space_ref',
  },
};

function requestIdOf(spaceRef) {
  return `space:${spaceRef}:deletion:idem-${spaceRef}`;
}

function eventIdOf(spaceRef, owner) {
  return `space-deletion:${requestIdOf(spaceRef)}:owner:${owner}`;
}

/**
 * One Space deletion: its fence, its request, the owner delivery rows in `rows`, and one
 * receipt per owner plane. Every receipt is `succeeded` unless `receipts` says otherwise.
 */
function deletion(spaceRef, { lifecycle = 'deleting', requestState = 'authorized', rows = [], receipts = {} } = {}) {
  const requestId = requestIdOf(spaceRef);
  return {
    spaces: [{
      _id: `space_${spaceRef}`, spaceRef, externalOrgId: ORG, kind: 'personal',
      ownerExternalAuthId: 'user_a', lifecycle, lifecycleRevision: 4,
    }],
    spaceDeletionRequests: [{
      _id: `request_${spaceRef}`, requestId, idempotencyKey: `idem-${spaceRef}`, spaceRef,
      externalOrgId: ORG, ownerExternalAuthId: 'user_a', state: requestState, createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionOwnerDeliveryEvents: rows.map((row) => ({
      _id: `event_${spaceRef}_${row.owner}`, eventId: eventIdOf(spaceRef, row.owner), requestId,
      ownerPlane: row.owner, deliveryState: row.state, deliveryAttempts: row.attempts ?? 0,
      nextAttemptAt: row.nextAttemptAt ?? 0, createdAt: 0,
      leaseOwner: row.leaseOwner, leaseExpiresAt: row.leaseExpiresAt, lastDeliveryError: row.lastDeliveryError,
    })),
    spaceDeletionOwnerReceipts: OWNER_PLANES.map((ownerPlane) => ({
      _id: `receipt_${spaceRef}_${ownerPlane}`, requestId, ownerPlane,
      status: receipts[ownerPlane] ?? 'succeeded', deadlineAt: 7_000, updatedAt: 0,
    })),
  };
}

/** Concatenates deletions into one set of tables. */
function tablesOf(...parts) {
  const tables = {};
  for (const part of parts) {
    for (const [table, rows] of Object.entries(part)) {
      tables[table] = [...(tables[table] ?? []), ...rows];
    }
  }
  return tables;
}

/**
 * A ctx whose runMutation runs the real spaces.ts handlers against the same in-memory
 * tables, so every assertion reads the rows production would have written.
 */
function harness(tables) {
  const handle = fake.makeCtx(tables, {
    runMutation(fn, args) {
      const [moduleName, exportName] = getFunctionName(fn).split(':');
      assert.equal(moduleName, 'spaces', `unexpected mutation ${getFunctionName(fn)}`);
      return spaces[exportName]._handler(handle.ctx, args);
    },
  });
  return handle;
}

function rowOf(tables, spaceRef, owner) {
  return tables.spaceDeletionOwnerDeliveryEvents.find((row) => row.eventId === eventIdOf(spaceRef, owner));
}

function spaceOf(tables, spaceRef) {
  return tables.spaces.find((row) => row.spaceRef === spaceRef);
}

function receiptOf(tables, spaceRef, owner) {
  return tables.spaceDeletionOwnerReceipts.find((row) => row.requestId === requestIdOf(spaceRef) && row.ownerPlane === owner);
}

/** The owner's exact 200 answer for one Space. */
function ownerReceipt(owner, spaceRef, outcome) {
  return { request_id: requestIdOf(spaceRef), owner_plane: owner, owner_outcome: outcome };
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

/** Counts the rows each read of one table returns, through the same ctx the handler uses. */
function countRowsRead(handle, table) {
  const counts = [];
  const realQuery = handle.ctx.db.query;
  handle.ctx.db.query = (name) => {
    const query = realQuery(name);
    if (name === table) {
      for (const method of ['collect', 'take', 'first']) {
        const real = query[method];
        query[method] = async (...args) => {
          const result = await real.apply(query, args);
          counts.push(Array.isArray(result) ? result.length : (result ? 1 : 0));
          return result;
        };
      }
    }
    return query;
  };
  return () => counts.reduce((sum, count) => sum + count, 0);
}

/** Answers every owner call with `reply()` while `body` runs, and returns the requests sent. */
async function withOwnerReply(reply, body) {
  const sent = [];
  const realFetch = global.fetch;
  global.fetch = async (url, init) => {
    sent.push({ url, init });
    return reply();
  };
  try {
    await body();
  } finally {
    global.fetch = realFetch;
  }
  return sent;
}

function jsonReply(status, payload) {
  return () => new Response(JSON.stringify(payload), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

/** Sets SPACE_DELETION_HOLD_RELEASE for one test (`undefined` removes it), restored afterwards. */
function setHoldReleaseFlag(t, value) {
  const prior = process.env.SPACE_DELETION_HOLD_RELEASE;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_DELETION_HOLD_RELEASE;
    else process.env.SPACE_DELETION_HOLD_RELEASE = prior;
  });
  if (value === undefined) delete process.env.SPACE_DELETION_HOLD_RELEASE;
  else process.env.SPACE_DELETION_HOLD_RELEASE = value;
}

for (const owner of Object.keys(OWNERS)) {
  const spec = OWNERS[owner];

  test(`${owner}: a deletion that completes over a pending delivery does not block the next Space's delivery`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', { rows: [{ owner, state: 'pending', nextAttemptAt: 1_000 }], receipts: { [owner]: 'pending' } }),
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    // The trusted ingress relays Space A's answer, the last one its deletion was waiting for.
    const finished = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: requestIdOf('space_a'), ownerPlane: owner, status: 'succeeded', now: NOW,
    });
    assert.deepEqual(finished, { ownerPlane: owner, status: 'succeeded', finalized: true });
    assert.equal(spaceOf(handle.tables, 'space_a').lifecycle, 'deleted');
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
    assert.equal(rowOf(handle.tables, 'space_a', owner).lastDeliveryError, 'deletion_completed');
    assert.equal(rowOf(handle.tables, 'space_a', owner).leaseOwner, undefined);

    // The re-drive skips Space A's closed row and sends Space B's delivery, not an error.
    let result;
    const sent = await withOwnerReply(jsonReply(200, ownerReceipt(owner, 'space_b', 'partial')), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });
    assert.deepEqual(result, { eventId: eventIdOf('space_b', owner), status: 'partial' });
    assert.equal(sent.length, 1, 'only Space B is sent to the owner');
    assert.equal(JSON.parse(sent[0].init.body)[spec.spaceField], 'space_b');
    assert.equal(rowOf(handle.tables, 'space_b', owner).deliveryState, 'acknowledged');
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed', 'Space A is never claimed again');
  });

  test(`${owner}: a stale row left by a completed deletion is closed by the next claim, and the live row is claimed`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', { lifecycle: 'deleted', rows: [{ owner, state: 'pending', nextAttemptAt: 1_000 }] }),
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    assert.equal(claimed.request.requestId, requestIdOf('space_b'));
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
    assert.equal(rowOf(handle.tables, 'space_a', owner).lastDeliveryError, 'deletion_completed');
    assert.equal(rowOf(handle.tables, 'space_b', owner).deliveryState, 'claimed');
    assert.equal(rowOf(handle.tables, 'space_b', owner).leaseOwner, 'worker-1');
  });

  test(`${owner}: a row whose request is no longer authorized is closed, not claimed`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', { lifecycle: 'active', requestState: 'rejected', rows: [{ owner, state: 'pending', nextAttemptAt: 1_000 }] }),
      deletion('space_b', { rows: [{ owner, state: 'failed', attempts: 1, nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
    assert.equal(rowOf(handle.tables, 'space_a', owner).lastDeliveryError, 'deletion_not_live');
    assert.equal(rowOf(handle.tables, 'space_b', owner).deliveryState, 'claimed');
  });

  test(`${owner}: an expired claim on a completed deletion is closed by the next claim`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', {
        lifecycle: 'deleted',
        rows: [{ owner, state: 'claimed', attempts: 1, leaseOwner: 'worker-old', leaseExpiresAt: 5_000, nextAttemptAt: 0 }],
      }),
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    const closed = rowOf(handle.tables, 'space_a', owner);
    assert.equal(closed.deliveryState, 'closed');
    assert.equal(closed.leaseOwner, undefined);
    assert.equal(closed.leaseExpiresAt, undefined);
  });

  test(`${owner}: a stale row under a live lease is left to its worker, and the claim moves past it`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', {
        lifecycle: 'deleted',
        rows: [{ owner, state: 'claimed', attempts: 1, leaseOwner: 'worker-live', leaseExpiresAt: NOW + 30_000, nextAttemptAt: 0 }],
      }),
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    const leased = rowOf(handle.tables, 'space_a', owner);
    assert.equal(leased.deliveryState, 'claimed');
    assert.equal(leased.leaseOwner, 'worker-live');
  });

  test(`${owner}: completing a deletion keeps a live lease, and its worker can still acknowledge the answer`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', {
        rows: [{ owner, state: 'claimed', attempts: 1, leaseOwner: 'worker-1', leaseExpiresAt: NOW + 30_000, nextAttemptAt: 0 }],
        receipts: { [owner]: 'pending' },
      }),
    ));

    const finished = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: requestIdOf('space_a'), ownerPlane: owner, status: 'succeeded', now: NOW,
    });
    assert.deepEqual(finished, { ownerPlane: owner, status: 'succeeded', finalized: true });
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'claimed', 'the lease holder is still inside its owner call');
    assert.equal(rowOf(handle.tables, 'space_a', owner).leaseOwner, 'worker-1');

    const acked = await spaces[spec.ack]._handler(handle.ctx, {
      eventId: eventIdOf('space_a', owner), workerId: 'worker-1', now: NOW,
      status: 'succeeded', receiptRef: `${owner}:space_a`, detail: 'erased',
    });
    assert.deepEqual(acked, { status: 'succeeded', finalized: true });
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'acknowledged');
    assert.equal(receiptOf(handle.tables, 'space_a', owner).status, 'succeeded');
  });

  test(`${owner}: a closed delivery is never claimed and can never be acknowledged`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', {
        lifecycle: 'deleted',
        rows: [{ owner, state: 'closed', attempts: 1, nextAttemptAt: 1_000, lastDeliveryError: 'deletion_completed' }],
      }),
    ));

    assert.equal(await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW }), null);
    await assert.rejects(
      spaces[spec.ack]._handler(handle.ctx, {
        eventId: eventIdOf('space_a', owner), workerId: 'worker-1', now: NOW,
        status: 'succeeded', receiptRef: `${owner}:space_a`, detail: 'erased',
      }),
      /active delivery worker/i,
    );
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
  });

  test(`${owner}: for a live deletion the claim picks the earliest delivery and writes the same claimed row as before`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 1_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    assert.equal(claimed.request.requestId, requestIdOf('space_b'));
    assert.deepEqual(
      {
        deliveryState: rowOf(handle.tables, 'space_b', owner).deliveryState,
        deliveryAttempts: rowOf(handle.tables, 'space_b', owner).deliveryAttempts,
        leaseOwner: rowOf(handle.tables, 'space_b', owner).leaseOwner,
        leaseExpiresAt: rowOf(handle.tables, 'space_b', owner).leaseExpiresAt,
        nextAttemptAt: rowOf(handle.tables, 'space_b', owner).nextAttemptAt,
      },
      { deliveryState: 'claimed', deliveryAttempts: 1, leaseOwner: 'worker-1', leaseExpiresAt: NOW + 30_000, nextAttemptAt: NOW },
    );
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'pending', 'the later row is untouched');
    assert.equal(rowOf(handle.tables, 'space_a', owner).nextAttemptAt, 2_000);
  });

  test(`${owner}: completing a deletion over a parked hold closes the parked delivery`, async (t) => {
    setHoldReleaseFlag(t, 'true');
    const handle = harness(tablesOf(
      deletion('space_a', {
        rows: [{ owner, state: 'blocked_legal_hold', attempts: 1, nextAttemptAt: 1_000, lastDeliveryError: HOLD_CODE }],
        receipts: { [owner]: 'blocked_legal_hold' },
      }),
    ));

    const finished = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: requestIdOf('space_a'), ownerPlane: owner, status: 'succeeded',
      receiptRef: 'coordinator:post-hold-answer', now: NOW,
    });
    assert.deepEqual(finished, { ownerPlane: owner, status: 'succeeded', finalized: true });
    assert.equal(spaceOf(handle.tables, 'space_a').lifecycle, 'deleted');
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
    assert.equal(rowOf(handle.tables, 'space_a', owner).lastDeliveryError, 'deletion_completed');
  });

  test(`${owner}: completing a deletion over a permanently refused delivery closes it`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', {
        rows: [{ owner, state: 'failed_permanent', attempts: 1, nextAttemptAt: 1_000, lastDeliveryError: 'permanent_refusal_http_403' }],
        receipts: { [owner]: 'pending' },
      }),
    ));

    const finished = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: requestIdOf('space_a'), ownerPlane: owner, status: 'succeeded', now: NOW,
    });
    assert.deepEqual(finished, { ownerPlane: owner, status: 'succeeded', finalized: true });
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
  });

  test(`${owner}: the re-drive sends a live Space's delivery when a stale row comes first`, async () => {
    const handle = harness(tablesOf(
      deletion('space_a', { lifecycle: 'deleted', rows: [{ owner, state: 'pending', nextAttemptAt: 1_000 }] }),
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    let result;
    const sent = await withOwnerReply(jsonReply(200, ownerReceipt(owner, 'space_b', 'succeeded')), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });
    assert.deepEqual(result, { eventId: eventIdOf('space_b', owner), status: 'succeeded' });
    assert.equal(sent.length, 1);
    assert.equal(JSON.parse(sent[0].init.body)[spec.spaceField], 'space_b');
    assert.equal(receiptOf(handle.tables, 'space_b', owner).status, 'succeeded');
    assert.equal(
      scheduledNames(handle.log).some((name) => name.startsWith('spaceDeletion')),
      false,
      'no owner retry is scheduled once the answer is recorded',
    );
    assert.equal(rowOf(handle.tables, 'space_a', owner).deliveryState, 'closed');
  });

  test(`${owner}: the claim reads only the due, claimable rows and never the acknowledged or closed history`, async () => {
    // Forty finished deletions leave acknowledged or closed rows for the same owner.
    const finished = Array.from({ length: 40 }, (_, index) => deletion(`space_old_${index}`, {
      lifecycle: 'deleted',
      rows: [{ owner, state: index % 2 === 0 ? 'acknowledged' : 'closed', attempts: 1, nextAttemptAt: 100 + index, lastDeliveryError: 'deletion_completed' }],
    }));
    const handle = harness(tablesOf(
      ...finished,
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));
    const rowsRead = countRowsRead(handle, 'spaceDeletionOwnerDeliveryEvents');

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    assert.equal(rowsRead(), 1, 'only the one due pending row is read');
  });

  test(`${owner}: a delivery whose request is missing is closed with a warning that carries ids only, and the live one is claimed`, async (t) => {
    const warn = t.mock.method(console, 'warn', () => {});
    const handle = harness(tablesOf(
      {
        spaceDeletionOwnerDeliveryEvents: [{
          _id: `orphan_${owner}`, eventId: eventIdOf('space_gone', owner), requestId: requestIdOf('space_gone'),
          ownerPlane: owner, deliveryState: 'pending', deliveryAttempts: 0, nextAttemptAt: 1_000, createdAt: 0,
        }],
      },
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    const orphan = handle.tables.spaceDeletionOwnerDeliveryEvents.find((row) => row._id === `orphan_${owner}`);
    assert.equal(orphan.deliveryState, 'closed');
    assert.equal(orphan.lastDeliveryError, 'deletion_not_live');
    assert.equal(warn.mock.callCount(), 1, 'the corrupt row is logged once');
    const payload = warn.mock.calls[0].arguments[1];
    assert.deepEqual(Object.keys(payload).sort(), ['eventId', 'reason', 'requestId', 'scope']);
    assert.equal(payload.reason, 'deletion_not_live');
    assert.equal(payload.scope, owner);
    assert.equal(payload.eventId, eventIdOf('space_gone', owner));
  });

  test(`${owner}: a delivery whose Space is missing is closed with the same warning`, async (t) => {
    const warn = t.mock.method(console, 'warn', () => {});
    const requestOnly = deletion('space_gone', { rows: [] });
    requestOnly.spaces = [];
    const handle = harness(tablesOf(
      requestOnly,
      { spaceDeletionOwnerDeliveryEvents: [{
        _id: `unspaced_${owner}`, eventId: eventIdOf('space_gone', owner), requestId: requestIdOf('space_gone'),
        ownerPlane: owner, deliveryState: 'pending', deliveryAttempts: 0, nextAttemptAt: 1_000, createdAt: 0,
      }] },
      deletion('space_b', { rows: [{ owner, state: 'pending', nextAttemptAt: 2_000 }], receipts: { [owner]: 'pending' } }),
    ));

    const claimed = await spaces[spec.claim]._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
    assert.equal(claimed.eventId, eventIdOf('space_b', owner));
    const row = handle.tables.spaceDeletionOwnerDeliveryEvents.find((candidate) => candidate._id === `unspaced_${owner}`);
    assert.equal(row.deliveryState, 'closed');
    assert.equal(row.lastDeliveryError, 'deletion_not_live');
    assert.equal(warn.mock.callCount(), 1);
    assert.equal(warn.mock.calls[0].arguments[1].reason, 'deletion_not_live');
  });
}

test('completing a deletion closes every live owner delivery of that request and leaves an acknowledged one as it is', async () => {
  const handle = harness(tablesOf(
    deletion('space_a', {
      rows: [
        { owner: 'model', state: 'acknowledged', attempts: 1, nextAttemptAt: 1_000 },
        { owner: 'data', state: 'failed', attempts: 1, nextAttemptAt: 3_000, lastDeliveryError: 'Data Space deletion returned HTTP 503' },
        { owner: 'ingestion', state: 'claimed', attempts: 1, nextAttemptAt: 0, leaseOwner: 'worker-old', leaseExpiresAt: 5_000 },
      ],
      receipts: { data: 'pending', ingestion: 'pending' },
    }),
  ));

  const first = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
    requestId: requestIdOf('space_a'), ownerPlane: 'data', status: 'succeeded', now: NOW,
  });
  assert.deepEqual(first, { ownerPlane: 'data', status: 'succeeded', finalized: false });
  assert.equal(rowOf(handle.tables, 'space_a', 'data').deliveryState, 'failed', 'nothing closes before the deletion completes');

  const second = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
    requestId: requestIdOf('space_a'), ownerPlane: 'ingestion', status: 'succeeded', now: NOW,
  });
  assert.deepEqual(second, { ownerPlane: 'ingestion', status: 'succeeded', finalized: true });
  assert.equal(spaceOf(handle.tables, 'space_a').lifecycle, 'deleted');
  assert.equal(rowOf(handle.tables, 'space_a', 'data').deliveryState, 'closed');
  assert.equal(rowOf(handle.tables, 'space_a', 'data').lastDeliveryError, 'deletion_completed');
  const ingestion = rowOf(handle.tables, 'space_a', 'ingestion');
  assert.equal(ingestion.deliveryState, 'closed');
  assert.equal(ingestion.leaseOwner, undefined);
  assert.equal(ingestion.leaseExpiresAt, undefined);
  assert.equal(rowOf(handle.tables, 'space_a', 'model').deliveryState, 'acknowledged', 'an acknowledged delivery is not rewritten');
});

test('the claim never selects a closed row for any owner, whatever the clock says', () => {
  const closed = { attempts: 1, nextAttemptAt: 0, state: 'closed' };
  for (const now of [1, NOW, 10_000_000]) {
    assert.equal(canClaimSpaceLifecycleDelivery(closed, now), false, `claimable at ${now}`);
  }
});

test('the claim time is checked for the Model and Data owners, as before', async () => {
  const handle = harness(tablesOf(
    deletion('space_a', { rows: [{ owner: 'model', state: 'pending', nextAttemptAt: 1_000 }] }),
    deletion('space_b', { rows: [{ owner: 'data', state: 'pending', nextAttemptAt: 1_000 }] }),
  ));
  await assert.rejects(
    spaces.claimNextModelDeletionOwnerDelivery._handler(handle.ctx, { workerId: 'worker-1', now: 0 }),
    /claim time is invalid/,
  );
  await assert.rejects(
    spaces.claimNextDataDeletionOwnerDelivery._handler(handle.ctx, { workerId: 'worker-1', now: 0 }),
    /claim time is invalid/,
  );
});

test('the claim time is checked for the Ingestion owner as well, even with no delivery to claim', async () => {
  const handle = harness(tablesOf(deletion('space_a', { lifecycle: 'active', rows: [] })));
  for (const now of [0, -1, 1.5, Number.NaN]) {
    await assert.rejects(
      spaces.claimNextIngestionDeletionOwnerDelivery._handler(handle.ctx, { workerId: 'worker-1', now }),
      /claim time is invalid/,
      String(now),
    );
  }
});
