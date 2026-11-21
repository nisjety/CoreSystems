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
const {
  canClaimSpaceLifecycleDelivery,
  rearmSpaceLifecycleDeliveryAfterPermanentFailure,
} = require('../convex/spaceLifecycle.ts');
const fake = require('./helpers/fake-convex.cjs');

const REQUEST_ID = 'space:space_a:deletion:idem-1';
const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000;
const OWNER_PLANES = ['application', 'control', 'data', 'ingestion', 'model', 'infra'];

const OWNERS = {
  model: { adapter: modelAdapter, module: 'spaceDeletionModel' },
  data: { adapter: dataAdapter, module: 'spaceDeletionData' },
  ingestion: { adapter: ingestionAdapter, module: 'spaceDeletionIngestion' },
};

function eventIdOf(requestId, owner) {
  return `space-deletion:${requestId}:owner:${owner}`;
}

/**
 * One personal Space mid-deletion. The owner's delivery row is `rowState`, the owner's receipt
 * is `ownerReceipt`, and the other owners' receipts are pending.
 */
function seedTables(owner, {
  rowState = 'failed_permanent',
  ownerReceipt = 'pending',
  lifecycle = 'deleting',
  requestState = 'authorized',
} = {}) {
  return {
    spaces: [{
      _id: 'space_row', spaceRef: 'space_a', externalOrgId: 'org_a', kind: 'personal',
      ownerExternalAuthId: 'user_a', lifecycle, lifecycleRevision: 4,
    }],
    spaceDeletionRequests: [{
      _id: 'request_row', requestId: REQUEST_ID, idempotencyKey: 'idem-1', spaceRef: 'space_a',
      externalOrgId: 'org_a', ownerExternalAuthId: 'user_a', state: requestState,
    }],
    spaceDeletionOwnerDeliveryEvents: [{
      _id: 'event_row', eventId: eventIdOf(REQUEST_ID, owner), requestId: REQUEST_ID, ownerPlane: owner,
      deliveryState: rowState, deliveryAttempts: 1, nextAttemptAt: 0, createdAt: 0,
      lastDeliveryError: 'permanent_refusal_http_403',
    }],
    spaceDeletionOwnerReceipts: OWNER_PLANES.map((ownerPlane) => ({
      _id: `receipt_${ownerPlane}`, requestId: REQUEST_ID, ownerPlane,
      status: ownerPlane === owner ? ownerReceipt : 'pending', deadlineAt: 7_000, updatedAt: 0,
    })),
  };
}

/** A parked or live delivery row of any request, for the list. */
function deliveryRow({ requestId, owner, deliveryState, lastDeliveryError, attempts = 1 }) {
  return {
    _id: `row_${requestId}_${owner}`, eventId: eventIdOf(requestId, owner), requestId, ownerPlane: owner,
    deliveryState, deliveryAttempts: attempts, nextAttemptAt: 0, createdAt: 0, lastDeliveryError,
  };
}

/**
 * A ctx whose runMutation runs the real spaces.ts handlers. `makeCtx` copies the
 * seed rows, so every assertion reads the live rows through `handle.tables`.
 */
function harness(seed) {
  const handle = fake.makeCtx(seed, {
    runMutation(fn, args) {
      const [moduleName, exportName] = getFunctionName(fn).split(':');
      assert.equal(moduleName, 'spaces', `unexpected mutation ${getFunctionName(fn)}`);
      return spaces[exportName]._handler(handle.ctx, args);
    },
  });
  return handle;
}

function eventRow(tables) {
  return tables.spaceDeletionOwnerDeliveryEvents[0];
}

