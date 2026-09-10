const assert = require('node:assert/strict');
const test = require('node:test');

const {
  presentMembers,
  normalizeStatus,
  PRESENCE_TTL_MS,
  TYPING_TTL_MS,
} = require('../convex/spacePresence.ts');

/**
 * The pure decision behind `recordSpacePresenceForGateway` (presence in a
 * room). Tested directly, the way every Space-side decision in this package
 * is: the handler is a thin shell around it plus the auth checks and indexed
 * reads a unit test cannot exercise here.
 */

const NOW = Date.parse('2026-09-08T12:00:00Z');
const row = (id, agoMs, status = 'online') => ({
  externalAuthId: id,
  status,
  updatedAt: NOW - agoMs,
});

test('a fresh heartbeat is present; the viewer is never in their own list', () => {
  const present = presentMembers([row('kari', 1_000), row('me', 1_000)], {
    now: NOW,
    viewerAuthId: 'me',
  });
  assert.deepEqual(
    present.map((p) => p.subject_id),
    ['kari'],
  );
});

// A browser that crashes or sleeps sends no goodbye, so absence has to be
// derived from staleness rather than believed from a stored value.
test('a heartbeat older than the presence window is gone, whatever it says', () => {
  const present = presentMembers([row('kari', PRESENCE_TTL_MS + 1, 'typing')], { now: NOW });
  assert.deepEqual(present, []);
});

test('typing decays to present rather than vanishing when it goes stale', () => {
  const [still] = presentMembers([row('kari', TYPING_TTL_MS + 1_000, 'typing')], { now: NOW });
  assert.equal(still.subject_id, 'kari');
  assert.equal(still.status, 'online', 'stopped typing, still in the room');

  const [writing] = presentMembers([row('ola', 1_000, 'typing')], { now: NOW });
  assert.equal(writing.status, 'typing');
});

test('an explicit goodbye is not present, and neither is the older schema’s "away"', () => {
  const present = presentMembers([row('kari', 500, 'offline'), row('ola', 500, 'away')], {
    now: NOW,
  });
  assert.deepEqual(present, []);
});

// A clock skewed into the future would otherwise read as permanently fresh.
test('a timestamp in the future, or an unreadable one, is not treated as here', () => {
  const present = presentMembers(
    [row('ahead', -60_000), { externalAuthId: 'broken', status: 'online', updatedAt: NaN }],
    { now: NOW },
  );
  assert.deepEqual(present, []);
});

// The line is re-rendered every six seconds; ordering by recency would make it
// reshuffle while nobody's state had actually changed.
test('the order is stable, not by recency', () => {
  const present = presentMembers([row('ola', 500), row('anne', 5_000), row('kari', 100)], {
    now: NOW,
  });
  assert.deepEqual(
    present.map((p) => p.subject_id),
    ['anne', 'kari', 'ola'],
  );
});

test('a blank identifier is dropped rather than rendered as an anonymous presence', () => {
  assert.deepEqual(presentMembers([row('   ', 500)], { now: NOW }), []);
});

test('only the three statuses a caller may send are accepted', () => {
  assert.equal(normalizeStatus(undefined), 'online');
  assert.equal(normalizeStatus(''), 'online');
  assert.equal(normalizeStatus(' TYPING '), 'typing');
  assert.equal(normalizeStatus('offline'), 'offline');
  assert.equal(normalizeStatus('away'), null, 'not a status this route offers');
  assert.equal(normalizeStatus('anything'), null);
});
