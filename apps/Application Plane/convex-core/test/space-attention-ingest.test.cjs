const assert = require('node:assert/strict');
const test = require('node:test');

const { getFunctionName } = require('convex/server');

const ingest = require('../convex/spaceAttentionIngest.ts');
const { makeCtx } = require('./helpers/fake-convex.cjs');
const {
  COURIER_MAX_ATTEMPTS,
  COURIER_REQUEST_TIMEOUT_MS,
  COURIER_RETRY_DELAYS_MS,
  COURIER_ZDR_REFUSAL,
} = require('../convex/spaceAttentionCourier.ts');

const ORG = 'org_ext';
const SPACE = 'space_1';
const THREAD = 'thread_1';

const { ingestSpaceEvent, deliverOutOfApp, purgeThread, purgeSpace, purgeSubject, sweep, takeBudget } = ingest;

/** Set env vars for one test and restore them after. */
function withEnv(t, vars) {
  const prior = {};
  for (const [key, value] of Object.entries(vars)) {
    prior[key] = process.env[key];
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  t.after(() => {
    for (const [key, value] of Object.entries(prior)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });
}

function quiet(t) {
  const original = console.warn;
  const lines = [];
  console.warn = (...args) => lines.push(args.join(' '));
  t.after(() => {
    console.warn = original;
  });
  return lines;
}

function spaceTables(audience, extra = {}) {
  return {
    spaces: [{ spaceRef: SPACE, externalOrgId: ORG, lifecycle: 'active' }],
    spaceRecipientAudiences: [
      {
        spaceRef: SPACE,
        state: 'active',
        controlState: 'acknowledged',
        externalOrgId: ORG,
        recipientExternalAuthIds: audience,
      },
    ],
    ...extra,
  };
}

function envelope(eventType, payload, overrides = {}) {
  return {
    schema_version: 1,
    producer: 'session-core',
    zdr: false,
    event_id: 'evt_1',
    org_id: ORG,
    event_type: eventType,
    resource_ref: `space:${SPACE}`,
    payload,
    ...overrides,
  };
}

function userMessage(extra = {}, overrides = {}) {
  return envelope(
    'SPACE_MESSAGE_APPENDED',
    {
      kind: 'message.appended',
      space_ref: SPACE,
      thread_id: THREAD,
      message_id: 'msg_1',
      role: 'user',
      author_subject_id: 'alice',
      thread_owner_subject_id: 'bob',
      mentions: [],
      ...extra,
    },
    overrides,
  );
}

function runEvent(state, extra = {}, overrides = {}) {
  return envelope(
    'SPACE_RUN_STATE_CHANGED',
    {
      kind: 'run.state_changed',
      space_ref: SPACE,
      thread_id: THREAD,
      run_id: 'run_1',
      state,
      starter_subject_id: 'alice',
      ...extra,
    },
    overrides,
  );
}

function threadDeleted(overrides = {}) {
  return envelope(
    'SPACE_THREAD_DELETED',
    { kind: 'thread.deleted', space_ref: SPACE, thread_id: THREAD },
    overrides,
  );
}

function follow(subject, state = 'following', extra = {}) {
  return {
    externalOrgId: ORG,
    externalAuthId: subject,
    spaceRef: SPACE,
    threadId: THREAD,
    state,
    reason: 'participated',
    updatedAt: 1,
    ...extra,
  };
}

function apply(ctx, event) {
  return ingestSpaceEvent._handler(ctx, { event });
}

function itemsFor(tables, recipient) {
  return (tables.spaceAttentionItems ?? []).filter((row) => row.recipientAuthId === recipient);
}

test('flag off (unset, 0, empty) acknowledges and drops without writing anything', async (t) => {
  for (const value of [undefined, '0', '', 'off']) {
    withEnv(t, { SPACE_ATTENTION_INGEST: value });
    const { ctx, log } = makeCtx(spaceTables(['alice', 'bob']));
    const result = await apply(ctx, userMessage({ mentions: ['bob'] }));
    assert.deepEqual(result, { status: 'disabled' }, String(value));
    assert.deepEqual(log.inserts, []);
    assert.deepEqual(log.patches, []);
    assert.deepEqual(log.scheduled, []);
  }
});

test('a rejected event is answered, never written, and not recorded for dedupe', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const warnings = quiet(t);
  const { ctx, log } = makeCtx(spaceTables(['alice', 'bob']));
  const result = await apply(ctx, userMessage({}, { zdr: true }));
  assert.deepEqual(result, { status: 'rejected', reason: 'zdr' });
  assert.deepEqual(log.inserts, []);
  assert.equal(warnings.length, 1);
  assert.ok(warnings[0].includes('zdr'));
});

test('a mention creates an item for the mentioned member and a reply item for the thread owner', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1', SPACE_ATTENTION_OUT_OF_APP: undefined });
  const { ctx, tables, log } = makeCtx(spaceTables(['alice', 'bob', 'carol']));
  const result = await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.deepEqual(result, { status: 'applied', items: 2, outOfApp: 0 });

  const carol = itemsFor(tables, 'carol');
  assert.equal(carol.length, 1);
  assert.equal(carol[0].kind, 'mention');
  assert.equal(carol[0].threadId, THREAD);
  assert.equal(carol[0].messageId, 'msg_1');
  assert.equal(carol[0].actorKind, 'user');
  assert.equal(carol[0].actorRef, 'alice');
  assert.equal(carol[0].state, 'open');
  assert.equal(carol[0].count, 1);
  assert.equal(carol[0].coalesceKey, `mention:t:${THREAD}`);
  assert.equal(carol[0].firstEventId, 'evt_1');
  assert.equal(itemsFor(tables, 'bob')[0].kind, 'reply');
  assert.equal(itemsFor(tables, 'alice').length, 0, 'the author is never a recipient');

  // Follows follow participation: owner started, author participated, mentioned mentioned.
  const follows = Object.fromEntries(tables.spaceThreadFollows.map((row) => [row.externalAuthId, row.reason]));
  assert.deepEqual(follows, { bob: 'started', alice: 'participated', carol: 'mentioned' });
  assert.equal(tables.spaceAttentionEvents.length, 1);
  assert.equal(tables.spaceAttentionEvents[0].eventId, 'evt_1');
  assert.deepEqual(log.scheduled, [], 'no out-of-app request while that flag is off');
});

