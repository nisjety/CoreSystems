const assert = require('node:assert/strict');
const test = require('node:test');

// The three adapters read these at import time. The values are test fixtures.
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
const fake = require('./helpers/fake-convex.cjs');

const REQUEST_ID = 'space:space_a:deletion:idem-1';
const HOLD_CODE = 'erasure_blocked_legal_hold';
const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000;
const OWNER_PLANES = ['application', 'control', 'data', 'ingestion', 'model', 'infra'];

// One entry per adapted owner: its adapter, its re-drive export, and the module its re-ask is scheduled on.
const OWNERS = {
  model: { adapter: modelAdapter, redrive: 'redriveModelDeletionAfterLegalHoldRelease', module: 'spaceDeletionModel' },
  data: { adapter: dataAdapter, redrive: 'redriveDataDeletionAfterLegalHoldRelease', module: 'spaceDeletionData' },
  ingestion: { adapter: ingestionAdapter, redrive: 'redriveIngestionDeletionAfterLegalHoldRelease', module: 'spaceDeletionIngestion' },
};

function eventIdOf(owner) {
  return `space-deletion:${REQUEST_ID}:owner:${owner}`;
}

/**
 * One personal Space mid-deletion: fenced and authorized, with the owner's delivery parked by a
 * legal hold (or not), the owner's receipt in `ownerReceipt`, and every other owner's receipt in `others`.
 */
