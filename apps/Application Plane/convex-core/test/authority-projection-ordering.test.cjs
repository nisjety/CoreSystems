const assert = require('node:assert/strict');
const test = require('node:test');

const {
  applyMembershipProjectionState,
  applyOrganizationProjectionState,
} = require('../convex/authorityProjection.ts');

function permutations(values) {
  if (values.length <= 1) return [values];
  return values.flatMap((value, index) =>
    permutations(values.filter((_, candidate) => candidate !== index)).map(
      (rest) => [value, ...rest],
    ),
  );
}

test('all membership delivery orders converge to the highest removal revision', () => {
  const events = [
    { action: 'upsert', revision: 1, eventId: 'add-1', fingerprint: 'member' },
    { action: 'upsert', revision: 2, eventId: 'role-2', fingerprint: 'admin' },
    { action: 'remove', revision: 3, eventId: 'remove-3', fingerprint: 'remove' },
  ];
  for (const order of permutations(events)) {
    const finalState = order.reduce(applyMembershipProjectionState, undefined);
    assert.deepEqual(finalState, {
      kind: 'removed',
      revision: 3,
      eventId: 'remove-3',
      fingerprint: 'remove',
    });
  }
});

test('a higher membership re-add supersedes a removal but equal conflicts fail', () => {
  const removed = applyMembershipProjectionState(undefined, {
    action: 'remove', revision: 3, eventId: 'remove-3', fingerprint: 'remove',
  });
  const readded = applyMembershipProjectionState(removed, {
    action: 'upsert', revision: 4, eventId: 'add-4', fingerprint: 'member',
  });
  assert.equal(readded.kind, 'active');
  assert.equal(readded.revision, 4);
  assert.deepEqual(
    applyMembershipProjectionState(readded, {
      action: 'upsert', revision: 4, eventId: 'add-4', fingerprint: 'member',
    }),
    readded,
  );
  assert.throws(
    () => applyMembershipProjectionState(readded, {
      action: 'remove', revision: 4, eventId: 'forged-4', fingerprint: 'remove',
    }),
    /conflict/i,
  );
});

test('organization removal is permanent and cannot be resurrected', () => {
  const active = applyOrganizationProjectionState(undefined, {
    action: 'upsert', revision: 7, eventId: 'upsert-7', fingerprint: 'org-seven',
  });
  const removed = applyOrganizationProjectionState(active, {
    action: 'remove', revision: 8, eventId: 'remove-8', fingerprint: 'remove',
  });
  assert.equal(removed.kind, 'removed');
  assert.deepEqual(
    applyOrganizationProjectionState(removed, {
      action: 'upsert', revision: 9, eventId: 'upsert-9', fingerprint: 'forged',
    }),
    removed,
  );
});
