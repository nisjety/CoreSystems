const assert = require('node:assert/strict');
const test = require('node:test');
const { getFunctionName } = require('convex/server');

const {
  ORG_ROOM_SYNC_DEBOUNCE_MS,
  clearOrganizationRoomSyncRequest,
  markOrganizationRoomSyncRunning,
  requestOrganizationRoomSync,
  runRequestedOrganizationRoomSync,
  syncAllOrganizationRooms,
} = require('../convex/spaceMembershipSync.ts');
const { makeCtx } = require('./helpers/fake-convex.cjs');

// DEPTH-TEST-LIVE-APP (a): the org-room sync handlers driven through `_handler`
// with a stubbed action ctx. The cron tick and the org-core / Control calls on a
// deployment stay live checks; this proves the loop, the kill switch, the
// per-organization filter and the debounce-row bookkeeping.

const LIST_ROOMS = 'spaces:listActiveOrganizationRooms';
const SYNC_ROOM = 'spaceMembershipSync:syncOrganizationRoom';
const MARK_RUNNING = 'spaceMembershipSync:markOrganizationRoomSyncRunning';
const CLEAR_REQUEST = 'spaceMembershipSync:clearOrganizationRoomSyncRequest';
const RUN_REQUESTED = 'spaceMembershipSync:runRequestedOrganizationRoomSync';

const ROOMS = [
  { spaceRef: 'space_a1', externalOrgId: 'org_a' },
  { spaceRef: 'space_b1', externalOrgId: 'org_b' },
  { spaceRef: 'space_a2', externalOrgId: 'org_a' },
];

function withReconcile(t, value) {
  const prior = process.env.SPACE_ORG_ROOM_RECONCILE;
  if (value === undefined) delete process.env.SPACE_ORG_ROOM_RECONCILE;
  else process.env.SPACE_ORG_ROOM_RECONCILE = value;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_ORG_ROOM_RECONCILE;
    else process.env.SPACE_ORG_ROOM_RECONCILE = prior;
  });
}

/** An action ctx that records every call in order, by function name. */
function actionCtx({ rooms = ROOMS, failSyncFor } = {}) {
  const calls = [];
  const ctx = {
    async runQuery(ref, args) {
      calls.push({ kind: 'query', name: getFunctionName(ref), args });
      if (getFunctionName(ref) === LIST_ROOMS) return rooms;
      throw new Error(`unexpected query ${getFunctionName(ref)}`);
    },
    async runAction(ref, args) {
      calls.push({ kind: 'action', name: getFunctionName(ref), args });
      if (failSyncFor && args.spaceRef === failSyncFor) throw new Error('sync failed');
      return { status: 'applied', members: 1 };
    },
    async runMutation(ref, args) {
      calls.push({ kind: 'mutation', name: getFunctionName(ref), args });
      return undefined;
    },
  };
  return { ctx, calls };
}

test('syncAllOrganizationRooms does nothing while the kill switch is off', async (t) => {
  withReconcile(t, undefined);
  const { ctx, calls } = actionCtx();
  assert.deepEqual(await syncAllOrganizationRooms._handler(ctx, {}), { status: 'disabled', rooms: 0 });
  assert.deepEqual(calls, []);
});

test('syncAllOrganizationRooms lists the active rooms once and syncs each room once, in order', async (t) => {
  withReconcile(t, '1');
  const { ctx, calls } = actionCtx();
  assert.deepEqual(await syncAllOrganizationRooms._handler(ctx, {}), { status: 'ok', rooms: 3 });
  assert.deepEqual(calls, [
    { kind: 'query', name: LIST_ROOMS, args: {} },
    ...ROOMS.map((room) => ({
      kind: 'action',
      name: SYNC_ROOM,
      args: { spaceRef: room.spaceRef, externalOrgId: room.externalOrgId },
    })),
  ]);
});

