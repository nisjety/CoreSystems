const assert = require('node:assert/strict');
const test = require('node:test');

const {
  buildNotificationIdempotencyKey,
  postNotification,
} = require('../dist/activities/notification-client.js');

test('posts the canonical Application Plane notification contract', async () => {
  let captured;
  const fetchImpl = async (url, init) => {
    captured = { url, init };
    return Response.json(
      { request_id: 'req_submitted', status: 'submitted' },
      { status: 202 },
    );
  };

  await postNotification(
    {
      baseUrl: 'http://notification-core:8080/',
      serviceToken: 'support-worker-test-secret-at-least-32-bytes',
      now: () => new Date('2026-07-13T12:00:00.000Z'),
      nonce: () => 'fixed-nonce-1234567890',
      request: {
        organization_id: 'org-1',
        idempotency_key: 'support:ticket.triaged:ticket:42:recipient:7',
        retention_mode: 'zdr',
        recipient: { kind: 'user', id: 'control-user-7' },
        type: 'ticket.triaged',
        payload: { ticketId: 42, message: 'Ready' },
      },
    },
    fetchImpl,
  );

  assert.equal(
    captured.url,
    'http://notification-core:8080/api/v1/notification-requests',
  );
  assert.equal(captured.init.method, 'POST');
  assert.equal(captured.init.redirect, 'manual');
  assert.ok(captured.init.signal instanceof AbortSignal);
  assert.equal(captured.init.headers['X-Service-Id'], 'support-worker');
  assert.equal(captured.init.headers['X-Org-Id'], 'org-1');
  assert.equal(captured.init.headers['X-Delegation-Nonce'], 'fixed-nonce-1234567890');
  assert.ok(captured.init.headers['X-Delegation-Signature']);
  assert.equal(captured.init.headers['X-Internal-Api-Key'], undefined);
  assert.deepEqual(JSON.parse(captured.init.body), {
    organization_id: 'org-1',
    idempotency_key: 'support:ticket.triaged:ticket:42:recipient:7',
    retention_mode: 'zdr',
    recipient: { kind: 'user', id: 'control-user-7' },
    type: 'ticket.triaged',
    payload: { ticketId: 42, message: 'Ready' },
  });
});

test('rejects an empty 202 response because submission was not proven', async () => {
  const fetchImpl = async () => new Response(null, { status: 202 });

  await assert.rejects(
    postNotification(
      {
        baseUrl: 'http://notification-core:8080',
        serviceToken: 'support-worker-test-secret-at-least-32-bytes',
        request: {
          organization_id: 'org-1',
          idempotency_key: 'support:ticket.triaged:ticket:42:recipient:7',
          retention_mode: 'zdr',
          recipient: { kind: 'user', id: 'control-user-7' },
          type: 'ticket.triaged',
          payload: { ticketId: 42 },
        },
      },
      fetchImpl,
    ),
    /missing submitted or suppressed status/,
  );
});

test('propagates non-2xx responses without logging a provider response body', async () => {
  const fetchImpl = async () =>
    new Response('sensitive downstream details', {
      status: 503,
      statusText: 'Service Unavailable',
    });

  await assert.rejects(
    postNotification(
      {
        baseUrl: 'http://notification-core:8080',
        serviceToken: 'support-worker-test-secret-at-least-32-bytes',
        request: {
          organization_id: 'org-1',
          idempotency_key: 'support:csat.survey:ticket:42',
          retention_mode: 'zdr',
          recipient: { kind: 'user', id: 'control-user-42' },
          type: 'csat.survey',
          payload: { ticketId: 42 },
        },
      },
      fetchImpl,
    ),
    (error) => {
      assert.match(error.message, /503 Service Unavailable/);
      assert.doesNotMatch(error.message, /sensitive downstream details/);
      return true;
    },
  );
});

test('rejects a failed notification status even when the HTTP response is 202', async () => {
  const fetchImpl = async () =>
    Response.json({ request_id: 'req_failed', status: 'failed' }, { status: 202 });

  await assert.rejects(
    postNotification(
      {
        baseUrl: 'http://notification-core:8080',
        serviceToken: 'support-worker-test-secret-at-least-32-bytes',
        request: {
          organization_id: 'org-1',
          idempotency_key: 'support:ticket.triaged:ticket:42:recipient:7',
          retention_mode: 'zdr',
          recipient: { kind: 'user', id: 'control-user-7' },
          type: 'ticket.triaged',
          payload: { ticketId: 42 },
        },
      },
      fetchImpl,
    ),
    /returned non-delivery status: failed/,
  );
});

test('builds deterministic keys from the resolved Control recipient without embedding identifiers', () => {
  assert.equal(
    buildNotificationIdempotencyKey({
      type: 'csat.survey',
      ticketId: 42,
    }),
    'support:csat.survey:ticket:42',
  );
  const key = buildNotificationIdempotencyKey({
    type: 'ticket.triaged',
    ticketId: 42,
    controlUserId: 'control-user-7',
  });
  assert.match(key, /^support:ticket\.triaged:ticket:42:recipient:[a-f0-9]{16}$/);
  assert.doesNotMatch(key, /control-user-7/);
  assert.equal(key, buildNotificationIdempotencyKey({
    type: 'ticket.triaged',
    ticketId: 42,
    controlUserId: 'control-user-7',
  }));
});
