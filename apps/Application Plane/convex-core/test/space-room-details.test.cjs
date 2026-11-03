const assert = require('node:assert/strict');
const test = require('node:test');

const {
  ROOM_NAME_MAX_LENGTH,
  ROOM_TOPIC_MAX_LENGTH,
  normalizeRoomDetails,
} = require('../convex/spaceRoomRules.ts');
const { updateRoomDetailsForGateway } = require('../convex/spaces.ts');

function rejected(input, field, reason) {
  const result = normalizeRoomDetails(input);
  assert.equal(result.ok, false, `expected rejection for ${JSON.stringify(input)}`);
  assert.equal(result.field, field);
  assert.equal(result.reason, reason);
}

test('limits are 120 for the name and 250 for the topic', () => {
  assert.equal(ROOM_NAME_MAX_LENGTH, 120);
  assert.equal(ROOM_TOPIC_MAX_LENGTH, 250);
});

test('name is NFC-normalised, trimmed and has inner whitespace collapsed', () => {
  // "e" + combining acute accent composes to a single code point.
  const result = normalizeRoomDetails({ name: '  Café \t  \n Team   room  ' });
  assert.equal(result.ok, true);
  assert.deepEqual(result.value, { name: 'Café Team room' });
});

test('topic gets the same normalisation as the name', () => {
  const result = normalizeRoomDetails({ topic: '  Weekly  \n sync  ' });
  assert.equal(result.ok, true);
  assert.deepEqual(result.value, { topic: 'Weekly sync' });
});

test('name length is counted in code points: 120 passes, 121 is too long', () => {
  assert.equal(normalizeRoomDetails({ name: 'a'.repeat(120) }).ok, true);
  rejected({ name: 'a'.repeat(121) }, 'name', 'name_too_long');
  // 120 astral characters are 240 UTF-16 units but still 120 code points.
  assert.equal(normalizeRoomDetails({ name: '\u{1F600}'.repeat(120) }).ok, true);
  rejected({ name: '\u{1F600}'.repeat(121) }, 'name', 'name_too_long');
});

test('topic length is counted in code points: 250 passes, 251 is too long', () => {
  assert.equal(normalizeRoomDetails({ topic: 'b'.repeat(250) }).ok, true);
  rejected({ topic: 'b'.repeat(251) }, 'topic', 'topic_too_long');
  assert.equal(normalizeRoomDetails({ topic: '\u{1F600}'.repeat(250) }).ok, true);
  rejected({ topic: '\u{1F600}'.repeat(251) }, 'topic', 'topic_too_long');
});

test('a blank name is refused: a room cannot be unnamed', () => {
  rejected({ name: '' }, 'name', 'name_required');
  rejected({ name: '   \t\n ' }, 'name', 'name_required');
});

test('an empty or blank topic clears it (null); an explicit null clears too', () => {
  assert.deepEqual(normalizeRoomDetails({ topic: '' }).value, { topic: null });
  assert.deepEqual(normalizeRoomDetails({ topic: '   ' }).value, { topic: null });
  assert.deepEqual(normalizeRoomDetails({ topic: null }).value, { topic: null });
});

test('a name-only update leaves the topic key absent; a topic-only one leaves the name absent', () => {
  const nameOnly = normalizeRoomDetails({ name: 'Ops' });
  assert.equal(nameOnly.ok, true);
  assert.equal('topic' in nameOnly.value, false);
  const topicOnly = normalizeRoomDetails({ topic: 'x' });
  assert.equal('name' in topicOnly.value, false);
  assert.deepEqual(normalizeRoomDetails({}).value, {});
});

test('control characters are rejected in the name and the topic', () => {
  for (const ch of ['\u0000', '\u0007', '\u001b', '\u007f', '\u0085', '\u009f']) {
    rejected({ name: `a${ch}b` }, 'name', 'control_character');
    rejected({ topic: `a${ch}b` }, 'topic', 'control_character');
  }
});

