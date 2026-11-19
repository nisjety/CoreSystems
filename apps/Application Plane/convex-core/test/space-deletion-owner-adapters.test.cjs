const assert = require('node:assert/strict');
const test = require('node:test');

// The Data and Ingestion adapters read these at import time. The values are test fixtures.
process.env.DATA_SPACE_DELETION_URL = 'http://data.invalid/api/v1/internal/space-deletion/bindings';
process.env.CONTROL_SPACE_DELETION_DATA_BEARER = 'data-bearer-aaaaaaaaaaaaaaaaaaaaaaaaaa';
process.env.INGESTION_SPACE_DELETION_URL = 'http://ingestion.invalid/api/v1/internal/space-deletion/jobs';
process.env.CONTROL_SPACE_DELETION_INGESTION_BEARER = 'ingestion-bearer-aaaaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const dataAdapter = require('../convex/spaceDeletionData.ts');
const ingestionAdapter = require('../convex/spaceDeletionIngestion.ts');
const spaces = require('../convex/spaces.ts');
const { permanentRefusalParkEnabled } = require('../convex/spaceDeletionReceipts.ts');
const fake = require('./helpers/fake-convex.cjs');

const REQUEST_ID = 'space:space_a:deletion:idem-1';
const HOLD_CODE = 'erasure_blocked_legal_hold';

// One entry per adapter. `success` is the owner's exact 200 answer for that owner.
const OWNERS = {
  data: {
    adapter: dataAdapter,
    label: 'Data',
    eventId: `space-deletion:${REQUEST_ID}:owner:data`,
    scheduled: 'spaceDeletionData:deliverOne',
    receiptRef: `data:${REQUEST_ID}`,
    success: {
      request_id: REQUEST_ID, owner_plane: 'data', owner_outcome: 'partial',
      remaining_work: 'resource-owner purge required',
    },
  },
  ingestion: {
    adapter: ingestionAdapter,
    label: 'Ingestion',
    eventId: `space-deletion:${REQUEST_ID}:owner:ingestion`,
    scheduled: 'spaceDeletionIngestion:deliverOne',
    receiptRef: `ingestion:${REQUEST_ID}`,
    success: {
      request_id: REQUEST_ID, owner_plane: 'ingestion', owner_outcome: 'partial',
      remaining_work: 'connector owners remain',
    },
  },
};