test('the same event id twice is applied once (dedupe by event id)', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob', 'carol']));
  const first = await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.equal(first.status, 'applied');
  const again = await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.deepEqual(again, { status: 'duplicate' });
  assert.equal(itemsFor(tables, 'carol')[0].count, 1);
  assert.equal(tables.spaceAttentionEvents.length, 1);
});

test('a second message in the same thread coalesces into one row with a higher count', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob', 'carol']));
  await apply(ctx, userMessage({ mentions: ['carol'] }, { event_id: 'evt_a' }));
  await apply(ctx, userMessage({ mentions: ['carol'], message_id: 'msg_2' }, { event_id: 'evt_b' }));
  const carol = itemsFor(tables, 'carol');
  assert.equal(carol.length, 1);
  assert.equal(carol[0].count, 2);
  assert.equal(carol[0].lastEventId, 'evt_b');
  assert.equal(carol[0].messageId, 'msg_2');
});

test('a non-member recipient gets no item and no follow row, and a mention reveals nothing', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob']));
  const result = await apply(ctx, userMessage({ mentions: ['zed'] }));
  assert.equal(result.status, 'applied');
  assert.equal(itemsFor(tables, 'zed').length, 0);
  assert.equal((tables.spaceThreadFollows ?? []).some((row) => row.externalAuthId === 'zed'), false);
});

test('a cross-org audience (acknowledged for another org) delivers nothing', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const base = spaceTables(['alice', 'bob', 'carol']);
  base.spaceRecipientAudiences[0].externalOrgId = 'org_other';
  const { ctx, tables } = makeCtx(base);
  const result = await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.deepEqual(result, { status: 'applied', items: 0, outOfApp: 0 });
  assert.equal((tables.spaceAttentionItems ?? []).length, 0);
  assert.equal((tables.spaceThreadFollows ?? []).length, 0);
});

test('an audience that is not acknowledged by Control delivers nothing', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const base = spaceTables(['alice', 'bob', 'carol']);
  base.spaceRecipientAudiences[0].controlState = 'pending';
  const { ctx, tables } = makeCtx(base);
  await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.equal((tables.spaceAttentionItems ?? []).length, 0);
});

test('an event for another org, an unknown space or an inactive space is dropped but recorded', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const cases = [
    [userMessage({}, { org_id: 'org_other' }), (tables) => tables],
    [userMessage(), (tables) => ({ ...tables, spaces: [] })],
    [userMessage(), (tables) => ({ ...tables, spaces: [{ ...tables.spaces[0], lifecycle: 'archived' }] })],
  ];
  for (const [event, shape] of cases) {
    const { ctx, tables } = makeCtx(shape(spaceTables(['alice', 'bob'])));
    const result = await apply(ctx, event);
    assert.deepEqual(result, { status: 'dropped', reason: 'space_unavailable' });
    assert.equal((tables.spaceAttentionItems ?? []).length, 0);
    assert.equal(tables.spaceAttentionEvents.length, 1);
  }
});

test('a muted thread is not overwritten by an auto-follow, and a reply to the muted owner makes no item', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(
    spaceTables(['alice', 'bob', 'carol'], { spaceThreadFollows: [follow('bob', 'muted', { reason: 'manual' })] }),
  );
  await apply(ctx, userMessage({ mentions: ['carol'] }));
  const bob = tables.spaceThreadFollows.find((row) => row.externalAuthId === 'bob');
  assert.equal(bob.state, 'muted');
  assert.equal(bob.reason, 'manual');
  assert.equal(itemsFor(tables, 'bob').length, 0, 'a muted thread drops the reply');
  assert.equal(itemsFor(tables, 'carol').length, 1);
});

test('a mention passes a muted thread and level off, a reply does not', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(
    spaceTables(['alice', 'bob', 'carol'], {
      spaceThreadFollows: [follow('carol', 'muted')],
      spaceRoomNotifyPrefs: [
        { externalOrgId: ORG, externalAuthId: 'carol', spaceRef: SPACE, level: 'off' },
        { externalOrgId: ORG, externalAuthId: 'bob', spaceRef: SPACE, level: 'off' },
      ],
    }),
  );
  await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.equal(itemsFor(tables, 'carol')[0].kind, 'mention');
  assert.equal(itemsFor(tables, 'bob').length, 0, 'level off drops the owner reply');
  assert.equal(tables.spaceThreadFollows.find((row) => row.externalAuthId === 'carol').state, 'muted');
});

test('another org\'s level row for the same member and space is ignored', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(
    spaceTables(['alice', 'bob'], {
      spaceRoomNotifyPrefs: [{ externalOrgId: 'org_other', externalAuthId: 'bob', spaceRef: SPACE, level: 'off' }],
    }),
  );
  await apply(ctx, userMessage());
  assert.equal(itemsFor(tables, 'bob').length, 1);
});

test('the per-thread follower cap is 200: the 201st auto-follow is skipped, the item still lands', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const warnings = quiet(t);
  // 200 followers outside the audience (they get no item, they only fill the cap).
  const crowd = Array.from({ length: 200 }, (_, i) => follow(`crowd_${i}`));
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob', 'carol'], { spaceThreadFollows: crowd }));
  const result = await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.equal(result.status, 'applied');
  assert.equal(tables.spaceThreadFollows.length, 200, 'no follow row was added past the cap');
  assert.equal(itemsFor(tables, 'carol').length, 1);
  assert.ok(warnings.some((line) => line.includes('follower cap reached')));
  // The warning carries no ids.
  assert.equal(warnings.some((line) => line.includes('carol') || line.includes(THREAD)), false);
});

test('an assistant message makes no item and no follow (the run signal does)', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob', 'carol']));
  const result = await apply(ctx, userMessage({ role: 'assistant', mentions: ['carol'] }));
  assert.deepEqual(result, { status: 'applied', reason: 'assistant_message' });
  assert.equal((tables.spaceAttentionItems ?? []).length, 0);
  assert.equal((tables.spaceThreadFollows ?? []).length, 0);
  assert.equal(tables.spaceAttentionEvents.length, 1);
});

