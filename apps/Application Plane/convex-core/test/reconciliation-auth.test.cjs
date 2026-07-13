const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const test = require('node:test');

const { authorizeReconciliationRequest } = require('../convex/reconciliationAuth.ts');

function signedRequest({
  body = '{"externalOrgId":"org-1"}',
  key = 'operator-key',
  nonce = 'nonce-1234567890',
  orgId = 'org-1',
  timestamp = 1_700_000_000_000,
} = {}) {
  const path = '/api/operator/reconcile-memberships';
  const canonical = `${timestamp}\nPOST\n${path}\n${orgId}\n${nonce}\n${body}`;
  const signature = crypto.createHmac('sha256', key).update(canonical).digest('hex');
  return new Request(`http://convex.test${path}`, {
    method: 'POST',
    headers: {
      'x-reconciliation-timestamp': String(timestamp),
      'x-reconciliation-nonce': nonce,
      'x-reconciliation-org': orgId,
      'x-reconciliation-signature': signature,
    },
    body,
  });
}

test('operator reconciliation signature binds tenant, path, timestamp, nonce, and body', async () => {
  const now = 1_700_000_000_000;
  const body = '{"externalOrgId":"org-1","apply":false}';
  const authorized = await authorizeReconciliationRequest(
    signedRequest({ body, timestamp: now }),
    body,
    { CONVEX_RECONCILIATION_KEY: 'operator-key' },
    now,
  );
  assert.deepEqual(authorized, {
    authorized: true,
    nonce: 'nonce-1234567890',
    orgId: 'org-1',
    timestamp: now,
  });

  const tampered = await authorizeReconciliationRequest(
    signedRequest({ body, timestamp: now }),
    `${body} `,
    { CONVEX_RECONCILIATION_KEY: 'operator-key' },
    now,
  );
  assert.equal(tampered.authorized, false);
});

test('operator reconciliation auth fails closed for missing config and stale requests', async () => {
  const now = 1_700_000_000_000;
  const request = signedRequest({ timestamp: now - 301_000 });
  await assert.rejects(
    authorizeReconciliationRequest(request, '{"externalOrgId":"org-1"}', {}, now),
    /not configured/,
  );
  const stale = await authorizeReconciliationRequest(
    request,
    '{"externalOrgId":"org-1"}',
    { CONVEX_RECONCILIATION_KEY: 'operator-key' },
    now,
  );
  assert.equal(stale.authorized, false);
});
