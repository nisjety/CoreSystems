const assert = require('node:assert/strict');
const test = require('node:test');

const attention = require('../convex/spaceAttention.ts');
const { AUTH, ORG, SERVICE_KEY, makeCtx, memberTables, rejectsWith, withServiceKey } = require('./helpers/fake-convex.cjs');

/**
 * The service-key reads behind the inbox (WS11-S7): the caller's own thread follow, and the caller's
 * inbox page with its hide-preview setting. Driven through the in-memory ctx, so the organization and
 * subject scoping is exercised the way the indexes would scope it.
 */

const base = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG };

test('getThreadFollowForGateway reads the caller\'s own row, in this organization only', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(
    memberTables({
      spaceThreadFollows: [
        { externalOrgId: ORG, externalAuthId: AUTH, spaceRef: 'space_1', threadId: 'thread_1', state: 'following', reason: 'started', updatedAt: 1 },
        { externalOrgId: ORG, externalAuthId: 'bob', spaceRef: 'space_1', threadId: 'thread_1', state: 'muted', reason: 'manual', updatedAt: 2 },
        { externalOrgId: ORG, externalAuthId: 'bob', spaceRef: 'space_1', threadId: 'thread_2', state: 'following', reason: 'manual', updatedAt: 3 },
        { externalOrgId: 'org_other', externalAuthId: AUTH, spaceRef: 'space_9', threadId: 'thread_9', state: 'muted', reason: 'manual', updatedAt: 4 },
      ],
    }),
  );
  const follow = (threadId) => attention.getThreadFollowForGateway._handler(ctx, { ...base, threadId });
  assert.deepEqual(await follow('thread_1'), { state: 'following', reason: 'started' });
  assert.deepEqual(await follow('thread_2'), { state: 'none', reason: null }, 'bob\'s follow is not the caller\'s');
  assert.deepEqual(await follow('thread_9'), { state: 'none', reason: null }, 'a row in another organization is not this one');
  assert.deepEqual(await follow('thread_404'), { state: 'none', reason: null }, 'a thread nobody follows');
  await rejectsWith(
    attention.getThreadFollowForGateway._handler(ctx, { ...base, serviceKey: 'wrong', threadId: 'thread_1' }),
    'Unauthorized',
  );
});

test('listItemsForGateway returns the hidePreview setting of the caller, false when none is set', async (t) => {
  withServiceKey(t);
  const list = (rows) => {
    const { ctx } = makeCtx(memberTables({ spaceNotifyUserSettings: rows }));
    return attention.listItemsForGateway._handler(ctx, { ...base, state: 'open' });
  };
  assert.equal((await list([{ externalOrgId: ORG, externalAuthId: AUTH, hidePreview: true, updatedAt: 1 }])).hidePreview, true);
  assert.equal((await list([{ externalOrgId: ORG, externalAuthId: AUTH, hidePreview: false, updatedAt: 1 }])).hidePreview, false);
  assert.equal((await list([])).hidePreview, false, 'no row means the default: previews shown');
  assert.equal(
    (await list([{ externalOrgId: ORG, externalAuthId: 'bob', hidePreview: true, updatedAt: 1 }])).hidePreview,
    false,
    'another member\'s setting is not the caller\'s',
  );
  assert.equal(
    (await list([{ externalOrgId: 'org_other', externalAuthId: AUTH, hidePreview: true, updatedAt: 1 }])).hidePreview,
    false,
    'a setting in another organization is not this one',
  );
});

test('D22r: the default level is all in a DM and mentions elsewhere; a chosen level wins; another org\'s Space is not read', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(
    memberTables({
      spaces: [
        { spaceRef: 'dm_1', externalOrgId: ORG, kind: 'dm', lifecycle: 'active' },
        { spaceRef: 'gdm_1', externalOrgId: ORG, kind: 'group_dm', lifecycle: 'active' },
        { spaceRef: 'room_1', externalOrgId: ORG, kind: 'room', lifecycle: 'active' },
        { spaceRef: 'dm_other', externalOrgId: 'org_other', kind: 'dm', lifecycle: 'active' },
      ],
      spaceRoomNotifyPrefs: [{ externalOrgId: ORG, externalAuthId: AUTH, spaceRef: 'gdm_1', level: 'off', updatedAt: 1 }],
    }),
  );
  const one = (spaceRef) => attention.getRoomLevelForGateway._handler(ctx, { ...base, spaceRef });
  assert.deepEqual(await one('dm_1'), { level: 'all', levelIsDefault: true, hidePreview: false });
  assert.deepEqual(await one('gdm_1'), { level: 'off', levelIsDefault: false, hidePreview: false });
  assert.deepEqual(await one('room_1'), { level: 'mentions', levelIsDefault: true, hidePreview: false });
  assert.deepEqual(await one('dm_other'), { level: 'mentions', levelIsDefault: true, hidePreview: false });

  const many = await attention.attentionForGateway._handler(ctx, { ...base, spaceRefs: ['dm_1', 'gdm_1', 'room_1', 'nope'] });
  assert.deepEqual(
    many.rooms.map((room) => [room.spaceRef, room.level, room.levelIsDefault]),
    [['dm_1', 'all', true], ['gdm_1', 'off', false], ['room_1', 'mentions', true], ['nope', 'mentions', true]],
  );
});

