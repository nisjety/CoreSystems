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
