const assert = require('node:assert/strict');
const test = require('node:test');

const rules = require('../convex/spaceRemindersRules.ts');
const reminders = require('../convex/spaceReminders.ts');
const { AUTH, ORG, SERVICE_KEY, makeCtx, memberTables, rejectsWith, withServiceKey } = require('./helpers/fake-convex.cjs');

const {
  HEAL_GRACE_MS,
  MAX_PENDING_REMINDERS,
  MAX_REMINDER_LEAD_MS,
  MIN_REMINDER_LEAD_MS,
  SETTLED_RETENTION_MS,
  checkReminderTime,
  reminderEventId,
} = rules;
const { cancelReminderForGateway, createReminderForGateway, fireReminder, healReminders, listPendingRemindersForGateway } = reminders;

const NOW = 1_800_000_000_000;
const DAY = 24 * 60 * 60 * 1000;

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

test('checkReminderTime refuses past, too-soon, too-far and non-finite times', () => {
  assert.equal(MIN_REMINDER_LEAD_MS, 5000);
  assert.equal(MAX_REMINDER_LEAD_MS, 366 * DAY);
  assert.deepEqual(checkReminderTime(NOW - 1, NOW), { ok: false });
  assert.deepEqual(checkReminderTime(NOW, NOW), { ok: false });
  assert.deepEqual(checkReminderTime(NOW + 4999, NOW), { ok: false });
  assert.deepEqual(checkReminderTime(NOW + MAX_REMINDER_LEAD_MS + 1, NOW), { ok: false });
  for (const bad of [NaN, Infinity, -Infinity, '123', null, undefined, {}, true]) {
    assert.deepEqual(checkReminderTime(bad, NOW), { ok: false }, String(bad));
  }
});

test('checkReminderTime accepts the edges and truncates to whole milliseconds', () => {
  assert.deepEqual(checkReminderTime(NOW + 5000, NOW), { ok: true, remindAt: NOW + 5000 });
  assert.deepEqual(checkReminderTime(NOW + MAX_REMINDER_LEAD_MS, NOW), { ok: true, remindAt: NOW + MAX_REMINDER_LEAD_MS });
  assert.deepEqual(checkReminderTime(NOW + 60000.9, NOW), { ok: true, remindAt: NOW + 60000 });
});

test('reminderEventId prefixes the reminder id so it cannot collide with a Session Core event id', () => {
  assert.equal(reminderEventId('abc123'), 'reminder:abc123');
});

test('the limits match the design', () => {
  assert.equal(MAX_PENDING_REMINDERS, 50);
  assert.equal(HEAL_GRACE_MS, 60_000);
  assert.equal(SETTLED_RETENTION_MS, 7 * DAY);
});

// --- create / list / cancel -----------------------------------------------------------------

const base = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG };
const create = { ...base, spaceRef: 'space_1', threadId: 'thread_1', messageId: 'msg_1' };

function world(extra = {}) {
  return memberTables({
    spaces: [{ spaceRef: 'space_1', externalOrgId: ORG, lifecycle: 'active' }],
    spaceReminders: [],
    ...extra,
  });
}

function pendingRow(i, extra = {}) {
  return {
    externalOrgId: ORG,
    externalAuthId: AUTH,
    spaceRef: 'space_1',
    threadId: `thread_${i}`,
    remindAt: Date.now() + DAY + i,
    state: 'pending',
    createdAt: 1,
    ...extra,
  };
}

test('createReminderForGateway stores a pending row, schedules fireReminder and returns the view', async (t) => {
  withServiceKey(t);
  const { ctx, tables, log } = makeCtx(world());
  const remindAt = Date.now() + 60_000;
  const result = await createReminderForGateway._handler(ctx, { ...create, remindAt });
  assert.equal(result.deduplicated, false);
  assert.equal(result.state, 'pending');
  assert.equal(result.remindAt, remindAt);
  assert.equal(result.messageId, 'msg_1');
  assert.equal(tables.spaceReminders.length, 1);
  assert.equal(tables.spaceReminders[0].externalAuthId, AUTH);
  assert.equal('note' in tables.spaceReminders[0], false, 'there is no note field');
  assert.equal(log.scheduled.length, 1);
  assert.deepEqual(log.scheduled[0].args, { reminderId: result.reminderId });
  assert.ok(log.scheduled[0].delay > 0 && log.scheduled[0].delay <= 60_000);
});