test('setRoomLevelForGateway echoes the level it stored, on the first write and on an update', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(
    memberTables({ spaces: [{ spaceRef: 'dm_1', externalOrgId: ORG, kind: 'dm', lifecycle: 'active' }] }),
  );
  const set = (level) => attention.setRoomLevelForGateway._handler(ctx, { ...base, spaceRef: 'dm_1', level });
  assert.deepEqual(await set('mentions'), { level: 'mentions', levelIsDefault: false, hidePreview: false });
  assert.deepEqual(await set('off'), { level: 'off', levelIsDefault: false, hidePreview: false }, 'an update returns the new level');
  assert.deepEqual(await attention.getRoomLevelForGateway._handler(ctx, { ...base, spaceRef: 'dm_1' }), {
    level: 'off',
    levelIsDefault: false,
    hidePreview: false,
  });
});

test('C-PRIO-05: the inbox window pages inside the caller\'s org, so newer rows of another org never crowd it out', async (t) => {
  withServiceKey(t);
  const item = (externalOrgId, id, updatedAt) => ({
    externalOrgId,
    recipientAuthId: AUTH,
    spaceRef: 'space_1',
    kind: 'mention',
    threadId: `thread_${id}`,
    actorKind: 'user',
    state: 'open',
    count: 1,
    firstEventId: `e_${id}`,
    lastEventId: `e_${id}`,
    coalesceKey: `k_${id}`,
    createdAt: updatedAt,
    updatedAt,
    expiresAt: updatedAt + 1_000_000,
  });
  // Mine first, then three newer rows of the same subject in another org.
  const { ctx } = makeCtx(
    memberTables({
      spaceAttentionItems: [
        item(ORG, 'a', 10),
        item(ORG, 'b', 11),
        item('org_other', 'x', 20),
        item('org_other', 'y', 21),
        item('org_other', 'z', 22),
      ],
    }),
  );
  const page = await attention.listItemsForGateway._handler(ctx, { ...base, state: 'open', limit: 2 });
  assert.deepEqual(
    page.items.map((row) => row.threadId).sort(),
    ['thread_a', 'thread_b'],
    'a page of two is two of the caller\'s rows, not two filtered-out rows of another org',
  );
});

test('C-PRIO-05 (D18): attentionForGateway counts no item created before its room\'s history window', async (t) => {
  withServiceKey(t);
  const FLOOR = 1_000;
  const item = (spaceRef, id, kind, createdAt) => ({
    externalOrgId: ORG,
    recipientAuthId: AUTH,
    spaceRef,
    kind,
    threadId: `thread_${id}`,
    actorKind: 'user',
    state: 'open',
    count: 1,
    firstEventId: `e_${id}`,
    lastEventId: `e_${id}`,
    coalesceKey: `k_${id}`,
    createdAt,
    updatedAt: createdAt,
    expiresAt: Date.now() + 1_000_000,
  });
  const { ctx } = makeCtx(
    memberTables({
      spaceAttentionItems: [
        item('room_w', 'old_m', 'mention', FLOOR - 1),
        item('room_w', 'old_r', 'reply', FLOOR - 500),
        item('room_w', 'old_d', 'agent_done', FLOOR - 2),
        item('room_w', 'new_m', 'mention', FLOOR),
        item('room_w', 'new_r', 'reply', FLOOR + 5),
        item('room_a', 'a_m', 'mention', 1),
      ],
    }),
  );
  const ask = (floors) =>
    attention.attentionForGateway._handler(ctx, { ...base, spaceRefs: ['room_w', 'room_a'], ...(floors ? { floors } : {}) });
  const counts = (answer, ref) => {
    const room = answer.rooms.find((row) => row.spaceRef === ref);
    return [room.mentions, room.replies, room.agentDone];
  };

  const plain = await ask(undefined);
  assert.deepEqual(counts(plain, 'room_w'), [2, 2, 1], 'without a window every open item counts');

  const windowed = await ask([{ spaceRef: 'room_w', floorAt: FLOOR }]);
  assert.deepEqual(counts(windowed, 'room_w'), [1, 1, 0], 'only the items created at or after the floor');
  assert.deepEqual(counts(windowed, 'room_a'), [1, 0, 0], 'a room without a window is unchanged');
  assert.equal(windowed.otherOpen, 0);

  // A pre-window row of a room not asked about is still not counted under "other open".
  const narrow = await attention.attentionForGateway._handler(ctx, {
    ...base,
    spaceRefs: ['room_a'],
    floors: [{ spaceRef: 'room_w', floorAt: FLOOR }],
  });
  assert.equal(narrow.otherOpen, 2, 'room_w\'s two in-window rows only');

  await rejectsWith(ask([{ spaceRef: 'room_w', floorAt: Number.NaN }]), 'invalid_floor');
  await rejectsWith(ask([{ spaceRef: '', floorAt: FLOOR }]), 'invalid_space_ref');
});

test('C-PRIO-05 (D18): two floors for one room keep the later one', () => {
  const floors = attention.cleanFloors([
    { spaceRef: 'room_w', floorAt: 10 },
    { spaceRef: 'room_w', floorAt: 30 },
    { spaceRef: 'room_w', floorAt: 20 },
  ]);
  assert.equal(floors.get('room_w'), 30);
  assert.equal(attention.insideFloor({ spaceRef: 'room_w', createdAt: 29 }, floors), false);
  assert.equal(attention.insideFloor({ spaceRef: 'room_w', createdAt: 30 }, floors), true);
  assert.equal(attention.insideFloor({ spaceRef: 'room_x', createdAt: 0 }, floors), true);
});
