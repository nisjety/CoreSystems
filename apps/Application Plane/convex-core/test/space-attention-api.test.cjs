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
