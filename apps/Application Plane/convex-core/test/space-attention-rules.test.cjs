const assert = require('node:assert/strict');
const test = require('node:test');

const rules = require('../convex/spaceAttentionRules.ts');
const {
  DEFAULT_ROOM_LEVEL,
  MAX_SNOOZE_MS,
  MIN_SNOOZE_MS,
  OUT_OF_APP_BUDGET_LIMIT,
  OUT_OF_APP_BUDGET_WINDOW_MS,
  checkSnooze,
  coalesceKey,
  consumeBudget,
  deliveryDecision,
  effectiveItemState,
  envFlagOn,
  itemExpiry,
  mergeAutoFollow,
  roomBadge,
  runStateToSignal,
} = rules;

function ctx(overrides = {}) {
  return {
    recipientAuthId: 'alice',
    actorAuthId: 'bob',
    recipientInAudience: true,
    following: true,
    isRunStarter: true,
    ...overrides,
  };
}

function decide(kind, overrides, expected) {
  const got = deliveryDecision(kind, ctx(overrides));
  assert.deepEqual(
    { item: got.item, outOfApp: got.outOfApp, reason: got.reason },
    expected,
    `${kind} ${JSON.stringify(overrides)}`,
  );
}

test('the default room level is mentions', () => {
  assert.equal(DEFAULT_ROOM_LEVEL, 'mentions');
});

test('self signals are dropped, except a reminder (the member\'s own signal)', () => {
  decide('mention', { actorAuthId: 'alice' }, { item: 'none', outOfApp: false, reason: 'self' });
  decide('reply', { actorAuthId: 'alice' }, { item: 'none', outOfApp: false, reason: 'self' });
  decide('reminder', { actorAuthId: 'alice' }, { item: 'create', outOfApp: true, reason: 'create' });
  decide('mention', { recipientAuthId: '  ' }, { item: 'none', outOfApp: false, reason: 'no_recipient' });
});

test('a recipient outside the audience gets nothing, even for a mention', () => {
  for (const kind of ['mention', 'invite', 'reply', 'agent_done', 'approval_needed']) {
    decide(kind, { recipientInAudience: false }, { item: 'none', outOfApp: false, reason: 'not_in_audience' });
  }
});

test('mention and invite pass level off and a muted thread', () => {
  for (const kind of ['mention', 'invite']) {
    decide(kind, { level: 'off', threadMuted: true }, { item: 'create', outOfApp: true, reason: 'create' });
  }
});

test('level off and a muted thread drop everything else', () => {
  for (const kind of ['reply', 'agent_done', 'agent_failed', 'agent_awaiting']) {
    decide(kind, { level: 'off' }, { item: 'none', outOfApp: false, reason: 'level_off' });
    decide(kind, { threadMuted: true }, { item: 'none', outOfApp: false, reason: 'thread_muted' });
  }
});

test('a reply needs a following recipient', () => {
  decide('reply', { following: false }, { item: 'none', outOfApp: false, reason: 'not_following' });
  decide('reply', { following: undefined }, { item: 'none', outOfApp: false, reason: 'not_following' });
  decide('reply', { following: true }, { item: 'create', outOfApp: true, reason: 'create' });
});

test('agent_done and agent_failed go to the run starter only', () => {
  for (const kind of ['agent_done', 'agent_failed']) {
    decide(kind, { isRunStarter: false }, { item: 'none', outOfApp: false, reason: 'not_run_starter' });
    decide(kind, { isRunStarter: true }, { item: 'create', outOfApp: true, reason: 'create' });
  }
});

test('an observed room suppresses reply and agent_done but keeps mentions and failures', () => {
  decide('reply', { observingAndSuppressed: true }, { item: 'none', outOfApp: false, reason: 'observed_room' });
  decide('agent_done', { observingAndSuppressed: true }, { item: 'none', outOfApp: false, reason: 'observed_room' });
  decide('mention', { observingAndSuppressed: true }, { item: 'create', outOfApp: true, reason: 'create' });
  decide('agent_failed', { observingAndSuppressed: true }, { item: 'create', outOfApp: true, reason: 'create' });
});

test('coalescing: create, increment on open, reopen on done, only count on snoozed', () => {
  decide('mention', { existing: null }, { item: 'create', outOfApp: true, reason: 'create' });
  decide('mention', { existing: { state: 'open' } }, { item: 'increment', outOfApp: false, reason: 'increment' });
  decide('mention', { existing: { state: 'done' } }, { item: 'reopen', outOfApp: true, reason: 'reopen' });
  decide(
    'mention',
    { existing: { state: 'snoozed', snoozedUntil: 9_999_999_999_999 } },
    { item: 'increment', outOfApp: false, reason: 'increment' },
  );
});