test('syncAllOrganizationRooms with no rooms reports zero and syncs nothing', async (t) => {
  withReconcile(t, 'true');
  const { ctx, calls } = actionCtx({ rooms: [] });
  assert.deepEqual(await syncAllOrganizationRooms._handler(ctx, {}), { status: 'ok', rooms: 0 });
  assert.deepEqual(
    calls.map((call) => call.name),
    [LIST_ROOMS],
  );
});

test('runRequestedOrganizationRoomSync with the kill switch off only clears the debounce row', async (t) => {
  withReconcile(t, '0');
  const { ctx, calls } = actionCtx();
  assert.deepEqual(await runRequestedOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_a' }), {
    status: 'disabled',
  });
  assert.deepEqual(calls, [{ kind: 'mutation', name: CLEAR_REQUEST, args: { externalOrgId: 'org_a' } }]);
});

test('runRequestedOrganizationRoomSync marks running, syncs only that organization\'s rooms, then clears', async (t) => {
  withReconcile(t, '1');
  const { ctx, calls } = actionCtx();
  assert.deepEqual(await runRequestedOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_a' }), {
    status: 'ok',
    rooms: 2,
  });
  assert.deepEqual(calls, [
    { kind: 'mutation', name: MARK_RUNNING, args: { externalOrgId: 'org_a' } },
    { kind: 'query', name: LIST_ROOMS, args: {} },
    { kind: 'action', name: SYNC_ROOM, args: { spaceRef: 'space_a1', externalOrgId: 'org_a' } },
    { kind: 'action', name: SYNC_ROOM, args: { spaceRef: 'space_a2', externalOrgId: 'org_a' } },
    { kind: 'mutation', name: CLEAR_REQUEST, args: { externalOrgId: 'org_a' } },
  ]);
});

test('runRequestedOrganizationRoomSync for an organization without a room syncs nothing and still clears', async (t) => {
  withReconcile(t, '1');
  const { ctx, calls } = actionCtx();
  assert.deepEqual(await runRequestedOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_none' }), {
    status: 'ok',
    rooms: 0,
  });
  assert.deepEqual(
    calls.map((call) => call.name),
    [MARK_RUNNING, LIST_ROOMS, CLEAR_REQUEST],
  );
});

test('runRequestedOrganizationRoomSync clears the debounce row even when a room sync throws', async (t) => {
  withReconcile(t, '1');
  const { ctx, calls } = actionCtx({ failSyncFor: 'space_a1' });
  await assert.rejects(runRequestedOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_a' }), /sync failed/);
  assert.deepEqual(
    calls.map((call) => call.name),
    [MARK_RUNNING, LIST_ROOMS, SYNC_ROOM, CLEAR_REQUEST],
  );
});

test('requestOrganizationRoomSync is a no-op while the kill switch is off', async (t) => {
  withReconcile(t, undefined);
  const { ctx, log } = makeCtx({ spaceOrgRoomSyncRequests: [] });
  assert.deepEqual(await requestOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_a' }), {
    status: 'disabled',
  });
  assert.equal(log.inserts.length + log.patches.length + log.scheduled.length, 0);
});

test('requestOrganizationRoomSync ignores a blank organization id', async (t) => {
  withReconcile(t, '1');
  const { ctx, log } = makeCtx({ spaceOrgRoomSyncRequests: [] });
  assert.deepEqual(await requestOrganizationRoomSync._handler(ctx, { externalOrgId: '   ' }), {
    status: 'ignored',
  });
  assert.equal(log.inserts.length + log.patches.length + log.scheduled.length, 0);
});