test('a completed run makes an agent_done item for the starter only', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob', 'carol']));
  const result = await apply(ctx, runEvent('completed'));
  assert.deepEqual(result, { status: 'applied', items: 1, outOfApp: 0 });
  assert.equal(tables.spaceAttentionItems.length, 1);
  const item = tables.spaceAttentionItems[0];
  assert.equal(item.recipientAuthId, 'alice');
  assert.equal(item.kind, 'agent_done');
  assert.equal(item.runId, 'run_1');
  assert.equal(item.actorKind, 'agent');
});

test('a failed run makes an agent_failed item; an unknown state makes nothing', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice', 'bob']));
  await apply(ctx, runEvent('failed', {}, { event_id: 'evt_f' }));
  assert.equal(tables.spaceAttentionItems[0].kind, 'agent_failed');
  const { ctx: ctx2, tables: tables2 } = makeCtx(spaceTables(['alice', 'bob']));
  const result = await apply(ctx2, runEvent('sparkling', {}, { event_id: 'evt_x' }));
  assert.deepEqual(result, { status: 'applied', items: 0, outOfApp: 0 });
  assert.equal((tables2.spaceAttentionItems ?? []).length, 0);
});

test('a run with no starter, or whose starter left the audience, makes no item', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const noStarter = makeCtx(spaceTables(['alice', 'bob']));
  await apply(noStarter.ctx, runEvent('completed', { starter_subject_id: undefined }));
  assert.equal((noStarter.tables.spaceAttentionItems ?? []).length, 0);
  const gone = makeCtx(spaceTables(['bob']));
  await apply(gone.ctx, runEvent('completed'));
  assert.equal((gone.tables.spaceAttentionItems ?? []).length, 0);
});

test('awaiting_approval makes no inbox item, only the agent activity changes', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1', SPACE_ATTENTION_OUT_OF_APP: undefined });
  const { ctx, tables, log } = makeCtx(spaceTables(['alice', 'bob']));
  const result = await apply(ctx, runEvent('awaiting_approval', { agent_id: 'agent_9' }));
  assert.equal(result.status, 'applied');
  assert.equal((tables.spaceAttentionItems ?? []).length, 0);
  assert.equal(tables.spaceAgentActivity.length, 1);
  assert.equal(tables.spaceAgentActivity[0].agentId, 'agent_9');
  assert.equal(tables.spaceAgentActivity[0].state, 'waiting');
  assert.deepEqual(log.scheduled, []);
});

test('agent activity is updated in place on the next run state, never duplicated', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const { ctx, tables } = makeCtx(spaceTables(['alice']));
  await apply(ctx, runEvent('running', { agent_id: 'agent_9' }, { event_id: 'e1' }));
  assert.equal(tables.spaceAgentActivity[0].state, 'working');
  await apply(ctx, runEvent('completed', { agent_id: 'agent_9' }, { event_id: 'e2' }));
  assert.equal(tables.spaceAgentActivity.length, 1);
  assert.equal(tables.spaceAgentActivity[0].state, 'idle');
});

test('with the out-of-app flag on, awaiting_approval schedules one refs-only courier action and takes budget', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1', SPACE_ATTENTION_OUT_OF_APP: '1' });
  const { ctx, tables, log } = makeCtx(spaceTables(['alice', 'bob']));
  const result = await apply(ctx, runEvent('awaiting_approval'));
  assert.deepEqual(result, { status: 'applied', items: 0, outOfApp: 1 });
  assert.equal(log.scheduled.length, 1);
  assert.equal(log.scheduled[0].delay, 0);
  assert.deepEqual(log.scheduled[0].args, {
    eventId: 'evt_1',
    externalOrgId: ORG,
    recipientAuthId: 'alice',
    spaceRef: SPACE,
    kind: 'approval_needed',
    threadId: THREAD,
    messageId: undefined,
    runId: 'run_1',
    actorKind: 'agent',
    zdr: false,
  });
  assert.equal(tables.spaceAttentionBudget.length, 1);
  assert.equal(tables.spaceAttentionBudget[0].count, 1);
});

test('a member in dnd gets the item but no out-of-app request', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1', SPACE_ATTENTION_OUT_OF_APP: '1' });
  const { ctx, tables, log } = makeCtx(
    spaceTables(['alice', 'bob', 'carol'], {
      spaceMemberStatus: [{ externalOrgId: ORG, externalAuthId: 'carol', state: 'dnd', updatedAt: 1 }],
    }),
  );
  await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.equal(itemsFor(tables, 'carol').length, 1);
  assert.equal(log.scheduled.some((entry) => entry.args.recipientAuthId === 'carol'), false);
});

test('an observing member (fresh presence) gets no reply item but still gets a mention', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1', SPACE_ATTENTION_SUPPRESS_OBSERVED: undefined });
  const present = (subject) => ({
    externalOrgId: ORG,
    spaceRef: SPACE,
    externalAuthId: subject,
    status: 'online',
    updatedAt: Date.now(),
  });
  const { ctx, tables } = makeCtx(
    spaceTables(['alice', 'bob', 'carol'], { spacePresence: [present('bob'), present('carol')] }),
  );
  await apply(ctx, userMessage({ mentions: ['carol'] }));
  assert.equal(itemsFor(tables, 'bob').length, 0, 'owner is looking at the room');
  assert.equal(itemsFor(tables, 'carol')[0].kind, 'mention');
});

test('thread.deleted purges that thread\'s items and follows, in that org only', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const item = (extra) => ({
    externalOrgId: ORG,
    recipientAuthId: 'bob',
    spaceRef: SPACE,
    kind: 'reply',
    threadId: THREAD,
    state: 'open',
    count: 1,
    ...extra,
  });
  // No space row and no audience at all: erasure does not depend on either.
  const { ctx, tables, log } = makeCtx({
    spaceAttentionItems: [item({}), item({ threadId: 'thread_2' }), item({ externalOrgId: 'org_other' })],
    spaceThreadFollows: [follow('bob'), follow('carol', 'following', { threadId: 'thread_2' }), follow('dave', 'following', { externalOrgId: 'org_other' })],
  });
  const result = await apply(ctx, threadDeleted());
  assert.deepEqual(result, { status: 'applied', reason: 'thread_purged' });
  assert.deepEqual(
    tables.spaceAttentionItems.map((row) => [row.threadId, row.externalOrgId]),
    [['thread_2', ORG], [THREAD, 'org_other']],
  );
  assert.deepEqual(
    tables.spaceThreadFollows.map((row) => row.externalAuthId),
    ['carol', 'dave'],
  );
  assert.deepEqual(log.scheduled, [], 'a short purge does not reschedule');
  assert.equal(tables.spaceAttentionEvents.length, 1);
});

