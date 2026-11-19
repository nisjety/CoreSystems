const assert = require('node:assert/strict');
const test = require('node:test');

// spaceDeletionModel.ts reads these at import time. The values are test fixtures.
process.env.MODEL_SPACE_DELETION_URL = 'http://model.invalid/api/v1/internal/space-deletion/threads';
process.env.CONTROL_SPACE_DELETION_MODEL_BEARER = 'model-bearer-aaaaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_SESSION_BEARER = 'session-bearer-aaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_CAPABILITY_BEARER = 'capability-bearer-aaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const { deliverOne, isModelLegalHoldRefusal } = require('../convex/spaceDeletionModel.ts');
const spaces = require('../convex/spaces.ts');
const {
  blockSpaceLifecycleDeliveryForLegalHold,
  canClaimSpaceLifecycleDelivery,
} = require('../convex/spaceLifecycle.ts');
const { assertReceiptTransition } = require('../convex/spaceDeletionReceipts.ts');
const fake = require('./helpers/fake-convex.cjs');

const REQUEST_ID = 'space:space_a:deletion:idem-1';
const EVENT_ID = `space-deletion:${REQUEST_ID}:owner:model`;
const HOLD_CODE = 'erasure_blocked_legal_hold';

/** One personal Space being deleted, with its pending Model delivery and Model receipt. */
function seedTables({ receiptStatus = 'pending', deliveryAttempts = 0 } = {}) {
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
      _id: 'event_row', eventId: EVENT_ID, requestId: REQUEST_ID, ownerPlane: 'model',
      deliveryState: 'pending', deliveryAttempts, nextAttemptAt: 0, createdAt: 0,
    }],
    spaceDeletionOwnerReceipts: [{
      _id: 'receipt_row', requestId: REQUEST_ID, ownerPlane: 'model', status: receiptStatus, updatedAt: 0,
    }],
  };
}

/**
 * A ctx whose runMutation runs the real spaces.ts handlers against the same
 * in-memory tables, so the delivery and receipt rows are what production writes.
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

function eventRow(tables) {
  return tables.spaceDeletionOwnerDeliveryEvents[0];
}

function receiptRow(tables) {
  return tables.spaceDeletionOwnerReceipts[0];
}

function mutationNames(log) {
  return log.mutations.map((entry) => getFunctionName(entry.fn));
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
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

/** Sets SPACE_DELETION_PERMANENT_REFUSAL_PARK for one test (`undefined` removes it), restored afterwards. */
function setPermanentParkFlag(t, value) {
  const prior = process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK;
    else process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK = prior;
  });
  if (value === undefined) delete process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK;
  else process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK = value;
}

/** The gateway's typed hold refusal, carrying upstream text that must never be stored. */
function holdReply() {
  return jsonReply(409, {
    error: {
      code: HOLD_CODE,
      message: 'upstream-hold-detail: 2 held conversations',
      details: { retryable: false },
    },
  });
}

test('a legal-hold refusal parks the Model delivery and is never retried', async () => {
  const handle = harness(seedTables());
  let first;
  let second;
  const sent = await withModelReply(holdReply(), async () => {
    first = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    second = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
  });

  assert.deepEqual(first, { eventId: EVENT_ID, status: 'blocked_legal_hold' });
  assert.deepEqual(second, { status: 'idle' });
  assert.equal(sent.length, 1, 'the refused erase is sent once, and the parked delivery is not sent again');
  assert.deepEqual(JSON.parse(sent[0].init.body), {
    org_id: 'org_a',
    space_id: 'space_a',
    owner_principal_id: 'user_a',
    deletion_request_id: REQUEST_ID,
  });
  assert.ok(mutationNames(handle.log).includes('spaces:blockModelDeletionOwnerDeliveryForLegalHold'));
  assert.equal(mutationNames(handle.log).includes('spaces:releaseModelDeletionOwnerDelivery'), false);
  assert.deepEqual(scheduledNames(handle.log), [], 'no retry is scheduled for a hold');

  const event = eventRow(handle.tables);
  assert.equal(event.deliveryState, 'blocked_legal_hold');
  assert.equal(event.lastDeliveryError, HOLD_CODE);
  assert.equal(event.leaseOwner, undefined);
  assert.equal(receiptRow(handle.tables).status, 'blocked_legal_hold');
  assert.equal(handle.tables.spaces[0].lifecycle, 'deleting', 'a hold keeps the Space fenced, not deleted');
  assert.equal(
    JSON.stringify(handle.tables).includes('upstream-hold-detail'),
    false,
    'upstream text is never stored',
  );
});