/** One personal Space being deleted, with a single owner's delivery and receipt. */
function seedTables(owner, { receiptStatus = 'pending', deliveryAttempts = 0 } = {}) {
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
      _id: 'event_row', eventId: OWNERS[owner].eventId, requestId: REQUEST_ID, ownerPlane: owner,
      deliveryState: 'pending', deliveryAttempts, nextAttemptAt: 0, createdAt: 0,
    }],
    spaceDeletionOwnerReceipts: [{
      _id: 'receipt_row', requestId: REQUEST_ID, ownerPlane: owner, status: receiptStatus,
      deadlineAt: 7_000, updatedAt: 0,
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

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
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

/** The gateway's typed hold refusal: the exact code with retryable false. */
function holdReply() {
  return jsonReply(409, {
    error: { code: HOLD_CODE, message: 'upstream-hold-detail', details: { retryable: false } },
  });
}

for (const owner of Object.keys(OWNERS)) {
  const spec = OWNERS[owner];

  test(`${owner}: a legal-hold refusal parks the delivery, blocks the receipt and is never retried`, async () => {
    const handle = harness(seedTables(owner));
    let first;
    let second;
    const sent = await withOwnerReply(holdReply(), async () => {
      first = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
      second = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });

    assert.deepEqual(first, { eventId: spec.eventId, status: 'blocked_legal_hold' });
    assert.deepEqual(second, { status: 'idle' });
    assert.equal(sent.length, 1, 'the refused erase is sent once');
    assert.deepEqual(scheduledNames(handle.log), [], 'a hold schedules no retry');
    assert.equal(eventRow(handle.tables).deliveryState, 'blocked_legal_hold');
    assert.equal(eventRow(handle.tables).lastDeliveryError, HOLD_CODE);
    assert.equal(eventRow(handle.tables).leaseOwner, undefined);
    assert.equal(receiptRow(handle.tables).status, 'blocked_legal_hold');
    assert.equal(handle.tables.spaces[0].lifecycle, 'deleting', 'a hold keeps the Space fenced, not deleted');
    assert.equal(
      JSON.stringify(handle.tables).includes('upstream-hold-detail'),
      false,
      'upstream text is never stored',
    );
  });

  test(`${owner}: a hold code that is not marked non-retryable keeps the retry path`, async () => {
    const handle = harness(seedTables(owner));
    let result;
    await withOwnerReply(jsonReply(409, { error: { code: HOLD_CODE, details: { retryable: true } } }), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });

    assert.equal(result.status, 'retry_scheduled');
    assert.equal(eventRow(handle.tables).deliveryState, 'failed');
    assert.equal(receiptRow(handle.tables).status, 'pending');
    assert.deepEqual(scheduledNames(handle.log), [spec.scheduled]);
  });

  test(`${owner}: with the permanent-refusal switch on, a permanent refusal parks the delivery as failed_permanent and is never retried`, async (t) => {
    setPermanentParkFlag(t, 'true');
    for (const status of [400, 401, 403, 412]) {
      const handle = harness(seedTables(owner));
      let first;
      let second;
      const sent = await withOwnerReply(jsonReply(status, {
        error: { code: 'bad_request', message: 'upstream-permanent-detail' },
      }), async () => {
        first = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
        second = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
      });

      assert.deepEqual(first, { eventId: spec.eventId, status: 'failed_permanent' }, String(status));
      assert.deepEqual(second, { status: 'idle' }, String(status));
      assert.equal(sent.length, 1, `HTTP ${status} is sent once`);
      assert.deepEqual(scheduledNames(handle.log), [], `HTTP ${status} schedules no retry`);
      assert.equal(eventRow(handle.tables).deliveryState, 'failed_permanent', String(status));
      assert.equal(eventRow(handle.tables).lastDeliveryError, `permanent_refusal_http_${status}`, String(status));
      assert.equal(receiptRow(handle.tables).status, 'pending', 'a refusal is not an owner answer');
      assert.equal(
        JSON.stringify(handle.tables).includes('upstream-permanent-detail'),
        false,
        'upstream text is never stored',
      );
    }
  });

  test(`${owner}: with the permanent-refusal switch off, 400, 401, 403 and 412 release for a retry exactly as before`, async (t) => {
    setPermanentParkFlag(t, undefined);
    for (const status of [400, 401, 403, 412]) {
      const handle = harness(seedTables(owner));
      let first;
      let second;
      const sent = await withOwnerReply(jsonReply(status, {
        error: { code: 'bad_request', message: 'upstream-permanent-detail' },
      }), async () => {
        first = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
        second = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
      });

      assert.equal(first.status, 'retry_scheduled', String(status));
      assert.deepEqual(second, { status: 'idle' }, `HTTP ${status} is not claimed again before its backoff`);
      assert.equal(sent.length, 1, `HTTP ${status} is sent once per attempt`);
      assert.deepEqual(scheduledNames(handle.log), [spec.scheduled], `HTTP ${status} schedules one retry`);
      assert.equal(eventRow(handle.tables).deliveryState, 'failed', String(status));
      assert.equal(
        eventRow(handle.tables).lastDeliveryError,
        `${spec.label} Space deletion returned HTTP ${status}`,
        String(status),
      );
      assert.equal(eventRow(handle.tables).leaseOwner, undefined, String(status));
      assert.equal(receiptRow(handle.tables).status, 'pending', 'a refusal is not an owner answer');
      assert.equal(
        JSON.stringify(handle.tables).includes('upstream-permanent-detail'),
        false,
        'upstream text is never stored',
      );
    }
  });

  test(`${owner}: with the permanent-refusal switch off, a refusal that clears on the retry still completes`, async (t) => {
    setPermanentParkFlag(t, undefined);
    const handle = harness(seedTables(owner));
    const replies = [
      jsonReply(401, { error: { code: 'unauthorized', message: 'upstream-permanent-detail' } }),
      jsonReply(200, spec.success),
    ];
    let second;
    const sent = await withOwnerReply(() => replies.shift()(), async () => {
      const first = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
      assert.equal(first.status, 'retry_scheduled');
      // The bearer has been fixed and the backoff has passed: the retry is claimed and acknowledged.
      eventRow(handle.tables).nextAttemptAt = 0;
      second = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });

    assert.deepEqual(second, { eventId: spec.eventId, status: 'partial' });
    assert.equal(sent.length, 2);
    assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
    assert.equal(receiptRow(handle.tables).status, 'partial');
  });

  test(`${owner}: an outage keeps the retry path and never moves the receipt`, async () => {
    const handle = harness(seedTables(owner));
    let result;
    await withOwnerReply(jsonReply(503, { error: { code: 'unavailable', message: 'upstream-outage-detail' } }), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });

    assert.equal(result.status, 'retry_scheduled');
    assert.equal(eventRow(handle.tables).deliveryState, 'failed');
    assert.equal(eventRow(handle.tables).lastDeliveryError, `${spec.label} Space deletion returned HTTP 503`);
    assert.deepEqual(scheduledNames(handle.log), [spec.scheduled]);
    assert.equal(receiptRow(handle.tables).status, 'pending');
  });

  test(`${owner}: a 200 receipt is acknowledged as before and never parks`, async () => {
    const handle = harness(seedTables(owner));
    let result;
    await withOwnerReply(jsonReply(200, spec.success), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });

    assert.deepEqual(result, { eventId: spec.eventId, status: 'partial' });
    assert.equal(eventRow(handle.tables).deliveryState, 'acknowledged');
    assert.equal(eventRow(handle.tables).lastDeliveryError, undefined);
    assert.equal(receiptRow(handle.tables).status, 'partial');
    assert.equal(receiptRow(handle.tables).receiptRef, spec.receiptRef);
    assert.deepEqual(scheduledNames(handle.log), []);
  });

  test(`${owner}: a throw after the claim releases the delivery for a retry instead of leaving it claimed`, async (t) => {
    t.mock.method(console, 'error', () => {});
    // The owner answers 200 over a blocked receipt, which no transition accepts, so the acknowledgement throws.
    const handle = harness(seedTables(owner, { receiptStatus: 'blocked_legal_hold' }));
    let result;
    const sent = await withOwnerReply(jsonReply(200, spec.success), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });

    assert.equal(result.status, 'retry_scheduled');
    assert.equal(sent.length, 1);
    assert.equal(eventRow(handle.tables).deliveryState, 'failed', 'released, not left claimed');
    assert.equal(eventRow(handle.tables).leaseOwner, undefined);
    assert.equal(
      eventRow(handle.tables).lastDeliveryError,
      `${spec.label} Space deletion delivery failed before an owner receipt was recorded`,
    );
    assert.deepEqual(scheduledNames(handle.log), [spec.scheduled]);
    assert.equal(receiptRow(handle.tables).status, 'blocked_legal_hold', 'the refused answer does not move the receipt');
  });

  test(`${owner}: a throw after the claim parks the delivery as failed_permanent once the claim count reaches the bound`, async (t) => {
    t.mock.method(console, 'error', () => {});
    // The claim takes the count to 12, the bound. The 200 answer over a blocked receipt throws on every attempt.
    const handle = harness(seedTables(owner, { receiptStatus: 'blocked_legal_hold', deliveryAttempts: 11 }));
    let result;
    const sent = await withOwnerReply(jsonReply(200, spec.success), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });

    assert.deepEqual(result, { eventId: spec.eventId, status: 'failed_permanent' });
    assert.equal(sent.length, 1, 'the owner is asked once for this claim');
    assert.equal(eventRow(handle.tables).deliveryAttempts, 12);
    assert.equal(eventRow(handle.tables).deliveryState, 'failed_permanent');
    assert.equal(eventRow(handle.tables).lastDeliveryError, 'delivery_retry_bound_reached');
    assert.equal(eventRow(handle.tables).leaseOwner, undefined);
    assert.deepEqual(scheduledNames(handle.log), [], 'a parked delivery schedules no retry');
    assert.equal(receiptRow(handle.tables).status, 'blocked_legal_hold', 'the refused answer does not move the receipt');
  });

  test(`${owner}: a throw below the claim bound still releases the delivery for a retry`, async (t) => {
    t.mock.method(console, 'error', () => {});
    const handle = harness(seedTables(owner, { receiptStatus: 'blocked_legal_hold', deliveryAttempts: 10 }));
    let result;
    await withOwnerReply(jsonReply(200, spec.success), async () => {
      result = await spec.adapter.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });

    assert.equal(result.status, 'retry_scheduled');
    assert.equal(eventRow(handle.tables).deliveryAttempts, 11);
    assert.equal(eventRow(handle.tables).deliveryState, 'failed');
    assert.deepEqual(scheduledNames(handle.log), [spec.scheduled]);
  });
}

test('SPACE_DELETION_PERMANENT_REFUSAL_PARK is read at call time as the literal "true" and is off by default', (t) => {
  setPermanentParkFlag(t, undefined);
  assert.equal(permanentRefusalParkEnabled(), false);
  for (const value of ['0', '1', 'yes', 'on', ' true', 'true ', 'TRUE', 'True', '']) {
    process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK = value;
    assert.equal(permanentRefusalParkEnabled(), false, JSON.stringify(value));
  }
  process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK = 'true';
  assert.equal(permanentRefusalParkEnabled(), true);
});
