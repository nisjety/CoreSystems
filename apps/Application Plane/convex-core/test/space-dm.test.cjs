const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

// spaceRegistration.ts reads these at import time.
process.env.CONTROL_SPACE_REGISTRATION_URL = 'http://control.invalid/space-registration';
process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN = 'token-aaaaaaaa';

const {
  MAX_DM_PARTICIPANTS,
  MIN_DM_PARTICIPANTS,
  DmParticipantError,
  canonicalParticipants,
  dmKindForCount,
  dmOpenEnabled,
  isDmKind,
  participantSetHash,
} = require('../convex/spaceParticipants.ts');
const { claimNextRegistration, openDmForGateway } = require('../convex/spaces.ts');
const { deliverOne } = require('../convex/spaceRegistration.ts');
const {
  bindExistingSpaceAgentForGateway,
  createSpaceAgentForGateway,
  upsertSpaceAgentBinding,
} = require('../convex/spaceAgents.ts');
const {
  clearForSpaceForGateway,
  hiddenForViewerForGateway,
  hideForGateway,
  unhideForGateway,
} = require('../convex/spaceDmHides.ts');
const fake = require('./helpers/fake-convex.cjs');

const VECTORS_PATH = path.join(
  __dirname,
  '..',
  '..',
  '..',
  'Control Plane',
  'user-core',
  'internal',
  'spaces',
  'testdata',
  'participant-set-vectors.v1.json',
);
const vectors = JSON.parse(fs.readFileSync(VECTORS_PATH, 'utf8'));

// ---------------------------------------------------------------------------
// Pure identity: Control's shared vectors.
// ---------------------------------------------------------------------------

test('participantSetHash equals every valid vector of Control\'s participant-set-vectors.v1.json', async () => {
  assert.equal(vectors.version, 1);
  assert.ok(vectors.valid.length >= 8);
  for (const vector of vectors.valid) {
    assert.deepEqual(canonicalParticipants(vector.input_ids), vector.canonical_ids, `${vector.name}: canonical order`);
    assert.equal(await participantSetHash(vector.input_ids), vector.hash, `${vector.name}: hash`);
    assert.equal(dmKindForCount(vector.canonical_ids.length), vector.kind, `${vector.name}: kind`);
  }
});

test('every invalid vector is refused with the same reason word as Control', async () => {
  assert.ok(vectors.invalid.length >= 6);
  for (const vector of vectors.invalid) {
    assert.throws(
      () => canonicalParticipants(vector.input_ids),
      (error) => error instanceof DmParticipantError && error.reason === vector.reason,
      vector.name,
    );
    await assert.rejects(participantSetHash(vector.input_ids), (error) => error.reason === vector.reason, vector.name);
  }
});

test('dmKindForCount: 2 is dm, 3..9 is group_dm, anything else is a count error', () => {
  assert.equal(MIN_DM_PARTICIPANTS, 2);
  assert.equal(MAX_DM_PARTICIPANTS, 9);
  assert.equal(dmKindForCount(2), 'dm');
  for (let n = 3; n <= 9; n += 1) assert.equal(dmKindForCount(n), 'group_dm');
  for (const n of [0, 1, 10, -1]) {
    assert.throws(() => dmKindForCount(n), (error) => error.reason === 'count');
  }
});

test('canonicalParticipants sorts by code point, not locale, and does not mutate the input', () => {
  const input = ['b', 'B', 'a', 'A', 'é', 'z'];
  const copy = [...input];
  assert.deepEqual(canonicalParticipants(input), ['A', 'B', 'a', 'b', 'z', 'é']);
  assert.deepEqual(input, copy);
});

test('isDmKind recognises only dm and group_dm', () => {
  assert.equal(isDmKind('dm'), true);
  assert.equal(isDmKind('group_dm'), true);
  for (const other of ['room', 'personal', '', undefined, null, 'DM']) assert.equal(isDmKind(other), false);
});

test('dmOpenEnabled reads SPACE_DM_OPEN at call time and is off by default', (t) => {
  const prior = process.env.SPACE_DM_OPEN;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_DM_OPEN;
    else process.env.SPACE_DM_OPEN = prior;
  });
  delete process.env.SPACE_DM_OPEN;
  assert.equal(dmOpenEnabled(), false);
  process.env.SPACE_DM_OPEN = '0';
  assert.equal(dmOpenEnabled(), false);
  process.env.SPACE_DM_OPEN = ' TRUE ';
  assert.equal(dmOpenEnabled(), true);
  process.env.SPACE_DM_OPEN = '1';
  assert.equal(dmOpenEnabled(), true);
});

