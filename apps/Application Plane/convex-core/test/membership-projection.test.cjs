const assert = require('node:assert/strict');
const test = require('node:test');

const {
  normalizeProjectionRole,
  shouldApplyMembershipAdd,
  shouldApplyMembershipRemoval,
} = require('../convex/membershipProjection.ts');

test('a removal tombstone blocks duplicate or older member-added events', () => {
  assert.equal(shouldApplyMembershipAdd(100, undefined, 100), false);
  assert.equal(shouldApplyMembershipAdd(100, undefined, 99), false);
  assert.equal(shouldApplyMembershipAdd(100, undefined, 101), true);
});

test('a newer membership update blocks an older add or removal', () => {
  assert.equal(shouldApplyMembershipAdd(undefined, 200, 199), false);
  assert.equal(shouldApplyMembershipRemoval(200, 199), false);
  assert.equal(shouldApplyMembershipRemoval(200, 200), true);
});

test('Control Plane roles are normalized without widening privileges', () => {
  assert.equal(normalizeProjectionRole('owner'), 'admin');
  assert.equal(normalizeProjectionRole('admin'), 'admin');
  assert.equal(normalizeProjectionRole('member'), 'member');
  assert.equal(normalizeProjectionRole('viewer'), 'viewer');
  assert.throws(() => normalizeProjectionRole('superuser'), /Unsupported/);
});
