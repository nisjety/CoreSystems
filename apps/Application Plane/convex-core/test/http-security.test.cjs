const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const { authorizeInternalRequest } = require('../convex/internalAuth.ts');

function request(authorization) {
  const headers = authorization === undefined ? {} : { Authorization: authorization };
  return new Request('http://convex.test/api/webhook/nats/onOrganizationCreated', {
    method: 'POST',
    headers,
  });
}

test('internal webhook authentication fails closed when the service key is absent', async () => {
  await assert.rejects(
    authorizeInternalRequest(request('Bearer any'), {}),
    /service key is not configured/,
  );
});

test('internal webhook authentication rejects missing, malformed, and incorrect bearer tokens', async () => {
  const env = { CONVEX_INTERNAL_SERVICE_KEY: 'correct-key' };
  assert.equal(await authorizeInternalRequest(request(), env), false);
  assert.equal(await authorizeInternalRequest(request('Basic correct-key'), env), false);
  assert.equal(await authorizeInternalRequest(request('Bearer wrong-key'), env), false);
});

test('internal webhook authentication accepts the dedicated service key', async () => {
  assert.equal(
    await authorizeInternalRequest(
      request('Bearer correct-key'),
      { CONVEX_INTERNAL_SERVICE_KEY: 'correct-key' },
    ),
    true,
  );
});

test('internal webhook responses do not expose thrown implementation errors', () => {
  const source = fs.readFileSync(path.join(__dirname, '..', 'convex', 'http.ts'), 'utf8');
  assert.doesNotMatch(source, /err instanceof Error \? err\.message : String\(err\)/);
  assert.match(source, /Projection handler failed/);
});

test('membership projection logs do not include member email addresses', () => {
  const source = fs.readFileSync(path.join(__dirname, '..', 'convex', 'nats.ts'), 'utf8');
  assert.doesNotMatch(source, /console\.(?:log|error)\(`[^`]*\$\{email\}/);
});

test('destructive reconciliation is not exposed through the generic NATS webhook', () => {
  const source = fs.readFileSync(path.join(__dirname, '..', 'convex', 'http.ts'), 'utf8');
  assert.doesNotMatch(source, /case "reconcileOrganizationMemberships"/);
  assert.match(source, /path: "\/api\/operator\/reconcile-memberships"/);
});