function receiptFor(tables, owner) {
  return tables.spaceDeletionOwnerReceipts.find((receipt) => receipt.ownerPlane === owner);
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

function writeCount(log) {
  return log.inserts.length + log.patches.length + log.deletes.length + log.scheduled.length;
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

/** The owner's exact 200 answer for this request. */
function ownerAnswer(owner, outcome) {
  return {
    request_id: REQUEST_ID,
    owner_plane: owner,
    owner_outcome: outcome,
    remaining_work: owner === 'model' ? [] : 'owner work remains',
  };
}

test('the re-arm and the parked-delivery list are internal functions, not public ones', () => {
  assert.equal(spaces.rearmParkedDeletionOwnerDelivery.isInternal, true);
  assert.equal(spaces.listParkedDeletionOwnerDeliveries.isInternal, true);
});

test('only a failed_permanent delivery takes the re-arm edge: it keeps its count and is claimable at the re-arm time', () => {
  const parked = { attempts: 12, nextAttemptAt: 500, state: 'failed_permanent' };
  const rearmed = rearmSpaceLifecycleDeliveryAfterPermanentFailure(parked, 9_000);
  assert.deepEqual(rearmed, { attempts: 12, nextAttemptAt: 9_000, state: 'pending' });
  assert.equal(canClaimSpaceLifecycleDelivery(rearmed, 8_999), false);
  assert.equal(canClaimSpaceLifecycleDelivery(rearmed, 9_000), true);
  for (const state of ['pending', 'claimed', 'acknowledged', 'failed', 'blocked_legal_hold', 'closed']) {
    assert.throws(
      () => rearmSpaceLifecycleDeliveryAfterPermanentFailure({ attempts: 1, nextAttemptAt: 0, state }, 9_000),
      /failed_permanent/,
      state,
    );
  }
  assert.throws(() => rearmSpaceLifecycleDeliveryAfterPermanentFailure(parked, 0), /positive safe integer/);
});

for (const owner of Object.keys(OWNERS)) {
  const spec = OWNERS[owner];
  const eventId = eventIdOf(REQUEST_ID, owner);

  test(`${owner}: a failed_permanent delivery is re-armed for its owner, the receipt gets a fresh window, and the owner's answer is acknowledged`, async () => {
    const handle = harness(seedTables(owner));

    const rearmed = await spaces.rearmParkedDeletionOwnerDelivery._handler(handle.ctx, { eventId, now: 1_000 });
    assert.deepEqual(rearmed, { eventId, status: 'pending' });
    assert.equal(eventRow(handle.tables).deliveryState, 'pending');
    assert.equal(eventRow(handle.tables).nextAttemptAt, 1_000);
    assert.equal(eventRow(handle.tables).lastDeliveryError, undefined);
    assert.equal(eventRow(handle.tables).deliveryAttempts, 1, 'the parked claim stays in the history');
    assert.equal(receiptFor(handle.tables, owner).status, 'pending');
    assert.equal(receiptFor(handle.tables, owner).deadlineAt, 1_000 + SEVEN_DAYS_MS, 'the owner gets a fresh deadline');
    assert.deepEqual(scheduledNames(handle.log), [
      'spaceDeletionReceiptReconciler:reconcileOne',
      `${spec.module}:deliverOne`,
    ]);
    assert.equal(handle.log.scheduled[0].delay, SEVEN_DAYS_MS);

    let result;
    const sent = await withOwnerReply(jsonReply(200, ownerAnswer(owner, 'succeeded')), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });
    assert.deepEqual(result, { eventId, status: 'succeeded' });
    assert.equal(sent.length, 1, 'the owner is asked again');
    assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
    assert.equal(receiptFor(handle.tables, owner).status, 'succeeded');
  });

  test(`${owner}: a re-arm on an unknown receipt keeps the receipt unknown, and the owner's later answer can still improve it`, async () => {
    const handle = harness(seedTables(owner, { ownerReceipt: 'unknown' }));

    await spaces.rearmParkedDeletionOwnerDelivery._handler(handle.ctx, { eventId, now: 1_000 });
    assert.equal(eventRow(handle.tables).deliveryState, 'pending');
    assert.equal(receiptFor(handle.tables, owner).status, 'unknown');
    assert.equal(receiptFor(handle.tables, owner).deadlineAt, 7_000, 'the elapsed window is not extended');
    assert.deepEqual(scheduledNames(handle.log), [`${spec.module}:deliverOne`]);

    await withOwnerReply(jsonReply(200, ownerAnswer(owner, 'partial')), async () => {
      await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });
    assert.equal(receiptFor(handle.tables, owner).status, 'partial');
    assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
  });

  test(`${owner}: a re-arm refuses every state it must not touch, and writes nothing`, async () => {
    const cases = [
      ['a row that is not failed_permanent', seedTables(owner, { rowState: 'pending' }), eventId,
        'only a failed_permanent owner delivery may be re-armed'],
      ['a receipt that already has an answer', seedTables(owner, { ownerReceipt: 'succeeded' }), eventId,
        'invalid Space deletion owner receipt state for a re-arm: succeeded'],
      ['a receipt that is blocked by a legal hold', seedTables(owner, { ownerReceipt: 'blocked_legal_hold' }), eventId,
        'invalid Space deletion owner receipt state for a re-arm: blocked_legal_hold'],
      ['a Space that is not fenced', seedTables(owner, { lifecycle: 'active' }), eventId,
        'parked deletion re-arm Space is not fenced'],
      ['a request that is not authorized', seedTables(owner, { requestState: 'rejected' }), eventId,
        'parked deletion re-arm lacks an authorized request'],
      ['a missing delivery row', seedTables(owner), eventIdOf('space:missing:deletion:x', owner),
        'parked deletion delivery event not found'],
    ];
    for (const [name, seed, target, message] of cases) {
      const handle = harness(seed);
      const before = JSON.stringify(handle.tables);
      await fake.rejectsWith(
        spaces.rearmParkedDeletionOwnerDelivery._handler(handle.ctx, { eventId: target, now: 1_000 }),
        message,
      );
      assert.equal(writeCount(handle.log), 0, `${owner}: ${name}`);
      assert.equal(JSON.stringify(handle.tables), before, `${owner}: ${name}`);
    }
  });
}

test('a claim-bound park is re-armed like any other failed_permanent delivery, and its count is kept', async () => {
  const seed = seedTables('model');
  eventRow(seed).deliveryAttempts = 12;
  eventRow(seed).lastDeliveryError = 'delivery_retry_bound_reached';
  const handle = harness(seed);

  const rearmed = await spaces.rearmParkedDeletionOwnerDelivery._handler(handle.ctx, {
    eventId: eventIdOf(REQUEST_ID, 'model'), now: 1_000,
  });
  assert.deepEqual(rearmed, { eventId: eventIdOf(REQUEST_ID, 'model'), status: 'pending' });
  assert.equal(eventRow(handle.tables).deliveryAttempts, 12);
  assert.equal(eventRow(handle.tables).lastDeliveryError, undefined);
});

test('the parked-delivery list names each parked row by ids, owner, state and reason, and nothing else', async () => {
  const handle = harness({
    spaceDeletionOwnerDeliveryEvents: [
      deliveryRow({ requestId: 'r1', owner: 'model', deliveryState: 'failed_permanent', lastDeliveryError: 'permanent_refusal_http_403' }),
      deliveryRow({ requestId: 'r2', owner: 'data', deliveryState: 'blocked_legal_hold', lastDeliveryError: 'erasure_blocked_legal_hold' }),
      deliveryRow({ requestId: 'r3', owner: 'ingestion', deliveryState: 'failed_permanent', lastDeliveryError: 'delivery_retry_bound_reached', attempts: 12 }),
      deliveryRow({ requestId: 'r4', owner: 'model', deliveryState: 'pending' }),
      deliveryRow({ requestId: 'r5', owner: 'data', deliveryState: 'acknowledged' }),
      deliveryRow({ requestId: 'r6', owner: 'ingestion', deliveryState: 'closed', lastDeliveryError: 'deletion_completed' }),
    ],
  });

  const { rows, nextCursor } = await spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, {});
  assert.equal(nextCursor, null, 'three parked rows fit one page');
  const byEvent = [...rows].sort((left, right) => left.eventId.localeCompare(right.eventId));
  assert.deepEqual(byEvent, [
    { eventId: eventIdOf('r1', 'model'), requestId: 'r1', ownerPlane: 'model', deliveryState: 'failed_permanent', reason: 'permanent_refusal_http_403', deliveryAttempts: 1 },
    { eventId: eventIdOf('r2', 'data'), requestId: 'r2', ownerPlane: 'data', deliveryState: 'blocked_legal_hold', reason: 'erasure_blocked_legal_hold', deliveryAttempts: 1 },
    { eventId: eventIdOf('r3', 'ingestion'), requestId: 'r3', ownerPlane: 'ingestion', deliveryState: 'failed_permanent', reason: 'delivery_retry_bound_reached', deliveryAttempts: 12 },
  ]);
  for (const row of rows) {
    assert.deepEqual(Object.keys(row).sort(), ['deliveryAttempts', 'deliveryState', 'eventId', 'ownerPlane', 'reason', 'requestId']);
  }
});

