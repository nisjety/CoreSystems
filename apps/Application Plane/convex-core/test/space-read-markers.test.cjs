const assert = require('node:assert/strict');
const test = require('node:test');

const { unreadThreadIds, advanceMarker } = require('../convex/spaceReadMarkers.ts');

/**
 * The pure decision behind `spaceReadMarkerForGateway` / `markSpaceReadForGateway`
 * (item 4b, "unread per thread"). Tested directly, the way every Space-side
 * decision in this package is: the handlers are thin shells around it plus the
 * auth checks and indexed reads a unit test cannot exercise here.
 */

const T = (iso) => Date.parse(iso);

const thread = (id, updatedAt, overrides = {}) => ({
  thread_id: id,
  updated_at: updatedAt,
  owner_subject_id: 'someone-else',
  ...overrides,
});

test('a thread that changed after the marker is unread', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [
      thread('old', '2026-09-07T09:00:00Z'),
      thread('new', '2026-09-07T11:00:00Z'),
    ],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, ['new']);
});

// No marker means no last visit to be new since. Badging a hundred posts on a
// first visit teaches people to ignore the badge.
test('a first visit badges nothing', () => {
  assert.deepEqual(unreadThreadIds([thread('t1', '2026-09-07T11:00:00Z')], null, 'viewer'), []);
  assert.deepEqual(unreadThreadIds([thread('t1', '2026-09-07T11:00:00Z')], undefined, 'viewer'), []);
  assert.deepEqual(unreadThreadIds([thread('t1', '2026-09-07T11:00:00Z')], Number.NaN, 'viewer'), []);
});

// Your own post arriving is not news.
test('the viewer’s own threads are never unread to them', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [
      thread('mine', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer' }),
      thread('theirs', '2026-09-07T11:00:00Z'),
    ],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, ['theirs']);
});

// The run's own timestamp counts as activity: a reply landing on an old thread
// is what makes it new again.
test('a newer run timestamp makes an older thread unread', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [thread('t1', '2026-09-07T09:00:00Z', { latest_run_updated_at: '2026-09-07T10:30:00Z' })],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, ['t1']);
});

// A thread that cannot be placed relative to the marker is left alone rather
// than guessed at, in either direction.
test('a thread with no readable timestamp is neither read nor unread', () => {
  const marker = T('2026-09-07T10:00:00Z');
  assert.deepEqual(unreadThreadIds([thread('t1', undefined), thread('t2', 'not a date')], marker, 'viewer'), []);
});

test('a blank thread id is skipped', () => {
  assert.deepEqual(unreadThreadIds([thread('  ', '2026-09-07T11:00:00Z')], T('2026-09-07T10:00:00Z'), 'viewer'), []);
});

// A slow poll landing after a fresh visit must not un-read what the fresh
// visit read.
test('the marker never moves backwards', () => {
  assert.equal(advanceMarker(2000, 1000), 2000);
  assert.equal(advanceMarker(1000, 2000), 2000);
  assert.equal(advanceMarker(null, 500), 500);
  assert.equal(advanceMarker(undefined, 500), 500);
  assert.equal(advanceMarker(Number.NaN, 500), 500);
});

// WS01-S7b: with `last_author_subject_id` the rule is "the latest message is
// mine", not "I started the thread". Kept identical to the SPA's
// space-unread.test.ts.
test('a thread I started whose latest message is someone else’s is unread', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [thread('t1', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer', last_author_subject_id: 'someone-else' })],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, ['t1']);
});

test('a thread whose latest message is mine is not unread, whoever started it', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [thread('t1', '2026-09-07T11:00:00Z', { owner_subject_id: 'someone-else', last_author_subject_id: 'viewer' })],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, []);
});

test('without last_author_subject_id the owner rule still applies', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [
      thread('mine', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer' }),
      thread('theirs', '2026-09-07T11:00:00Z'),
    ],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, ['theirs']);
});

