const assert = require('node:assert/strict');
const test = require('node:test');

const { getSpacePrivatePanelForGateway, recordSpacePrivatePanelForGateway } = require('../convex/spacePrivatePanels.ts');
const { purgeSpacePrivatePanelPointers } = require('../convex/spaceDeletionData.ts');
const { removeOrganizationMemberProjection } = require('../convex/nats.ts');
const fake = require('./helpers/fake-convex.cjs');

/**
 * WS17-S6 and WS17-S6b: private-panel pointers. A pointer names one person's private thread for one room.
 * The gateway proves room membership through Control before any call here, so these tests cover what Convex
 * enforces: the caller's organization membership, one pointer per person and room (first writer wins), stable
 * id validation, and the two purge paths (a Space deletion in batches, and an organization member removal that
 * runs only for a fresh event, neither stale nor already removed). Driven through the in-memory ctx in test/helpers/fake-convex.cjs.
 */

const ROOM = 'space-a';

function getPanel(ctx, overrides = {}) {
  return getSpacePrivatePanelForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    spaceRef: ROOM,
    ...overrides,
  });
}

function recordPanel(ctx, overrides = {}) {
  return recordSpacePrivatePanelForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    spaceRef: ROOM,
    threadId: 'thread-owner-1',
    ...overrides,
  });
}

/** Two members of one organization: the owner (fake.AUTH) and a colleague in the same room. */
function panelCtx() {
  return fake.makeCtx({
    organizations: [{ _id: 'org_row', externalOrgId: fake.ORG, syncStatus: 'active' }],
    users: [
      { _id: 'user_row', externalAuthId: fake.AUTH, orgId: 'org_row', syncStatus: 'active' },
      { _id: 'user_colleague', externalAuthId: 'auth_user_2', orgId: 'org_row', syncStatus: 'active' },
    ],
    spacePrivatePanels: [],
  });
}

test('the pointer is null before a record and names the stored thread after it', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = panelCtx();
  assert.equal(await getPanel(ctx), null);
  const created = await recordPanel(ctx, { agentSubjectId: 'agent-agent-1' });
  assert.deepEqual(created, { threadId: 'thread-owner-1', agentSubjectId: 'agent-agent-1', created: true });
  assert.deepEqual(await getPanel(ctx), { threadId: 'thread-owner-1', agentSubjectId: 'agent-agent-1' });
});

test('recordSpacePrivatePanelForGateway is first writer wins, per person and room', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = panelCtx();
  await recordPanel(ctx, { threadId: 'thread-owner-1' });
  const second = await recordPanel(ctx, { threadId: 'thread-owner-2' });
  assert.deepEqual(second, { threadId: 'thread-owner-1', agentSubjectId: null, created: false });
  assert.equal(log.inserts.length, 1, 'the second record writes no new pointer');
  assert.equal(tables.spacePrivatePanels.length, 1);
  assert.equal(tables.spacePrivatePanels[0].threadId, 'thread-owner-1');
  // a colleague in the same room gets a pointer of their own, never the owner's thread
  const colleague = await recordPanel(ctx, { externalAuthId: 'auth_user_2', threadId: 'thread-colleague' });
  assert.equal(colleague.created, true);
  assert.deepEqual(await getPanel(ctx, { externalAuthId: 'auth_user_2' }), {
    threadId: 'thread-colleague',
    agentSubjectId: null,
  });
});

test('an agent move patches the agent preference and its timestamp, never the thread', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = panelCtx();
  await recordPanel(ctx, { threadId: 'thread-owner-1', agentSubjectId: 'agent-agent-1' });
  const moved = await recordPanel(ctx, { threadId: 'thread-owner-2', agentSubjectId: 'agent-agent-2' });
  assert.deepEqual(moved, { threadId: 'thread-owner-1', agentSubjectId: 'agent-agent-2', created: false });
  assert.equal(log.patches.length, 1);
  assert.deepEqual(Object.keys(log.patches[0].value).sort(), ['agentSubjectId', 'updatedAt']);
  assert.equal(tables.spacePrivatePanels[0].threadId, 'thread-owner-1');
  assert.equal(tables.spacePrivatePanels[0].agentSubjectId, 'agent-agent-2');
});

test('a caller outside the organization, or with a wrong service key, is refused and writes nothing', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = panelCtx();
  await fake.rejectsWith(recordPanel(ctx, { externalAuthId: 'stranger' }), 'Unauthorized');
  await fake.rejectsWith(recordPanel(ctx, { serviceKey: 'wrong' }), 'Unauthorized');
  await fake.rejectsWith(getPanel(ctx, { externalAuthId: 'stranger' }), 'Unauthorized');
  assert.equal(log.inserts.length, 0);
});

