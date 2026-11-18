const assert = require('node:assert/strict');
const test = require('node:test');

// spaceDeletionModel.ts reads these at import time. The values are test fixtures.
process.env.MODEL_SPACE_DELETION_URL = 'http://model.invalid/api/v1/internal/space-deletion/threads';
process.env.CONTROL_SPACE_DELETION_MODEL_BEARER = 'model-bearer-aaaaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_SESSION_BEARER = 'session-bearer-aaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_CAPABILITY_BEARER = 'capability-bearer-aaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const { deliverOne } = require('../convex/spaceDeletionModel.ts');
const spaces = require('../convex/spaces.ts');
const {
  canClaimSpaceLifecycleDelivery,
  rearmSpaceLifecycleDeliveryAfterLegalHold,
} = require('../convex/spaceLifecycle.ts');
const {
  assertReceiptRearmAfterLegalHold,
  assertReceiptTransition,
  legalHoldReleaseEnabled,
} = require('../convex/spaceDeletionReceipts.ts');
const fake = require('./helpers/fake-convex.cjs');

const REQUEST_ID = 'space:space_a:deletion:idem-1';
const MODEL_EVENT_ID = `space-deletion:${REQUEST_ID}:owner:model`;
const HOLD_CODE = 'erasure_blocked_legal_hold';
const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000;
const OTHER_OWNERS = ['application', 'control', 'data', 'ingestion', 'infra'];

/**
 * One personal Space mid-deletion: fenced and authorized, with the Model delivery
 * parked by a legal hold (or not), the Model receipt in `modelReceipt`, and every
 * other owner's receipt in `others`.
 */
function seedTables({
  modelReceipt = 'blocked_legal_hold',
  parked = true,
  lifecycle = 'deleting',
  requestState = 'authorized',
  others = 'pending',
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
      _id: 'event_row', eventId: MODEL_EVENT_ID, requestId: REQUEST_ID, ownerPlane: 'model',
      deliveryState: parked ? 'blocked_legal_hold' : 'pending', deliveryAttempts: 1,
      nextAttemptAt: 0, createdAt: 0, lastDeliveryError: parked ? HOLD_CODE : undefined,
    }],
    spaceDeletionOwnerReceipts: [
      {
        _id: 'receipt_model', requestId: REQUEST_ID, ownerPlane: 'model', status: modelReceipt,
        deadlineAt: 7_000, updatedAt: 0,
      },
      ...OTHER_OWNERS.map((ownerPlane) => ({
        _id: `receipt_${ownerPlane}`, requestId: REQUEST_ID, ownerPlane, status: others,
        deadlineAt: 7_000, updatedAt: 0,
      })),
    ],
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

function receiptFor(tables, ownerPlane = 'model') {
  return tables.spaceDeletionOwnerReceipts.find((receipt) => receipt.ownerPlane === ownerPlane);
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

function writeCount(log) {
  return log.inserts.length + log.patches.length + log.deletes.length + log.scheduled.length;
}

/** Answers every Model call with `reply()` while `body` runs, and returns the requests sent. */
async function withModelReply(reply, body) {
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

/** The gateway's typed hold refusal: the exact code with retryable false. */
function holdReply() {
  return jsonReply(409, {
    error: { code: HOLD_CODE, message: 'upstream-hold-detail', details: { retryable: false } },
  });
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

test('SPACE_DELETION_HOLD_RELEASE is read at call time as the literal "true" and is off by default', (t) => {
  const prior = process.env.SPACE_DELETION_HOLD_RELEASE;
  t.after(() => restoreFlag(prior));
  delete process.env.SPACE_DELETION_HOLD_RELEASE;
  assert.equal(legalHoldReleaseEnabled(), false);
  for (const value of ['0', '1', 'yes', 'on', ' true', 'true ', 'TRUE', 'True', '']) {
    process.env.SPACE_DELETION_HOLD_RELEASE = value;
    assert.equal(legalHoldReleaseEnabled(), false, JSON.stringify(value));
  }
  process.env.SPACE_DELETION_HOLD_RELEASE = 'true';
  assert.equal(legalHoldReleaseEnabled(), true);
});

test('with the flag off a legal-hold block stays terminal and nothing is written', async (t) => {
  setFlag(t, undefined);
  const handle = harness(seedTables({ others: 'succeeded' }));
  const before = JSON.stringify(handle.tables);

  await fake.rejectsWith(
    spaces.redriveModelDeletionAfterLegalHoldRelease._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 }),
    'legal_hold_release_disabled',
  );
  await fake.rejectsWith(
    spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: REQUEST_ID, ownerPlane: 'model', status: 'succeeded', now: 1_000,
    }),
    'invalid Space deletion owner receipt transition: blocked_legal_hold -> succeeded',
  );
  assert.equal(JSON.stringify(handle.tables), before, 'the held Space, delivery and receipt are untouched');
  assert.equal(writeCount(handle.log), 0);
  for (const to of ['pending', 'succeeded', 'partial', 'failed', 'unknown']) {
    assert.throws(() => assertReceiptTransition('blocked_legal_hold', to), /invalid/i, to);
    assert.throws(
      () => assertReceiptTransition('blocked_legal_hold', to, { legalHoldRelease: legalHoldReleaseEnabled() }),
      /invalid/i,
      to,
    );
  }
});