function seedTables(owner, { ownerReceipt = 'blocked_legal_hold', parked = true, others = 'pending' } = {}) {
  return {
    spaces: [{
      _id: 'space_row', spaceRef: 'space_a', externalOrgId: 'org_a', kind: 'personal',
      ownerExternalAuthId: 'user_a', lifecycle: 'deleting', lifecycleRevision: 4,
    }],
    spaceDeletionRequests: [{
      _id: 'request_row', requestId: REQUEST_ID, idempotencyKey: 'idem-1', spaceRef: 'space_a',
      externalOrgId: 'org_a', ownerExternalAuthId: 'user_a', state: 'authorized',
    }],
    spaceDeletionOwnerDeliveryEvents: [{
      _id: 'event_row', eventId: eventIdOf(owner), requestId: REQUEST_ID, ownerPlane: owner,
      deliveryState: parked ? 'blocked_legal_hold' : 'pending', deliveryAttempts: 1, nextAttemptAt: 0, createdAt: 0,
      lastDeliveryError: parked ? HOLD_CODE : undefined,
    }],
    spaceDeletionOwnerReceipts: OWNER_PLANES.map((ownerPlane) => ({
      _id: `receipt_${ownerPlane}`, requestId: REQUEST_ID, ownerPlane,
      status: ownerPlane === owner ? ownerReceipt : others, deadlineAt: 7_000, updatedAt: 0,
    })),
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

/** The owner's typed hold refusal: the exact code with retryable false. */
function holdReply() {
  return jsonReply(409, {
    error: { code: HOLD_CODE, message: 'upstream-hold-detail', details: { retryable: false } },
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

function restoreFlag(prior) {
  if (prior === undefined) delete process.env.SPACE_DELETION_HOLD_RELEASE;
  else process.env.SPACE_DELETION_HOLD_RELEASE = prior;
}

/** Sets SPACE_DELETION_HOLD_RELEASE for one test (`undefined` removes it), restored afterwards. */
function setFlag(t, value) {
  const prior = process.env.SPACE_DELETION_HOLD_RELEASE;
  t.after(() => restoreFlag(prior));
  if (value === undefined) delete process.env.SPACE_DELETION_HOLD_RELEASE;
  else process.env.SPACE_DELETION_HOLD_RELEASE = value;
}

for (const owner of Object.keys(OWNERS)) {
  const spec = OWNERS[owner];
  const redrive = () => spaces[spec.redrive];

  test(`${owner}: with the flag off the re-drive refuses before any read or write`, async (t) => {
    setFlag(t, undefined);
    const handle = harness(seedTables(owner));
    const before = JSON.stringify(handle.tables);
    await fake.rejectsWith(
      redrive()._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 }),
      'legal_hold_release_disabled',
    );
    assert.equal(writeCount(handle.log), 0);
    assert.equal(JSON.stringify(handle.tables), before, 'the held Space, delivery and receipt are untouched');
  });

  test(`${owner}: with the flag on the re-drive re-arms the parked delivery, asks the owner again, and a fresh hold answer blocks it again`, async (t) => {
    setFlag(t, 'true');
    const handle = harness(seedTables(owner));

    const redriven = await redrive()._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 });
    assert.deepEqual(redriven, { eventId: eventIdOf(owner), status: 'pending' });
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
    assert.deepEqual(handle.log.scheduled[0].args, { requestId: REQUEST_ID });

    let result;
    const sent = await withOwnerReply(holdReply(), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });
    assert.deepEqual(result, { eventId: eventIdOf(owner), status: 'blocked_legal_hold' });
    assert.equal(sent.length, 1, 'the owner is asked once for this claim');
    assert.equal(eventRow(handle.tables).deliveryState, 'blocked_legal_hold');
    assert.equal(receiptFor(handle.tables, owner).status, 'blocked_legal_hold', 'a fresh hold answer blocks the receipt again');
    assert.equal(scheduledNames(handle.log).length, 2, 'a hold schedules no retry');
  });

  test(`${owner}: after a re-drive the owner's answer is acknowledged and finishes the held deletion`, async (t) => {
    setFlag(t, 'true');
    const handle = harness(seedTables(owner, { others: 'succeeded' }));
    await redrive()._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 });

    let result;
    await withOwnerReply(jsonReply(200, ownerAnswer(owner, 'succeeded')), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });
    assert.deepEqual(result, { eventId: eventIdOf(owner), status: 'succeeded' });
    assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
    assert.equal(receiptFor(handle.tables, owner).status, 'succeeded');
    assert.equal(handle.tables.spaces[0].lifecycle, 'deleted', 'every owner succeeded, so the fenced Space is deleted');
  });

  test(`${owner}: the re-drive refuses a receipt that already has an answer, and writes nothing`, async (t) => {
    setFlag(t, 'true');
    const handle = harness(seedTables(owner, { ownerReceipt: 'failed' }));
    const before = JSON.stringify(handle.tables);
    await fake.rejectsWith(
      redrive()._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 }),
      'failed -> pending',
    );
    assert.equal(writeCount(handle.log), 0);
    assert.equal(JSON.stringify(handle.tables), before);
  });

  test(`${owner}: with the flag off a coordinator answer over a held receipt is refused and nothing is written`, async (t) => {
    setFlag(t, undefined);
    const handle = harness(seedTables(owner));
    const before = JSON.stringify(handle.tables);
    await fake.rejectsWith(
      spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
        requestId: REQUEST_ID, ownerPlane: owner, status: 'failed', now: 1_000,
      }),
      'invalid Space deletion owner receipt transition: blocked_legal_hold -> failed',
    );
    assert.equal(writeCount(handle.log), 0);
    assert.equal(JSON.stringify(handle.tables), before);
  });

  test(`${owner}: with the flag on a coordinator answer over a held receipt closes the parked delivery and keeps the Space fenced`, async (t) => {
    setFlag(t, 'true');
    const handle = harness(seedTables(owner));

    const result = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: REQUEST_ID, ownerPlane: owner, status: 'failed', receiptRef: 'coordinator:answer', now: 1_000,
    });
    assert.deepEqual(result, { ownerPlane: owner, status: 'failed', finalized: false });
    assert.equal(receiptFor(handle.tables, owner).status, 'failed');
    assert.equal(eventRow(handle.tables).deliveryState, 'closed', 'the parked delivery is moot once the receipt is answered');
    assert.equal(eventRow(handle.tables).lastDeliveryError, 'receipt_answered_by_coordinator');
    assert.equal(handle.tables.spaces[0].lifecycle, 'deleting');
    assert.deepEqual(scheduledNames(handle.log), [], 'nothing re-asks the owner');
  });
}
