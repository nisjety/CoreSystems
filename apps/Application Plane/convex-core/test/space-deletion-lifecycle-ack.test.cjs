const assert = require('node:assert/strict');
const test = require('node:test');

const fake = require('./helpers/fake-convex.cjs');
const spaces = require('../convex/spaces.ts');

/**
 * R-app-10: the Control deleting lifecycle ack. The control receipt moves only for the authorized deletion request of its
 * own Space and organization, found through the spaceDeletionRequests index. A missing request or receipt is logged with
 * ids and a reason code, and the ack still commits. A redelivered ack changes nothing and does not throw.
 */

const NOW = 2_000;
const WORKER = 'worker-1';
const EVENT_ID = 'space:space_a:lifecycle:5';
const RECEIPT_DETAIL = 'Control Space projections purged with accepted deleting lifecycle revision';

function spaceRow(spaceRef, externalOrgId, { lifecycle = 'deleting', kind = 'room' } = {}) {
  return {
    _id: `space_${spaceRef}`, spaceRef, externalOrgId, kind, lifecycle, lifecycleRevision: 5,
    ownerExternalAuthId: 'user_a', createdByExternalAuthId: 'user_a', createdAt: 0, updatedAt: 0,
  };
}

function eventRow({
  eventId = EVENT_ID, spaceRef = 'space_a', externalOrgId = 'org_a', lifecycle = 'deleting', revision = 5,
} = {}) {
  return {
    _id: `event_${eventId}`, eventId, spaceRef, externalOrgId, lifecycle, revision,
    deliveryState: 'claimed', deliveryAttempts: 1, leaseOwner: WORKER, leaseExpiresAt: 99_999,
    nextAttemptAt: 0, createdAt: 0,
  };
}

function requestRow(requestId, spaceRef, externalOrgId, state) {
  return {
    _id: `request_${requestId}`, requestId, idempotencyKey: `idem-${requestId}`, spaceRef, externalOrgId,
    ownerExternalAuthId: 'user_a', state, createdAt: 0, updatedAt: 0,
  };
}

function controlReceipt(requestId, status = 'pending') {
  return { _id: `receipt_${requestId}_control`, requestId, ownerPlane: 'control', status, deadlineAt: 9_000, updatedAt: 0 };
}

function ack(ctx, { eventId = EVENT_ID, workerId = WORKER, now = NOW } = {}) {
  return spaces.acknowledgeRegistrationClaim._handler(ctx, {
    controlResourceRef: 'control:space_a', eventId, workerId, now,
  });
}

function eventOf(tables, eventId = EVENT_ID) {
  return tables.spaceLifecycleEvents.find((event) => event.eventId === eventId);
}

function controlReceiptOf(tables, requestId) {
  return tables.spaceDeletionOwnerReceipts.find((receipt) => receipt.requestId === requestId && receipt.ownerPlane === 'control');
}

/** Records every query a handler opens and the index each one names, so a test can see how a table was read. */
function recordQueries(ctx) {
  const opened = [];
  const query = ctx.db.query;
  ctx.db.query = (table) => {
    const entry = { table, index: null };
    opened.push(entry);
    const inner = query(table);
    const wrapped = {
      withIndex(name, build) {
        entry.index = name;
        inner.withIndex(name, build);
        return wrapped;
      },
      order(direction) {
        inner.order(direction);
        return wrapped;
      },
      filter() {
        return wrapped;
      },
      collect: () => inner.collect(),
      take: (n) => inner.take(n),
      first: () => inner.first(),
      unique: () => inner.unique(),
    };
    return wrapped;
  };
  return opened;
}

test('a deleting ack with an authorized request marks its control receipt succeeded, with the same receiptRef and detail as before', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_a', 'org_a')],
    spaceLifecycleEvents: [eventRow()],
    spaceDeletionRequests: [requestRow('req_a', 'space_a', 'org_a', 'authorized')],
    spaceDeletionOwnerReceipts: [controlReceipt('req_a')],
  });

  await ack(ctx);

  assert.equal(eventOf(tables).deliveryState, 'acknowledged');
  assert.equal(eventOf(tables).deliveredAt, NOW);
  const receipt = controlReceiptOf(tables, 'req_a');
  assert.equal(receipt.status, 'succeeded');
  assert.equal(receipt.receiptRef, 'control:space_a:lifecycle:5');
  assert.equal(receipt.detail, RECEIPT_DETAIL);
  assert.equal(receipt.updatedAt, NOW);
  assert.equal(warn.mock.callCount(), 0, 'a matched ack logs nothing');
});

test('a deleting ack with no authorized request commits: the event is acknowledged, nothing throws, and no receipt changes', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_a', 'org_a')],
    spaceLifecycleEvents: [eventRow()],
    spaceDeletionRequests: [requestRow('req_a', 'space_a', 'org_a', 'pending_authorization')],
    spaceDeletionOwnerReceipts: [controlReceipt('req_a')],
  });

  await ack(ctx);

  assert.equal(eventOf(tables).deliveryState, 'acknowledged');
  assert.equal(controlReceiptOf(tables, 'req_a').status, 'pending');
  assert.deepEqual(log.patches.map((patch) => patch.id), [eventOf(tables)._id], 'only the event is written');
  assert.equal(warn.mock.callCount(), 1, 'the skipped receipt is logged once');
  const payload = warn.mock.calls[0].arguments[1];
  assert.equal(payload.reason, 'control_deleting_without_authorized_request');
  assert.deepEqual(Object.keys(payload).sort(), ['eventId', 'reason', 'spaceRef']);
  assert.equal(payload.eventId, EVENT_ID);
  assert.equal(payload.spaceRef, 'space_a');
});

