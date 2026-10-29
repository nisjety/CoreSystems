const assert = require('node:assert/strict');
const test = require('node:test');

const {
  normalizeAuthorityResponse,
  parseOptions,
  run,
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

test('reconciliation reads canonical memberships from Auth with a dedicated principal', async () => {
  const calls = [];
  const fetchImpl = async (url, init = {}) => {
    calls.push({ url, init });
    if (calls.length === 1) {
      return {
        ok: true,
        json: async () => ({
          version: 'v1',
          organizationId: 'org-1',
          members: [
            { user_id: 'user-1', role: 'owner', status: 'active' },
          ],
        }),
      };
    }
    return { ok: true, json: async () => ({ applied: false }) };
  };

  await run(
    ['--org', 'org-1'],
    {
      CONTROL_PLANE_AUTH_CORE_URL: 'http://auth-core:3011',
      APPLICATION_RECONCILER_AUTH_TOKEN:
        '0123456789abcdef0123456789abcdef',
      CONVEX_HTTP_ACTIONS_URL: 'http://convex:3211',
      CONVEX_RECONCILIATION_KEY: 'operator-reconciliation-key',
    },
    fetchImpl,
  );

  assert.equal(
    calls[0].url,
    'http://auth-core:3011/api/v1/internal/membership/organizations/org-1/members',
  );
  assert.deepEqual(calls[0].init.headers, {
    'X-Service-Id': 'application-reconciler',
    'X-Service-Token': '0123456789abcdef0123456789abcdef',
  });
  assert.equal(calls[0].init.redirect, 'manual');
  assert.equal(calls[0].init.headers['X-Internal-Api-Key'], undefined);
  assert.deepEqual(JSON.parse(calls[1].init.body).authoritativeMembers, [
    { userId: 'user-1', role: 'owner' },
  ]);
});