test('thread.deleted with a full batch reschedules purgeThread', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const rows = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    recipientAuthId: `u${i}`,
    spaceRef: SPACE,
    kind: 'reply',
    threadId: THREAD,
    state: 'open',
    count: 1,
  }));
  const { ctx, tables, log } = makeCtx({ spaceAttentionItems: rows });
  await apply(ctx, threadDeleted());
  assert.equal(tables.spaceAttentionItems.length, 0);
  assert.equal(log.scheduled.length, 1);
  assert.deepEqual(log.scheduled[0].args, { externalOrgId: ORG, spaceRef: SPACE, threadId: THREAD });
});

test('purgeThread deletes one batch and reschedules itself while a full batch was deleted', async () => {
  const rows = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    externalAuthId: `u${i}`,
    spaceRef: SPACE,
    threadId: THREAD,
    state: 'following',
  }));
  const { ctx, tables, log } = makeCtx({ spaceThreadFollows: rows });
  const first = await purgeThread._handler(ctx, { externalOrgId: ORG, spaceRef: SPACE, threadId: THREAD });
  assert.deepEqual(first, { more: true });
  assert.equal(tables.spaceThreadFollows.length, 0);
  assert.equal(log.scheduled.length, 1);
  const second = await purgeThread._handler(ctx, { externalOrgId: ORG, spaceRef: SPACE, threadId: THREAD });
  assert.deepEqual(second, { more: false });
  assert.equal(log.scheduled.length, 1, 'an empty pass does not reschedule');
});

test('thread.deleted purges the thread\'s message reactions in its Space and nothing else', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '1' });
  const reaction = (extra = {}) => ({
    externalOrgId: ORG,
    spaceRef: SPACE,
    threadId: THREAD,
    messageId: 'msg_1',
    emoji: '\u{1F44D}',
    externalAuthId: 'alice',
    createdAt: 1,
    ...extra,
  });
  const { ctx, tables, log } = makeCtx({
    spaceReactions: [
      reaction(),
      reaction({ messageId: 'msg_2', externalAuthId: 'bob' }),
      reaction({ threadId: 'thread_2' }),
      reaction({ spaceRef: 'space_2' }),
      reaction({ externalOrgId: 'org_other' }),
    ],
  });
  const result = await apply(ctx, threadDeleted());
  assert.deepEqual(result, { status: 'applied', reason: 'thread_purged' });
  assert.deepEqual(
    tables.spaceReactions.map((row) => [row.spaceRef, row.threadId, row.externalOrgId]),
    [[SPACE, 'thread_2', ORG], ['space_2', THREAD, ORG], [SPACE, THREAD, 'org_other']],
  );
  assert.deepEqual(log.scheduled, [], 'a short purge does not reschedule');
});

test('purgeThread pages through a full batch of reactions and reschedules itself', async () => {
  const rows = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    spaceRef: SPACE,
    threadId: THREAD,
    messageId: `msg_${i}`,
    emoji: '\u{1F44D}',
    externalAuthId: `u${i}`,
    createdAt: 1,
  }));
  const args = { externalOrgId: ORG, spaceRef: SPACE, threadId: THREAD };
  const { ctx, tables, log } = makeCtx({ spaceReactions: rows });
  assert.deepEqual(await purgeThread._handler(ctx, args), { more: true });
  assert.equal(tables.spaceReactions.length, 0);
  assert.deepEqual(log.scheduled.map((call) => call.args), [args]);
  assert.deepEqual(await purgeThread._handler(ctx, args), { more: false });
  assert.equal(log.scheduled.length, 1, 'an empty pass does not reschedule');
});

test('flag off: thread.deleted still purges items, follows and reactions, and writes no dedupe row', async (t) => {
  for (const value of [undefined, '0']) {
    withEnv(t, { SPACE_ATTENTION_INGEST: value });
    const { ctx, tables, log } = makeCtx({
      spaceAttentionItems: [
        { externalOrgId: ORG, recipientAuthId: 'bob', spaceRef: SPACE, kind: 'reply', threadId: THREAD, state: 'open', count: 1 },
        { externalOrgId: ORG, recipientAuthId: 'bob', spaceRef: SPACE, kind: 'reply', threadId: 'thread_2', state: 'open', count: 1 },
      ],
      spaceThreadFollows: [follow('bob')],
      spaceReactions: [
        { externalOrgId: ORG, spaceRef: SPACE, threadId: THREAD, messageId: 'msg_1', emoji: '\u{1F44D}', externalAuthId: 'alice', createdAt: 1 },
      ],
    });
    const result = await apply(ctx, threadDeleted());
    assert.deepEqual(result, { status: 'applied', reason: 'thread_purged' }, String(value));
    assert.deepEqual(tables.spaceAttentionItems.map((row) => row.threadId), ['thread_2']);
    assert.equal(tables.spaceThreadFollows.length, 0);
    assert.equal(tables.spaceReactions.length, 0);
    assert.deepEqual(log.inserts, [], 'no dedupe row while the switch is off');
    assert.deepEqual(log.scheduled, []);
  }
});

test('flag off: a malformed or zdr thread.deleted event is still only acknowledged', async (t) => {
  withEnv(t, { SPACE_ATTENTION_INGEST: '0' });
  const { ctx, tables, log } = makeCtx({ spaceThreadFollows: [follow('bob')] });
  assert.deepEqual(await apply(ctx, threadDeleted({ zdr: true })), { status: 'disabled' });
  assert.equal(tables.spaceThreadFollows.length, 1);
  assert.deepEqual(log.inserts, []);
});