test('a hold-check outage (503, retryable) keeps the retry path', async () => {
  const handle = harness(seedTables());
  let result;
  const sent = await withModelReply(jsonReply(503, {
    error: {
      code: 'erasure_hold_check_unavailable',
      message: 'upstream-check-detail',
      details: { retryable: true },
    },
  }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
  });

  assert.equal(result.status, 'retry_scheduled');
  assert.equal(typeof result.retryAt, 'number');
  assert.equal(sent.length, 1);
  assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionModel:deliverOne']);
  const event = eventRow(handle.tables);
  assert.equal(event.deliveryState, 'failed');
  assert.equal(event.lastDeliveryError, 'Model Space deletion returned HTTP 503');
  assert.equal(receiptRow(handle.tables).status, 'pending', 'an outage never moves the receipt');
});

test('a malformed refusal body is retried exactly as before', async () => {
  const handle = harness(seedTables());
  let result;
  await withModelReply(() => new Response('<html>gateway timeout</html>', { status: 409 }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
  });

  assert.equal(result.status, 'retry_scheduled');
  const event = eventRow(handle.tables);
  assert.equal(event.deliveryState, 'failed');
  assert.equal(event.lastDeliveryError, 'Model Space deletion returned HTTP 409');
  assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionModel:deliverOne']);
  assert.equal(receiptRow(handle.tables).status, 'pending');
});

test('only a non-retryable hold is parked; every other refusal keeps the retry path', async () => {
  const cases = [
    ['the hold code marked retryable', jsonReply(409, { error: { code: HOLD_CODE, details: { retryable: true } } })],
    ['the hold code with no retryable flag', jsonReply(409, { error: { code: HOLD_CODE } })],
    ['the hold code with a string retryable flag', jsonReply(409, { error: { code: HOLD_CODE, details: { retryable: 'false' } } })],
    ['a JSON body with no typed error', jsonReply(409, { error: 'a legal hold covers this' })],
    ['a different typed refusal that is not permanent', jsonReply(409, { error: { code: 'thread_persistence_not_permitted', message: 'x' } })],
    ['a rate limit with a typed body', jsonReply(429, { error: { code: 'rate_limited', details: { retryable: true } } })],
    ['a 200 receipt for another request', jsonReply(200, { request_id: 'other', owner_plane: 'model', owner_outcome: 'succeeded' })],
  ];
  for (const [name, reply] of cases) {
    const handle = harness(seedTables());
    let result;
    await withModelReply(reply, async () => {
      result = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });
    assert.equal(result.status, 'retry_scheduled', name);
    assert.equal(eventRow(handle.tables).deliveryState, 'failed', name);
    assert.equal(scheduledNames(handle.log).length, 1, name);
    assert.equal(receiptRow(handle.tables).status, 'pending', name);
  }
});

test('a 200 receipt is acknowledged exactly as before and never parks', async () => {
  const handle = harness(seedTables());
  let result;
  await withModelReply(jsonReply(200, {
    request_id: REQUEST_ID,
    owner_plane: 'model',
    owner_outcome: 'partial',
    remaining_work: ['historical_uncorrelated_letta_memory'],
  }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
  });

  assert.deepEqual(result, { eventId: EVENT_ID, status: 'partial' });
  assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
  assert.equal(eventRow(handle.tables).lastDeliveryError, undefined);
  const receipt = receiptRow(handle.tables);
  assert.equal(receipt.status, 'partial');
  assert.equal(receipt.receiptRef, `model:${REQUEST_ID}`);
  assert.equal(receipt.detail, 'Model owner receipt: historical_uncorrelated_letta_memory');
  assert.deepEqual(scheduledNames(handle.log), []);
  assert.equal(mutationNames(handle.log).includes('spaces:blockModelDeletionOwnerDeliveryForLegalHold'), false);
});

