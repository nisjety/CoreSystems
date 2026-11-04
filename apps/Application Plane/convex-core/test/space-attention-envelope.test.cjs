const assert = require('node:assert/strict');
const test = require('node:test');

const {
  MAX_EVENT_ID_CHARS,
  MAX_MENTIONS,
  isSubjectSafeSpaceRef,
  parseSpaceEvent,
  planMessageRecipients,
} = require('../convex/spaceAttentionEnvelope.ts');

function envelope(eventType, payload, overrides = {}) {
  return {
    schema_version: 1,
    producer: 'session-core',
    zdr: false,
    event_id: 'evt_1',
    org_id: 'org_ext',
    event_type: eventType,
    resource_ref: 'space:space_1',
    payload,
    ...overrides,
  };
}

function message(extra = {}, overrides = {}) {
  return envelope(
    'SPACE_MESSAGE_APPENDED',
    {
      kind: 'message.appended',
      space_ref: 'space_1',
      thread_id: 'thread_1',
      message_id: 'msg_1',
      role: 'user',
      author_subject_id: 'alice',
      thread_owner_subject_id: 'bob',
      mentions: ['carol'],
      ...extra,
    },
    overrides,
  );
}

function runState(extra = {}, overrides = {}) {
  return envelope(
    'SPACE_RUN_STATE_CHANGED',
    {
      kind: 'run.state_changed',
      space_ref: 'space_1',
      thread_id: 'thread_1',
      run_id: 'run_1',
      state: 'completed',
      starter_subject_id: 'alice',
      ...extra,
    },
    overrides,
  );
}

function threadDeleted(extra = {}, overrides = {}) {
  return envelope(
    'SPACE_THREAD_DELETED',
    { kind: 'thread.deleted', space_ref: 'space_1', thread_id: 'thread_1', ...extra },
    overrides,
  );
}

function reason(raw) {
  const parsed = parseSpaceEvent(raw);
  assert.equal(parsed.ok, false, JSON.stringify(parsed));
  return parsed.reason;
}

test('a message.appended event parses to ids and closed words only', () => {
  const parsed = parseSpaceEvent(message());
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.event, {
    eventId: 'evt_1',
    orgId: 'org_ext',
    spaceRef: 'space_1',
    kind: 'message.appended',
    threadId: 'thread_1',
    messageId: 'msg_1',
    role: 'user',
    authorSubjectId: 'alice',
    threadOwnerSubjectId: 'bob',
    mentions: ['carol'],
    starterSubjectId: '',
  });
});

test('an assistant message and absent optional subjects parse', () => {
  const parsed = parseSpaceEvent(
    message({ role: 'assistant', author_subject_id: undefined, thread_owner_subject_id: null, mentions: undefined }),
  );
  assert.equal(parsed.ok, true);
  assert.equal(parsed.event.role, 'assistant');
  assert.equal(parsed.event.authorSubjectId, '');
  assert.equal(parsed.event.threadOwnerSubjectId, '');
  assert.deepEqual(parsed.event.mentions, []);
});

test('mentions are de-duplicated in first-seen order', () => {
  const parsed = parseSpaceEvent(message({ mentions: ['a', 'b', 'a', 'c', 'b'] }));
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.event.mentions, ['a', 'b', 'c']);
});

test('a run.state_changed event parses, and an agent id is kept only when present', () => {
  const plain = parseSpaceEvent(runState());
  assert.equal(plain.ok, true);
  assert.deepEqual(plain.event, {
    eventId: 'evt_1',
    orgId: 'org_ext',
    spaceRef: 'space_1',
    kind: 'run.state_changed',
    threadId: 'thread_1',
    runId: 'run_1',
    state: 'completed',
    authorSubjectId: '',
    threadOwnerSubjectId: '',
    mentions: [],
    starterSubjectId: 'alice',
  });
  assert.equal('agentId' in plain.event, false);

  const withAgent = parseSpaceEvent(runState({ agent_id: 'agent_9' }));
  assert.equal(withAgent.ok, true);
  assert.equal(withAgent.event.agentId, 'agent_9');
});