test('with the flag on a blocked receipt takes only an owner answer, never a hand-set pending', () => {
  const on = { legalHoldRelease: true };
  for (const to of ['succeeded', 'partial', 'failed', 'unknown']) {
    assert.doesNotThrow(() => assertReceiptTransition('blocked_legal_hold', to, on), to);
  }
  assert.throws(() => assertReceiptTransition('blocked_legal_hold', 'pending', on), /invalid/i);
  // The option widens only the blocked edge: answered receipts stay answered.
  assert.throws(() => assertReceiptTransition('succeeded', 'blocked_legal_hold', on), /invalid/i);
  assert.throws(() => assertReceiptTransition('succeeded', 'failed', on), /invalid/i);
  assert.throws(() => assertReceiptTransition('partial', 'pending', on), /invalid/i);
  assert.doesNotThrow(() => assertReceiptTransition('pending', 'succeeded', on));
  assert.doesNotThrow(() => assertReceiptTransition('unknown', 'succeeded', on));
  // The re-drive edge is its own function, and only a blocked receipt may take it.
  assert.doesNotThrow(() => assertReceiptRearmAfterLegalHold('blocked_legal_hold'));
  for (const from of ['pending', 'unknown', 'succeeded', 'partial', 'failed']) {
    assert.throws(() => assertReceiptRearmAfterLegalHold(from), /invalid/i, from);
  }
});

test('a parked delivery is re-armed at the release time only and keeps its attempt count', () => {
  const parked = { attempts: 2, nextAttemptAt: 500, state: 'blocked_legal_hold' };
  const rearmed = rearmSpaceLifecycleDeliveryAfterLegalHold(parked, 9_000);
  assert.deepEqual(rearmed, { attempts: 2, nextAttemptAt: 9_000, state: 'pending' });
  assert.equal(canClaimSpaceLifecycleDelivery(rearmed, 8_999), false);
  assert.equal(canClaimSpaceLifecycleDelivery(rearmed, 9_000), true);
  for (const state of ['pending', 'claimed', 'acknowledged', 'failed', 'rejected']) {
    assert.throws(
      () => rearmSpaceLifecycleDeliveryAfterLegalHold({ attempts: 1, nextAttemptAt: 0, state }, 9_000),
      /parked by a legal hold/i,
      state,
    );
  }
  assert.throws(() => rearmSpaceLifecycleDeliveryAfterLegalHold(parked, 0), /positive safe integer/);
});

test('with the flag on a re-drive re-arms the parked Model delivery and asks the owner again', async (t) => {
  setFlag(t, 'true');
  const handle = harness(seedTables());

  const redriven = await spaces.redriveModelDeletionAfterLegalHoldRelease._handler(handle.ctx, {
    requestId: REQUEST_ID, now: 1_000,
  });
  assert.deepEqual(redriven, { eventId: MODEL_EVENT_ID, status: 'pending' });
  const event = eventRow(handle.tables);
  assert.equal(event.deliveryState, 'pending');
  assert.equal(event.nextAttemptAt, 1_000);
  assert.equal(event.lastDeliveryError, undefined);
  assert.equal(event.deliveryAttempts, 1, 'the parked claim stays in the history');
  const receipt = receiptFor(handle.tables);
  assert.equal(receipt.status, 'pending');
  assert.equal(receipt.deadlineAt, 1_000 + SEVEN_DAYS_MS, 'the owner gets a fresh deadline');
  assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionReceiptReconciler:reconcileOne', 'spaceDeletionModel:deliverOne']);
  assert.equal(handle.log.scheduled[0].delay, SEVEN_DAYS_MS);
  assert.deepEqual(handle.log.scheduled[0].args, { requestId: REQUEST_ID });

  // The worker claims the re-armed delivery. The owner still holds the erase and refuses it again.
  let result;
  const sent = await withModelReply(holdReply(), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
  });
  assert.deepEqual(result, { eventId: MODEL_EVENT_ID, status: 'blocked_legal_hold' });
  assert.equal(sent.length, 1, 'the owner is asked once for this claim');
  assert.equal(eventRow(handle.tables).deliveryState, 'blocked_legal_hold');
  assert.equal(eventRow(handle.tables).lastDeliveryError, HOLD_CODE);
  assert.equal(receiptFor(handle.tables).status, 'blocked_legal_hold', 'a fresh hold answer blocks the receipt again');
  assert.equal(scheduledNames(handle.log).length, 2, 'a hold schedules no retry');
});

