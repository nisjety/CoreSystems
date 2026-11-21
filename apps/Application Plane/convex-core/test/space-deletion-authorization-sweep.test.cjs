const assert = require('node:assert/strict');
const test = require('node:test');

// The authorization worker reads these at import time. The values are test fixtures.
process.env.CONTROL_SPACE_DELETION_AUTHORIZATION_URL = 'http://control.invalid/api/v1/internal/space-deletion/authorize';
process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN = 'lifecycle-token-aaaaaaaaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const spaces = require('../convex/spaces.ts');
const authorizationWorker = require('../convex/spaceDeletionAuthorization.ts');
const fake = require('./helpers/fake-convex.cjs');

const ORG = 'org_a';

function requestIdOf(spaceRef) {
  return `space:${spaceRef}:deletion:idem-${spaceRef}`;
}

function authorizationEventIdOf(spaceRef) {
  return `${requestIdOf(spaceRef)}:authorize`;
}

/**
 * One personal deletion intent pending Control's authorization, and its authorization event in `deliveryState`. A claimed
 * event carries the lease its worker took: `leaseExpiresAt` is what decides whether a sweep may re-drive it.
 */
function authorization(spaceRef, { deliveryState = 'pending', nextAttemptAt = 1_000, leaseExpiresAt, leaseOwner } = {}) {
  const requestId = requestIdOf(spaceRef);
  const event = {
    _id: `auth_${spaceRef}`, eventId: authorizationEventIdOf(spaceRef), requestId,
    deliveryState, deliveryAttempts: deliveryState === 'claimed' ? 1 : 0, nextAttemptAt, createdAt: 0,
  };
  if (leaseExpiresAt !== undefined) event.leaseExpiresAt = leaseExpiresAt;
  if (leaseOwner !== undefined) event.leaseOwner = leaseOwner;
  return {
    spaceDeletionRequests: [{
      _id: `request_${spaceRef}`, requestId, idempotencyKey: `idem-${spaceRef}`, spaceRef,
      externalOrgId: ORG, ownerExternalAuthId: 'user_a', state: 'pending_authorization', createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionAuthorizationEvents: [event],
  };
}

/** Concatenates table fragments into one set of tables. */
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
 * A ctx whose runMutation and runQuery run the real spaces.ts handlers against the same in-memory tables. With `failAck`, the
 * acknowledgement throws, as a failed mutation would after Control has answered.
 */
function harness(tables, { failAck = false } = {}) {
  const handle = fake.makeCtx(tables, {
    async runMutation(fn, args) {
      const [moduleName, exportName] = getFunctionName(fn).split(':');
      assert.equal(moduleName, 'spaces', `unexpected mutation ${getFunctionName(fn)}`);
      if (failAck && exportName === 'acknowledgeDeletionAuthorizationClaim') {
        throw new Error('simulated acknowledgement failure');
      }
      return spaces[exportName]._handler(handle.ctx, args);
    },
  });
  handle.queries = 0;
  handle.ctx.runQuery = async (fn, args) => {
    handle.queries += 1;
    const [moduleName, exportName] = getFunctionName(fn).split(':');
    assert.equal(moduleName, 'spaces', `unexpected query ${getFunctionName(fn)}`);
    return spaces[exportName]._handler(handle.ctx, args);
  };
  return handle;
}

function eventOf(tables, spaceRef) {
  return tables.spaceDeletionAuthorizationEvents.find((row) => row.eventId === authorizationEventIdOf(spaceRef));
}

function requestOf(tables, spaceRef) {
  return tables.spaceDeletionRequests.find((row) => row.requestId === requestIdOf(spaceRef));
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

/** Runs every scheduled authorization delivery, as the scheduler would, and returns the results in order. */
async function runScheduledDeliveries(handle) {
  const results = [];
  for (const entry of handle.log.scheduled.splice(0)) {
    const [moduleName, exportName] = getFunctionName(entry.fn).split(':');
    assert.equal(moduleName, 'spaceDeletionAuthorization', `unexpected scheduled ${getFunctionName(entry.fn)}`);
    results.push(await authorizationWorker[exportName]._handler(handle.ctx, entry.args));
  }
  return results;
}

/** Answers every Control authorization call with `status` while `body` runs, and returns the requests sent. */
async function withControlAnswer(status, body) {
  const sent = [];
  const realFetch = global.fetch;
  global.fetch = async (url, init) => {
    const payload = JSON.parse(init.body);
    sent.push({ url, init, payload });
    return new Response(JSON.stringify({ data: { request_id: payload.request_id, status } }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  };
  try {
    await body();
  } finally {
    global.fetch = realFetch;
  }
  return sent;
}

/** Sets SPACE_DELETION_AUTH_SWEEP for one test (`undefined` removes it), restored afterwards. */
function setSweepSwitch(t, value) {
  const prior = process.env.SPACE_DELETION_AUTH_SWEEP;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_DELETION_AUTH_SWEEP;
    else process.env.SPACE_DELETION_AUTH_SWEEP = prior;
  });
  if (value === undefined) delete process.env.SPACE_DELETION_AUTH_SWEEP;
  else process.env.SPACE_DELETION_AUTH_SWEEP = value;
}

test('an expired claim is re-driven: the sweep schedules one delivery, and that delivery acknowledges the event', async (t) => {
  setSweepSwitch(t, 'true');
  t.mock.method(console, 'warn', () => {});
  const now = Date.now();
  const handle = harness(tablesOf(authorization('space_a', {
    deliveryState: 'claimed', nextAttemptAt: now - 60_000, leaseExpiresAt: now - 1_000, leaseOwner: 'dead-worker',
  })));

  assert.deepEqual(await authorizationWorker.sweepAuthorizationEvents._handler(handle.ctx, {}), { status: 'swept', scheduled: 1 });
  assert.equal(handle.queries, 1, 'the sweep reads the expired leases once');
  assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionAuthorization:deliverOne']);
  assert.match(handle.log.scheduled[0].args.workerId, /^space-deletion-authorization:/);

  let results;
  const sent = await withControlAnswer('blocked_legal_hold', async () => {
    results = await runScheduledDeliveries(handle);
  });
  assert.deepEqual(results, [{ eventId: authorizationEventIdOf('space_a'), status: 'blocked_legal_hold' }]);
  assert.equal(sent.length, 1, 'Control is asked once, by the re-driven delivery');
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'acknowledged');
  assert.equal(requestOf(handle.tables, 'space_a').state, 'blocked_legal_hold');
});

test('a claim whose lease is still live is not re-driven', async (t) => {
  setSweepSwitch(t, 'true');
  const now = Date.now();
  const handle = harness(tablesOf(authorization('space_a', {
    deliveryState: 'claimed', nextAttemptAt: now - 1_000, leaseExpiresAt: now + 60_000, leaseOwner: 'live-worker',
  })));

  assert.deepEqual(await authorizationWorker.sweepAuthorizationEvents._handler(handle.ctx, {}), { status: 'swept', scheduled: 0 });
  assert.equal(handle.queries, 1);
  assert.deepEqual(scheduledNames(handle.log), [], 'nothing is scheduled for a live lease');
  const sent = await withControlAnswer('authorized', async () => {
    assert.deepEqual(await runScheduledDeliveries(handle), []);
  });
  assert.equal(sent.length, 0, 'Control is not asked about a claim its worker still holds');
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'claimed');
  assert.equal(eventOf(handle.tables, 'space_a').leaseOwner, 'live-worker');
});

test('the switch is read as the literal true: unset, empty and any other value re-drives nothing and reads nothing', async (t) => {
  setSweepSwitch(t, undefined);
  const now = Date.now();
  for (const value of [undefined, '', 'false', '0', '1', 'TRUE', 'True', ' true', 'true ', 'yes']) {
    if (value === undefined) delete process.env.SPACE_DELETION_AUTH_SWEEP;
    else process.env.SPACE_DELETION_AUTH_SWEEP = value;
    const handle = harness(tablesOf(authorization('space_a', {
      deliveryState: 'claimed', nextAttemptAt: now - 60_000, leaseExpiresAt: now - 1_000, leaseOwner: 'dead-worker',
    })));

    assert.deepEqual(await authorizationWorker.sweepAuthorizationEvents._handler(handle.ctx, {}), { status: 'disabled', scheduled: 0 }, String(value));
    assert.equal(handle.queries, 0, `${JSON.stringify(value)} reads no lease`);
    assert.deepEqual(scheduledNames(handle.log), [], `${JSON.stringify(value)} schedules nothing`);
    assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'claimed', `${JSON.stringify(value)} leaves the claim alone`);
  }
});

