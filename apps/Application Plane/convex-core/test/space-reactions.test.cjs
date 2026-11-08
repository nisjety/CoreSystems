const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const reactions = require('../convex/spaceReactions.ts');
const { AUTH, ORG, SERVICE_KEY, makeCtx, memberTables, rejectsWith, withServiceKey } = require('./helpers/fake-convex.cjs');

const {
  MAX_DISTINCT_EMOJI_PER_MESSAGE,
  MAX_EMOJI_BYTES,
  clearForMessageForGateway,
  groupForThread,
  reactionsEnabled,
  reactionsForThreadForGateway,
  setForGateway,
  summarizeMessage,
  validateEmoji,
} = reactions;

function withReactions(t, value = '1') {
  const prior = process.env.SPACE_REACTIONS;
  if (value === null) delete process.env.SPACE_REACTIONS;
  else process.env.SPACE_REACTIONS = value;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_REACTIONS;
    else process.env.SPACE_REACTIONS = prior;
  });
}

const base = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG, spaceRef: 'space_1', threadId: 'thread_1', messageId: 'msg_1' };

function row(emoji, who, createdAt, extra = {}) {
  return { externalOrgId: ORG, spaceRef: 'space_1', threadId: 'thread_1', messageId: 'msg_1', emoji, externalAuthId: who, createdAt, ...extra };
}

test('validateEmoji accepts one pictographic cluster in its several forms', () => {
  const accepted = {
    'plain': '\u{1F44D}',
    'variation selector': '❤️',
    'bare symbol': '✅',
    'skin tone': '\u{1F44D}\u{1F3FD}',
    'keycap': '1️⃣',
    'keycap without selector': '#⃣',
    'flag': '\u{1F1F3}\u{1F1F4}',
    'ZWJ family': '\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}',
    'tag flag (Scotland)': '\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}',
  };
  for (const [name, emoji] of Object.entries(accepted)) {
    assert.equal(validateEmoji(emoji), emoji, name);
    assert.ok(new TextEncoder().encode(emoji).length <= MAX_EMOJI_BYTES, name);
  }
});

test('validateEmoji returns the NFC form', () => {
  // NFC leaves emoji alone; a decomposed keycap-free input stays one cluster and is idempotent.
  const emoji = '❤️';
  assert.equal(validateEmoji(emoji), emoji.normalize('NFC'));
});

test('validateEmoji rejects text, two emoji, whitespace, empty and non-strings', () => {
  const rejected = {
    'text': 'ok',
    'letter': 'a',
    'two emoji': '\u{1F44D}\u{1F44D}',
    'emoji plus text': '\u{1F44D}x',
    'leading space': ' \u{1F44D}',
    'trailing space': '\u{1F44D} ',
    'only space': ' ',
    'newline': '\u{1F44D}\n',
    'empty': '',
    'a lone regional indicator': '\u{1F1F3}',
    'a lone ZWJ': '‍',
    'a lone modifier': '\u{1F3FD}',
    'a keycap with a letter': 'a⃣',
    'too long (over 32 bytes)': '\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}‍\u{1F468}‍\u{1F469}',
  };
  for (const [name, value] of Object.entries(rejected)) assert.equal(validateEmoji(value), null, name);
  for (const value of [undefined, null, 5, {}, ['\u{1F44D}']]) assert.equal(validateEmoji(value), null);
});

test('summarizeMessage counts per emoji, marks reactedByMe and orders by first use', () => {
  const rows = [
    row('\u{1F389}', 'bob', 30),
    row('\u{1F44D}', 'alice', 10),
    row('\u{1F44D}', 'bob', 20),
    row('\u{1F389}', 'alice', 40),
    row('❤️', 'bob', 15),
  ];
  assert.deepEqual(summarizeMessage(rows, 'alice'), [
    { emoji: '\u{1F44D}', count: 2, reactedByMe: true },
    { emoji: '❤️', count: 1, reactedByMe: false },
    { emoji: '\u{1F389}', count: 2, reactedByMe: true },
  ]);
  assert.deepEqual(summarizeMessage(rows, 'nobody').map((entry) => entry.reactedByMe), [false, false, false]);
  assert.deepEqual(summarizeMessage([], 'alice'), []);
});

test('groupForThread groups by message (sorted) and keeps first-use order inside each', () => {
  const rows = [
    row('\u{1F44D}', 'alice', 5, { messageId: 'msg_b' }),
    row('\u{1F44D}', 'bob', 1, { messageId: 'msg_a' }),
    row('\u{1F389}', 'alice', 2, { messageId: 'msg_a' }),
    row('\u{1F44D}', 'alice', 3, { messageId: 'msg_a' }),
  ];
  assert.deepEqual(groupForThread(rows, 'alice'), [
    { messageId: 'msg_a', emoji: '\u{1F44D}', count: 2, reactedByMe: true },
    { messageId: 'msg_a', emoji: '\u{1F389}', count: 1, reactedByMe: true },
    { messageId: 'msg_b', emoji: '\u{1F44D}', count: 1, reactedByMe: true },
  ]);
});

