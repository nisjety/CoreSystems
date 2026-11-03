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
    /projection key is not configured/,
  );
});

test('internal webhook authentication rejects missing, malformed, and incorrect bearer tokens', async () => {
  const env = { CONVEX_CONTROL_PROJECTION_KEY: 'correct-key' };
  assert.equal(await authorizeInternalRequest(request(), env), false);
  assert.equal(await authorizeInternalRequest(request('Basic correct-key'), env), false);
  assert.equal(await authorizeInternalRequest(request('Bearer wrong-key'), env), false);
});

test('internal webhook authentication accepts the dedicated service key', async () => {
  assert.equal(
    await authorizeInternalRequest(
      request('Bearer correct-key'),
      { CONVEX_CONTROL_PROJECTION_KEY: 'correct-key' },
    ),
    true,
  );
});

test('the shared fleet and Convex-internal keys cannot invoke Control projection handlers', async () => {
  const env = {
    CONVEX_CONTROL_PROJECTION_KEY: 'projection-key',
    CONVEX_INTERNAL_SERVICE_KEY: 'convex-internal-key',
    INTERNAL_API_KEY: 'shared-fleet-key',
  };
  assert.equal(
    await authorizeInternalRequest(request('Bearer shared-fleet-key'), env),
    false,
  );
  assert.equal(
    await authorizeInternalRequest(request('Bearer convex-internal-key'), env),
    false,
  );
  assert.equal(
    await authorizeInternalRequest(request('Bearer projection-key'), env),
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

// --- WS11-S6: the Space event webhook ----------------------------------------------------------

function spaceEventRequest(authorization) {
  const headers = { 'Content-Type': 'application/json' };
  if (authorization !== undefined) headers.Authorization = authorization;
  return new Request('http://convex.test/api/webhook/nats/onSpaceEvent', {
    method: 'POST',
    headers,
    body: JSON.stringify({ event_id: 'evt_1' }),
  });
}

function withProjectionKey(t, value = 'projection-key') {
  const prior = process.env.CONVEX_CONTROL_PROJECTION_KEY;
  if (value === null) delete process.env.CONVEX_CONTROL_PROJECTION_KEY;
  else process.env.CONVEX_CONTROL_PROJECTION_KEY = value;
  t.after(() => {
    if (prior === undefined) delete process.env.CONVEX_CONTROL_PROJECTION_KEY;
    else process.env.CONVEX_CONTROL_PROJECTION_KEY = prior;
  });
}

function recordingCtx(result = { status: 'applied' }) {
  const calls = [];
  return {
    calls,
    ctx: {
      async runMutation(fn, args) {
        calls.push(['mutation', args]);
        return result;
      },
      async runAction(fn, args) {
        calls.push(['action', args]);
        return result;
      },
    },
  };
}

test('onSpaceEvent requires the Control projection key: no header, a wrong key and other keys are 401 and call nothing', async (t) => {
  withProjectionKey(t);
  process.env.INTERNAL_API_KEY = 'shared-fleet-key';
  t.after(() => { delete process.env.INTERNAL_API_KEY; });
  const { natsWebhook } = require('../convex/http.ts');
  for (const authorization of [undefined, 'Bearer wrong', 'Basic projection-key', 'Bearer shared-fleet-key']) {
    const { ctx, calls } = recordingCtx();
    const response = await natsWebhook._handler(ctx, spaceEventRequest(authorization));
    assert.equal(response.status, 401, String(authorization));
    assert.deepEqual(calls, []);
  }
});

test('onSpaceEvent with the projection key passes the envelope to the ingest and answers 200 with the status only', async (t) => {
  withProjectionKey(t);
  const { natsWebhook } = require('../convex/http.ts');
  const { ctx, calls } = recordingCtx({ status: 'rejected', reason: 'zdr' });
  const response = await natsWebhook._handler(ctx, spaceEventRequest('Bearer projection-key'));
  assert.equal(response.status, 200, 'a rejected event is still acknowledged');
  assert.deepEqual(calls, [['mutation', { event: { event_id: 'evt_1' } }]]);
  assert.deepEqual(await response.json(), { ok: true, handler: 'onSpaceEvent', status: 'rejected' });
});

test('onSpaceEvent answers 503 when the projection key is not configured, and never runs the ingest', async (t) => {
  withProjectionKey(t, null);
  const { natsWebhook } = require('../convex/http.ts');
  const quiet = console.error;
  console.error = () => {};
  t.after(() => { console.error = quiet; });
  const { ctx, calls } = recordingCtx();
  const response = await natsWebhook._handler(ctx, spaceEventRequest('Bearer anything'));
  assert.equal(response.status, 503);
  assert.deepEqual(calls, []);
});

test('onSpaceEvent answers 500 without the error text when the ingest throws (the subscriber redelivers)', async (t) => {
  withProjectionKey(t);
  const { natsWebhook } = require('../convex/http.ts');
  const quiet = console.error;
  console.error = () => {};
  t.after(() => { console.error = quiet; });
  const ctx = { async runMutation() { throw new Error('secret internal detail'); } };
  const response = await natsWebhook._handler(ctx, spaceEventRequest('Bearer projection-key'));
  assert.equal(response.status, 500);
  assert.equal((await response.text()).includes('secret internal detail'), false);
});