test('a blank or control-character id throws private_panel_invalid with the field name, and writes nothing', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = panelCtx();
  // thunks, so each rejection is awaited where it is asserted
  const cases = [
    [() => recordPanel(ctx, { spaceRef: '   ' }), 'private_panel_invalid:space_ref'],
    [() => recordPanel(ctx, { spaceRef: 'space\u001fa' }), 'private_panel_invalid:space_ref'],
    [() => recordPanel(ctx, { threadId: '' }), 'private_panel_invalid:thread_id'],
    [() => recordPanel(ctx, { threadId: 'thread\u00071' }), 'private_panel_invalid:thread_id'],
    [() => recordPanel(ctx, { agentSubjectId: 'agent-1\u0000' }), 'private_panel_invalid:agent_subject_id'],
    [() => getPanel(ctx, { spaceRef: ' \t ' }), 'private_panel_invalid:space_ref'],
  ];
  for (const [call, message] of cases) {
    await assert.rejects(call(), { message });
  }
  assert.equal(log.inserts.length, 0, 'no invalid id writes a pointer');
});

// ---------------------------------------------------------------------------------------------
// WS17-S6b: purge paths. Space deletion purges one Space's pointers in batches; an organization member
// removal purges that member's pointers in that organization, and only for a fresh event (not stale, not already removed).
// ---------------------------------------------------------------------------------------------

function pointerRow(externalOrgId, externalAuthId, spaceRef, threadId) {
  return { externalOrgId, externalAuthId, spaceRef, threadId, createdAt: 1, updatedAt: 1 };
}

test('purgeSpacePrivatePanelPointers deletes only the named Space and reports more while a batch is full', async () => {
  const rows = [];
  for (let i = 0; i < 201; i += 1) {
    rows.push(pointerRow(fake.ORG, `auth_user_${i}`, 'space-a', `thread-a-${i}`));
  }
  rows.push(pointerRow(fake.ORG, 'auth_user_0', 'space-b', 'thread-b-0'));
  rows.push(pointerRow(fake.ORG, 'auth_user_1', 'space-b', 'thread-b-1'));
  const { ctx, tables, log } = fake.makeCtx({ spacePrivatePanels: rows });

  assert.deepEqual(await purgeSpacePrivatePanelPointers._handler(ctx, { spaceRef: 'space-a' }), {
    deleted: 200,
    more: true,
  });
  assert.deepEqual(await purgeSpacePrivatePanelPointers._handler(ctx, { spaceRef: 'space-a' }), {
    deleted: 1,
    more: false,
  });
  assert.deepEqual(await purgeSpacePrivatePanelPointers._handler(ctx, { spaceRef: 'space-a' }), {
    deleted: 0,
    more: false,
  });
  assert.deepEqual(
    tables.spacePrivatePanels.map((row) => row.spaceRef),
    ['space-b', 'space-b'],
    'the other Space keeps its pointers',
  );
  assert.equal(log.deletes.length, 201);
});

function memberPanel(id, externalOrgId, externalAuthId, spaceRef) {
  return { _id: id, ...pointerRow(externalOrgId, externalAuthId, spaceRef, `thread-${id}`) };
}

function removalCtx({ users = [], tombstones = [] }) {
  return fake.makeCtx({
    organizations: [{ _id: 'org_row', externalOrgId: fake.ORG, syncStatus: 'active' }],
    users,
    membershipTombstones: tombstones,
    controlSessions: [],
    spacePrivatePanels: [
      memberPanel('panel_mine_a', fake.ORG, 'auth_user_1', 'space-a'),
      memberPanel('panel_mine_b', fake.ORG, 'auth_user_1', 'space-b'),
      memberPanel('panel_colleague', fake.ORG, 'auth_user_2', 'space-a'),
      memberPanel('panel_other_org', 'org_other', 'auth_user_1', 'space-a'),
    ],
  });
}

const MEMBER = { _id: 'user_row', externalAuthId: 'auth_user_1', orgId: 'org_row', syncStatus: 'active', sourceUpdatedAt: 100 };

function removeMember(ctx, sourceUpdatedAt) {
  return removeOrganizationMemberProjection._handler(ctx, {
    orgId: fake.ORG,
    userId: 'auth_user_1',
    sourceUpdatedAt,
  });
}

test('removing a member purges only that member\'s pointers in that organization', async () => {
  const { ctx, tables, log } = removalCtx({ users: [MEMBER] });
  assert.equal((await removeMember(ctx, 200)).status, 'removed');
  assert.deepEqual(log.deletes.slice().sort(), ['panel_mine_a', 'panel_mine_b']);
  assert.deepEqual(tables.spacePrivatePanels.map((row) => row._id).sort(), ['panel_colleague', 'panel_other_org']);
});

test('a stale removal event purges nothing', async () => {
  const { ctx, tables, log } = removalCtx({ users: [{ ...MEMBER, sourceUpdatedAt: 300 }] });
  assert.equal((await removeMember(ctx, 200)).status, 'stale_ignored');
  assert.equal(log.deletes.length, 0);
  assert.equal(tables.spacePrivatePanels.length, 4);
});

test('an already-removed event purges nothing', async () => {
  const { ctx, tables, log } = removalCtx({
    tombstones: [
      { _id: 'tombstone_1', externalOrgId: fake.ORG, externalAuthId: 'auth_user_1', sourceUpdatedAt: 300, removedAt: 300 },
    ],
  });
  assert.equal((await removeMember(ctx, 200)).status, 'already_removed');
  assert.equal(log.deletes.length, 0);
  assert.equal(tables.spacePrivatePanels.length, 4);
});