test('one sweep re-drives at most twenty expired claims, the longest-expired first, and the rest wait for a later run', async (t) => {
  setSweepSwitch(t, 'true');
  t.mock.method(console, 'warn', () => {});
  const now = Date.now();
  const parts = [];
  for (let index = 0; index < 25; index += 1) {
    parts.push(authorization(`space_${index}`, {
      deliveryState: 'claimed', nextAttemptAt: now - 120_000 + index, leaseExpiresAt: now - 90_000 + index, leaseOwner: `dead-${index}`,
    }));
  }
  const handle = harness(tablesOf(...parts));

  assert.deepEqual(await authorizationWorker.sweepAuthorizationEvents._handler(handle.ctx, {}), { status: 'swept', scheduled: 20 });
  const workerIds = handle.log.scheduled.map((entry) => entry.args.workerId);
  assert.equal(workerIds.length, 20);
  assert.ok(workerIds[0].includes(authorizationEventIdOf('space_0')), 'the longest-expired lease is re-driven first');
  assert.ok(workerIds[19].includes(authorizationEventIdOf('space_19')));
  assert.equal(workerIds.some((workerId) => workerId.includes(authorizationEventIdOf('space_20'))), false, 'the 21st is left for a later run');
});

test('the sweep does not select a pending or failed authorization, which has its own retry', async (t) => {
  setSweepSwitch(t, 'true');
  const now = Date.now();
  const handle = harness(tablesOf(
    authorization('space_a', { deliveryState: 'pending', nextAttemptAt: now - 1_000 }),
    authorization('space_b', { deliveryState: 'failed', nextAttemptAt: now - 1_000 }),
  ));

  assert.deepEqual(await authorizationWorker.sweepAuthorizationEvents._handler(handle.ctx, {}), { status: 'swept', scheduled: 0 });
  assert.deepEqual(scheduledNames(handle.log), []);
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'pending');
  assert.equal(eventOf(handle.tables, 'space_b').deliveryState, 'failed');
});