test('reactionsEnabled accepts only 1 or true', (t) => {
  for (const [value, expected] of [[null, false], ['', false], ['0', false], ['off', false], ['1', true], ['true', true], [' TRUE ', true]]) {
    withReactions(t, value);
    assert.equal(reactionsEnabled(), expected, String(value));
  }
});

test('adding a reaction stores ids and one emoji, and returns the message summary', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx, tables } = makeCtx(memberTables({ spaceReactions: [] }));
  const result = await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: true });
  assert.deepEqual(result, { messageId: 'msg_1', reactions: [{ emoji: '\u{1F44D}', count: 1, reactedByMe: true }] });
  assert.equal(tables.spaceReactions.length, 1);
  const stored = tables.spaceReactions[0];
  assert.deepEqual(
    { org: stored.externalOrgId, space: stored.spaceRef, thread: stored.threadId, message: stored.messageId, who: stored.externalAuthId, emoji: stored.emoji },
    { org: ORG, space: 'space_1', thread: 'thread_1', message: 'msg_1', who: AUTH, emoji: '\u{1F44D}' },
  );
  assert.deepEqual(Object.keys(stored).sort(), ['_creationTime', '_id', 'createdAt', 'emoji', 'externalAuthId', 'externalOrgId', 'messageId', 'spaceRef', 'threadId']);
});

test('adding the same reaction twice is idempotent (one row)', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx, tables, log } = makeCtx(memberTables({ spaceReactions: [] }));
  await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: true });
  const again = await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: true });
  assert.equal(tables.spaceReactions.length, 1);
  assert.equal(log.inserts.length, 1);
  assert.equal(again.reactions[0].count, 1);
});

test('removing a reaction deletes the row, and removing it twice is a no-op', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx, tables } = makeCtx(memberTables({ spaceReactions: [row('\u{1F44D}', AUTH, 1), row('\u{1F44D}', 'other', 2)] }));
  const first = await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: false });
  assert.deepEqual(first.reactions, [{ emoji: '\u{1F44D}', count: 1, reactedByMe: false }]);
  assert.equal(tables.spaceReactions.length, 1);
  const second = await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: false });
  assert.deepEqual(second.reactions, first.reactions);
  assert.equal(tables.spaceReactions.length, 1);
});

test('removing a reaction that was never added changes nothing', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx, log } = makeCtx(memberTables({ spaceReactions: [] }));
  const result = await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: false });
  assert.deepEqual(result.reactions, []);
  assert.deepEqual(log.deletes, []);
});

test('the 21st distinct emoji on a message is refused with reaction_limit; an existing emoji still counts up', async (t) => {
  withServiceKey(t);
  withReactions(t);
  assert.equal(MAX_DISTINCT_EMOJI_PER_MESSAGE, 20);
  const emojis = Array.from({ length: 20 }, (_, i) => String.fromCodePoint(0x1f600 + i));
  const existing = emojis.map((emoji, i) => row(emoji, 'other', i));
  const { ctx, tables } = makeCtx(memberTables({ spaceReactions: existing }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: true }), 'reaction_limit');
  assert.equal(tables.spaceReactions.length, 20);
  const joined = await setForGateway._handler(ctx, { ...base, emoji: emojis[0], present: true });
  assert.equal(joined.reactions.find((entry) => entry.emoji === emojis[0]).count, 2);
  assert.equal(tables.spaceReactions.length, 21);
});

test('an invalid emoji is invalid_emoji and an invalid id is reaction_invalid:<field>', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx, log } = makeCtx(memberTables({ spaceReactions: [] }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, emoji: 'hello', present: true }), 'invalid_emoji');
  await rejectsWith(setForGateway._handler(ctx, { ...base, spaceRef: '  ', emoji: '\u{1F44D}', present: true }), 'reaction_invalid:space_ref');
  await rejectsWith(setForGateway._handler(ctx, { ...base, threadId: 'a\u0000b', emoji: '\u{1F44D}', present: true }), 'reaction_invalid:thread_id');
  await rejectsWith(setForGateway._handler(ctx, { ...base, messageId: 'm'.repeat(201), emoji: '\u{1F44D}', present: true }), 'reaction_invalid:message_id');
  assert.deepEqual(log.inserts, []);
});

test('reactions are scoped by organization, Space and thread', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx, tables } = makeCtx(
    memberTables({
      spaceReactions: [
        row('\u{1F44D}', 'o1', 1, { externalOrgId: 'org_other' }),
        row('\u{1F44D}', 'o2', 2, { spaceRef: 'space_2' }),
        row('\u{1F44D}', 'o3', 3, { threadId: 'thread_2' }),
        row('\u{1F44D}', 'o4', 4),
      ],
    }),
  );
  const result = await setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: true });
  assert.deepEqual(result.reactions, [{ emoji: '\u{1F44D}', count: 2, reactedByMe: true }], 'only the same org, space and thread are counted');
  assert.equal(tables.spaceReactions.length, 5);
});