// ---------------------------------------------------------------------------
// openDmForGateway
// ---------------------------------------------------------------------------

const ALICE = 'alice';
const BOB = 'bob';
const CAROL = 'carol';

function dmTables(extra = {}) {
  return {
    organizations: [{ _id: 'org_row', externalOrgId: fake.ORG, syncStatus: 'active' }],
    users: [ALICE, BOB, CAROL].map((id) => ({ _id: `u_${id}`, externalAuthId: id, orgId: 'org_row', syncStatus: 'active' })),
    spaces: [],
    spaceLifecycleEvents: [],
    spaceDmHides: [],
    ...extra,
  };
}

function open(ctx, opener, others, extra = {}) {
  return openDmForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: opener,
    externalOrgId: fake.ORG,
    participantExternalAuthIds: others,
    ...extra,
  });
}

function withDmOpen(t, value = '1') {
  fake.withServiceKey(t);
  const prior = process.env.SPACE_DM_OPEN;
  process.env.SPACE_DM_OPEN = value;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_DM_OPEN;
    else process.env.SPACE_DM_OPEN = prior;
  });
}

test('with the flag off openDmForGateway refuses dm_disabled before any read or write', async (t) => {
  withDmOpen(t, '0');
  const { ctx, log } = fake.makeCtx(dmTables());
  await fake.rejectsWith(open(ctx, ALICE, [BOB]), 'dm_disabled');
  assert.equal(log.inserts.length + log.patches.length + log.deletes.length, 0);
});

test('opening a DM creates a pending dm Space with the sorted roster and hash, and a lifecycle event', async (t) => {
  withDmOpen(t);
  const { ctx, tables, log } = fake.makeCtx(dmTables());
  const result = await open(ctx, BOB, [ALICE]);
  assert.equal(result.created, true);
  const space = result.space;
  assert.equal(space.kind, 'dm');
  assert.equal(space.lifecycle, 'pending_registration');
  assert.deepEqual(space.participantExternalAuthIds, [ALICE, BOB]);
  assert.equal(space.participantHash, await participantSetHash([ALICE, BOB]));
  assert.equal(space.ownerExternalAuthId, BOB);
  assert.equal(space.name, 'Direct message', 'the stored name never carries a person\'s name');
  assert.equal(space.spaceRef, space._id);
  assert.equal(tables.spaceLifecycleEvents.length, 1);
  assert.equal(log.scheduled.length, 1, 'registration delivery is scheduled');
});

test('three or more people make a group_dm', async (t) => {
  withDmOpen(t);
  const { ctx } = fake.makeCtx(dmTables());
  const result = await open(ctx, ALICE, [BOB, CAROL]);
  assert.equal(result.space.kind, 'group_dm');
  assert.deepEqual(result.space.participantExternalAuthIds, [ALICE, BOB, CAROL]);
});

test('the same set opens the same Space from either opener, created false, and deletes the caller\'s hide row', async (t) => {
  withDmOpen(t);
  const { ctx, tables } = fake.makeCtx(dmTables());
  const first = await open(ctx, ALICE, [BOB]);
  const spaceRef = first.space.spaceRef;
  tables.spaceDmHides.push({ _id: 'h_alice', spaceRef, externalAuthId: ALICE, externalOrgId: fake.ORG, hiddenAt: 1 });
  tables.spaceDmHides.push({ _id: 'h_bob', spaceRef, externalAuthId: BOB, externalOrgId: fake.ORG, hiddenAt: 1 });

  const second = await open(ctx, BOB, [ALICE]);
  assert.equal(second.created, false);
  assert.equal(second.space.spaceRef, spaceRef);
  assert.equal(tables.spaces.length, 1);
  assert.deepEqual(tables.spaceDmHides.map((row) => row._id), ['h_alice'], 'only Bob\'s own hide is removed');

  const third = await open(ctx, ALICE, [BOB]);
  assert.equal(third.created, false);
  assert.equal(third.space.spaceRef, spaceRef);
  assert.equal(tables.spaceDmHides.length, 0);
});