test('purgeThread queued with the older arguments (no spaceRef) validates and purges items and follows', async () => {
  const { ctx, tables, log } = makeCtx({
    spaceThreadFollows: [follow('bob')],
    spaceReactions: [
      { externalOrgId: ORG, spaceRef: SPACE, threadId: THREAD, messageId: 'msg_1', emoji: '\u{1F44D}', externalAuthId: 'alice', createdAt: 1 },
    ],
  });
  assert.deepEqual(await purgeThread._handler(ctx, { externalOrgId: ORG, threadId: THREAD }), { more: false });
  assert.equal(tables.spaceThreadFollows.length, 0);
  assert.equal(tables.spaceReactions.length, 1, 'without a Space the reaction index cannot be addressed');
  assert.deepEqual(log.scheduled, []);
});

test('purgeSpace deletes every table\'s rows for the Space and nothing of another Space or org', async () => {
  const mine = (extra = {}) => ({ externalOrgId: ORG, spaceRef: SPACE, ...extra });
  const tablesIn = {
    spaceAttentionItems: [mine({ recipientAuthId: 'a' }), mine({ spaceRef: 'space_2' }), mine({ externalOrgId: 'org_other' })],
    spaceThreadFollows: [mine({ externalAuthId: 'a' })],
    spaceAgentActivity: [mine({ agentId: 'ag' })],
    spaceReadMarkers: [mine({ externalAuthId: 'a' })],
    spacePresence: [mine({ externalAuthId: 'a' })],
    spaceRoomNotifyPrefs: [mine({ externalAuthId: 'a' })],
    spaceReminders: [mine({ externalAuthId: 'a' })],
    spaceReactions: [mine({ threadId: THREAD })],
    spacePins: [mine({ threadId: THREAD, pinnedAt: 1 }), mine({ externalOrgId: 'org_other', threadId: THREAD, pinnedAt: 2 })],
    spaceRoomNotices: [
      mine({ noticeId: 'n1', kind: 'room_renamed', createdAt: 1 }),
      mine({ spaceRef: 'space_2', noticeId: 'n2', kind: 'room_renamed', createdAt: 1 }),
      mine({ externalOrgId: 'org_other', noticeId: 'n3', kind: 'room_renamed', createdAt: 1 }),
    ],
    spaceDmHides: [
      mine({ externalAuthId: 'a', hiddenAt: 1 }),
      mine({ spaceRef: 'space_2', externalAuthId: 'a', hiddenAt: 1 }),
      mine({ externalOrgId: 'org_other', externalAuthId: 'a', hiddenAt: 1 }),
    ],
  };
  const { ctx, tables, log } = makeCtx(tablesIn);
  const result = await purgeSpace._handler(ctx, { externalOrgId: ORG, spaceRef: SPACE });
  assert.deepEqual(result, { more: false });
  for (const name of Object.keys(tablesIn)) {
    const left = tables[name].filter((row) => row.spaceRef === SPACE && row.externalOrgId === ORG);
    assert.equal(left.length, 0, name);
  }
  assert.equal(tables.spaceAttentionItems.length, 2, 'the other Space and the other org are untouched');
  assert.equal(tables.spacePins.length, 1, 'the Space\'s pins go; the other org\'s pin stays');
  assert.equal(tables.spaceRoomNotices.length, 2, 'room notices of the other Space and the other org stay');
  assert.equal(tables.spaceDmHides.length, 2, 'DM hides of the other Space and the other org stay');
  assert.deepEqual(log.scheduled, []);
});

test('purgeSpace reschedules itself when a table returned a full batch', async () => {
  const rows = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    spaceRef: SPACE,
    recipientAuthId: `u${i}`,
    kind: 'reply',
  }));
  const { ctx, tables, log } = makeCtx({ spaceAttentionItems: rows });
  const result = await purgeSpace._handler(ctx, { externalOrgId: ORG, spaceRef: SPACE });
  assert.deepEqual(result, { more: true });
  assert.equal(tables.spaceAttentionItems.length, 0);
  assert.deepEqual(log.scheduled[0].args, { externalOrgId: ORG, spaceRef: SPACE });
});

test('purgeSpace reschedules itself when a room-notice or DM-hide batch was full', async () => {
  const notices = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    spaceRef: SPACE,
    noticeId: `n${i}`,
    kind: 'room_renamed',
    createdAt: i,
  }));
  const notice = makeCtx({ spaceRoomNotices: notices });
  assert.deepEqual(await purgeSpace._handler(notice.ctx, { externalOrgId: ORG, spaceRef: SPACE }), { more: true });
  assert.equal(notice.tables.spaceRoomNotices.length, 0);
  assert.deepEqual(notice.log.scheduled[0].args, { externalOrgId: ORG, spaceRef: SPACE });

  const hides = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    spaceRef: SPACE,
    externalAuthId: `u${i}`,
    hiddenAt: 1,
  }));
  const hide = makeCtx({ spaceDmHides: hides });
  assert.deepEqual(await purgeSpace._handler(hide.ctx, { externalOrgId: ORG, spaceRef: SPACE }), { more: true });
  assert.equal(hide.tables.spaceDmHides.length, 0);
  assert.equal(hide.log.scheduled.length, 1);
});

test('purgeSubject removes the member\'s rows in that org only', async () => {
  const mine = (extra = {}) => ({ externalOrgId: ORG, externalAuthId: 'alice', ...extra });
  const tablesIn = {
    spaceAttentionItems: [mine({ recipientAuthId: 'alice' }), { externalOrgId: ORG, recipientAuthId: 'bob' }, { externalOrgId: 'org_other', recipientAuthId: 'alice' }],
    spaceThreadFollows: [mine(), mine({ externalOrgId: 'org_other' }), { externalOrgId: ORG, externalAuthId: 'bob' }],
    spaceRoomNotifyPrefs: [mine()],
    spaceNotifyUserSettings: [mine()],
    spaceReadMarkers: [mine()],
    spaceMemberStatus: [mine({ state: 'away' })],
    spaceReminders: [mine({ state: 'pending' })],
  };
  const { ctx, tables } = makeCtx(tablesIn);
  const result = await purgeSubject._handler(ctx, { externalOrgId: ORG, externalAuthId: 'alice' });
  assert.deepEqual(result, { more: false });
  assert.deepEqual(tables.spaceAttentionItems.map((row) => [row.recipientAuthId, row.externalOrgId]), [['bob', ORG], ['alice', 'org_other']]);
  assert.deepEqual(tables.spaceThreadFollows.map((row) => [row.externalAuthId, row.externalOrgId]), [['alice', 'org_other'], ['bob', ORG]]);
  for (const name of ['spaceRoomNotifyPrefs', 'spaceNotifyUserSettings', 'spaceReadMarkers', 'spaceMemberStatus', 'spaceReminders']) {
    assert.equal(tables[name].length, 0, name);
  }
});

