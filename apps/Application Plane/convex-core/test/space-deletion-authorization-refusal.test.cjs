const assert = require('node:assert/strict');
const test = require('node:test');

// The authorization worker reads these at import time. The values are test fixtures.
process.env.CONTROL_SPACE_DELETION_AUTHORIZATION_URL = 'http://control.invalid/api/v1/internal/space-deletion/authorize';
process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN = 'lifecycle-token-aaaaaaaaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const spaces = require('../convex/spaces.ts');
const authorizationWorker = require('../convex/spaceDeletionAuthorization.ts');
const fake = require('./helpers/fake-convex.cjs');

const NOW = 10_000;
const ORG = 'org_a';

// A Control authorization the Space can no longer take: its fence (or deletion) came
// from an earlier authorized request. Each row carries the reason the event closes with.
const REFUSALS = [
  { lifecycle: 'deleting', reason: 'deletion_in_progress' },
  { lifecycle: 'deleted', reason: 'deletion_completed' },
  { lifecycle: null, reason: 'deletion_not_live' },
];

function requestIdOf(spaceRef) {
  return `space:${spaceRef}:deletion:idem-${spaceRef}`;
}

function authorizationEventIdOf(spaceRef) {
  return `${requestIdOf(spaceRef)}:authorize`;
}

/**
 * One personal deletion intent (pending Control authorization) and its authorization event,
 * with its Space in `lifecycle`. A `null` lifecycle leaves the Space row out, which is
 * data corruption.
 */