test('an empty last_author_subject_id counts as absent', () => {
  const marker = T('2026-09-07T10:00:00Z');
  const ids = unreadThreadIds(
    [
      thread('mine', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer', last_author_subject_id: '' }),
      thread('blank', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer', last_author_subject_id: '   ' }),
      thread('theirs', '2026-09-07T11:00:00Z', { last_author_subject_id: '' }),
    ],
    marker,
    'viewer',
  );
  assert.deepEqual(ids, ['theirs']);
});

// ---------------------------------------------------------------------------
// WS11-S3: manual unread floor (planMarkRead / planMarkUnread / effectiveMarker)
// ---------------------------------------------------------------------------

const {
  effectiveMarker,
  markSpaceReadForGateway,
  markUnreadForGateway,
  planMarkRead,
  planMarkUnread,
  spaceReadMarkerForGateway,
} = require('../convex/spaceReadMarkers.ts');
const fake = require('./helpers/fake-convex.cjs');

test('effectiveMarker is the floor when set (never above lastReadAt), else lastReadAt', () => {
  assert.equal(effectiveMarker(null), null);
  assert.equal(effectiveMarker(undefined), null);
  assert.equal(effectiveMarker({ lastReadAt: 5000 }), 5000);
  assert.equal(effectiveMarker({ lastReadAt: 5000, unreadFloor: 3000 }), 3000);
  assert.equal(effectiveMarker({ lastReadAt: 5000, unreadFloor: 9000 }), 5000, 'a floor above the marker is clamped');
  assert.equal(effectiveMarker({ lastReadAt: 5000, unreadFloor: Number.NaN }), 5000);
});

test('planMarkRead with no row writes now and no floor', () => {
  assert.deepEqual(planMarkRead(null, 7000, 'while_open'), { lastReadAt: 7000, unreadFloor: null, write: true });
  assert.deepEqual(planMarkRead(undefined, 7000, 'enter'), { lastReadAt: 7000, unreadFloor: null, write: true });
});

test('planMarkRead while_open advances the marker and never moves it backwards', () => {
  assert.deepEqual(planMarkRead({ lastReadAt: 1000 }, 2000, 'while_open'), {
    lastReadAt: 2000,
    unreadFloor: null,
    write: true,
  });
  assert.deepEqual(planMarkRead({ lastReadAt: 3000 }, 2000, 'while_open'), {
    lastReadAt: 3000,
    unreadFloor: null,
    write: false,
  });
  assert.deepEqual(planMarkRead({ lastReadAt: 3000 }, 2000, 'enter'), {
    lastReadAt: 3000,
    unreadFloor: null,
    write: false,
  });
});

test('planMarkRead while_open with a floor is a no-op that reports the floor', () => {
  assert.deepEqual(planMarkRead({ lastReadAt: 3000, unreadFloor: 1500 }, 9000, 'while_open'), {
    lastReadAt: 3000,
    unreadFloor: 1500,
    write: false,
  });
});

test('planMarkRead enter clears the floor and advances; a cleared floor is itself a write', () => {
  assert.deepEqual(planMarkRead({ lastReadAt: 3000, unreadFloor: 1500 }, 9000, 'enter'), {
    lastReadAt: 9000,
    unreadFloor: null,
    write: true,
  });
  // clock behind the marker: the marker holds but the floor still has to be removed
  assert.deepEqual(planMarkRead({ lastReadAt: 3000, unreadFloor: 1500 }, 2000, 'enter'), {
    lastReadAt: 3000,
    unreadFloor: null,
    write: true,
  });
});

test('planMarkUnread lowers the floor to beforeMs only when that is earlier than the effective marker', () => {
  assert.equal(planMarkUnread(null, 1000), null, 'no marker, nothing to un-read');
  assert.equal(planMarkUnread({ lastReadAt: 5000 }, 5000), null, 'equal to the marker is ignored');
  assert.equal(planMarkUnread({ lastReadAt: 5000 }, 6000), null, 'later than the marker is ignored');
  assert.equal(planMarkUnread({ lastReadAt: 5000 }, 4000), 4000);
  assert.equal(planMarkUnread({ lastReadAt: 5000 }, 4000.9), 4000, 'floored to an integer');
  // min rule: an existing floor only ever moves earlier
  assert.equal(planMarkUnread({ lastReadAt: 5000, unreadFloor: 3000 }, 4000), null);
  assert.equal(planMarkUnread({ lastReadAt: 5000, unreadFloor: 3000 }, 2000), 2000);
  for (const bad of [0, -5, Number.NaN, Infinity, '1000']) {
    assert.equal(planMarkUnread({ lastReadAt: 5000 }, bad), null);
  }
});

test('markUnreadForGateway / markSpaceReadForGateway round trip through the in-memory ctx', async (t) => {
  fake.withServiceKey(t);
  const marker = { spaceRef: 'space_1', externalOrgId: fake.ORG, externalAuthId: fake.AUTH, lastReadAt: 5000 };
  const { ctx, tables } = fake.makeCtx(fake.memberTables({ spaceReadMarkers: [marker] }));
  const base = { serviceKey: fake.SERVICE_KEY, externalAuthId: fake.AUTH, externalOrgId: fake.ORG, spaceRef: 'space_1' };

  const view = await markUnreadForGateway._handler(ctx, { ...base, beforeMs: 4000 });
  assert.deepEqual(view, { lastReadAt: 4000, readThroughAt: 5000, manualUnread: true });
  assert.equal(tables.spaceReadMarkers[0].unreadFloor, 4000);

  // a poll while the floor is set changes nothing
  const poll = await markSpaceReadForGateway._handler(ctx, base);
  assert.deepEqual(poll, { lastReadAt: 4000, readThroughAt: 5000, manualUnread: true });
  assert.equal(tables.spaceReadMarkers[0].unreadFloor, 4000);

  const readback = await spaceReadMarkerForGateway._handler(ctx, base);
  assert.equal(readback.manualUnread, true);

  // entering the room clears it
  const entered = await markSpaceReadForGateway._handler(ctx, { ...base, reason: 'enter' });
  assert.equal(entered.manualUnread, false);
  assert.ok(entered.lastReadAt >= 5000);
  assert.equal('unreadFloor' in tables.spaceReadMarkers[0], false);

  // no marker for another room: markUnread is ignored
  const none = await markUnreadForGateway._handler(ctx, { ...base, spaceRef: 'space_2', beforeMs: 1 });
  assert.deepEqual(none, { lastReadAt: null, readThroughAt: null, manualUnread: false });
});