test('purgeSubject reschedules itself on a full batch', async () => {
  const rows = Array.from({ length: 200 }, (_, i) => ({
    externalOrgId: ORG,
    recipientAuthId: 'alice',
    kind: 'reply',
    n: i,
  }));
  const { ctx, log } = makeCtx({ spaceAttentionItems: rows });
  const result = await purgeSubject._handler(ctx, { externalOrgId: ORG, externalAuthId: 'alice' });
  assert.deepEqual(result, { more: true });
  assert.deepEqual(log.scheduled[0].args, { externalOrgId: ORG, externalAuthId: 'alice' });
});

test('sweep deletes expired items and dedupe rows older than 48 hours, and keeps the rest', async () => {
  const now = Date.now();
  const hour = 60 * 60 * 1000;
  const { ctx, tables, log } = makeCtx({
    spaceAttentionItems: [
      { recipientAuthId: 'a', expiresAt: now - 1000 },
      { recipientAuthId: 'b', expiresAt: now + hour },
    ],
    spaceAttentionEvents: [
      { eventId: 'old', createdAt: now - 49 * hour },
      { eventId: 'new', createdAt: now - 47 * hour },
    ],
  });
  const result = await sweep._handler(ctx, {});
  assert.deepEqual(result, { items: 1, events: 1 });
  assert.deepEqual(tables.spaceAttentionItems.map((row) => row.recipientAuthId), ['b']);
  assert.deepEqual(tables.spaceAttentionEvents.map((row) => row.eventId), ['new']);
  assert.deepEqual(log.scheduled, []);
});

test('sweep reschedules itself when a batch was full', async () => {
  const now = Date.now();
  const rows = Array.from({ length: 500 }, (_, i) => ({ recipientAuthId: `u${i}`, expiresAt: now - 1000 }));
  const { ctx, tables, log } = makeCtx({ spaceAttentionItems: rows });
  const result = await sweep._handler(ctx, {});
  assert.deepEqual(result, { items: 500, events: 0 });
  assert.equal(tables.spaceAttentionItems.length, 0);
  assert.equal(log.scheduled.length, 1);
  assert.deepEqual(log.scheduled[0].args, {});
});

test('takeBudget allows 30 out-of-app requests per recipient per window, then refuses', async () => {
  const { ctx, tables } = makeCtx({});
  const now = Date.now();
  for (let i = 0; i < 30; i += 1) assert.equal(await takeBudget(ctx, 'alice', now), true, `request ${i + 1}`);
  assert.equal(await takeBudget(ctx, 'alice', now), false);
  assert.equal(tables.spaceAttentionBudget[0].count, 30, 'a denied call does not count');
  assert.equal(await takeBudget(ctx, 'bob', now), true, 'budget is per recipient');
  assert.equal(await takeBudget(ctx, 'alice', now + 60 * 60 * 1000), true, 'a new window starts after an hour');
});

// --- deliverOutOfApp (WS11-S9, DEPTH-WS11-COURIER) ------------------------------------------

function stubFetch(t, behavior) {
  const original = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return behavior(url, init);
  };
  t.after(() => {
    globalThis.fetch = original;
  });
  return calls;
}

const COURIER_ARGS = {
  eventId: 'evt_1',
  externalOrgId: ORG,
  recipientAuthId: 'alice',
  spaceRef: SPACE,
  kind: 'mention',
  threadId: THREAD,
  messageId: 'msg_1',
  actorKind: 'user',
  zdr: false,
};
const COURIER_ENV = {
  SPACE_ATTENTION_OUT_OF_APP: '1',
  NOTIFICATION_CORE_URL: 'http://notification-core:3140/',
  NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN: 'k'.repeat(40),
};

/**
 * An action ctx over the same fake tables. `runQuery` runs the real
 * `courierGate` handler, so the gate reads whatever rows a test sets;
 * `ctx.runQueryCalls` counts the reads.
 */
function courierCtx(tables = spaceTables(['alice'])) {
  const made = makeCtx(tables);
  made.ctx.runQueryCalls = 0;
  made.ctx.runQuery = async (ref, args) => {
    made.ctx.runQueryCalls += 1;
    assert.equal(getFunctionName(ref), 'spaceAttentionIngest:courierGate');
    return ingest.courierGate._handler(made.ctx, args);
  };
  return made;
}

/** The most recent `ctx.scheduler.runAfter` call: its delay, function and args. */
function lastScheduled(log) {
  return log.scheduled[log.scheduled.length - 1];
}

test('deliverOutOfApp is disabled without the flag, and never calls the network', async (t) => {
  withEnv(t, { ...COURIER_ENV, SPACE_ATTENTION_OUT_OF_APP: undefined });
  const calls = stubFetch(t, async () => ({ ok: true, status: 200 }));
  const { ctx, log } = courierCtx();
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'disabled' });
  assert.equal(calls.length, 0);
  assert.equal(ctx.runQueryCalls, 0);
  assert.equal(log.scheduled.length, 0);
});

test('deliverOutOfApp is not_configured without the URL or the token, and that is final', async (t) => {
  const calls = stubFetch(t, async () => ({ ok: true, status: 200 }));
  const { ctx, log } = courierCtx();
  withEnv(t, { ...COURIER_ENV, NOTIFICATION_CORE_URL: undefined });
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'not_configured' });
  withEnv(t, { ...COURIER_ENV, NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN: '   ' });
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'not_configured' });
  assert.equal(calls.length, 0);
  assert.equal(log.scheduled.length, 0);
});

test('deliverOutOfApp does not send an unsupported kind', async (t) => {
  withEnv(t, COURIER_ENV);
  const calls = stubFetch(t, async () => ({ ok: true, status: 200 }));
  const { ctx } = courierCtx();
  assert.deepEqual(await deliverOutOfApp._handler(ctx, { ...COURIER_ARGS, kind: 'party' }), {
    delivered: false,
    reason: 'unsupported_kind',
  });
  assert.equal(calls.length, 0);
});