test('with the flag on an outage on the re-ask takes the ordinary retry path and leaves the receipt pending', async (t) => {
  setFlag(t, 'true');
  const handle = harness(seedTables());
  await spaces.redriveModelDeletionAfterLegalHoldRelease._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 });

  let result;
  await withModelReply(jsonReply(503, {
    error: { code: 'erasure_hold_check_unavailable', message: 'upstream-check-detail', details: { retryable: true } },
  }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
  });
  assert.equal(result.status, 'retry_scheduled');
  assert.equal(eventRow(handle.tables).deliveryState, 'failed');
  assert.equal(eventRow(handle.tables).lastDeliveryError, 'Model Space deletion returned HTTP 503');
  assert.equal(receiptFor(handle.tables).status, 'pending', 'an outage never moves the receipt');
  assert.deepEqual(scheduledNames(handle.log), [
    'spaceDeletionReceiptReconciler:reconcileOne',
    'spaceDeletionModel:deliverOne',
    'spaceDeletionModel:deliverOne',
  ]);
});

test('with the flag on the owner answer after a re-drive finishes the held deletion', async (t) => {
  setFlag(t, 'true');
  const handle = harness(seedTables({ others: 'succeeded' }));
  await spaces.redriveModelDeletionAfterLegalHoldRelease._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 });

  let result;
  await withModelReply(jsonReply(200, {
    request_id: REQUEST_ID, owner_plane: 'model', owner_outcome: 'succeeded', remaining_work: [],
  }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
  });
  assert.deepEqual(result, { eventId: MODEL_EVENT_ID, status: 'succeeded' });
  assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
  assert.equal(receiptFor(handle.tables).status, 'succeeded');
  assert.equal(handle.tables.spaces[0].lifecycle, 'deleted', 'every owner succeeded, so the fenced Space is deleted');
});

test('with the flag on the re-drive refuses a delivery that is not parked, a receipt with an answer, an unfenced Space and an unauthorized request', async (t) => {
  setFlag(t, 'true');
  const cases = [
    ['a delivery that is not parked', seedTables({ parked: false }), 'only a delivery parked by a legal hold'],
    ['a receipt that already has an answer', seedTables({ modelReceipt: 'failed' }), 'failed -> pending'],
    ['a Space that is not fenced', seedTables({ lifecycle: 'active' }), 'Space is not fenced'],
    ['a request that is not authorized', seedTables({ requestState: 'pending_authorization' }), 'lacks an authorized request'],
  ];
  for (const [name, seed, message] of cases) {
    const handle = harness(seed);
    const before = JSON.stringify(handle.tables);
    await fake.rejectsWith(
      spaces.redriveModelDeletionAfterLegalHoldRelease._handler(handle.ctx, { requestId: REQUEST_ID, now: 1_000 }),
      message,
    );
    assert.equal(writeCount(handle.log), 0, name);
    assert.equal(JSON.stringify(handle.tables), before, name);
  }
});

test('with the flag on the trusted ingress cannot re-open a held receipt to pending by hand', async (t) => {
  setFlag(t, 'true');
  const handle = harness(seedTables({ others: 'succeeded' }));

  await fake.rejectsWith(
    spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
      requestId: REQUEST_ID, ownerPlane: 'model', status: 'pending', now: 1_000,
    }),
    'invalid Space deletion owner receipt transition: blocked_legal_hold -> pending',
  );
  assert.equal(receiptFor(handle.tables).status, 'blocked_legal_hold');
  assert.equal(handle.tables.spaces[0].lifecycle, 'deleting');
});