function deletion(spaceRef, { lifecycle = 'active', nextAttemptAt = 1_000 } = {}) {
  const requestId = requestIdOf(spaceRef);
  const tables = {
    spaceDeletionRequests: [{
      _id: `request_${spaceRef}`, requestId, idempotencyKey: `idem-${spaceRef}`, spaceRef,
      externalOrgId: ORG, ownerExternalAuthId: 'user_a', state: 'pending_authorization', createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionAuthorizationEvents: [{
      _id: `auth_${spaceRef}`, eventId: authorizationEventIdOf(spaceRef), requestId,
      deliveryState: 'pending', deliveryAttempts: 0, nextAttemptAt, createdAt: 0,
    }],
  };
  if (lifecycle !== null) {
    tables.spaces = [{
      _id: `space_${spaceRef}`, spaceRef, externalOrgId: ORG, kind: 'personal',
      ownerExternalAuthId: 'user_a', lifecycle, lifecycleRevision: 4,
    }];
  }
  return tables;
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

/** A ctx whose runMutation runs the real spaces.ts handlers against the same in-memory tables. */
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

function eventOf(tables, spaceRef) {
  return tables.spaceDeletionAuthorizationEvents.find((row) => row.eventId === authorizationEventIdOf(spaceRef));
}

function requestOf(tables, spaceRef) {
  return tables.spaceDeletionRequests.find((row) => row.requestId === requestIdOf(spaceRef));
}

function spaceOf(tables, spaceRef) {
  return tables.spaces.find((row) => row.spaceRef === spaceRef);
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

/** Claims the earliest due authorization event as worker-1, as the authorization worker does. */
async function claimAuthorization(handle, now = NOW) {
  const claimed = await spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now });
  assert.ok(claimed, 'an authorization event is claimed');
  return claimed;
}

function acknowledge(handle, spaceRef, status, { workerId = 'worker-1', now = NOW } = {}) {
  return spaces.acknowledgeDeletionAuthorizationClaim._handler(handle.ctx, {
    eventId: authorizationEventIdOf(spaceRef), workerId, now, status,
  });
}

/**
 * Answers every Control authorization call with `status` for the request each call names,
 * while `body` runs, and returns the requests sent.
 */
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

for (const { lifecycle, reason } of REFUSALS) {
  test(`an authorized request for a Space that is ${lifecycle ?? 'missing'} is rejected, its event closes as ${reason}, and nothing is fenced or fanned out`, async (t) => {
    const warn = t.mock.method(console, 'warn', () => {});
    const handle = harness(tablesOf(deletion('space_a', { lifecycle })));
    await claimAuthorization(handle);

    const result = await acknowledge(handle, 'space_a', 'authorized');
    assert.deepEqual(result, { state: 'rejected' });
    assert.equal(requestOf(handle.tables, 'space_a').state, 'rejected');
    assert.equal(requestOf(handle.tables, 'space_a').updatedAt, NOW);
    const event = eventOf(handle.tables, 'space_a');
    assert.equal(event.deliveryState, 'closed');
    assert.equal(event.lastDeliveryError, reason);
    assert.equal(event.leaseOwner, undefined);
    assert.equal(event.leaseExpiresAt, undefined);
    assert.deepEqual(handle.log.inserts, [], 'no owner receipt, owner delivery or lifecycle event is written');
    assert.deepEqual(scheduledNames(handle.log), [], 'nothing is scheduled');
    if (lifecycle !== null) {
      assert.equal(spaceOf(handle.tables, 'space_a').lifecycle, lifecycle, 'the Space keeps its lifecycle');
      assert.equal(spaceOf(handle.tables, 'space_a').lifecycleRevision, 4, 'the Space keeps its revision');
    }

    assert.equal(warn.mock.callCount(), 1, 'the refusal is logged once');
    const payload = warn.mock.calls[0].arguments[1];
    assert.deepEqual(Object.keys(payload).sort(), ['eventId', 'reason', 'requestId', 'scope']);
    assert.equal(payload.reason, reason);
    assert.equal(payload.scope, 'authorization');
    assert.equal(payload.eventId, authorizationEventIdOf('space_a'));
    assert.equal(payload.requestId, requestIdOf('space_a'));
  });
}

for (const status of ['blocked_legal_hold', 'rejected']) {
  test(`a Control ${status} answer is recorded as before, even when the Space is already deleting`, async () => {
    const handle = harness(tablesOf(deletion('space_a', { lifecycle: 'deleting' })));
    await claimAuthorization(handle);

    assert.deepEqual(await acknowledge(handle, 'space_a', status), { state: status });
    assert.equal(requestOf(handle.tables, 'space_a').state, status);
    assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'acknowledged');
    assert.equal(eventOf(handle.tables, 'space_a').lastDeliveryError, undefined);
    assert.equal(spaceOf(handle.tables, 'space_a').lifecycle, 'deleting');
    assert.deepEqual(handle.log.inserts, []);
    assert.deepEqual(scheduledNames(handle.log), []);
  });
}

test('a worker that does not hold the claim is refused before any refusal is written', async () => {
  const handle = harness(tablesOf(deletion('space_a', { lifecycle: 'deleting' })));
  await claimAuthorization(handle);
  const patchesBefore = handle.log.patches.length;

  await assert.rejects(
    acknowledge(handle, 'space_a', 'authorized', { workerId: 'worker-2' }),
    /only the active delivery worker may acknowledge the claim/,
  );
  assert.equal(handle.log.patches.length, patchesBefore, 'nothing is written');
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'claimed');
  assert.equal(requestOf(handle.tables, 'space_a').state, 'pending_authorization');
});