test('a thread.deleted event carries ids only', () => {
  const parsed = parseSpaceEvent(threadDeleted());
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.event, {
    eventId: 'evt_1',
    orgId: 'org_ext',
    spaceRef: 'space_1',
    kind: 'thread.deleted',
    threadId: 'thread_1',
    authorSubjectId: '',
    threadOwnerSubjectId: '',
    mentions: [],
    starterSubjectId: '',
  });
});

test('zdr must be the literal false: true, missing, null and strings are refused', () => {
  assert.equal(reason(message({}, { zdr: true })), 'zdr');
  assert.equal(reason(message({}, { zdr: undefined })), 'zdr');
  assert.equal(reason(message({}, { zdr: null })), 'zdr');
  assert.equal(reason(message({}, { zdr: 'false' })), 'zdr');
  assert.equal(reason(message({}, { zdr: 0 })), 'zdr');
});

test('the producer and the schema version are checked', () => {
  assert.equal(reason(message({}, { producer: 'gateway' })), 'producer');
  assert.equal(reason(message({}, { producer: undefined })), 'producer');
  assert.equal(reason(message({}, { schema_version: 2 })), 'schema_version');
  assert.equal(reason(message({}, { schema_version: '1' })), 'schema_version');
});

test('non-objects, unknown event types and a kind that disagrees are refused', () => {
  for (const bad of [null, undefined, 'x', 7, [], [message()]]) {
    assert.equal(reason(bad), 'not_an_object');
  }
  assert.equal(reason(message({}, { event_type: 'SPACE_NOPE' })), 'event_type');
  assert.equal(reason(message({}, { event_type: 'constructor' })), 'event_type');
  assert.equal(reason(message({}, { event_type: 7 })), 'event_type');
  // event_type says message, payload.kind says run.
  assert.equal(reason(message({ kind: 'run.state_changed' })), 'kind_mismatch');
  assert.equal(reason(runState({ kind: 'message.appended' })), 'kind_mismatch');
  assert.equal(reason(message({}, { payload: 'text' })), 'payload');
  assert.equal(reason(message({}, { payload: [] })), 'payload');
});

test('resource_ref must be exactly space:<space_ref>', () => {
  assert.equal(reason(message({}, { resource_ref: 'space:other' })), 'resource_ref');
  assert.equal(reason(message({}, { resource_ref: 'space_1' })), 'resource_ref');
  assert.equal(reason(message({}, { resource_ref: undefined })), 'resource_ref');
  assert.equal(reason(message({}, { resource_ref: 'space:space_1 ' })), 'resource_ref');
});

test('a space_ref with a dot, star, greater-than or whitespace is refused (it becomes one subject token)', () => {
  for (const ref of ['a.b', 'a*b', 'a>b', 'a b', 'a\tb', 'a\nb', ' a', '', 'x'.repeat(129), 7, null]) {
    assert.equal(isSubjectSafeSpaceRef(ref), false, JSON.stringify(ref));
  }
  assert.equal(isSubjectSafeSpaceRef('space_1-A:b'), true);
  assert.equal(isSubjectSafeSpaceRef('x'.repeat(128)), true);

  assert.equal(reason(message({ space_ref: 'a.b' }, { resource_ref: 'space:a.b' })), 'space_ref');
  assert.equal(reason(message({ space_ref: 'a>' }, { resource_ref: 'space:a>' })), 'space_ref');
  assert.equal(reason(message({ space_ref: 'a b' }, { resource_ref: 'space:a b' })), 'space_ref');
});

test('ids are bounded, trimmed and free of control characters', () => {
  assert.equal(reason(message({}, { event_id: '' })), 'event_id');
  assert.equal(reason(message({}, { event_id: ' evt' })), 'event_id');
  assert.equal(reason(message({}, { event_id: 'e\u0000' })), 'event_id');
  assert.equal(reason(message({}, { event_id: 'e'.repeat(MAX_EVENT_ID_CHARS + 1) })), 'event_id');
  assert.equal(parseSpaceEvent(message({}, { event_id: 'e'.repeat(MAX_EVENT_ID_CHARS) })).ok, true);
  assert.equal(reason(message({}, { org_id: '' })), 'org_id');
  assert.equal(reason(message({}, { org_id: 5 })), 'org_id');
  assert.equal(reason(message({ thread_id: '' })), 'thread_id');
  assert.equal(reason(message({ message_id: undefined })), 'message_id');
  assert.equal(reason(runState({ run_id: '' })), 'run_id');
});