test('two opens in a row (the serialised outcome of concurrent opens) leave exactly one Space', async (t) => {
  withDmOpen(t);
  const { ctx, tables } = fake.makeCtx(dmTables());
  const a = await open(ctx, ALICE, [BOB]);
  const b = await open(ctx, BOB, [ALICE]);
  assert.equal(a.created, true);
  assert.equal(b.created, false);
  assert.equal(tables.spaces.length, 1);
});

test('opening a DM with yourself, nobody, a duplicate or ten people is refused with a reason', async (t) => {
  withDmOpen(t);
  const { ctx, log } = fake.makeCtx(dmTables());
  await fake.rejectsWith(open(ctx, ALICE, [ALICE]), 'dm_participants_invalid:self');
  await fake.rejectsWith(open(ctx, ALICE, [' alice ', BOB]), 'dm_participants_invalid:self');
  await fake.rejectsWith(open(ctx, ALICE, []), 'dm_participants_invalid:count');
  await fake.rejectsWith(open(ctx, ALICE, [BOB, BOB]), 'dm_participants_invalid:duplicate');
  await fake.rejectsWith(open(ctx, ALICE, [BOB, '']), 'dm_participants_invalid:empty');
  await fake.rejectsWith(open(ctx, ALICE, Array.from({ length: 9 }, (_, i) => `p${i}`)), 'dm_participants_invalid:count');
  assert.equal(log.inserts.length, 0);
});

test('an expectedHash that does not match the computed hash is refused', async (t) => {
  withDmOpen(t);
  const { ctx, log } = fake.makeCtx(dmTables());
  await fake.rejectsWith(open(ctx, ALICE, [BOB], { expectedHash: 'sha256:00' }), 'dm_participant_hash_mismatch');
  assert.equal(log.inserts.length, 0);
  const good = await open(ctx, ALICE, [BOB], { expectedHash: await participantSetHash([ALICE, BOB]) });
  assert.equal(good.created, true);
});

test('a participant who is not an active member of the organization is refused', async (t) => {
  withDmOpen(t);
  const tables = dmTables();
  tables.users.push({ _id: 'u_dan', externalAuthId: 'dan', orgId: 'org_row', syncStatus: 'deleted' });
  const { ctx, log } = fake.makeCtx(tables);
  await fake.rejectsWith(open(ctx, ALICE, ['stranger']), 'dm_participant_not_member');
  await fake.rejectsWith(open(ctx, ALICE, ['dan']), 'dm_participant_not_member');
  assert.equal(log.inserts.length, 0);
  await fake.rejectsWith(open(ctx, 'stranger', [ALICE]), 'Unauthorized');
});

test('a failed_registration DM for the set answers dm_registration_failed and a suspended one is reused', async (t) => {
  withDmOpen(t);
  const hash = await participantSetHash([ALICE, BOB]);
  const failed = fake.makeCtx(
    dmTables({
      spaces: [{ _id: 's_failed', spaceRef: 's_failed', externalOrgId: fake.ORG, kind: 'dm', participantHash: hash, lifecycle: 'failed_registration' }],
    }),
  );
  await fake.rejectsWith(open(failed.ctx, ALICE, [BOB]), 'dm_registration_failed');
  assert.equal(failed.log.inserts.length, 0);

  const suspended = fake.makeCtx(
    dmTables({
      spaces: [{ _id: 's_susp', spaceRef: 's_susp', externalOrgId: fake.ORG, kind: 'dm', participantHash: hash, lifecycle: 'suspended' }],
    }),
  );
  const reused = await open(suspended.ctx, ALICE, [BOB]);
  assert.equal(reused.created, false);
  assert.equal(reused.space.spaceRef, 's_susp');
});

test('a deleted DM for the same set does not block a new one', async (t) => {
  withDmOpen(t);
  const hash = await participantSetHash([ALICE, BOB]);
  const { ctx, tables } = fake.makeCtx(
    dmTables({
      spaces: [{ _id: 's_old', spaceRef: 's_old', externalOrgId: fake.ORG, kind: 'dm', participantHash: hash, lifecycle: 'deleted' }],
    }),
  );
  const fresh = await open(ctx, ALICE, [BOB]);
  assert.equal(fresh.created, true);
  assert.equal(tables.spaces.length, 2);
});

