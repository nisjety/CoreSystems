const assert = require('node:assert/strict');
const test = require('node:test');

const {
  COURIER_AUDIENCE,
  COURIER_PATH,
  COURIER_SERVICE_ID,
  COURIER_TYPES,
  buildCourierRequest,
  courierCanonical,
  courierDeepLink,
  signCourierRequest,
} = require('../convex/spaceAttentionCourier.ts');

function signal(overrides = {}) {
  return {
    eventId: 'evt_1',
    externalOrgId: 'org_ext',
    recipientAuthId: 'alice',
    spaceRef: 'space_1',
    kind: 'mention',
    threadId: 'thread_1',
    messageId: 'msg_1',
    actorKind: 'user',
    ...overrides,
  };
}

test('the courier names its principal, audience and endpoint', () => {
  assert.equal(COURIER_SERVICE_ID, 'space-attention');
  assert.equal(COURIER_AUDIENCE, 'notification-core');
  assert.equal(COURIER_PATH, '/api/v1/notification-requests');
});

test('every attention signal maps to a space.* type and nothing else does', () => {
  assert.deepEqual(Object.keys(COURIER_TYPES).sort(), [
    'agent_awaiting',
    'agent_done',
    'agent_failed',
    'approval_needed',
    'invite',
    'mention',
    'reminder',
    'reply',
  ]);
  for (const [kind, type] of Object.entries(COURIER_TYPES)) assert.equal(type, `space.${kind}`);
});

test('buildCourierRequest is refs-only: ids and closed words, no title and no text', () => {
  const request = buildCourierRequest(
    signal({ title: 'Secret plan', text: 'hello there', name: 'Alice Smith', runId: 'run_1' }),
  );
  assert.deepEqual(request, {
    organization_id: 'org_ext',
    idempotency_key: 'evt_1:alice',
    recipient: { kind: 'user', id: 'alice' },
    type: 'space.mention',
    retention_mode: 'standard',
    payload: {
      space_ref: 'space_1',
      actor_kind: 'user',
      deep_link_path: '/spaces/space_1?thread_id=thread_1#chat',
      thread_id: 'thread_1',
      message_id: 'msg_1',
      run_id: 'run_1',
    },
  });
  const text = JSON.stringify(request);
  for (const leaked of ['Secret plan', 'hello there', 'Alice Smith']) {
    assert.equal(text.includes(leaked), false, leaked);
  }
});

test('the idempotency key is <eventId>:<recipient>, so a retry cannot double-send', () => {
  const a = buildCourierRequest(signal());
  const b = buildCourierRequest(signal());
  assert.equal(a.idempotency_key, b.idempotency_key);
  assert.notEqual(a.idempotency_key, buildCourierRequest(signal({ recipientAuthId: 'bob' })).idempotency_key);
  assert.notEqual(a.idempotency_key, buildCourierRequest(signal({ eventId: 'evt_2' })).idempotency_key);
});

test('an unknown kind builds nothing', () => {
  assert.equal(buildCourierRequest(signal({ kind: 'party' })), null);
  assert.equal(buildCourierRequest(signal({ kind: '' })), null);
});

test('optional ids are left out when absent, and the link falls back to the room', () => {
  const request = buildCourierRequest(signal({ threadId: undefined, messageId: undefined, kind: 'invite' }));
  assert.deepEqual(request.payload, {
    space_ref: 'space_1',
    actor_kind: 'user',
    deep_link_path: '/spaces/space_1#chat',
  });
  assert.equal(request.type, 'space.invite');
  assert.equal(request.retention_mode, 'standard');
});

test('deep links encode ids instead of interpolating them', () => {
  assert.equal(courierDeepLink('a b/c', 'x&y=1#'), '/spaces/a%20b%2Fc?thread_id=x%26y%3D1%23#chat');
  assert.equal(courierDeepLink('room'), '/spaces/room#chat');
});

test('courierCanonical joins "v2" and ten fields with newlines, user and role empty', () => {
  const canonical = courierCanonical({
    timestamp: '2026-11-02T10:00:00Z',
    nonce: 'n',
    method: 'POST',
    uri: COURIER_PATH,
    organizationId: 'org_ext',
    bodySha256: 'digest',
  });
  assert.equal(
    canonical,
    ['v2', 'space-attention', 'notification-core', '2026-11-02T10:00:00Z', 'n', 'POST', COURIER_PATH, '', 'org_ext', '', 'digest'].join('\n'),
  );
  assert.equal(canonical.split('\n').length, 11);
});

// Vector computed by notification-core's own delegation.Canonical + HMAC
// (apps/Application Plane/notification-core/internal/delegation/courier_vector_test.go).
// If either side changes the scheme, both tests fail together.
const VECTOR = {
  secret: 's'.repeat(43),
  body: '{"organization_id":"org_ext","type":"space.mention"}',
  now: new Date('2026-11-02T10:00:00.456Z'),
  nonceBytes: Uint8Array.from({ length: 18 }, (_, i) => i),
  nonce: 'AAECAwQFBgcICQoLDA0ODxAR',
  digest: 'hvOdQlMVNi0BY1TkhzQeR_EBUrdga-YnlQsmeIty8RU',
  signature: '0EZpEyYo_37rN8-kL155WG9K1aJOtk7MtfszOF-YCro',
};

test('signCourierRequest matches the Go-derived HMAC vector (v2 join, RawURLEncoding)', async () => {
  const headers = await signCourierRequest(VECTOR.secret, 'org_ext', VECTOR.body, VECTOR.now, VECTOR.nonceBytes);
  assert.deepEqual(headers, {
    'Content-Type': 'application/json',
    'x-service-id': 'space-attention',
    'x-org-id': 'org_ext',
    'x-delegation-timestamp': '2026-11-02T10:00:00Z',
    'x-delegation-nonce': VECTOR.nonce,
    'x-delegation-body-sha256': VECTOR.digest,
    'x-delegation-signature': VECTOR.signature,
  });
});

test('the signature is unpadded base64url and changes with the body, the org and the secret', async () => {
  const sign = (secret, org, body) => signCourierRequest(secret, org, body, VECTOR.now, VECTOR.nonceBytes);
  const base = await sign(VECTOR.secret, 'org_ext', VECTOR.body);
  for (const value of [base['x-delegation-signature'], base['x-delegation-body-sha256'], base['x-delegation-nonce']]) {
    assert.match(value, /^[A-Za-z0-9_-]+$/);
  }
  const baseSig = base['x-delegation-signature'];
  assert.notEqual((await sign(VECTOR.secret, 'org_ext', VECTOR.body + ' '))['x-delegation-signature'], baseSig);
  assert.notEqual((await sign(VECTOR.secret, 'org_other', VECTOR.body))['x-delegation-signature'], baseSig);
  assert.notEqual((await sign('t'.repeat(43), 'org_ext', VECTOR.body))['x-delegation-signature'], baseSig);
});

test('a fresh nonce is generated per request when none is given and fits the verifier pattern', async () => {
  const one = await signCourierRequest(VECTOR.secret, 'org_ext', VECTOR.body);
  const two = await signCourierRequest(VECTOR.secret, 'org_ext', VECTOR.body);
  assert.notEqual(one['x-delegation-nonce'], two['x-delegation-nonce']);
  assert.match(one['x-delegation-nonce'], /^[A-Za-z0-9_-]{16,128}$/);
  assert.match(one['x-delegation-timestamp'], /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/);
});
