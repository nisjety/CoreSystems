const assert = require('node:assert/strict');
const test = require('node:test');

const {
  ORG_ROOM_SYNC_DEBOUNCE_MS,
  orgRoomReconcileEnabled,
  planOrgRoomSyncRequest,
  spaceMembersFromOrgRoster,
} = require('../convex/spaceMembershipSync.ts');
const crons = require('../convex/crons.ts').default;

test('only active org members become editors; invited and suspended are dropped', () => {
  const grants = spaceMembersFromOrgRoster([
    { user_id: 'u_active', status: 'active' },
    { user_id: 'u_invited', status: 'invited' },
    { user_id: 'u_suspended', status: 'suspended' },
    { user_id: 'u_removed', status: 'removed' },
    { user_id: 'u_nostatus' },
  ]);
  assert.deepEqual(grants, [{ subject_type: 'user', subject_id: 'u_active', role: 'editor' }]);
});

test('org admins and owners are not elevated: every member maps to editor', () => {
  const grants = spaceMembersFromOrgRoster([
    { user_id: 'u_admin', status: 'active', role: 'admin' },
    { user_id: 'u_owner', status: 'active', role: 'owner' },
  ]);
  assert.deepEqual(
    grants.map((g) => g.role),
    ['editor', 'editor'],
  );
});

test('the roster is deduped, trimmed, and skips blank or non-string ids', () => {
  const grants = spaceMembersFromOrgRoster([
    { user_id: 'u_a', status: 'active' },
    { user_id: '  u_a  ', status: 'active' },
    { user_id: 'u_b', status: 'active' },
    { user_id: '   ', status: 'active' },
    { user_id: 42, status: 'active' },
    { user_id: null, status: 'active' },
    null,
    undefined,
  ]);
  assert.deepEqual(
    grants.map((g) => g.subject_id),
    ['u_a', 'u_b'],
  );
  assert.ok(grants.every((g) => g.subject_type === 'user'));
});

test('an empty roster maps to an empty grant list', () => {
  assert.deepEqual(spaceMembersFromOrgRoster([]), []);
});

test('the debounce window is five seconds', () => {
  assert.equal(ORG_ROOM_SYNC_DEBOUNCE_MS, 5000);
});

test('a scheduled request that is not yet due absorbs further events', () => {
  assert.deepEqual(planOrgRoomSyncRequest({ state: 'scheduled', dueAt: 10_001 }, 10_000), { action: 'skip' });
});

test('no row schedules a run five seconds out', () => {
  assert.deepEqual(planOrgRoomSyncRequest(null, 10_000), {
    action: 'schedule',
    dueAt: 15_000,
    delayMs: 5000,
  });
});

test('a run already in flight gets a fresh scheduled run so later changes are not lost', () => {
  assert.deepEqual(planOrgRoomSyncRequest({ state: 'running', dueAt: 20_000 }, 10_000), {
    action: 'schedule',
    dueAt: 15_000,
    delayMs: 5000,
  });
});

test('an overdue scheduled row (lost run) is rescheduled, including exactly at dueAt', () => {
  assert.deepEqual(planOrgRoomSyncRequest({ state: 'scheduled', dueAt: 9_000 }, 10_000), {
    action: 'schedule',
    dueAt: 15_000,
    delayMs: 5000,
  });
  assert.equal(planOrgRoomSyncRequest({ state: 'scheduled', dueAt: 10_000 }, 10_000).action, 'schedule');
});

test('the reconcile kill switch is off when the env var is unset, empty or 0', () => {
  const prior = process.env.SPACE_ORG_ROOM_RECONCILE;
  try {
    delete process.env.SPACE_ORG_ROOM_RECONCILE;
    assert.equal(orgRoomReconcileEnabled(), false);
    for (const value of ['', '0', 'false', 'off', 'yes', '2']) {
      process.env.SPACE_ORG_ROOM_RECONCILE = value;
      assert.equal(orgRoomReconcileEnabled(), false, `value ${JSON.stringify(value)} must keep it off`);
    }
  } finally {
    if (prior === undefined) delete process.env.SPACE_ORG_ROOM_RECONCILE;
    else process.env.SPACE_ORG_ROOM_RECONCILE = prior;
  }
});

test('the reconcile kill switch is on for 1 and true, read at call time', () => {
  const prior = process.env.SPACE_ORG_ROOM_RECONCILE;
  try {
    for (const value of ['1', 'true', 'TRUE', ' true ']) {
      process.env.SPACE_ORG_ROOM_RECONCILE = value;
      assert.equal(orgRoomReconcileEnabled(), true, `value ${JSON.stringify(value)} must turn it on`);
    }
    process.env.SPACE_ORG_ROOM_RECONCILE = '0';
    assert.equal(orgRoomReconcileEnabled(), false);
  } finally {
    if (prior === undefined) delete process.env.SPACE_ORG_ROOM_RECONCILE;
    else process.env.SPACE_ORG_ROOM_RECONCILE = prior;
  }
});

test('crons.ts registers the org-room roster reconcile every 10 minutes', () => {
  const job = crons.crons['space org-room roster reconcile'];
  assert.ok(job, 'cron "space org-room roster reconcile" is not registered');
  assert.equal(job.name, 'spaceMembershipSync:syncAllOrganizationRooms');
  assert.deepEqual(job.schedule, { type: 'interval', minutes: 10 });
});