test('create is idempotent on an identical pending reminder', async (t) => {
  withServiceKey(t);
  const { ctx, tables, log } = makeCtx(world());
  const remindAt = Date.now() + 60_000;
  const first = await createReminderForGateway._handler(ctx, { ...create, remindAt });
  const again = await createReminderForGateway._handler(ctx, { ...create, remindAt });
  assert.equal(again.deduplicated, true);
  assert.equal(again.reminderId, first.reminderId);
  assert.equal(tables.spaceReminders.length, 1);
  assert.equal(log.scheduled.length, 1, 'no second schedule');
  // A different time or message is a different reminder.
  await createReminderForGateway._handler(ctx, { ...create, remindAt: remindAt + 1000 });
  await createReminderForGateway._handler(ctx, { ...create, messageId: undefined, remindAt });
  assert.equal(tables.spaceReminders.length, 3);
});

test('the 51st pending reminder is refused with too_many_reminders', async (t) => {
  withServiceKey(t);
  const rows = Array.from({ length: 50 }, (_, i) => pendingRow(i));
  const { ctx, tables } = makeCtx(world({ spaceReminders: rows }));
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, remindAt: Date.now() + 2 * DAY }), 'too_many_reminders');
  assert.equal(tables.spaceReminders.length, 50);
  // Other people's and other orgs' reminders do not count against the caller.
  const others = [pendingRow(1, { externalAuthId: 'someone' }), pendingRow(2, { externalOrgId: 'org_other' })];
  const roomy = makeCtx(world({ spaceReminders: others }));
  await createReminderForGateway._handler(roomy.ctx, { ...create, remindAt: Date.now() + 2 * DAY });
  assert.equal(roomy.tables.spaceReminders.length, 3);
});

test('create refuses a bad time, bad ids, an unknown space and another org\'s space', async (t) => {
  withServiceKey(t);
  const { ctx, log } = makeCtx(world({ spaces: [{ spaceRef: 'space_1', externalOrgId: ORG, lifecycle: 'active' }, { spaceRef: 'space_x', externalOrgId: 'org_other', lifecycle: 'active' }] }));
  const ok = Date.now() + 60_000;
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, remindAt: Date.now() - 1000 }), 'invalid_reminder_time');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, remindAt: Date.now() + 1000 }), 'invalid_reminder_time');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, remindAt: Date.now() + 400 * DAY }), 'invalid_reminder_time');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, remindAt: NaN }), 'invalid_reminder_time');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, spaceRef: ' ', remindAt: ok }), 'invalid_space_ref');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, threadId: '', remindAt: ok }), 'invalid_thread_id');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, messageId: 'x'.repeat(201), remindAt: ok }), 'invalid_message_id');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, spaceRef: 'nope', remindAt: ok }), 'space_not_found');
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, spaceRef: 'space_x', remindAt: ok }), 'space_not_found');
  assert.deepEqual(log.inserts, []);
});

test('create and list need the service key and an organization member', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(world());
  const remindAt = Date.now() + 60_000;
  await assert.rejects(createReminderForGateway._handler(ctx, { ...create, serviceKey: 'wrong', remindAt }));
  await rejectsWith(createReminderForGateway._handler(ctx, { ...create, externalAuthId: 'stranger', remindAt }), 'Unauthorized');
  await assert.rejects(listPendingRemindersForGateway._handler(ctx, { ...base, serviceKey: 'wrong' }));
});

