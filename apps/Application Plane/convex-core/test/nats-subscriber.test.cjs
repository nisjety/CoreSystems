const assert = require('node:assert/strict');
const test = require('node:test');

process.env.CONVEX_CONTROL_PROJECTION_KEY = 'test-convex-projection-key';
process.env.NATS_USER = 'application-convex-control';
process.env.NATS_PASSWORD = '0123456789abcdef0123456789abcdef';

const {
  CONTROL_PLANE_SUBJECTS,
  ConvexNatsSubscriber,
  controlPlaneConnectionOptions,
  modelPlaneConnectionOptions,
  normalizeControlPlaneEvent,
  processJetStreamMessage,
} = require('../nats-subscriber.js');

test('Control Plane connection requires its scoped principal and never accepts a token', () => {
  assert.deepEqual(
    controlPlaneConnectionOptions({
      url: 'nats://control-shared-nats:4222',
      user: 'application-convex-control',
      password: '0123456789abcdef0123456789abcdef',
      legacyToken: 'must-not-be-used',
    }),
    {
      servers: ['nats://control-shared-nats:4222'],
      user: 'application-convex-control',
      pass: '0123456789abcdef0123456789abcdef',
      name: 'convex-subscriber-control',
      inboxPrefix: '_INBOX.APPLICATION_CONVEX_CONTROL',
      maxReconnectAttempts: 10,
      reconnectDelayMs: 2000,
    },
  );
  assert.throws(
    () => controlPlaneConnectionOptions({
      url: 'nats://control-shared-nats:4222',
      user: '',
      password: '',
      legacyToken: 'old-token',
    }),
    /scoped user\/password/i,
  );
});

test('Model Plane connection requires scoped user/password and never falls back to a token', () => {
  assert.deepEqual(
    modelPlaneConnectionOptions({
      url: 'nats://model-nats:4222',
      user: 'application-convex-model',
      password: '0123456789abcdef0123456789abcdef',
      legacyToken: 'must-not-be-used',
    }),
    {
      servers: ['nats://model-nats:4222'],
      user: 'application-convex-model',
      pass: '0123456789abcdef0123456789abcdef',
      name: 'convex-subscriber-mp',
      maxReconnectAttempts: 10,
      reconnectDelayMs: 2000,
    },
  );
  assert.throws(
    () => modelPlaneConnectionOptions({
      url: 'nats://model-nats:4222',
      user: '',
      password: '',
      legacyToken: 'old-token',
    }),
    /scoped user\/password/i,
  );
});

test('Application compose no longer wires the removed Model Plane token', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const planeSource = fs.readFileSync(
    path.join(__dirname, '..', '..', 'docker-compose.yml'),
    'utf8',
  );
  const standaloneSource = fs.readFileSync(
    path.join(__dirname, '..', 'docker-compose.yml'),
    'utf8',
  );
  for (const source of [planeSource, standaloneSource]) {
    assert.doesNotMatch(source, /MODEL_PLANE_NATS_TOKEN/);
    assert.match(source, /MODEL_PLANE_NATS_USER: application-convex-model/);
    assert.match(source, /APPLICATION_CONVEX_MODEL_NATS_PASSWORD/);
  }
  assert.match(planeSource, /MODEL_PLANE_NATS_USER: application-insight-model/);
  assert.match(planeSource, /APPLICATION_INSIGHT_MODEL_NATS_PASSWORD/);
});

test('uses the two Auth-canonical revisioned Control Plane subjects', () => {
  assert.equal(
    CONTROL_PLANE_SUBJECTS.organizationChanged,
    'aqencia.controlplane.org.changed',
  );
  assert.equal(
    CONTROL_PLANE_SUBJECTS.memberChanged,
    'aqencia.controlplane.org.member_changed',
  );
  assert.deepEqual(Object.keys(CONTROL_PLANE_SUBJECTS).sort(), [
    'memberChanged',
    'organizationChanged',
  ]);
});