test('the first request inserts a scheduled row and queues one run five seconds out', async (t) => {
  withReconcile(t, '1');
  const { ctx, tables, log } = makeCtx({ spaceOrgRoomSyncRequests: [] });
  const before = Date.now();
  const result = await requestOrganizationRoomSync._handler(ctx, { externalOrgId: ' org_a ' });
  assert.equal(result.status, 'scheduled');
  assert.ok(result.dueAt >= before + ORG_ROOM_SYNC_DEBOUNCE_MS);
  assert.equal(tables.spaceOrgRoomSyncRequests.length, 1);
  assert.equal(tables.spaceOrgRoomSyncRequests[0].externalOrgId, 'org_a');
  assert.equal(tables.spaceOrgRoomSyncRequests[0].state, 'scheduled');
  assert.equal(tables.spaceOrgRoomSyncRequests[0].dueAt, result.dueAt);
  assert.equal(log.scheduled.length, 1);
  assert.equal(log.scheduled[0].delay, ORG_ROOM_SYNC_DEBOUNCE_MS);
  assert.equal(getFunctionName(log.scheduled[0].fn), RUN_REQUESTED);
  assert.deepEqual(log.scheduled[0].args, { externalOrgId: 'org_a' });
});

test('a second request inside the quiet period is coalesced with no write and no run', async (t) => {
  withReconcile(t, '1');
  const { ctx, log } = makeCtx({
    spaceOrgRoomSyncRequests: [{ externalOrgId: 'org_a', state: 'scheduled', dueAt: Date.now() + 60_000 }],
  });
  assert.deepEqual(await requestOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_a' }), {
    status: 'coalesced',
  });
  assert.equal(log.inserts.length + log.patches.length + log.scheduled.length, 0);
});

test('a request during a running sync reschedules the existing row and queues a fresh run', async (t) => {
  withReconcile(t, '1');
  const { ctx, tables, log } = makeCtx({
    spaceOrgRoomSyncRequests: [{ externalOrgId: 'org_a', state: 'running', dueAt: 1 }],
  });
  const result = await requestOrganizationRoomSync._handler(ctx, { externalOrgId: 'org_a' });
  assert.equal(result.status, 'scheduled');
  assert.equal(log.inserts.length, 0, 'the existing row is reused');
  assert.equal(tables.spaceOrgRoomSyncRequests.length, 1);
  assert.equal(tables.spaceOrgRoomSyncRequests[0].state, 'scheduled');
  assert.equal(tables.spaceOrgRoomSyncRequests[0].dueAt, result.dueAt);
  assert.equal(log.scheduled.length, 1);
  assert.equal(getFunctionName(log.scheduled[0].fn), RUN_REQUESTED);
});

test('mark-running then clear deletes the row, but a row rescheduled mid-run survives the clear', async () => {
  const { ctx, tables } = makeCtx({
    spaceOrgRoomSyncRequests: [{ externalOrgId: 'org_a', state: 'scheduled', dueAt: 1 }],
  });
  assert.deepEqual(await markOrganizationRoomSyncRunning._handler(ctx, { externalOrgId: 'org_a' }), { found: true });
  assert.equal(tables.spaceOrgRoomSyncRequests[0].state, 'running');
  // A request lands mid-run and flips the row back to scheduled.
  tables.spaceOrgRoomSyncRequests[0].state = 'scheduled';
  assert.deepEqual(await clearOrganizationRoomSyncRequest._handler(ctx, { externalOrgId: 'org_a' }), {
    cleared: false,
  });
  assert.equal(tables.spaceOrgRoomSyncRequests.length, 1, 'the queued request survives');
  await markOrganizationRoomSyncRunning._handler(ctx, { externalOrgId: 'org_a' });
  assert.deepEqual(await clearOrganizationRoomSyncRequest._handler(ctx, { externalOrgId: 'org_a' }), {
    cleared: true,
  });
  assert.equal(tables.spaceOrgRoomSyncRequests.length, 0);
});

test('mark-running and clear tolerate a missing row (the cron got there first)', async () => {
  const { ctx } = makeCtx({ spaceOrgRoomSyncRequests: [] });
  assert.deepEqual(await markOrganizationRoomSyncRunning._handler(ctx, { externalOrgId: 'org_a' }), { found: false });
  assert.deepEqual(await clearOrganizationRoomSyncRequest._handler(ctx, { externalOrgId: 'org_a' }), {
    cleared: false,
  });
});