// ---------------------------------------------------------------------------
// Registration payload
// ---------------------------------------------------------------------------

function pendingEvent(spaceRef) {
  return {
    eventId: `space:${spaceRef}:lifecycle:1`,
    externalOrgId: fake.ORG,
    lifecycle: 'pending_registration',
    revision: 1,
    spaceRef,
    createdAt: 10,
    nextAttemptAt: 10,
    deliveryState: 'pending',
    deliveryAttempts: 0,
  };
}

test('claimNextRegistration returns the participants and hash for a DM only', async () => {
  const room = { _id: 'room_row', spaceRef: 'room_1', externalOrgId: fake.ORG, kind: 'room', ownerExternalAuthId: ALICE };
  const dm = {
    _id: 'dm_row',
    spaceRef: 'dm_1',
    externalOrgId: fake.ORG,
    kind: 'dm',
    ownerExternalAuthId: ALICE,
    participantExternalAuthIds: [ALICE, BOB],
    participantHash: 'sha256:abc',
  };
  const roomCtx = fake.makeCtx({ spaces: [room], spaceLifecycleEvents: [pendingEvent('room_1')] });
  const claimedRoom = await claimNextRegistration._handler(roomCtx.ctx, { workerId: 'w1', now: 1000 });
  assert.equal(claimedRoom.space.kind, 'room');
  assert.equal(claimedRoom.space.participantExternalAuthIds, undefined);
  assert.equal(claimedRoom.space.participantHash, undefined);

  const dmCtx = fake.makeCtx({ spaces: [dm], spaceLifecycleEvents: [pendingEvent('dm_1')] });
  const claimedDm = await claimNextRegistration._handler(dmCtx.ctx, { workerId: 'w1', now: 1000 });
  assert.equal(claimedDm.space.kind, 'dm');
  assert.deepEqual(claimedDm.space.participantExternalAuthIds, [ALICE, BOB]);
  assert.equal(claimedDm.space.participantHash, 'sha256:abc');
  assert.equal(claimedDm.space.ownerPrincipalId, ALICE);
});

async function registrationBody(space) {
  const sent = [];
  const realFetch = global.fetch;
  global.fetch = async (url, init) => {
    sent.push({ url, init });
    return new Response(JSON.stringify({}), { status: 409 });
  };
  try {
    const { ctx } = fake.makeCtx({}, {
      runMutation: async (_fn, args) => {
        if (args && args.now && args.workerId && !args.eventId) {
          return { event: { eventId: 'e1', externalOrgId: fake.ORG, lifecycle: 'pending_registration', revision: 1, spaceRef: 's1' }, space };
        }
        return undefined;
      },
    });
    await deliverOne._handler(ctx, { workerId: 'w1' });
  } finally {
    global.fetch = realFetch;
  }
  assert.equal(sent.length, 1);
  return JSON.parse(sent[0].init.body);
}

test('the registration body carries participants and participant_hash only for DM kinds', async () => {
  const dmBody = await registrationBody({
    kind: 'group_dm',
    ownerPrincipalId: ALICE,
    participantExternalAuthIds: [ALICE, BOB, CAROL],
    participantHash: 'sha256:def',
  });
  assert.deepEqual(dmBody.participants, [ALICE, BOB, CAROL]);
  assert.equal(dmBody.participant_hash, 'sha256:def');
  assert.equal(dmBody.kind, 'group_dm');

  const roomBody = await registrationBody({ kind: 'room', ownerPrincipalId: ALICE });
  assert.equal('participants' in roomBody, false);
  assert.equal('participant_hash' in roomBody, false);
  assert.equal(roomBody.owner_principal_id, ALICE);
});

// ---------------------------------------------------------------------------
// Legacy agent guards
// ---------------------------------------------------------------------------

function agentTables(extra = {}) {
  return fake.memberTables({
    spaces: [
      { _id: 'dm_row', spaceRef: 'dm_1', externalOrgId: fake.ORG, kind: 'dm' },
      { _id: 'gdm_row', spaceRef: 'gdm_1', externalOrgId: fake.ORG, kind: 'group_dm' },
      { _id: 'room_row', spaceRef: 'room_1', externalOrgId: fake.ORG, kind: 'room' },
      { _id: 'foreign_dm_row', spaceRef: 'dm_foreign', externalOrgId: 'org_other', kind: 'dm' },
    ],
    agents: [{ _id: 'agent-1', orgId: 'org_row', name: 'Bot' }],
    spaceAgentBindings: [],
    ...extra,
  });
}