test('out-of-app is held back by dnd and an exhausted budget, but the item is still created', () => {
  decide('mention', { memberState: 'dnd' }, { item: 'create', outOfApp: false, reason: 'create' });
  decide('mention', { budgetAvailable: false }, { item: 'create', outOfApp: false, reason: 'create' });
  decide('mention', { budgetAvailable: true, memberState: 'away' }, { item: 'create', outOfApp: true, reason: 'create' });
});

test('level off keeps out-of-app for mention, invite and approval only', () => {
  decide('reminder', { level: 'off' }, { item: 'create', outOfApp: false, reason: 'create' });
  decide('invite', { level: 'off' }, { item: 'create', outOfApp: true, reason: 'create' });
  decide('approval_needed', { level: 'off' }, { item: 'none', outOfApp: true, reason: 'out_of_app_only' });
});

test('approval_needed and agent_awaiting create no item and only decide out-of-app', () => {
  decide('approval_needed', {}, { item: 'none', outOfApp: true, reason: 'out_of_app_only' });
  decide('agent_awaiting', {}, { item: 'none', outOfApp: true, reason: 'out_of_app_only' });
  decide('agent_awaiting', { memberState: 'dnd' }, { item: 'none', outOfApp: false, reason: 'out_of_app_only' });
  decide('approval_needed', { threadMuted: true }, { item: 'none', outOfApp: true, reason: 'out_of_app_only' });
  decide('agent_awaiting', { threadMuted: true }, { item: 'none', outOfApp: false, reason: 'thread_muted' });
});

test('precedence: self beats audience beats level beats follow', () => {
  decide(
    'reply',
    { actorAuthId: 'alice', recipientInAudience: false, level: 'off', following: false },
    { item: 'none', outOfApp: false, reason: 'self' },
  );
  decide(
    'reply',
    { recipientInAudience: false, level: 'off', following: false },
    { item: 'none', outOfApp: false, reason: 'not_in_audience' },
  );
  decide('reply', { level: 'off', following: false }, { item: 'none', outOfApp: false, reason: 'level_off' });
  decide(
    'reply',
    { following: false, observingAndSuppressed: true },
    { item: 'none', outOfApp: false, reason: 'not_following' },
  );
});

test('consumeBudget: first call opens a window, count rises to the limit, then denies without counting', () => {
  assert.equal(OUT_OF_APP_BUDGET_LIMIT, 30);
  assert.equal(OUT_OF_APP_BUDGET_WINDOW_MS, 3_600_000);
  let state = null;
  for (let i = 1; i <= 30; i += 1) {
    const step = consumeBudget(state, 1000 + i);
    assert.equal(step.allowed, true);
    assert.equal(step.next.count, i);
    assert.equal(step.next.windowStart, i === 1 ? 1001 : 1001, 'window start is fixed by the first call');
    state = step.next;
  }
  const denied = consumeBudget(state, 5000);
  assert.equal(denied.allowed, false);
  assert.equal(denied.next.count, 30);
});

test('consumeBudget windows: restarts at the window end, and on a clock that moved backwards', () => {
  const state = { windowStart: 1000, count: 30 };
  assert.equal(consumeBudget(state, 1000 + 3_600_000 - 1).allowed, false);
  const reopened = consumeBudget(state, 1000 + 3_600_000);
  assert.deepEqual(reopened, { allowed: true, next: { windowStart: 1000 + 3_600_000, count: 1 } });
  const backwards = consumeBudget(state, 500);
  assert.deepEqual(backwards, { allowed: true, next: { windowStart: 500, count: 1 } });
  assert.equal(consumeBudget(null, 10, 0).allowed, false, 'a zero limit never allows');
  assert.equal(consumeBudget({ windowStart: 0, count: 2 }, 10, 2, 100).allowed, false, 'custom limit');
});

test('runStateToSignal maps run states to signals', () => {
  for (const word of ['completed', 'succeeded', 'done', 'unseen_done', ' Completed ']) {
    assert.deepEqual(runStateToSignal(word), { item: 'agent_done', outOfApp: null, activity: 'idle' });
  }
  for (const word of ['failed', 'errored', 'error']) {
    assert.deepEqual(runStateToSignal(word), { item: 'agent_failed', outOfApp: null, activity: 'idle' });
  }
  assert.deepEqual(runStateToSignal('awaiting_approval'), { item: null, outOfApp: 'approval_needed', activity: 'waiting' });
  assert.deepEqual(runStateToSignal('needs_input'), { item: null, outOfApp: 'agent_awaiting', activity: 'waiting' });
  for (const word of ['running', 'queued']) {
    assert.deepEqual(runStateToSignal(word), { item: null, outOfApp: null, activity: 'working' });
  }
  for (const word of ['cancelled', 'canceled', 'parked', 'idle']) {
    assert.deepEqual(runStateToSignal(word), { item: null, outOfApp: null, activity: 'idle' });
  }
  for (const word of ['mystery', '', null, undefined]) {
    assert.deepEqual(runStateToSignal(word), { item: null, outOfApp: null, activity: null });
  }
});