test('with the flag on the trusted ingress can record the post-hold answer and finish the deletion', async (t) => {
  setFlag(t, 'true');
  const handle = harness(seedTables({ others: 'succeeded' }));

  const finished = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
    requestId: REQUEST_ID,
    ownerPlane: 'model',
    status: 'succeeded',
    receiptRef: 'coordinator:model-answer',
    detail: 'operator relayed the post-hold Model answer',
    now: 1_000,
  });
  assert.deepEqual(finished, { ownerPlane: 'model', status: 'succeeded', finalized: true });
  assert.equal(receiptFor(handle.tables).status, 'succeeded');
  assert.equal(handle.tables.spaces[0].lifecycle, 'deleted');
});

test('with the flag on a relayed non-success answer on a held receipt keeps the Space fenced', async (t) => {
  setFlag(t, 'true');
  const handle = harness(seedTables({ others: 'succeeded' }));

  const result = await spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
    requestId: REQUEST_ID, ownerPlane: 'model', status: 'failed', now: 1_000,
  });
  assert.deepEqual(result, { ownerPlane: 'model', status: 'failed', finalized: false });
  assert.equal(receiptFor(handle.tables).status, 'failed');
  assert.equal(handle.tables.spaces[0].lifecycle, 'deleting');
});

/** Runs `body` with SPACE_DELETION_HOLD_RELEASE set to `value` (`undefined` removes it), then restores it. */
async function withFlag(value, body) {
  const prior = process.env.SPACE_DELETION_HOLD_RELEASE;
  try {
    if (value === undefined) delete process.env.SPACE_DELETION_HOLD_RELEASE;
    else process.env.SPACE_DELETION_HOLD_RELEASE = value;
    await body();
  } finally {
    restoreFlag(prior);
  }
}

test('the trusted ingress cannot record a legal hold for any owner but Model, flag on or off', async () => {
  for (const flag of [undefined, 'true']) {
    await withFlag(flag, async () => {
      for (const ownerPlane of OTHER_OWNERS) {
        const handle = harness(seedTables({ others: 'pending' }));
        const before = JSON.stringify(handle.tables);
        await fake.rejectsWith(
          spaces.recordDeletionOwnerReceipt._handler(handle.ctx, {
            requestId: REQUEST_ID, ownerPlane, status: 'blocked_legal_hold', now: 1_000,
          }),
          `${ownerPlane} owner may not record blocked_legal_hold`,
        );
        assert.equal(writeCount(handle.log), 0, `${ownerPlane} with flag ${flag}`);
        assert.equal(JSON.stringify(handle.tables), before, `${ownerPlane} with flag ${flag}`);
      }
    });
  }
});

test('the trusted ingress blocks a Model receipt only once the Model owner has refused the erase', async () => {
  for (const flag of [undefined, 'true']) {
    await withFlag(flag, async () => {
      // A live Model delivery (pending, or claimed by a worker): the owner has not refused the erase.
      for (const [receiptStatus, deliveryState] of [['pending', 'pending'], ['unknown', 'pending'], ['pending', 'claimed']]) {
        const live = harness(seedTables({ modelReceipt: receiptStatus, parked: false, others: 'pending' }));
        eventRow(live.tables).deliveryState = deliveryState;
        const before = JSON.stringify(live.tables);
        await fake.rejectsWith(
          spaces.recordDeletionOwnerReceipt._handler(live.ctx, {
            requestId: REQUEST_ID, ownerPlane: 'model', status: 'blocked_legal_hold', now: 1_000,
          }),
          'a Model legal hold is recorded only once the Model owner has refused the erasure',
        );
        assert.equal(writeCount(live.log), 0, `${receiptStatus} receipt, ${deliveryState} delivery, flag ${flag}`);
        assert.equal(JSON.stringify(live.tables), before, `${receiptStatus} receipt, ${deliveryState} delivery, flag ${flag}`);
      }

      // The owner refused the erase and its delivery is parked: the hold is recorded and the Space stays fenced.
      const parked = harness(seedTables({ modelReceipt: 'pending', parked: true, others: 'pending' }));
      const result = await spaces.recordDeletionOwnerReceipt._handler(parked.ctx, {
        requestId: REQUEST_ID, ownerPlane: 'model', status: 'blocked_legal_hold', now: 1_000,
      });
      assert.deepEqual(result, { ownerPlane: 'model', status: 'blocked_legal_hold', finalized: false });
      assert.equal(receiptFor(parked.tables).status, 'blocked_legal_hold', `flag ${flag}`);
      assert.equal(parked.tables.spaces[0].lifecycle, 'deleting', `flag ${flag}`);
    });
  }
});