test('tab, line feed and carriage return are whitespace, not control-character rejections', () => {
  assert.deepEqual(normalizeRoomDetails({ name: 'a\tb\nc\rd' }).value, { name: 'a b c d' });
});

test('bidi overrides U+202A-202E and isolates U+2066-2069 are rejected', () => {
  const bidi = ['‪', '‫', '‬', '‭', '‮', '⁦', '⁧', '⁨', '⁩'];
  for (const ch of bidi) {
    rejected({ name: `room${ch}` }, 'name', 'bidi_override');
    rejected({ topic: `${ch}topic` }, 'topic', 'bidi_override');
  }
});

test('non-string values are rejected as not_a_string (null topic is the only clear)', () => {
  rejected({ name: 5 }, 'name', 'not_a_string');
  rejected({ name: null }, 'name', 'not_a_string');
  rejected({ name: {} }, 'name', 'not_a_string');
  rejected({ topic: 7 }, 'topic', 'not_a_string');
  rejected({ topic: ['a'] }, 'topic', 'not_a_string');
});

test('the name is checked before the topic when both are bad', () => {
  const result = normalizeRoomDetails({ name: '', topic: 'x'.repeat(300) });
  assert.equal(result.field, 'name');
});

// ---------------------------------------------------------------------------
// updateRoomDetailsForGateway error codes, driven through a mocked Convex ctx.
// ---------------------------------------------------------------------------

const SERVICE_KEY = 'service-key-aaaa';
const ORG = 'org_a';
const AUTH = 'auth_user_1';

function makeCtx({ spaces, members = true, organizations = true }) {
  const patches = [];
  const inserts = [];
  const tables = {
    organizations: organizations ? [{ _id: 'org_row', externalOrgId: ORG, syncStatus: 'active' }] : [],
    users: members ? [{ _id: 'user_row', externalAuthId: AUTH, orgId: 'org_row', syncStatus: 'active' }] : [],
    spaces,
  };
  const ctx = {
    db: {
      query(table) {
        const rows = tables[table] ?? [];
        return {
          withIndex(_name, build) {
            build({ eq() { return this; } });
            return { collect: async () => rows };
          },
        };
      },
      async patch(id, value) {
        patches.push({ id, value });
      },
      // WS15-S12: a changed name or topic records one room notice in the same pass.
      async insert(table, value) {
        inserts.push({ table, value });
        return `${table}_new`;
      },
    },
  };
  return { ctx, patches, inserts };
}

function room(overrides = {}) {
  return {
    _id: 'space_row',
    spaceRef: 'space_1',
    externalOrgId: ORG,
    kind: 'room',
    lifecycle: 'active',
    name: 'Old name',
    topic: 'Old topic',
    ...overrides,
  };
}

async function callUpdate(ctx, args) {
  return updateRoomDetailsForGateway._handler(ctx, {
    serviceKey: SERVICE_KEY,
    externalAuthId: AUTH,
    externalOrgId: ORG,
    spaceRef: 'space_1',
    ...args,
  });
}