test('a throw in the acknowledgement releases the claim for a retry, and the warning carries ids only', async (t) => {
  setSweepSwitch(t, 'true');
  const warn = t.mock.method(console, 'warn', () => {});
  const handle = harness(tablesOf(authorization('space_a', { nextAttemptAt: 1_000 })), { failAck: true });

  let result;
  const sent = await withControlAnswer('authorized', async () => {
    result = await authorizationWorker.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
  });

  assert.equal(result.status, 'retry_scheduled');
  assert.equal(sent.length, 1, 'Control answered once');
  const event = eventOf(handle.tables, 'space_a');
  assert.equal(event.deliveryState, 'failed', 'released, not left claimed');
  assert.equal(event.leaseOwner, undefined);
  assert.equal(event.leaseExpiresAt, undefined);
  assert.equal(event.lastDeliveryError, 'Control Space deletion authorization delivery failed before a receipt was recorded');
  assert.deepEqual(scheduledNames(handle.log), ['spaceDeletionAuthorization:deliverOne']);
  assert.equal(requestOf(handle.tables, 'space_a').state, 'pending_authorization', 'the failed acknowledgement decides nothing');

  assert.equal(warn.mock.callCount(), 1);
  const payload = warn.mock.calls[0].arguments[1];
  assert.deepEqual(Object.keys(payload).sort(), ['eventId', 'requestId']);
  assert.equal(payload.eventId, authorizationEventIdOf('space_a'));
  assert.equal(payload.requestId, requestIdOf('space_a'));
});

test('with the switch off, a throw in the acknowledgement propagates and the claim stays exactly as before', async (t) => {
  setSweepSwitch(t, undefined);
  const warn = t.mock.method(console, 'warn', () => {});
  const handle = harness(tablesOf(authorization('space_a', { nextAttemptAt: 1_000 })), { failAck: true });

  await withControlAnswer('authorized', async () => {
    await assert.rejects(
      authorizationWorker.deliverOne._handler(handle.ctx, { workerId: 'worker-1' }),
      /simulated acknowledgement failure/,
    );
  });

  const event = eventOf(handle.tables, 'space_a');
  assert.equal(event.deliveryState, 'claimed', 'not released: the switch is off');
  assert.equal(event.leaseOwner, 'worker-1');
  assert.deepEqual(scheduledNames(handle.log), [], 'no retry is scheduled');
  assert.equal(warn.mock.callCount(), 0, 'nothing is logged by the recovery path');
  assert.equal(handle.log.mutations.some((entry) => getFunctionName(entry.fn) === 'spaces:releaseDeletionAuthorizationClaim'), false);
});
