const assert = require('node:assert/strict');
const test = require('node:test');

process.env.CONVEX_API_KEY = 'test-convex-key';
process.env.NATS_TOKEN = 'test-nats-token';

const {
  CONTROL_PLANE_SUBJECTS,
  ConvexNatsSubscriber,
  normalizeControlPlaneEvent,
  processJetStreamMessage,
} = require('../nats-subscriber.js');

test('uses the canonical durable Control Plane member-removal subject', () => {
  assert.equal(
    CONTROL_PLANE_SUBJECTS.memberRemoved,
    'aqencia.controlplane.org.member_removed',
  );
});

test('subscriber has no predictable NATS credential fallback', () => {
  const source = require('node:fs').readFileSync(
    require('node:path').join(__dirname, '..', 'nats-subscriber.js'),
    'utf8',
  );
  assert.doesNotMatch(source, /process\.env\.NATS_TOKEN \|\| ["']nats["']/);
  assert.match(source, /NATS_TOKEN must be configured/);
});

test('normalizes the authoritative snake-case removal envelope', () => {
  assert.deepEqual(
    normalizeControlPlaneEvent('memberRemoved', {
      org_id: 'org-1',
      user_id: 'user-1',
      _published_at: '2026-07-13T08:00:00.000Z',
      _source: 'org-core',
    }),
    {
      orgId: 'org-1',
      userId: 'user-1',
      source: 'org-core',
      sourceUpdatedAt: Date.parse('2026-07-13T08:00:00.000Z'),
    },
  );
});

test('member removal propagates projection failure for redelivery', async () => {
  const subscriber = new ConvexNatsSubscriber();
  subscriber.callConvexMutation = async () => {
    throw new Error('projection unavailable');
  };

  await assert.rejects(
    subscriber.handleMemberRemoved({
      org_id: 'org-1',
      user_id: 'user-1',
      _published_at: '2026-07-13T08:00:00.000Z',
      _source: 'org-core',
    }),
    /projection unavailable/,
  );
});

function jetStreamMessage(redeliveryCount = 1) {
  return {
    subject: CONTROL_PLANE_SUBJECTS.memberRemoved,
    data: new TextEncoder().encode(JSON.stringify({
      org_id: 'org-1',
      user_id: 'user-1',
      user_email: 'must-not-enter-dlq@example.test',
      _published_at: '2026-07-13T08:00:00.000Z',
      _source: 'org-core',
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