test('a message needs a user or assistant role; any other role is refused', () => {
  for (const role of ['system', 'tool', 'USER', '', undefined, 3]) {
    assert.equal(reason(message({ role })), 'role', String(role));
  }
});

test('more than ten mentions, or a malformed mention, is refused', () => {
  const ten = Array.from({ length: MAX_MENTIONS }, (_, i) => `u${i}`);
  assert.equal(parseSpaceEvent(message({ mentions: ten })).ok, true);
  assert.equal(reason(message({ mentions: [...ten, 'u11'] })), 'mentions');
  assert.equal(reason(message({ mentions: 'carol' })), 'mentions');
  assert.equal(reason(message({ mentions: [''] })), 'mentions');
  assert.equal(reason(message({ mentions: [5] })), 'mentions');
  assert.equal(reason(message({ mentions: [' carol'] })), 'mentions');
});

test('a malformed author or owner subject is refused', () => {
  assert.equal(reason(message({ author_subject_id: ' alice' })), 'subject_id');
  assert.equal(reason(message({ thread_owner_subject_id: 'b\u0007' })), 'subject_id');
  assert.equal(reason(runState({ starter_subject_id: 5 })), 'subject_id');
  assert.equal(reason(runState({ agent_id: 'a\u007f' })), 'subject_id');
});

test('a run state must be a short non-empty string', () => {
  assert.equal(reason(runState({ state: '' })), 'state');
  assert.equal(reason(runState({ state: 5 })), 'state');
  assert.equal(reason(runState({ state: 's'.repeat(41) })), 'state');
  assert.equal(parseSpaceEvent(runState({ state: 's'.repeat(40) })).ok, true);
});

test('planMessageRecipients: mentions first, then followers, then the owner', () => {
  const plan = planMessageRecipients({
    actor: 'alice',
    mentions: ['carol', 'dave'],
    followers: ['erin', 'frank'],
    owner: 'gina',
  });
  assert.deepEqual(plan, [
    { recipient: 'carol', kind: 'mention' },
    { recipient: 'dave', kind: 'mention' },
    { recipient: 'erin', kind: 'reply' },
    { recipient: 'frank', kind: 'reply' },
    { recipient: 'gina', kind: 'reply' },
  ]);
});

test('planMessageRecipients: a mention wins over reply, and nobody appears twice', () => {
  const plan = planMessageRecipients({
    actor: 'alice',
    mentions: ['carol', 'carol'],
    followers: ['carol', 'erin', 'erin'],
    owner: 'erin',
  });
  assert.deepEqual(plan, [
    { recipient: 'carol', kind: 'mention' },
    { recipient: 'erin', kind: 'reply' },
  ]);
});

test('planMessageRecipients: the actor is never a recipient, whatever role they hold', () => {
  const plan = planMessageRecipients({
    actor: 'alice',
    mentions: ['alice'],
    followers: ['alice', 'erin'],
    owner: 'alice',
  });
  assert.deepEqual(plan, [{ recipient: 'erin', kind: 'reply' }]);
});

test('planMessageRecipients: an owner who is also a follower or mentioned keeps the earlier kind; empty owner adds nobody', () => {
  assert.deepEqual(
    planMessageRecipients({ actor: 'a', mentions: ['own'], followers: [], owner: 'own' }),
    [{ recipient: 'own', kind: 'mention' }],
  );
  assert.deepEqual(planMessageRecipients({ actor: 'a', mentions: [], followers: [], owner: '' }), []);
  // The owner comes last among replies.
  assert.deepEqual(
    planMessageRecipients({ actor: 'a', mentions: [], followers: ['f1'], owner: 'own' }).map((r) => r.recipient),
    ['f1', 'own'],
  );
});
