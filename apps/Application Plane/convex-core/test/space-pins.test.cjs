const assert = require('node:assert/strict');
const test = require('node:test');

const {
  MAX_PINS_PER_ROOM,
  clearForGateway,
  listForGateway,
  orderPins,
  requireId,
  setForGateway,
} = require('../convex/spacePins.ts');
const { AUTH, ORG, SERVICE_KEY, makeCtx, memberTables, rejectsWith, withServiceKey } = require('./helpers/fake-convex.cjs');

const base = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG, spaceRef: 'space_1' };

function pin(threadId, pinnedAt, extra = {}) {
  return { spaceRef: 'space_1', externalOrgId: ORG, threadId, pinnedByExternalAuthId: 'auth_pinner', pinnedAt, ...extra };
}

test('orderPins puts the newest pin first and breaks ties by thread id', () => {
  const ordered = orderPins([
    { threadId: 'b', pinnedAt: 10 },
    { threadId: 'z', pinnedAt: 30 },
    { threadId: 'a', pinnedAt: 10 },
  ]);
  assert.deepEqual(ordered.map((p) => p.threadId), ['z', 'a', 'b']);
  const input = [{ threadId: 'x', pinnedAt: 1 }, { threadId: 'y', pinnedAt: 2 }];
  orderPins(input);
  assert.deepEqual(input.map((p) => p.threadId), ['x', 'y'], 'the input is not mutated');
});

test('requireId trims and rejects empty, over-long and control-character ids', () => {
  assert.equal(requireId('  thread_1 ', 'thread_id'), 'thread_1');
  assert.throws(() => requireId('', 'thread_id'), /pin_invalid:thread_id/);
  assert.throws(() => requireId('   ', 'space_ref'), /pin_invalid:space_ref/);
  assert.throws(() => requireId(undefined, 'thread_id'), /pin_invalid:thread_id/);
  assert.throws(() => requireId('x'.repeat(201), 'thread_id'), /pin_invalid:thread_id/);
  assert.equal(requireId('x'.repeat(200), 'thread_id').length, 200);
  for (const ch of ['\u0000', '\u0007', '\u001f', '\u007f']) {
    assert.throws(() => requireId(`a${ch}b`, 'thread_id'), /pin_invalid:thread_id/);
  }
});

test('setForGateway pins once, is idempotent and keeps the original pinner and time', async (t) => {
  withServiceKey(t);
  const { ctx, tables, log } = makeCtx(memberTables({ spacePins: [] }));
  const first = await setForGateway._handler(ctx, { ...base, threadId: 'thread_a' });
  assert.equal(first.changed, true);
  assert.equal(first.threadId, 'thread_a');
  assert.equal(tables.spacePins.length, 1);
  assert.equal(tables.spacePins[0].pinnedByExternalAuthId, AUTH);

  const again = await setForGateway._handler(ctx, { ...base, externalAuthId: AUTH, threadId: ' thread_a ' });
  assert.equal(again.changed, false);
  assert.equal(again.pinnedAt, first.pinnedAt);
  assert.equal(tables.spacePins.length, 1);
  assert.equal(log.inserts.length, 1);
});

test('setForGateway keeps the first pinner when someone else pins the same thread', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(memberTables({ spacePins: [pin('thread_a', 5)] }));
  const result = await setForGateway._handler(ctx, { ...base, threadId: 'thread_a' });
  assert.deepEqual(result, { threadId: 'thread_a', pinnedAt: 5, changed: false });
  assert.equal(tables.spacePins[0].pinnedByExternalAuthId, 'auth_pinner');
});

test('setForGateway refuses the 51st pin with room_pins_limit', async (t) => {
  withServiceKey(t);
  assert.equal(MAX_PINS_PER_ROOM, 50);
  const rows = Array.from({ length: 50 }, (_, i) => pin(`t_${i}`, i + 1));
  const { ctx, tables } = makeCtx(memberTables({ spacePins: rows }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, threadId: 'thread_new' }), 'room_pins_limit');
  assert.equal(tables.spacePins.length, 50);
  // an already pinned thread at the limit is still an idempotent success
  const again = await setForGateway._handler(ctx, { ...base, threadId: 't_3' });
  assert.equal(again.changed, false);
});

test('setForGateway refuses a bad service key, a non-member and invalid ids', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(memberTables({ spacePins: [] }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, serviceKey: 'wrong', threadId: 'a' }), 'Unauthorized');
  await rejectsWith(setForGateway._handler(ctx, { ...base, externalAuthId: 'stranger', threadId: 'a' }), 'Unauthorized');
  await rejectsWith(setForGateway._handler(ctx, { ...base, threadId: '' }), 'pin_invalid:thread_id');
  await rejectsWith(setForGateway._handler(ctx, { ...base, spaceRef: '\u0000' }), 'pin_invalid:space_ref');
  assert.equal(tables.spacePins.length, 0);
});

test('setForGateway will not take over a pin row that belongs to another organization', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(memberTables({ spacePins: [pin('thread_a', 5, { externalOrgId: 'org_other' })] }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, threadId: 'thread_a' }), 'Unauthorized');
});

test('clearForGateway removes a pin and is idempotent when absent', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(memberTables({ spacePins: [pin('thread_a', 5), pin('thread_b', 6)] }));
  assert.deepEqual(await clearForGateway._handler(ctx, { ...base, threadId: 'thread_a' }), {
    threadId: 'thread_a',
    changed: true,
  });
  assert.deepEqual(tables.spacePins.map((p) => p.threadId), ['thread_b']);
  assert.deepEqual(await clearForGateway._handler(ctx, { ...base, threadId: 'thread_a' }), {
    threadId: 'thread_a',
    changed: false,
  });
  assert.equal(tables.spacePins.length, 1);
});

test('clearForGateway leaves another organization\'s row alone', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(memberTables({ spacePins: [pin('thread_a', 5, { externalOrgId: 'org_other' })] }));
  const result = await clearForGateway._handler(ctx, { ...base, threadId: 'thread_a' });
  assert.equal(result.changed, false);
  assert.equal(tables.spacePins.length, 1);
});

test('listForGateway returns pins newest first and drops rows of another organization', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(
    memberTables({
      spacePins: [
        pin('old', 1),
        pin('foreign', 99, { externalOrgId: 'org_other' }),
        pin('new', 3),
        pin('mid', 2),
        { ...pin('elsewhere', 50), spaceRef: 'space_2' },
      ],
    }),
  );
  const { pins } = await listForGateway._handler(ctx, base);
  assert.deepEqual(pins.map((p) => p.threadId), ['new', 'mid', 'old']);
  assert.deepEqual(Object.keys(pins[0]).sort(), ['pinnedAt', 'pinnedByExternalAuthId', 'threadId']);
});

test('listForGateway requires membership', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(memberTables({ spacePins: [] }));
  await rejectsWith(listForGateway._handler(ctx, { ...base, externalAuthId: 'stranger' }), 'Unauthorized');
});