test('only the authorized request of the same Space and organization moves the receipt, among mixed deletion requests', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const requests = [
    requestRow('req_rejected', 'space_a', 'org_a', 'rejected'),
    requestRow('req_other_org', 'space_a', 'org_b', 'authorized'),
    requestRow('req_other_space', 'space_b', 'org_a', 'authorized'),
    requestRow('req_match', 'space_a', 'org_a', 'authorized'),
    requestRow('req_b_pending', 'space_b', 'org_b', 'pending_authorization'),
  ];
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_a', 'org_a'), spaceRow('space_b', 'org_b')],
    spaceLifecycleEvents: [eventRow()],
    spaceDeletionRequests: requests,
    spaceDeletionOwnerReceipts: requests.map((request) => controlReceipt(request.requestId)),
  });
  const opened = recordQueries(ctx);

  await ack(ctx);

  assert.equal(controlReceiptOf(tables, 'req_match').status, 'succeeded');
  for (const other of ['req_rejected', 'req_other_org', 'req_other_space', 'req_b_pending']) {
    assert.equal(controlReceiptOf(tables, other).status, 'pending', `${other} is not the request of this Space and organization`);
  }
  assert.equal(warn.mock.callCount(), 0);
  const requestReads = opened.filter((entry) => entry.table === 'spaceDeletionRequests');
  assert.deepEqual(
    requestReads.map((entry) => entry.index),
    ['by_space_and_state'],
    'the requests are read through the index for the Space, never the whole table',
  );
});

test('a second ack of the same event leaves the receipt succeeded, writes nothing, and does not throw', async (t) => {
  t.mock.method(console, 'warn', () => {});
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_a', 'org_a')],
    spaceLifecycleEvents: [eventRow()],
    spaceDeletionRequests: [requestRow('req_a', 'space_a', 'org_a', 'authorized')],
    spaceDeletionOwnerReceipts: [controlReceipt('req_a')],
  });

  await ack(ctx);
  const receiptAfterFirst = { ...controlReceiptOf(tables, 'req_a') };
  const writesAfterFirst = log.patches.length + log.inserts.length;

  await ack(ctx, { now: NOW + 500 });

  assert.equal(log.patches.length + log.inserts.length, writesAfterFirst, 'the redelivered ack writes nothing');
  assert.deepEqual(controlReceiptOf(tables, 'req_a'), receiptAfterFirst);
  assert.equal(controlReceiptOf(tables, 'req_a').status, 'succeeded');
  assert.equal(eventOf(tables).deliveredAt, NOW, 'the first acknowledgement time is kept');
});

test('a deleting ack whose control receipt row is missing commits with a typed warning and does not throw', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_a', 'org_a')],
    spaceLifecycleEvents: [eventRow()],
    spaceDeletionRequests: [requestRow('req_a', 'space_a', 'org_a', 'authorized')],
    spaceDeletionOwnerReceipts: [],
  });

  await ack(ctx);

  assert.equal(eventOf(tables).deliveryState, 'acknowledged');
  assert.equal(warn.mock.callCount(), 1);
  const payload = warn.mock.calls[0].arguments[1];
  assert.equal(payload.reason, 'control_deletion_receipt_missing');
  assert.deepEqual(Object.keys(payload).sort(), ['eventId', 'reason', 'spaceRef']);
});

test('an invalid receipt transition still refuses the deleting ack, and the receipt is left as it was', async (t) => {
  t.mock.method(console, 'warn', () => {});
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_a', 'org_a')],
    spaceLifecycleEvents: [eventRow()],
    spaceDeletionRequests: [requestRow('req_a', 'space_a', 'org_a', 'authorized')],
    spaceDeletionOwnerReceipts: [controlReceipt('req_a', 'blocked_legal_hold')],
  });

  await assert.rejects(ack(ctx), /invalid Space deletion owner receipt transition: blocked_legal_hold -> succeeded/);
  assert.equal(controlReceiptOf(tables, 'req_a').status, 'blocked_legal_hold');
});

test('a redelivered registration ack changes nothing: the Space is activated once, and the second ack does not throw', async () => {
  const registrationEvent = eventRow({
    eventId: 'space:space_p:lifecycle:1', spaceRef: 'space_p', lifecycle: 'pending_registration', revision: 1,
  });
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [{ ...spaceRow('space_p', 'org_a', { lifecycle: 'pending_registration', kind: 'personal' }), lifecycleRevision: 1 }],
    spaceLifecycleEvents: [registrationEvent],
  });
  const registrationAck = (now) => spaces.acknowledgeRegistrationClaim._handler(ctx, {
    controlResourceRef: 'control:space_p', eventId: 'space:space_p:lifecycle:1', workerId: WORKER, now,
  });

  await registrationAck(NOW);
  assert.equal(tables.spaces[0].lifecycle, 'active');
  const writesAfterFirst = log.patches.length + log.inserts.length;

  await registrationAck(NOW + 500);
  assert.equal(tables.spaces[0].lifecycle, 'active');
  assert.equal(log.patches.length + log.inserts.length, writesAfterFirst, 'the redelivered ack writes nothing');
});