test('deliverOutOfApp posts one signed refs-only request to /api/v1/notification-requests', async (t) => {
  withEnv(t, COURIER_ENV);
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  const { ctx, log } = courierCtx();
  const result = await deliverOutOfApp._handler(ctx, COURIER_ARGS);
  assert.deepEqual(result, { delivered: true, reason: 'accepted' });
  assert.equal(calls.length, 1);
  assert.equal(log.scheduled.length, 0, 'an accepted request schedules nothing');
  assert.equal(calls[0].url, 'http://notification-core:3140/api/v1/notification-requests');
  assert.equal(calls[0].init.method, 'POST');
  const headers = calls[0].init.headers;
  assert.equal(headers['x-service-id'], 'space-attention');
  assert.equal(headers['x-org-id'], ORG);
  assert.ok(headers['x-delegation-signature']);
  const body = JSON.parse(calls[0].init.body);
  assert.equal(body.idempotency_key, 'evt_1:alice');
  assert.equal(body.type, 'space.mention');
  assert.equal(body.retention_mode, 'standard');
  assert.deepEqual(body.recipient, { kind: 'user', id: 'alice' });
});

test('deliverOutOfApp maps a non-2xx answer to http_<status> and logs the status only', async (t) => {
  withEnv(t, COURIER_ENV);
  const warnings = quiet(t);
  stubFetch(t, async () => ({ ok: false, status: 403, text: async () => 'SECRET BODY' }));
  const { ctx, log } = courierCtx();
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'http_403' });
  assert.deepEqual(warnings, ['[space-attention] courier refused: status 403']);
  assert.equal(log.scheduled.length, 0, 'a refusal is final');
});

test('deliverOutOfApp never throws: a network failure is "unreachable", and the first retry is scheduled', async (t) => {
  withEnv(t, COURIER_ENV);
  const warnings = quiet(t);
  stubFetch(t, async () => {
    throw new Error('connect ECONNREFUSED 10.0.0.9:3140');
  });
  const { ctx, log } = courierCtx();
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'unreachable' });
  assert.equal(warnings.some((line) => line.includes('10.0.0.9')), false, 'the error text is not logged');
  assert.equal(log.scheduled.length, 1);
  assert.equal(lastScheduled(log).delay, COURIER_RETRY_DELAYS_MS[0]);
});

test('the courier sends nothing unless the event carried a literal false ZDR posture, and reads nothing either', async (t) => {
  withEnv(t, COURIER_ENV);
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  for (const zdr of [undefined, true, 'false', 0, null]) {
    const { ctx, log } = courierCtx();
    assert.deepEqual(
      await deliverOutOfApp._handler(ctx, { ...COURIER_ARGS, zdr }),
      { delivered: false, reason: COURIER_ZDR_REFUSAL },
      String(zdr),
    );
    assert.equal(ctx.runQueryCalls, 0, String(zdr));
    assert.equal(log.scheduled.length, 0, String(zdr));
  }
  assert.equal(calls.length, 0);
});

test('the gate reads the current member status: a member in dnd gets no request', async (t) => {
  withEnv(t, COURIER_ENV);
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  const { ctx } = courierCtx(
    spaceTables(['alice'], {
      spaceMemberStatus: [{ externalOrgId: ORG, externalAuthId: 'alice', state: 'dnd', updatedAt: 1 }],
    }),
  );
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'dnd' });
  assert.equal(calls.length, 0);
});

test('an expired dnd no longer stops the request', async (t) => {
  withEnv(t, COURIER_ENV);
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  const { ctx } = courierCtx(
    spaceTables(['alice'], {
      spaceMemberStatus: [
        { externalOrgId: ORG, externalAuthId: 'alice', state: 'dnd', updatedAt: 1, expiresAt: Date.now() - 1000 },
      ],
    }),
  );
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: true, reason: 'accepted' });
  assert.equal(calls.length, 1);
});

test('the gate stops a closed Space, another organization\'s Space, and a recipient no longer in the audience', async (t) => {
  withEnv(t, COURIER_ENV);
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  const active = spaceTables(['alice']);
  const cases = [
    [{ ...active, spaces: [{ spaceRef: SPACE, externalOrgId: ORG, lifecycle: 'archived' }] }, 'space_unavailable'],
    [{ ...active, spaces: [{ spaceRef: SPACE, externalOrgId: 'org_other', lifecycle: 'active' }] }, 'space_unavailable'],
    [spaceTables(['bob']), 'not_in_audience'],
    [
      { ...active, spaceRecipientAudiences: [{ ...active.spaceRecipientAudiences[0], controlState: 'pending' }] },
      'not_in_audience',
    ],
  ];
  for (const [tables, reason] of cases) {
    const { ctx } = courierCtx(tables);
    assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason }, reason);
  }
  assert.equal(calls.length, 0);
});

test('a failed gate read is retried, not dropped', async (t) => {
  withEnv(t, COURIER_ENV);
  quiet(t);
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  const { ctx, log } = courierCtx();
  ctx.runQuery = async () => {
    throw new Error('read timed out');
  };
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'gate_unavailable' });
  assert.equal(calls.length, 0, 'nothing is sent without a gate');
  assert.equal(lastScheduled(log).delay, COURIER_RETRY_DELAYS_MS[0]);
  assert.equal(lastScheduled(log).args.attempt, 2);
});

test('a network failure is retried with the same body and idempotency key, and the retry can deliver', async (t) => {
  withEnv(t, COURIER_ENV);
  quiet(t);
  let answers = 0;
  const calls = stubFetch(t, async () => {
    answers += 1;
    if (answers === 1) throw new Error('socket hang up');
    return { ok: true, status: 202 };
  });
  const { ctx, log } = courierCtx();
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: 'unreachable' });
  const retry = lastScheduled(log);
  assert.equal(getFunctionName(retry.fn), 'spaceAttentionIngest:deliverOutOfApp');
  assert.equal(retry.delay, COURIER_RETRY_DELAYS_MS[0]);
  assert.equal(retry.args.attempt, 2);
  assert.deepEqual(await deliverOutOfApp._handler(ctx, retry.args), { delivered: true, reason: 'accepted' });
  assert.equal(calls.length, 2);
  assert.equal(calls[1].init.body, calls[0].init.body, 'the retry sends the same body');
  assert.equal(JSON.parse(calls[1].init.body).idempotency_key, 'evt_1:alice');
  assert.notEqual(
    calls[1].init.headers['x-delegation-nonce'],
    calls[0].init.headers['x-delegation-nonce'],
    'each attempt is signed afresh',
  );
  assert.equal(log.scheduled.length, 1, 'an accepted retry schedules nothing more');
});