test('mergeAutoFollow: muted is never overwritten, manual is never downgraded', () => {
  assert.deepEqual(mergeAutoFollow(null, 'started'), { state: 'following', reason: 'started' });
  assert.equal(mergeAutoFollow({ state: 'muted', reason: 'manual' }, 'mentioned'), null);
  assert.equal(mergeAutoFollow({ state: 'muted', reason: 'participated' }, 'started'), null);
  assert.equal(mergeAutoFollow({ state: 'following', reason: 'manual' }, 'started'), null);
  assert.equal(mergeAutoFollow({ state: 'following', reason: 'started' }, 'started'), null);
  assert.deepEqual(mergeAutoFollow({ state: 'following', reason: 'participated' }, 'mentioned'), {
    state: 'following',
    reason: 'mentioned',
  });
});

test('roomBadge table', () => {
  const badge = (needsYou, mentions, level, unread) => roomBadge({ needsYou, mentions, level, unread });
  for (const level of ['all', 'mentions', 'off']) {
    assert.deepEqual(badge(1, 0, level, false), { kind: 'count', n: 1 });
    assert.deepEqual(badge(2, 3, level, true), { kind: 'count', n: 5 });
    assert.deepEqual(badge(0, 4, level, false), { kind: 'count', n: 4 });
    assert.deepEqual(badge(0, 0, level, false), { kind: 'none', n: 0 });
  }
  assert.deepEqual(badge(0, 0, 'all', true), { kind: 'count', n: 0 });
  assert.deepEqual(badge(0, 0, 'mentions', true), { kind: 'dot', n: 0 });
  assert.deepEqual(badge(0, 0, 'off', true), { kind: 'none', n: 0 });
  assert.deepEqual(badge(-3, 0, 'all', false), { kind: 'none', n: 0 }, 'negative counts clamp to zero');
});

test('checkSnooze accepts now+1 minute through now+30 days only', () => {
  const now = 1_000_000;
  assert.deepEqual(checkSnooze(now + MIN_SNOOZE_MS, now), { ok: true, snoozedUntil: now + MIN_SNOOZE_MS });
  assert.deepEqual(checkSnooze(now + MAX_SNOOZE_MS, now), { ok: true, snoozedUntil: now + MAX_SNOOZE_MS });
  assert.deepEqual(checkSnooze(now + MIN_SNOOZE_MS - 1, now), { ok: false });
  assert.deepEqual(checkSnooze(now + MAX_SNOOZE_MS + 1, now), { ok: false });
  assert.deepEqual(checkSnooze(now + 90_000.7, now), { ok: true, snoozedUntil: now + 90_000 });
  for (const bad of ['10', NaN, Infinity, null, undefined, {}]) {
    assert.deepEqual(checkSnooze(bad, now), { ok: false });
  }
});

test('effectiveItemState treats an elapsed snooze as open', () => {
  assert.equal(effectiveItemState({ state: 'snoozed', snoozedUntil: 100 }, 100), 'open');
  assert.equal(effectiveItemState({ state: 'snoozed', snoozedUntil: 101 }, 100), 'snoozed');
  assert.equal(effectiveItemState({ state: 'done' }, 100), 'done');
  assert.equal(effectiveItemState({ state: 'open' }, 100), 'open');
});

test('itemExpiry is 30 days for live rows and 14 days for done rows', () => {
  const day = 24 * 60 * 60 * 1000;
  assert.equal(itemExpiry('open', 0), 30 * day);
  assert.equal(itemExpiry('snoozed', 0), 30 * day);
  assert.equal(itemExpiry('done', 0), 14 * day);
});

test('coalesceKey prefers the thread and falls back to the Space', () => {
  assert.equal(coalesceKey('mention', { threadId: ' t1 ', spaceRef: 's1' }), 'mention:t:t1');
  assert.equal(coalesceKey('mention', { threadId: '', spaceRef: ' s1 ' }), 'mention:s:s1');
  assert.equal(coalesceKey('invite', { spaceRef: 's1' }), 'invite:s:s1');
});

test('envFlagOn: unset takes the default, empty and off words are off, anything else on', () => {
  assert.equal(envFlagOn(undefined), false);
  assert.equal(envFlagOn(undefined, true), true);
  assert.equal(envFlagOn(null, true), true);
  assert.equal(envFlagOn(''), false);
  assert.equal(envFlagOn('  ', true), false);
  for (const off of ['0', 'false', 'OFF', ' No ']) assert.equal(envFlagOn(off, true), false);
  for (const on of ['1', 'true', 'yes', 'on']) assert.equal(envFlagOn(on), true);
});