test('subscriber uses pre-provisioned Control consumers and has no token/admin path', () => {
  const source = require('node:fs').readFileSync(
    require('node:path').join(__dirname, '..', 'nats-subscriber.js'),
    'utf8',
  );
  assert.doesNotMatch(source, /NATS_TOKEN|VEREVON_NATS_TOKEN/);
  assert.doesNotMatch(source, /CONVEX_API_KEY|INTERNAL_API_KEY/);
  assert.match(source, /CONVEX_CONTROL_PROJECTION_KEY/);
  assert.doesNotMatch(source, /jetstreamManager|streams\.add|consumers\.add/);
  assert.match(source, /options\.bind\(CONTROL_PLANE_STREAM/);
  assert.match(source, /options\.queue\(durableName\)/);
  assert.match(source, /options\.manualAck\(\)/);
  assert.match(source, /scoped Ingestion mirror is disabled/);
});

test('normalizes canonical membership upsert and removal by revision, not time', () => {
  assert.deepEqual(
    normalizeControlPlaneEvent('memberChanged', {
      schema_version: 1,
      event_id: 'organization:org-1:member:user-1:4:upsert',
      action: 'upsert',
      org_id: 'org-1',
      user_id: 'user-1',
      user_email: ' User@One.Example ',
      role: 'member',
      revision: 4,
      organization_revision: 2,
      _published_at: '2026-07-13T08:00:00.000Z',
      _source: 'auth-core',
    }),
    {
      action: 'upsert',
      eventId: 'organization:org-1:member:user-1:4:upsert',
      orgId: 'org-1',
      userId: 'user-1',
      email: 'user@one.example',
      role: 'member',
      revision: 4,
      organizationRevision: 2,
    },
  );
  assert.deepEqual(
    normalizeControlPlaneEvent('memberChanged', {
      schema_version: 1,
      event_id: 'organization:org-1:member:user-1:5:remove',
      action: 'remove',
      org_id: 'org-1',
      user_id: 'user-1',
      revision: 5,
      organization_revision: 2,
      _source: 'auth-core',
    }),
    {
      action: 'remove',
      eventId: 'organization:org-1:member:user-1:5:remove',
      orgId: 'org-1',
      userId: 'user-1',
      revision: 5,
      organizationRevision: 2,
    },
  );
});

test('normalizes canonical organization upsert and permanent removal', () => {
  assert.deepEqual(
    normalizeControlPlaneEvent('organizationChanged', {
      schema_version: 1,
      event_id: 'organization:org-2:7:upsert',
      action: 'upsert',
      org_id: 'org-2',
      name: 'Org Two',
      slug: 'org-two',
      revision: 7,
      _source: 'auth-core',
      _published_at: '2099-01-01T00:00:00.000Z',
    }),
    {
      action: 'upsert',
      eventId: 'organization:org-2:7:upsert',
      orgId: 'org-2',
      name: 'Org Two',
      slug: 'org-two',
      revision: 7,
    },
  );
  assert.deepEqual(
    normalizeControlPlaneEvent('organizationChanged', {
      schema_version: 1,
      event_id: 'organization:org-2:8:deleted',
      action: 'remove',
      org_id: 'org-2',
      revision: 8,
      _source: 'auth-core',
    }),
    {
      action: 'remove',
      eventId: 'organization:org-2:8:deleted',
      orgId: 'org-2',
      revision: 8,
    },
  );
});

test('fails closed for forged, unversioned, unsafe, or incomplete projection events', () => {
  const valid = {
    schema_version: 1,
    event_id: 'organization:org-1:member:user-1:1:upsert',
    action: 'upsert',
    org_id: 'org-1',
    user_id: 'user-1',
    user_email: 'user@example.invalid',
    role: 'member',
    revision: 1,
    organization_revision: 1,
    _source: 'auth-core',
  };
  assert.throws(
    () => normalizeControlPlaneEvent('memberChanged', { ...valid, _source: 'org-core' }),
    /source/i,
  );
  assert.throws(
    () => normalizeControlPlaneEvent('memberChanged', { ...valid, revision: 0 }),
    /revision/i,
  );
  assert.throws(
    () => normalizeControlPlaneEvent('memberChanged', { ...valid, revision: Number.MAX_SAFE_INTEGER + 1 }),
    /revision/i,
  );
  assert.throws(
    () => normalizeControlPlaneEvent('memberChanged', { ...valid, user_email: undefined }),
    /user_email/i,
  );
  assert.throws(
    () => normalizeControlPlaneEvent('memberChanged', { ...valid, schema_version: 2 }),
    /schema_version/i,
  );
});

test('canonical membership projection propagates failure for redelivery', async () => {
  const subscriber = new ConvexNatsSubscriber();
  subscriber.callConvexMutation = async () => {
    throw new Error('projection unavailable');
  };

  await assert.rejects(
    subscriber.handleMemberChanged({
      schema_version: 1,
      event_id: 'organization:org-1:member:user-1:2:remove',
      action: 'remove',
      org_id: 'org-1',
      user_id: 'user-1',
      revision: 2,
      organization_revision: 1,
      _source: 'auth-core',
    }),
    /projection unavailable/,
  );
});

function jetStreamMessage(redeliveryCount = 1) {
  return {
    subject: CONTROL_PLANE_SUBJECTS.memberChanged,
    data: new TextEncoder().encode(JSON.stringify({
      org_id: 'org-1',
      user_id: 'user-1',
      user_email: 'must-not-enter-dlq@example.test',
      revision: 2,
      organization_revision: 1,
      action: 'remove',
      schema_version: 1,
      event_id: 'organization:org-1:member:user-1:2:remove',
      _source: 'auth-core',
    })),
    info: { redeliveryCount },
    ackCalls: 0,
    nakCalls: 0,
    termCalls: 0,
    ack() { this.ackCalls += 1; },
    nak() { this.nakCalls += 1; },
    term() { this.termCalls += 1; },
  };
}

test('acks a durable event only after the projection succeeds', async () => {
  const message = jetStreamMessage();
  const result = await processJetStreamMessage(
    message,
    async () => {},
    async () => assert.fail('DLQ must not be used for a successful event'),
  );
  assert.equal(result, 'applied');
  assert.equal(message.ackCalls, 1);
  assert.equal(message.nakCalls, 0);
});

test('naks a failed projection so JetStream redelivers it', async () => {
  const message = jetStreamMessage(2);
  const result = await processJetStreamMessage(
    message,
    async () => { throw new Error('projection unavailable'); },
    async () => assert.fail('DLQ threshold has not been reached'),
  );
  assert.equal(result, 'retrying');
  assert.equal(message.ackCalls, 0);
  assert.equal(message.nakCalls, 1);
  assert.equal(message.termCalls, 0);
});

test('dead-letters a repeatedly failing event without storing email', async () => {
  const message = jetStreamMessage(5);
  let deadLetter;
  const result = await processJetStreamMessage(
    message,
    async () => { throw new Error('invalid projection'); },
    async (payload) => { deadLetter = payload; },
  );
  assert.equal(result, 'dead_lettered');
  assert.equal(message.termCalls, 1);
  assert.equal(message.nakCalls, 0);
  assert.equal(deadLetter.payload.org_id, 'org-1');
  assert.equal(deadLetter.payload.user_id, 'user-1');
  assert.equal(deadLetter.payload.user_email, undefined);
  assert.equal(JSON.stringify(deadLetter).includes('must-not-enter-dlq'), false);
});