test('408, 429 and 5xx are retried; any other refusal is final', async (t) => {
  withEnv(t, COURIER_ENV);
  quiet(t);
  let status = 500;
  stubFetch(t, async () => ({ ok: false, status, text: async () => '' }));
  for (const code of [408, 429, 500, 502, 503, 504]) {
    status = code;
    const { ctx, log } = courierCtx();
    assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: `http_${code}` });
    assert.equal(log.scheduled.length, 1, `${code} is retried`);
  }
  for (const code of [400, 401, 403, 404, 409, 413, 422]) {
    status = code;
    const { ctx, log } = courierCtx();
    assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: false, reason: `http_${code}` });
    assert.equal(log.scheduled.length, 0, `${code} is final`);
  }
});

test('five attempts in all: the waits follow the schedule, then the last failure is final', async (t) => {
  withEnv(t, COURIER_ENV);
  quiet(t);
  const calls = stubFetch(t, async () => ({ ok: false, status: 503, text: async () => '' }));
  const { ctx, log } = courierCtx();
  const delays = [];
  let args = COURIER_ARGS;
  let result = null;
  for (let i = 0; i < 10; i += 1) {
    result = await deliverOutOfApp._handler(ctx, args);
    if (log.scheduled.length === delays.length) break;
    const next = lastScheduled(log);
    delays.push(next.delay);
    args = next.args;
  }
  assert.deepEqual(result, { delivered: false, reason: 'http_503' });
  assert.equal(calls.length, COURIER_MAX_ATTEMPTS);
  assert.deepEqual(delays, COURIER_RETRY_DELAYS_MS);
  assert.equal(args.attempt, COURIER_MAX_ATTEMPTS);
});

test('a retry is stopped by a dnd set while it waited, and by the flag going off', async (t) => {
  withEnv(t, COURIER_ENV);
  quiet(t);
  const calls = stubFetch(t, async () => ({ ok: false, status: 503, text: async () => '' }));

  const dnd = courierCtx();
  await deliverOutOfApp._handler(dnd.ctx, COURIER_ARGS);
  const retry = lastScheduled(dnd.log);
  dnd.tables.spaceMemberStatus = [
    { _id: 'status_1', externalOrgId: ORG, externalAuthId: 'alice', state: 'dnd', updatedAt: 2 },
  ];
  assert.deepEqual(await deliverOutOfApp._handler(dnd.ctx, retry.args), { delivered: false, reason: 'dnd' });
  assert.equal(dnd.log.scheduled.length, 1, 'nothing more is scheduled');

  const off = courierCtx();
  await deliverOutOfApp._handler(off.ctx, COURIER_ARGS);
  const later = lastScheduled(off.log);
  withEnv(t, { SPACE_ATTENTION_OUT_OF_APP: undefined });
  assert.deepEqual(await deliverOutOfApp._handler(off.ctx, later.args), { delivered: false, reason: 'disabled' });
  assert.equal(calls.length, 2, 'only the two first attempts reached the network');
});

test('a request that never answers is bounded by the timeout and retried', async (t) => {
  withEnv(t, COURIER_ENV);
  const warnings = quiet(t);
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let sent;
  const reached = new Promise((resolve) => {
    sent = resolve;
  });
  // Never settles and ignores the abort signal: only the timeout can end the attempt.
  const calls = stubFetch(t, () => {
    sent();
    return new Promise(() => {});
  });
  const { ctx, log } = courierCtx();
  const pending = deliverOutOfApp._handler(ctx, COURIER_ARGS);
  await reached;
  assert.ok(calls[0].init.signal, 'the request carries an abort signal');
  t.mock.timers.tick(COURIER_REQUEST_TIMEOUT_MS - 1);
  await Promise.resolve();
  assert.equal(calls[0].init.signal.aborted, false, 'not aborted before the timeout');
  t.mock.timers.tick(1);
  assert.deepEqual(await pending, { delivered: false, reason: 'timeout' });
  assert.equal(calls[0].init.signal.aborted, true, 'the request is aborted');
  assert.equal(warnings[0], '[space-attention] courier timed out');
  assert.equal(log.scheduled.length, 1);
  assert.equal(lastScheduled(log).delay, COURIER_RETRY_DELAYS_MS[0]);
  assert.equal(lastScheduled(log).args.attempt, 2);
});

test('a fetch that rejects on abort is also a retryable timeout, not "unreachable"', async (t) => {
  withEnv(t, COURIER_ENV);
  quiet(t);
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let sent;
  const reached = new Promise((resolve) => {
    sent = resolve;
  });
  stubFetch(t, (url, init) => {
    sent();
    return new Promise((resolve, reject) => {
      init.signal.addEventListener('abort', () => reject(new Error('This operation was aborted')));
    });
  });
  const { ctx, log } = courierCtx();
  const pending = deliverOutOfApp._handler(ctx, COURIER_ARGS);
  await reached;
  t.mock.timers.tick(COURIER_REQUEST_TIMEOUT_MS);
  assert.deepEqual(await pending, { delivered: false, reason: 'timeout' });
  assert.equal(log.scheduled.length, 1);
});

test('a prompt answer clears the timeout', async (t) => {
  withEnv(t, COURIER_ENV);
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const calls = stubFetch(t, async () => ({ ok: true, status: 202 }));
  const { ctx, log } = courierCtx();
  assert.deepEqual(await deliverOutOfApp._handler(ctx, COURIER_ARGS), { delivered: true, reason: 'accepted' });
  t.mock.timers.tick(COURIER_REQUEST_TIMEOUT_MS);
  assert.equal(calls[0].init.signal.aborted, false, 'a settled request is never aborted afterwards');
  assert.equal(log.scheduled.length, 0);
});