test('a legal hold records on a pending or unknown Model receipt and never rewrites an answered one', async () => {
  const cases = [
    ['pending', 'blocked_legal_hold'],
    ['unknown', 'blocked_legal_hold'],
    ['partial', 'partial'],
    ['succeeded', 'succeeded'],
    ['failed', 'failed'],
  ];
  for (const [receiptStatus, expected] of cases) {
    const tables = seedTables({ receiptStatus });
    // The lease holder has claimed the delivery, as deliverOne has when the hold arrives.
    Object.assign(eventRow(tables), {
      deliveryState: 'claimed',
      deliveryAttempts: 1,
      leaseOwner: 'worker-1',
      leaseExpiresAt: Date.now() + 30_000,
    });
    const handle = harness(tables);
    await assert.rejects(
      spaces.blockModelDeletionOwnerDeliveryForLegalHold._handler(handle.ctx, {
        eventId: EVENT_ID, workerId: 'worker-2', now: 1_000,
      }),
      /active delivery worker/i,
    );
    const result = await spaces.blockModelDeletionOwnerDeliveryForLegalHold._handler(handle.ctx, {
      eventId: EVENT_ID, workerId: 'worker-1', now: 1_000,
    });
    assert.deepEqual(result, { eventId: EVENT_ID, status: 'blocked_legal_hold' });
    assert.equal(eventRow(handle.tables).deliveryState, 'blocked_legal_hold', receiptStatus);
    assert.equal(eventRow(handle.tables).leaseOwner, undefined, receiptStatus);
    assert.equal(receiptRow(handle.tables).status, expected, `receipt ${receiptStatus}`);
  }
});

test('a parked delivery is never claimable, and only the lease holder can record a hold', () => {
  const claimed = { attempts: 1, leaseExpiresAt: 30_000, leaseOwner: 'worker-a', nextAttemptAt: 0, state: 'claimed' };
  const parked = blockSpaceLifecycleDeliveryForLegalHold(claimed, 'worker-a', 500);
  assert.deepEqual(parked, { attempts: 1, nextAttemptAt: 500, state: 'blocked_legal_hold' });
  for (const now of [500, 30_000, 10_000_000]) {
    assert.equal(canClaimSpaceLifecycleDelivery(parked, now), false, `claimable at ${now}`);
  }
  assert.throws(() => blockSpaceLifecycleDeliveryForLegalHold(claimed, 'worker-b', 500), /active delivery worker/i);
  assert.throws(
    () => blockSpaceLifecycleDeliveryForLegalHold({ attempts: 1, nextAttemptAt: 0, state: 'failed' }, 'worker-a', 500),
    /active delivery worker/i,
  );
});

test('a legal hold may resolve an unknown receipt, and a blocked receipt is terminal', () => {
  assert.doesNotThrow(() => assertReceiptTransition('pending', 'blocked_legal_hold'));
  assert.doesNotThrow(() => assertReceiptTransition('unknown', 'blocked_legal_hold'));
  assert.throws(() => assertReceiptTransition('blocked_legal_hold', 'pending'), /invalid/i);
  assert.throws(() => assertReceiptTransition('blocked_legal_hold', 'succeeded'), /invalid/i);
});

test('the hold shape is exact: the code with retryable false and nothing weaker', () => {
  assert.equal(isModelLegalHoldRefusal({ error: { code: HOLD_CODE, message: 'x', details: { retryable: false } } }), true);
  assert.equal(isModelLegalHoldRefusal({ error: { code: HOLD_CODE, details: { retryable: true } } }), false);
  assert.equal(isModelLegalHoldRefusal({ error: { code: 'erasure_hold_check_unavailable', details: { retryable: false } } }), false);
  assert.equal(isModelLegalHoldRefusal({ error: HOLD_CODE }), false);
  assert.equal(isModelLegalHoldRefusal(null), false);
  assert.equal(isModelLegalHoldRefusal([HOLD_CODE]), false);
});

test('with the permanent-refusal switch on, a permanent refusal parks the Model delivery as failed_permanent and is never retried', async (t) => {
  setPermanentParkFlag(t, 'true');
  for (const status of [400, 401, 403, 412]) {
    const handle = harness(seedTables());
    let first;
    let second;
    const sent = await withModelReply(jsonReply(status, {
      error: { code: 'bad_request', message: 'upstream-permanent-detail', details: { retryable: false } },
    }), async () => {
      first = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
      second = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });

    assert.deepEqual(first, { eventId: EVENT_ID, status: 'failed_permanent' }, String(status));
    assert.deepEqual(second, { status: 'idle' }, String(status));
    assert.equal(sent.length, 1, `HTTP ${status} is sent once`);
    assert.deepEqual(scheduledNames(handle.log), [], `HTTP ${status} schedules no retry`);
    const event = eventRow(handle.tables);
    assert.equal(event.deliveryState, 'failed_permanent', String(status));
    assert.equal(event.lastDeliveryError, `permanent_refusal_http_${status}`, String(status));
    assert.equal(event.leaseOwner, undefined, String(status));
    assert.equal(receiptRow(handle.tables).status, 'pending', 'a refusal is not an owner answer');
    assert.equal(
      JSON.stringify(handle.tables).includes('upstream-permanent-detail'),
      false,
      'upstream text is never stored',
    );
  }
});