test('with the flag off a write throws reactions_disabled and nothing is written', async (t) => {
  withServiceKey(t);
  withReactions(t, null);
  const { ctx, log } = makeCtx(memberTables({ spaceReactions: [] }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: true }), 'reactions_disabled');
  await rejectsWith(setForGateway._handler(ctx, { ...base, emoji: '\u{1F44D}', present: false }), 'reactions_disabled');
  assert.deepEqual(log.inserts, []);
});

test('with the flag off a thread read answers enabled:false and an empty list', async (t) => {
  withServiceKey(t);
  withReactions(t, null);
  const { ctx } = makeCtx(memberTables({ spaceReactions: [row('\u{1F44D}', AUTH, 1)] }));
  const args = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG, spaceRef: 'space_1', threadId: 'thread_1' };
  assert.deepEqual(await reactionsForThreadForGateway._handler(ctx, args), { enabled: false, truncated: false, reactions: [] });
});

test('a thread read groups per message and scopes by org', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx } = makeCtx(
    memberTables({
      spaceReactions: [
        row('\u{1F44D}', AUTH, 1),
        row('\u{1F44D}', 'bob', 2),
        row('\u{1F389}', 'bob', 3, { messageId: 'msg_2' }),
        row('\u{1F44D}', 'spy', 4, { externalOrgId: 'org_other' }),
      ],
    }),
  );
  const args = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG, spaceRef: 'space_1', threadId: 'thread_1' };
  assert.deepEqual(await reactionsForThreadForGateway._handler(ctx, args), {
    enabled: true,
    truncated: false,
    reactions: [
      { messageId: 'msg_1', emoji: '\u{1F44D}', count: 2, reactedByMe: true },
      { messageId: 'msg_2', emoji: '\u{1F389}', count: 1, reactedByMe: false },
    ],
  });
});

test('reads and writes need the service key and an organization member', async (t) => {
  withServiceKey(t);
  withReactions(t);
  const { ctx } = makeCtx(memberTables({ spaceReactions: [] }));
  await assert.rejects(setForGateway._handler(ctx, { ...base, serviceKey: 'wrong', emoji: '\u{1F44D}', present: true }));
  await rejectsWith(setForGateway._handler(ctx, { ...base, externalAuthId: 'stranger', emoji: '\u{1F44D}', present: true }), 'Unauthorized');
  await assert.rejects(reactionsForThreadForGateway._handler(ctx, { ...base, serviceKey: 'wrong' }));
  await assert.rejects(clearForMessageForGateway._handler(ctx, { ...base, serviceKey: 'wrong' }));
});

test('clearForMessage removes every reaction of that message only, even with the flag off', async (t) => {
  withServiceKey(t);
  withReactions(t, null);
  const { ctx, tables } = makeCtx(
    memberTables({
      spaceReactions: [
        row('\u{1F44D}', AUTH, 1),
        row('\u{1F389}', 'bob', 2),
        row('\u{1F44D}', 'bob', 3, { messageId: 'msg_2' }),
        row('\u{1F44D}', 'spy', 4, { externalOrgId: 'org_other' }),
        row('\u{1F44D}', 'bob', 5, { threadId: 'thread_2' }),
      ],
    }),
  );
  const result = await clearForMessageForGateway._handler(ctx, base);
  assert.deepEqual(result, { cleared: 2, more: false });
  assert.deepEqual(tables.spaceReactions.map((entry) => entry.externalAuthId), ['bob', 'spy', 'bob']);
});

test('clearForMessage reports more when a full batch of 500 was read', async (t) => {
  withServiceKey(t);
  const rows = Array.from({ length: 500 }, (_, i) => row('\u{1F44D}', `u${i}`, i));
  const { ctx, tables } = makeCtx(memberTables({ spaceReactions: rows }));
  const result = await clearForMessageForGateway._handler(ctx, base);
  assert.deepEqual(result, { cleared: 500, more: true });
  assert.equal(tables.spaceReactions.length, 0);
});

test('approval and wake code do not import the reactions module (a reaction grants nothing)', () => {
  const convexDir = path.join(__dirname, '..', 'convex');
  const importers = [];
  for (const name of fs.readdirSync(convexDir)) {
    if (!name.endsWith('.ts') || name === 'spaceReactions.ts') continue;
    const source = fs.readFileSync(path.join(convexDir, name), 'utf8');
    if (/from\s+["']\.\/spaceReactions["']/.test(source) || /internal\.spaceReactions\b|api\.spaceReactions\b/.test(source)) {
      importers.push(name);
    }
  }
  // Only the erasure path (purgeSpace) may name the table, and it does so by table name, not by import.
  assert.deepEqual(importers, []);
  const wakeAndApproval = fs.readdirSync(convexDir).filter((name) => /wake|approval/i.test(name) && name.endsWith('.ts'));
  for (const name of wakeAndApproval) {
    const source = fs.readFileSync(path.join(convexDir, name), 'utf8');
    assert.equal(/spaceReactions/.test(source), false, `${name} must not reference reactions`);
  }
});
