const assert = require('node:assert/strict');
const test = require('node:test');

const { planMembershipReconciliation } = require('../convex/reconciliation.ts');

test('reconciliation removes excess access and never infers grants from Convex', () => {
  const plan = planMembershipReconciliation(
    [
      { userId: 'user-1', role: 'admin', syncStatus: 'synced' },
      { userId: 'user-2', role: 'member', syncStatus: 'synced' },
      { userId: 'user-deleted', role: 'viewer', syncStatus: 'deleted' },
    ],
    [
      { userId: 'user-1', role: 'viewer' },
      { userId: 'user-3', role: 'member' },
    ],
  );

  assert.deepEqual(plan.removals, ['user-2']);
  assert.deepEqual(plan.roleChanges, [
    { userId: 'user-1', previousRole: 'admin', nextRole: 'viewer' },
  ]);
  assert.deepEqual(plan.missing, ['user-3']);
  assert.equal(plan.grants.length, 0);
  assert.deepEqual(plan.unsafePromotions, []);
});

test('reconciliation reports privilege widening without applying it', () => {
  const plan = planMembershipReconciliation(
    [{ userId: 'user-1', role: 'viewer', syncStatus: 'synced' }],
    [{ userId: 'user-1', role: 'admin' }],
  );

  assert.deepEqual(plan.roleChanges, []);
  assert.deepEqual(plan.unsafePromotions, [
    { userId: 'user-1', previousRole: 'viewer', requestedRole: 'admin' },
  ]);
});

test('duplicate authority rows are rejected instead of producing an ambiguous plan', () => {
  assert.throws(
    () => planMembershipReconciliation([], [
      { userId: 'user-1', role: 'member' },
      { userId: 'user-1', role: 'viewer' },
    ]),
    /Duplicate authoritative membership/,
  );
});