test('an authorized request for a live Space fences it and fans out exactly as before', async () => {
  const handle = harness(tablesOf(deletion('space_a', { lifecycle: 'active' })));
  await claimAuthorization(handle);

  assert.deepEqual(await acknowledge(handle, 'space_a', 'authorized'), { state: 'authorized' });
  assert.equal(requestOf(handle.tables, 'space_a').state, 'authorized');
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'acknowledged');
  assert.equal(spaceOf(handle.tables, 'space_a').lifecycle, 'deleting');
  assert.equal(spaceOf(handle.tables, 'space_a').lifecycleRevision, 5);

  const inserted = handle.log.inserts.map((insert) => insert.table);
  assert.equal(inserted.filter((table) => table === 'spaceDeletionOwnerReceipts').length, 6);
  assert.equal(inserted.filter((table) => table === 'spaceDeletionOwnerDeliveryEvents').length, 3);
  assert.equal(inserted.filter((table) => table === 'spaceLifecycleEvents').length, 1);
  const receipts = handle.tables.spaceDeletionOwnerReceipts.filter((row) => row.requestId === requestIdOf('space_a'));
  assert.equal(receipts.find((row) => row.ownerPlane === 'application').status, 'succeeded');
  assert.ok(
    receipts.filter((row) => row.ownerPlane !== 'application').every((row) => row.status === 'pending'),
    'every other owner receipt is pending',
  );
  assert.deepEqual(
    handle.tables.spaceDeletionOwnerDeliveryEvents.map((row) => [row.ownerPlane, row.deliveryState]).sort(),
    [['data', 'pending'], ['ingestion', 'pending'], ['model', 'pending']],
  );
  assert.deepEqual(scheduledNames(handle.log), [
    'spaceRegistration:deliverOne',
    'spaceAttentionIngest:purgeSpace',
    'spaceDeletionModel:deliverOne',
    'spaceDeletionData:deliverOne',
    'spaceDeletionIngestion:deliverOne',
    'spaceDeletionReceiptReconciler:reconcileOne',
  ]);
});

test('after a refused authorization, the next pending authorization is claimed and the refused one is not claimed again', async (t) => {
  t.mock.method(console, 'warn', () => {});
  const handle = harness(tablesOf(
    deletion('space_a', { lifecycle: 'deleting', nextAttemptAt: 1_000 }),
    deletion('space_b', { lifecycle: 'active', nextAttemptAt: 2_000 }),
  ));
  const first = await claimAuthorization(handle);
  assert.equal(first.eventId, authorizationEventIdOf('space_a'));
  assert.deepEqual(await acknowledge(handle, 'space_a', 'authorized'), { state: 'rejected' });

  // Past the 30 s lease a stuck row would still have held, the claim finds Space B first.
  const second = await claimAuthorization(handle, NOW + 60_000);
  assert.equal(second.eventId, authorizationEventIdOf('space_b'));
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'closed');
  assert.equal(eventOf(handle.tables, 'space_b').deliveryState, 'claimed');
});

for (const { lifecycle, reason } of REFUSALS) {
  test(`the authorization worker resolves a refusal for a ${lifecycle ?? 'missing'} Space and then delivers the next authorization`, async (t) => {
    const warn = t.mock.method(console, 'warn', () => {});
    const handle = harness(tablesOf(
      deletion('space_a', { lifecycle, nextAttemptAt: 1_000 }),
      deletion('space_b', { lifecycle: 'active', nextAttemptAt: 2_000 }),
    ));

    let refused;
    const firstSent = await withControlAnswer('authorized', async () => {
      refused = await authorizationWorker.deliverOne._handler(handle.ctx, { workerId: 'worker-1' });
    });
    assert.deepEqual(refused, { eventId: authorizationEventIdOf('space_a'), status: 'rejected' });
    assert.deepEqual(firstSent.map((sent) => sent.payload.request_id), [requestIdOf('space_a')]);
    assert.equal(eventOf(handle.tables, 'space_a').lastDeliveryError, reason);
    assert.equal(warn.mock.callCount(), 1);

    let next;
    const secondSent = await withControlAnswer('authorized', async () => {
      next = await authorizationWorker.deliverOne._handler(handle.ctx, { workerId: 'worker-2' });
    });
    assert.deepEqual(next, { eventId: authorizationEventIdOf('space_b'), status: 'authorized' });
    assert.deepEqual(secondSent.map((sent) => sent.payload.request_id), [requestIdOf('space_b')]);
    assert.equal(eventOf(handle.tables, 'space_b').deliveryState, 'acknowledged');
    assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'closed', 'the refused event is never claimed again');
  });
}