test('a throw after the claim releases the delivery for a retry instead of leaving it claimed', async (t) => {
  t.mock.method(console, 'error', () => {});
  // The owner answers 200 over a blocked receipt, which no transition accepts, so the acknowledgement throws.
  const handle = harness(seedTables({ receiptStatus: 'blocked_legal_hold' }));
  let result;
  const sent = await withModelReply(jsonReply(200, {
    request_id: REQUEST_ID, owner_plane: 'model', owner_outcome: 'succeeded', remaining_work: [],
  }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
  });

  assert.equal(result.status, 'retry_scheduled');
  assert.equal(sent.length, 1);
  const event = eventRow(handle.tables);
  assert.equal(event.deliveryState, 'failed', 'released, not left claimed');
  assert.equal(event.leaseOwner, undefined);
  assert.equal(event.lastDeliveryError, 'Model Space deletion delivery failed before an owner receipt was recorded');
  assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionModel:deliverOne']);
  assert.equal(receiptRow(handle.tables).status, 'blocked_legal_hold', 'the refused answer does not move the receipt');
});

test('with the permanent-refusal switch off, a permanent refusal releases for a retry exactly as before', async (t) => {
  setPermanentParkFlag(t, undefined);
  for (const status of [400, 401, 403, 412]) {
    const handle = harness(seedTables());
    let first;
    let second;
    const sent = await withModelReply(jsonReply(status, {
      error: { code: 'bad_request', message: 'upstream-permanent-detail', details: { retryable: false } },
    }), async () => {
      first = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
      second = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });

    assert.equal(first.status, 'retry_scheduled', String(status));
    assert.deepEqual(second, { status: 'idle' }, `HTTP ${status} is not claimed again before its backoff`);
    assert.equal(sent.length, 1, `HTTP ${status} is sent once per attempt`);
    assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionModel:deliverOne'], `HTTP ${status} schedules one retry`);
    const event = eventRow(handle.tables);
    assert.equal(event.deliveryState, 'failed', String(status));
    assert.equal(event.lastDeliveryError, `Model Space deletion returned HTTP ${status}`, String(status));
    assert.equal(event.leaseOwner, undefined, String(status));
    assert.equal(receiptRow(handle.tables).status, 'pending', 'a refusal is not an owner answer');
    assert.equal(
      JSON.stringify(handle.tables).includes('upstream-permanent-detail'),
      false,
      'upstream text is never stored',
    );
  }
});

test('with the permanent-refusal switch off, a refusal that clears on the retry still completes the deletion', async (t) => {
  setPermanentParkFlag(t, undefined);
  const handle = harness(seedTables());
  const replies = [
    jsonReply(401, { error: { code: 'unauthorized', message: 'upstream-permanent-detail' } }),
    jsonReply(200, { request_id: REQUEST_ID, owner_plane: 'model', owner_outcome: 'partial', remaining_work: [] }),
  ];
  let second;
  const sent = await withModelReply(() => replies.shift()(), async () => {
    const first = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    assert.equal(first.status, 'retry_scheduled');
    // The bearer has been fixed and the backoff has passed: the retry is claimed and acknowledged.
    eventRow(handle.tables).nextAttemptAt = 0;
    second = await deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
  });

  assert.deepEqual(second, { eventId: EVENT_ID, status: 'partial' });
  assert.equal(sent.length, 2);
  assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
  assert.equal(receiptRow(handle.tables).status, 'partial');
});

test('a throw after the claim parks the Model delivery as failed_permanent once the claim count reaches the bound', async (t) => {
  t.mock.method(console, 'error', () => {});
  // The claim takes the count to 12, the bound. The 200 answer over a blocked receipt throws on every attempt.
  const handle = harness(seedTables({ receiptStatus: 'blocked_legal_hold', deliveryAttempts: 11 }));
  let result;
  const sent = await withModelReply(jsonReply(200, {
    request_id: REQUEST_ID, owner_plane: 'model', owner_outcome: 'succeeded', remaining_work: [],
  }), async () => {
    result = await deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
  });

  assert.deepEqual(result, { eventId: EVENT_ID, status: 'failed_permanent' });
  assert.equal(sent.length, 1);
  const event = eventRow(handle.tables);
  assert.equal(event.deliveryAttempts, 12);
  assert.equal(event.deliveryState, 'failed_permanent');
  assert.equal(event.lastDeliveryError, 'delivery_retry_bound_reached');
  assert.equal(event.leaseOwner, undefined);
  assert.deepEqual(scheduledNames(handle.log), [], 'a parked delivery schedules no retry');
  assert.equal(receiptRow(handle.tables).status, 'blocked_legal_hold', 'the refused answer does not move the receipt');
});