test('list returns only the caller\'s pending reminders, soonest first', async (t) => {
  withServiceKey(t);
  const soon = Date.now() + 10 * DAY;
  const { ctx } = makeCtx(
    world({
      spaceReminders: [
        pendingRow(1, { remindAt: soon + 3000 }),
        pendingRow(2, { remindAt: soon + 1000 }),
        pendingRow(3, { remindAt: soon + 2000, state: 'fired' }),
        pendingRow(4, { remindAt: soon, externalAuthId: 'someone' }),
        pendingRow(5, { remindAt: soon, externalOrgId: 'org_other' }),
      ],
    }),
  );
  const list = await listPendingRemindersForGateway._handler(ctx, base);
  assert.deepEqual(list.map((entry) => entry.threadId), ['thread_2', 'thread_1']);
  assert.deepEqual(Object.keys(list[0]).sort(), ['createdAt', 'messageId', 'remindAt', 'reminderId', 'spaceRef', 'state', 'threadId']);
});

test('cancel marks the caller\'s pending reminder cancelled, and cancelling again is still cancelled:true', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(world({ spaceReminders: [pendingRow(1)] }));
  const id = tables.spaceReminders[0]._id;
  assert.deepEqual(await cancelReminderForGateway._handler(ctx, { ...base, reminderId: id }), { reminderId: id, cancelled: true });
  assert.equal(tables.spaceReminders[0].state, 'cancelled');
  assert.deepEqual(await cancelReminderForGateway._handler(ctx, { ...base, reminderId: id }), { reminderId: id, cancelled: true });
});

test('cancelling a foreign, unknown, malformed or already-fired reminder is reminder_not_found', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(
    world({
      spaceReminders: [
        pendingRow(1, { externalAuthId: 'someone' }),
        pendingRow(2, { externalOrgId: 'org_other' }),
        pendingRow(3, { state: 'fired' }),
        pendingRow(4, { state: 'skipped' }),
      ],
    }),
  );
  for (const row of tables.spaceReminders) {
    await rejectsWith(cancelReminderForGateway._handler(ctx, { ...base, reminderId: row._id }), 'reminder_not_found');
  }
  await rejectsWith(cancelReminderForGateway._handler(ctx, { ...base, reminderId: 'spaceReminders_999' }), 'reminder_not_found');
  await rejectsWith(cancelReminderForGateway._handler(ctx, { ...base, reminderId: 'garbage' }), 'reminder_not_found');
  assert.deepEqual(
    tables.spaceReminders.map((row) => row.state),
    ['pending', 'pending', 'fired', 'skipped'],
    'nothing changed',
  );
});

// --- fireReminder ---------------------------------------------------------------------------

function fireWorld(rowExtra = {}, extra = {}) {
  return memberTables({
    spaces: [{ spaceRef: 'space_1', externalOrgId: ORG, lifecycle: 'active' }],
    spaceRecipientAudiences: [
      { spaceRef: 'space_1', state: 'active', controlState: 'acknowledged', externalOrgId: ORG, recipientExternalAuthIds: [AUTH, 'bob'] },
    ],
    spaceReminders: [
      {
        externalOrgId: ORG,
        externalAuthId: AUTH,
        spaceRef: 'space_1',
        threadId: 'thread_1',
        messageId: 'msg_1',
        remindAt: Date.now() - 1000,
        state: 'pending',
        createdAt: 1,
        ...rowExtra,
      },
    ],
    ...extra,
  });
}

test('fireReminder writes a reminder item (ids only), marks the row fired and sends nothing out of app by default', async (t) => {
  withEnv(t, { SPACE_ATTENTION_OUT_OF_APP: undefined });
  const { ctx, tables, log } = makeCtx(fireWorld());
  const reminderId = tables.spaceReminders[0]._id;
  const result = await fireReminder._handler(ctx, { reminderId });
  assert.deepEqual(result, { status: 'fired', reason: 'create' });
  assert.equal(tables.spaceReminders[0].state, 'fired');
  assert.equal(tables.spaceAttentionItems.length, 1);
  const item = tables.spaceAttentionItems[0];
  assert.equal(item.recipientAuthId, AUTH);
  assert.equal(item.kind, 'reminder');
  assert.equal(item.threadId, 'thread_1');
  assert.equal(item.messageId, 'msg_1');
  assert.equal(item.actorKind, 'system');
  assert.equal(item.firstEventId, `reminder:${reminderId}`);
  assert.equal(item.coalesceKey, 'reminder:t:thread_1');
  assert.deepEqual(log.scheduled, []);
});

