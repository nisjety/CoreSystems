const assert = require('node:assert/strict');
const test = require('node:test');

const {
  normalizeAuthorityResponse,
  parseOptions,
} = require('../scripts/reconcile-memberships.cjs');

test('reconciliation CLI defaults to dry-run and requires one tenant', () => {
  assert.deepEqual(parseOptions(['--org', 'org-1']), {
    apply: false,
    orgId: 'org-1',
  });
  assert.deepEqual(parseOptions([
    '--org', 'org-1', '--apply', '--confirm-org', 'org-1',
  ]), {
    apply: true,
    orgId: 'org-1',
  });
  assert.throws(() => parseOptions(['--apply']), /--org/);
  assert.throws(
    () => parseOptions(['--org', 'org-1', '--apply']),
    /--confirm-org/,
  );
});

test('only active Control Plane memberships enter reconciliation authority', () => {
  assert.deepEqual(normalizeAuthorityResponse({ members: [
    { user_id: 'user-1', role: 'owner', status: 'active', invited_email: 'secret@example.test' },
    { user_id: 'user-2', role: 'member', status: 'suspended' },
    { user_id: '', role: 'member', status: 'active' },
  ] }), [
    { userId: 'user-1', role: 'owner' },
  ]);
});