test('updateRoomDetailsForGateway error codes and patch', async (t) => {
  const prior = process.env.CONVEX_INTERNAL_SERVICE_KEY;
  process.env.CONVEX_INTERNAL_SERVICE_KEY = SERVICE_KEY;
  t.after(() => {
    if (prior === undefined) delete process.env.CONVEX_INTERNAL_SERVICE_KEY;
    else process.env.CONVEX_INTERNAL_SERVICE_KEY = prior;
  });

  await t.test('a wrong service key is Unauthorized before any read', async () => {
    const { ctx, patches } = makeCtx({ spaces: [room()] });
    await assert.rejects(() => callUpdate(ctx, { serviceKey: 'nope', name: 'New' }), /Unauthorized/);
    assert.deepEqual(patches, []);
  });

  await t.test('a caller who is not an org member is Unauthorized and nothing is written', async () => {
    const { ctx, patches } = makeCtx({ spaces: [room()], members: false });
    await assert.rejects(() => callUpdate(ctx, { name: 'New' }), /Unauthorized/);
    assert.deepEqual(patches, []);
  });

  await t.test('space_not_found when the ref is unknown', async () => {
    const { ctx, patches } = makeCtx({ spaces: [] });
    await assert.rejects(() => callUpdate(ctx, { name: 'New' }), /^Error: space_not_found$/);
    assert.deepEqual(patches, []);
  });

  await t.test('space_not_found when the Space belongs to another organization', async () => {
    const { ctx } = makeCtx({ spaces: [room({ externalOrgId: 'org_other' })] });
    await assert.rejects(() => callUpdate(ctx, { name: 'New' }), /^Error: space_not_found$/);
  });

  await t.test('space_kind_not_editable for a personal Space or any non-room kind', async () => {
    for (const kind of ['personal', 'case', 'project']) {
      const { ctx, patches } = makeCtx({ spaces: [room({ kind })] });
      await assert.rejects(() => callUpdate(ctx, { name: 'New' }), /^Error: space_kind_not_editable$/);
      assert.deepEqual(patches, []);
    }
  });

  await t.test('room_not_active unless the lifecycle is active', async () => {
    for (const lifecycle of ['pending_registration', 'suspended', 'deleting', 'deleted']) {
      const { ctx, patches } = makeCtx({ spaces: [room({ lifecycle })] });
      await assert.rejects(() => callUpdate(ctx, { name: 'New' }), /^Error: room_not_active$/);
      assert.deepEqual(patches, []);
    }
  });

  await t.test('an update with neither name nor topic is room_details_invalid:details:empty_update', async () => {
    const { ctx } = makeCtx({ spaces: [room()] });
    await assert.rejects(() => callUpdate(ctx, {}), /^Error: room_details_invalid:details:empty_update$/);
  });

  await t.test('invalid text maps to room_details_invalid:<field>:<reason> and writes nothing', async () => {
    const cases = [
      [{ name: '   ' }, 'room_details_invalid:name:name_required'],
      [{ name: 'x'.repeat(121) }, 'room_details_invalid:name:name_too_long'],
      [{ topic: 'x'.repeat(251) }, 'room_details_invalid:topic:topic_too_long'],
      [{ name: 'a\u0000b' }, 'room_details_invalid:name:control_character'],
      [{ topic: 'a‮b' }, 'room_details_invalid:topic:bidi_override'],
    ];
    for (const [args, code] of cases) {
      const { ctx, patches } = makeCtx({ spaces: [room()] });
      await assert.rejects(() => callUpdate(ctx, args), (error) => error.message === code);
      assert.deepEqual(patches, []);
    }
  });

  await t.test('a name-only update patches the name and returns the existing topic', async () => {
    const { ctx, patches, inserts } = makeCtx({ spaces: [room()] });
    const result = await callUpdate(ctx, { name: '  New   name ' });
    assert.deepEqual(result, { spaceRef: 'space_1', name: 'New name', topic: 'Old topic' });
    assert.equal(patches.length, 1);
    assert.equal(patches[0].id, 'space_row');
    assert.equal(patches[0].value.name, 'New name');
    assert.equal('topic' in patches[0].value, false);
    assert.equal(patches[0].value.updatedByExternalAuthId, AUTH);
    assert.deepEqual(inserts.map((row) => [row.table, row.value.kind]), [['spaceRoomNotices', 'room_renamed']]);
  });

  await t.test('an empty topic clears it and keeps the name', async () => {
    const { ctx, patches, inserts } = makeCtx({ spaces: [room()] });
    const result = await callUpdate(ctx, { topic: '' });
    assert.deepEqual(result, { spaceRef: 'space_1', name: 'Old name', topic: null });
    assert.equal('topic' in patches[0].value, true);
    assert.equal(patches[0].value.topic, undefined);
    assert.equal('name' in patches[0].value, false);
    assert.deepEqual(inserts.map((row) => [row.table, row.value.kind]), [['spaceRoomNotices', 'topic_changed']]);
  });
});