test('fireReminder with the out-of-app flag on schedules one courier action and takes budget', async (t) => {
  withEnv(t, { SPACE_ATTENTION_OUT_OF_APP: '1' });
  const { ctx, tables, log } = makeCtx(fireWorld());
  const reminderId = tables.spaceReminders[0]._id;
  await fireReminder._handler(ctx, { reminderId });
  assert.equal(log.scheduled.length, 1);
  assert.equal(log.scheduled[0].args.kind, 'reminder');
  assert.equal(log.scheduled[0].args.eventId, `reminder:${reminderId}`);
  assert.equal(log.scheduled[0].args.recipientAuthId, AUTH);
  assert.equal(log.scheduled[0].args.actorKind, 'system');
  assert.equal(tables.spaceAttentionBudget[0].count, 1);
});

test('fireReminder does not go out of the app for a member in dnd, but still writes the item', async (t) => {
  withEnv(t, { SPACE_ATTENTION_OUT_OF_APP: '1' });
  const { ctx, tables, log } = makeCtx(
    fireWorld({}, { spaceMemberStatus: [{ externalOrgId: ORG, externalAuthId: AUTH, state: 'dnd', updatedAt: 1 }] }),
  );
  const result = await fireReminder._handler(ctx, { reminderId: tables.spaceReminders[0]._id });
  assert.equal(result.status, 'fired');
  assert.equal(tables.spaceAttentionItems.length, 1);
  assert.deepEqual(log.scheduled, []);
});

test('a second reminder on the same thread counts up the open item instead of adding a row', async (t) => {
  withEnv(t, { SPACE_ATTENTION_OUT_OF_APP: undefined });
  const second = {
    externalOrgId: ORG,
    externalAuthId: AUTH,
    spaceRef: 'space_1',
    threadId: 'thread_1',
    remindAt: Date.now() - 500,
    state: 'pending',
    createdAt: 2,
  };
  const { ctx, tables } = makeCtx(fireWorld({}, {}));
  tables.spaceReminders.push({ _id: 'spaceReminders_77', _creationTime: 77, ...second });
  await fireReminder._handler(ctx, { reminderId: tables.spaceReminders[0]._id });
  await fireReminder._handler(ctx, { reminderId: 'spaceReminders_77' });
  assert.equal(tables.spaceAttentionItems.length, 1);
  assert.equal(tables.spaceAttentionItems[0].count, 2);
});

test('fireReminder skips a removed member, a member outside the audience and an inactive space', async () => {
  const gone = makeCtx(fireWorld({ externalAuthId: 'departed' }));
  assert.deepEqual(await fireReminder._handler(gone.ctx, { reminderId: gone.tables.spaceReminders[0]._id }), { status: 'skipped', reason: 'not_in_org' });
  assert.equal(gone.tables.spaceReminders[0].state, 'skipped');

  const outside = makeCtx(fireWorld());
  outside.tables.spaceRecipientAudiences[0].recipientExternalAuthIds = ['bob'];
  assert.deepEqual(await fireReminder._handler(outside.ctx, { reminderId: outside.tables.spaceReminders[0]._id }), { status: 'skipped', reason: 'not_in_audience' });

  const unacknowledged = makeCtx(fireWorld());
  unacknowledged.tables.spaceRecipientAudiences[0].controlState = 'pending';
  assert.equal((await fireReminder._handler(unacknowledged.ctx, { reminderId: unacknowledged.tables.spaceReminders[0]._id })).reason, 'not_in_audience');

  const archived = makeCtx(fireWorld());
  archived.tables.spaces[0].lifecycle = 'archived';
  assert.deepEqual(await fireReminder._handler(archived.ctx, { reminderId: archived.tables.spaceReminders[0]._id }), { status: 'skipped', reason: 'space_unavailable' });

  const otherOrg = makeCtx(fireWorld());
  otherOrg.tables.spaces[0].externalOrgId = 'org_other';
  assert.equal((await fireReminder._handler(otherOrg.ctx, { reminderId: otherOrg.tables.spaceReminders[0]._id })).reason, 'space_unavailable');

  for (const w of [gone, outside, unacknowledged, archived, otherOrg]) {
    assert.equal((w.tables.spaceAttentionItems ?? []).length, 0);
  }
});