// R-app-03: a refused request's receipt names why it was refused, from the closed vocabulary. Only a
// rejected request carries a reason. Every other state has none, and so does a refusal Control gave itself.
function membership() {
  return fake.memberTables({}, { orgId: ORG, authId: 'user_a' });
}

function receiptForGateway(handle, spaceRef) {
  return spaces.getPersonalSpaceDeletionReceiptForGateway._handler(handle.ctx, {
    externalAuthId: 'user_a',
    externalOrgId: ORG,
    serviceKey: fake.SERVICE_KEY,
    requestId: requestIdOf(spaceRef),
  });
}

function receiptForViewer(handle, spaceRef) {
  const ctx = { ...handle.ctx, auth: { getUserIdentity: async () => ({ 'properties.externalAuthId': 'user_a' }) } };
  return spaces.getPersonalSpaceDeletionReceipt._handler(ctx, { externalOrgId: ORG, requestId: requestIdOf(spaceRef) });
}

for (const { lifecycle, reason } of REFUSALS) {
  test(`the receipt of a request refused for a ${lifecycle ?? 'missing'} Space names the refusal ${reason}, to the gateway and to the viewer`, async (t) => {
    t.mock.method(console, 'warn', () => {});
    fake.withServiceKey(t);
    const handle = harness(tablesOf(deletion('space_a', { lifecycle }), membership()));
    await claimAuthorization(handle);
    assert.deepEqual(await acknowledge(handle, 'space_a', 'authorized'), { state: 'rejected' });

    const gateway = await receiptForGateway(handle, 'space_a');
    assert.equal(gateway.request.state, 'rejected');
    assert.equal(gateway.purgeStatus, 'pending');
    assert.equal(gateway.rejectionReason, reason);

    const viewer = await receiptForViewer(handle, 'space_a');
    assert.equal(viewer.request.state, 'rejected');
    assert.equal(viewer.rejectionReason, reason);
  });
}

test('a receipt carries no rejection reason while the request is pending, authorized, blocked, or refused by Control', async (t) => {
  fake.withServiceKey(t);
  const cases = [
    { name: 'pending', answer: null, state: 'pending_authorization' },
    { name: 'authorized', answer: 'authorized', state: 'authorized' },
    { name: 'blocked_legal_hold', answer: 'blocked_legal_hold', state: 'blocked_legal_hold' },
    { name: 'refused by Control', answer: 'rejected', state: 'rejected' },
  ];
  for (const { name, answer, state } of cases) {
    const handle = harness(tablesOf(deletion('space_a', { lifecycle: 'active' }), membership()));
    if (answer !== null) {
      await claimAuthorization(handle);
      assert.deepEqual(await acknowledge(handle, 'space_a', answer), { state: answer }, name);
    }
    const gateway = await receiptForGateway(handle, 'space_a');
    const viewer = await receiptForViewer(handle, 'space_a');
    assert.equal(gateway.request.state, state, name);
    assert.equal(gateway.rejectionReason, undefined, name);
    assert.equal(viewer.rejectionReason, undefined, name);
    assert.doesNotMatch(JSON.stringify(gateway), /rejectionReason/, name);
  }
});

test('a rejected request whose event carries text outside the closed vocabulary shows no reason', async (t) => {
  fake.withServiceKey(t);
  const tables = tablesOf(deletion('space_a', { lifecycle: 'active' }), membership());
  tables.spaceDeletionRequests[0].state = 'rejected';
  tables.spaceDeletionAuthorizationEvents[0].lastDeliveryError = 'Control: user_a is under a hold';
  const gateway = await receiptForGateway(harness(tables), 'space_a');
  assert.equal(gateway.request.state, 'rejected');
  assert.equal(gateway.rejectionReason, undefined);
  assert.doesNotMatch(JSON.stringify(gateway), /hold/);
});