const agentBase = { serviceKey: fake.SERVICE_KEY, externalAuthId: fake.AUTH, externalOrgId: fake.ORG };

test('createSpaceAgentForGateway refuses a DM and a group DM with dm_agent_bind_refused', async (t) => {
  fake.withServiceKey(t);
  for (const spaceRef of ['dm_1', 'gdm_1']) {
    const { ctx, log } = fake.makeCtx(agentTables());
    await fake.rejectsWith(
      createSpaceAgentForGateway._handler(ctx, { ...agentBase, spaceRef, name: 'Scribe' }),
      'dm_agent_bind_refused',
    );
    assert.equal(log.inserts.length, 0, 'no agent definition is created either');
  }
});

test('bindExistingSpaceAgentForGateway refuses a DM with dm_agent_bind_refused', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = fake.makeCtx(agentTables());
  await fake.rejectsWith(
    bindExistingSpaceAgentForGateway._handler(ctx, { ...agentBase, spaceRef: 'dm_1', agentId: 'agent-1' }),
    'dm_agent_bind_refused',
  );
  assert.equal(log.mutations.length, 0);
  assert.equal(log.inserts.length, 0);
});

test('upsertSpaceAgentBinding refuses a DM but not a room, and ignores another organization\'s DM', async () => {
  const args = { externalOrgId: fake.ORG, agentId: 'agent-1', subjectId: 'agent-agent-1', status: 'active' };
  const { ctx, log } = fake.makeCtx(agentTables());
  await fake.rejectsWith(upsertSpaceAgentBinding._handler(ctx, { ...args, spaceRef: 'dm_1' }), 'dm_agent_bind_refused');
  await fake.rejectsWith(upsertSpaceAgentBinding._handler(ctx, { ...args, spaceRef: 'gdm_1' }), 'dm_agent_bind_refused');
  assert.equal(log.inserts.length, 0);
  const ok = await upsertSpaceAgentBinding._handler(ctx, { ...args, spaceRef: 'room_1' });
  assert.equal(ok.created, true);
  // a Space ref that is a DM only in another org is not this org's DM
  const foreign = await upsertSpaceAgentBinding._handler(ctx, { ...args, spaceRef: 'dm_foreign' });
  assert.equal(foreign.created, true);
});

// ---------------------------------------------------------------------------
// Hide / unhide / hiddenForViewer / clearForSpace
// ---------------------------------------------------------------------------

function hideTables(extra = {}) {
  return fake.memberTables({
    spaces: [
      { _id: 'dm_row', spaceRef: 'dm_1', externalOrgId: fake.ORG, kind: 'dm' },
      { _id: 'dm2_row', spaceRef: 'dm_2', externalOrgId: fake.ORG, kind: 'group_dm' },
      { _id: 'room_row', spaceRef: 'room_1', externalOrgId: fake.ORG, kind: 'room' },
      { _id: 'foreign_row', spaceRef: 'dm_foreign', externalOrgId: 'org_other', kind: 'dm' },
    ],
    spaceDmHides: [],
    ...extra,
  });
}

const hideArgs = (spaceRef, extra = {}) => ({ ...agentBase, spaceRef, ...extra });

test('hideForGateway hides a DM idempotently and keeps the first time', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(hideTables());
  const first = await hideForGateway._handler(ctx, hideArgs('dm_1'));
  assert.equal(first.hidden, true);
  assert.equal(tables.spaceDmHides.length, 1);
  const again = await hideForGateway._handler(ctx, hideArgs(' dm_1 '));
  assert.equal(again.hiddenAt, first.hiddenAt);
  assert.equal(tables.spaceDmHides.length, 1);
});