test('a page holds at most its limit, and the cursor walks every parked row behind it in creation order', async () => {
  const handle = harness({
    spaceDeletionOwnerDeliveryEvents: [
      { ...deliveryRow({ requestId: 'r1', owner: 'model', deliveryState: 'failed_permanent', lastDeliveryError: 'permanent_refusal_http_403' }), _creationTime: 1 },
      { ...deliveryRow({ requestId: 'r2', owner: 'data', deliveryState: 'failed_permanent', lastDeliveryError: 'permanent_refusal_http_401' }), _creationTime: 2 },
      { ...deliveryRow({ requestId: 'r3', owner: 'ingestion', deliveryState: 'blocked_legal_hold', lastDeliveryError: 'erasure_blocked_legal_hold' }), _creationTime: 3 },
      { ...deliveryRow({ requestId: 'r4', owner: 'model', deliveryState: 'pending' }), _creationTime: 4 },
      { ...deliveryRow({ requestId: 'r5', owner: 'model', deliveryState: 'failed_permanent', lastDeliveryError: 'delivery_retry_bound_reached', attempts: 12 }), _creationTime: 5 },
      { ...deliveryRow({ requestId: 'r6', owner: 'data', deliveryState: 'blocked_legal_hold', lastDeliveryError: 'erasure_blocked_legal_hold' }), _creationTime: 6 },
    ],
  });

  const pages = [];
  let cursor;
  for (;;) {
    const page = await spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, { cursor, limit: 2 });
    pages.push(page);
    if (page.nextCursor === null) break;
    cursor = page.nextCursor;
  }
  assert.deepEqual(pages.map((page) => page.rows.map((row) => row.requestId)), [['r1', 'r2'], ['r3', 'r5'], ['r6']]);
  assert.equal(pages[0].nextCursor, '2|row_r2_data', 'the cursor is the creation time and id of the page\'s last row');
});