test('fireReminder ignores a row that is not pending, or not yet due', async () => {
  for (const state of ['fired', 'cancelled', 'skipped']) {
    const { ctx, tables, log } = makeCtx(fireWorld({ state }));
    assert.deepEqual(await fireReminder._handler(ctx, { reminderId: tables.spaceReminders[0]._id }), { status: 'ignored', reason: 'not_pending' });
    assert.equal((tables.spaceAttentionItems ?? []).length, 0);
    assert.deepEqual(log.patches, []);
  }
  const early = makeCtx(fireWorld({ remindAt: Date.now() + 60_000 }));
  assert.deepEqual(await fireReminder._handler(early.ctx, { reminderId: early.tables.spaceReminders[0]._id }), { status: 'ignored', reason: 'not_due' });
  assert.equal(early.tables.spaceReminders[0].state, 'pending');
  const missing = makeCtx(fireWorld());
  assert.deepEqual(await fireReminder._handler(missing.ctx, { reminderId: 'spaceReminders_404' }), { status: 'ignored', reason: 'not_pending' });
});

test('a cancelled reminder fires nothing when its schedule still runs', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(fireWorld());
  const id = tables.spaceReminders[0]._id;
  await cancelReminderForGateway._handler(ctx, { ...base, reminderId: id });
  assert.equal((await fireReminder._handler(ctx, { reminderId: id })).status, 'ignored');
  assert.equal((tables.spaceAttentionItems ?? []).length, 0);
});

// --- healReminders --------------------------------------------------------------------------

test('healReminders reschedules overdue pending reminders and leaves fresh and future ones', async () => {
  const now = Date.now();
  const { ctx, log } = makeCtx({
    spaceReminders: [
      { externalOrgId: ORG, externalAuthId: AUTH, state: 'pending', remindAt: now - HEAL_GRACE_MS - 1000 },
      { externalOrgId: ORG, externalAuthId: AUTH, state: 'pending', remindAt: now - 1000 },
      { externalOrgId: ORG, externalAuthId: AUTH, state: 'pending', remindAt: now + DAY },
    ],
  });
  const result = await healReminders._handler(ctx, {});
  assert.deepEqual(result, { rescheduled: 1, deleted: 0 });
  assert.equal(log.scheduled.length, 1);
  assert.equal(log.scheduled[0].delay, 0);
  assert.equal(typeof log.scheduled[0].args.reminderId, 'string');
});

test('healReminders deletes settled rows past seven days and keeps recent ones', async () => {
  const now = Date.now();
  const old = now - SETTLED_RETENTION_MS - 1000;
  const recent = now - DAY;
  const { ctx, tables } = makeCtx({
    spaceReminders: [
      { externalOrgId: ORG, state: 'fired', remindAt: old },
      { externalOrgId: ORG, state: 'cancelled', remindAt: old },
      { externalOrgId: ORG, state: 'skipped', remindAt: old },
      { externalOrgId: ORG, state: 'fired', remindAt: recent },
      { externalOrgId: ORG, state: 'pending', remindAt: old + 1 },
    ],
  });
  const result = await healReminders._handler(ctx, {});
  assert.equal(result.deleted, 3);
  assert.deepEqual(
    tables.spaceReminders.map((row) => [row.state, row.remindAt === recent]),
    [['fired', true], ['pending', false]],
  );
});