test('hide of a non-DM is space_not_hideable; unknown, blank and other-org Spaces are space_not_found', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(hideTables());
  await fake.rejectsWith(hideForGateway._handler(ctx, hideArgs('room_1')), 'space_not_hideable');
  await fake.rejectsWith(hideForGateway._handler(ctx, hideArgs('dm_foreign')), 'space_not_found');
  await fake.rejectsWith(hideForGateway._handler(ctx, hideArgs('nope')), 'space_not_found');
  await fake.rejectsWith(hideForGateway._handler(ctx, hideArgs('  ')), 'space_not_found');
  await fake.rejectsWith(unhideForGateway._handler(ctx, hideArgs('room_1')), 'space_not_hideable');
  await fake.rejectsWith(clearForSpaceForGateway._handler(ctx, hideArgs('room_1')), 'space_not_hideable');
  await fake.rejectsWith(clearForSpaceForGateway._handler(ctx, hideArgs('dm_foreign')), 'space_not_found');
  assert.equal(tables.spaceDmHides.length, 0);
});

test('hide mutations need the service key and an org member', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(hideTables());
  await fake.rejectsWith(hideForGateway._handler(ctx, hideArgs('dm_1', { serviceKey: 'wrong' })), 'Unauthorized');
  await fake.rejectsWith(hideForGateway._handler(ctx, hideArgs('dm_1', { externalAuthId: 'stranger' })), 'Unauthorized');
  await fake.rejectsWith(
    hiddenForViewerForGateway._handler(ctx, { ...agentBase, externalAuthId: 'stranger' }),
    'Unauthorized',
  );
});

test('unhideForGateway removes only the caller\'s row and is idempotent', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(
    hideTables({
      spaceDmHides: [
        { _id: 'h1', spaceRef: 'dm_1', externalOrgId: fake.ORG, externalAuthId: fake.AUTH, hiddenAt: 1 },
        { _id: 'h2', spaceRef: 'dm_1', externalOrgId: fake.ORG, externalAuthId: 'someone_else', hiddenAt: 2 },
      ],
    }),
  );
  assert.deepEqual(await unhideForGateway._handler(ctx, hideArgs('dm_1')), { spaceRef: 'dm_1', hidden: false });
  assert.deepEqual(tables.spaceDmHides.map((row) => row._id), ['h2']);
  assert.deepEqual(await unhideForGateway._handler(ctx, hideArgs('dm_1')), { spaceRef: 'dm_1', hidden: false });
  assert.equal(tables.spaceDmHides.length, 1);
});

test('hiddenForViewerForGateway lists the caller\'s hides only, as refs and times', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(
    hideTables({
      spaceDmHides: [
        { _id: 'h1', spaceRef: 'dm_1', externalOrgId: fake.ORG, externalAuthId: fake.AUTH, hiddenAt: 11 },
        { _id: 'h2', spaceRef: 'dm_2', externalOrgId: fake.ORG, externalAuthId: fake.AUTH, hiddenAt: 12 },
        { _id: 'h3', spaceRef: 'dm_1', externalOrgId: fake.ORG, externalAuthId: 'someone_else', hiddenAt: 13 },
        { _id: 'h4', spaceRef: 'dm_9', externalOrgId: 'org_other', externalAuthId: fake.AUTH, hiddenAt: 14 },
      ],
    }),
  );
  const rows = await hiddenForViewerForGateway._handler(ctx, agentBase);
  assert.deepEqual(rows, [
    { spaceRef: 'dm_1', hiddenAt: 11 },
    { spaceRef: 'dm_2', hiddenAt: 12 },
  ]);
});

test('clearForSpaceForGateway removes every viewer\'s hide row for the DM and counts them', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(
    hideTables({
      spaceDmHides: [
        { _id: 'h1', spaceRef: 'dm_1', externalOrgId: fake.ORG, externalAuthId: fake.AUTH, hiddenAt: 1 },
        { _id: 'h2', spaceRef: 'dm_1', externalOrgId: fake.ORG, externalAuthId: 'someone_else', hiddenAt: 2 },
        { _id: 'h3', spaceRef: 'dm_2', externalOrgId: fake.ORG, externalAuthId: fake.AUTH, hiddenAt: 3 },
      ],
    }),
  );
  assert.deepEqual(await clearForSpaceForGateway._handler(ctx, hideArgs('dm_1')), { spaceRef: 'dm_1', cleared: 2 });
  assert.deepEqual(tables.spaceDmHides.map((row) => row._id), ['h3']);
  assert.deepEqual(await clearForSpaceForGateway._handler(ctx, hideArgs('dm_1')), { spaceRef: 'dm_1', cleared: 0 });
});