test('250 parked rows come back in two pages, in creation order, with no gap and no duplicate', async () => {
  const owners = ['model', 'data', 'ingestion'];
  const states = ['failed_permanent', 'blocked_legal_hold'];
  const seeded = [];
  for (let index = 0; index < 250; index += 1) {
    // The creation times are a permutation, so the rows are not already in creation order when the list reads them.
    seeded.push({
      ...deliveryRow({ requestId: `r${index}`, owner: owners[index % 3], deliveryState: states[index % 2], lastDeliveryError: 'permanent_refusal_http_403' }),
      _creationTime: 1_000 + ((index * 37) % 250),
    });
  }
  for (let index = 0; index < 40; index += 1) {
    seeded.push({ ...deliveryRow({ requestId: `p${index}`, owner: owners[index % 3], deliveryState: 'pending' }), _creationTime: 1_000 + index });
  }
  const handle = harness({ spaceDeletionOwnerDeliveryEvents: seeded });
  const expected = seeded
    .filter((row) => row.deliveryState !== 'pending')
    .sort((left, right) => left._creationTime - right._creationTime)
    .map((row) => row.eventId);
  assert.equal(expected.length, 250);

  const first = await spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, {});
  assert.equal(first.rows.length, 200, 'the default page is 200 rows');
  assert.notEqual(first.nextCursor, null);
  const second = await spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, { cursor: first.nextCursor });
  assert.equal(second.rows.length, 50);
  assert.equal(second.nextCursor, null, 'the last page has no cursor');

  const listed = [...first.rows, ...second.rows].map((row) => row.eventId);
  assert.equal(new Set(listed).size, 250, 'no row is repeated');
  assert.deepEqual(listed, expected, 'every parked row, in creation order, with no gap');
});

test('rows that share a creation time are ordered by id, and a cursor splits them without a gap or a repeat', async () => {
  const handle = harness({
    spaceDeletionOwnerDeliveryEvents: [
      { ...deliveryRow({ requestId: 'b', owner: 'model', deliveryState: 'failed_permanent', lastDeliveryError: 'permanent_refusal_http_403' }), _creationTime: 10 },
      { ...deliveryRow({ requestId: 'a', owner: 'data', deliveryState: 'blocked_legal_hold', lastDeliveryError: 'erasure_blocked_legal_hold' }), _creationTime: 10 },
      { ...deliveryRow({ requestId: 'c', owner: 'ingestion', deliveryState: 'failed_permanent', lastDeliveryError: 'permanent_refusal_http_403' }), _creationTime: 9 },
    ],
  });

  const pages = [];
  let cursor;
  for (;;) {
    const page = await spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, { cursor, limit: 1 });
    pages.push(page);
    if (page.nextCursor === null) break;
    cursor = page.nextCursor;
  }
  assert.deepEqual(pages.map((page) => page.rows[0].requestId), ['c', 'a', 'b']);
  assert.equal(pages[1].nextCursor, '10|row_a_data');
});

test('a malformed cursor and a bad limit are refused, and a cursor past every row lists nothing', async () => {
  const handle = harness({
    spaceDeletionOwnerDeliveryEvents: [
      deliveryRow({ requestId: 'r1', owner: 'model', deliveryState: 'failed_permanent', lastDeliveryError: 'permanent_refusal_http_403' }),
    ],
  });

  for (const cursor of ['', 'nonsense', '|row_r1_model', '10|', 'NaN|row_r1_model', 'Infinity|row_r1_model']) {
    await fake.rejectsWith(
      spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, { cursor }),
      'parked deletion list cursor is invalid',
    );
  }
  for (const limit of [0, -1, 1.5, Number.NaN]) {
    await fake.rejectsWith(
      spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, { limit }),
      'parked deletion list limit is invalid',
    );
  }
  assert.deepEqual(
    await spaces.listParkedDeletionOwnerDeliveries._handler(handle.ctx, { cursor: '999999|zzz' }),
    { rows: [], nextCursor: null },
  );
});
