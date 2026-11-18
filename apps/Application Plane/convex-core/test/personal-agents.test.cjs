const assert = require('node:assert/strict');
const test = require('node:test');

const {
  agentInstallationsForOrgForGateway,
  createPersonalAgentForGateway,
  listInstallableSpaceAgentsForGateway,
} = require('../convex/spaceAgents.ts');
const { setPrimaryAgentForGateway } = require('../convex/users.ts');
const fake = require('./helpers/fake-convex.cjs');

/**
 * WS17-S5 personal agents in one organization with two members. `user_row` (fake.AUTH, "Ana") owns `agent_mine`
 * and a draft; `user_third` ("Cara") owns `agent_cara` and a draft. An owner-visible definition is shown to its
 * creator, or to a subject id the gateway has cleared at Control (`visibleOwnerAgentIds`). A draft is never
 * listed. `agent_org` is an org-visible agent that everyone sees. Driven through test/helpers/fake-convex.cjs.
 */

const ANA = fake.AUTH;
const CARA = 'auth_user_3';

function personal(id, createdBy, name, status) {
  return { _id: id, orgId: 'org_row', name, status, visibility: 'owner', addPolicy: 'owner_only', createdBy };
}

function orgTables(extra = {}) {
  return {
    organizations: [{ _id: 'org_row', externalOrgId: fake.ORG, syncStatus: 'active' }],
    users: [
      { _id: 'user_row', externalAuthId: ANA, orgId: 'org_row', syncStatus: 'active', name: 'Ana Owner' },
      { _id: 'user_third', externalAuthId: CARA, orgId: 'org_row', syncStatus: 'active', name: 'Cara Colleague' },
    ],
    agents: [
      personal('agent_mine', 'user_row', 'Ana Helper', 'active'),
      personal('agent_mine_draft', 'user_row', 'Ana Draft', 'draft'),
      personal('agent_cara', 'user_third', 'Cara Helper', 'active'),
      personal('agent_cara_draft', 'user_third', 'Cara Draft', 'draft'),
      { _id: 'agent_org', orgId: 'org_row', name: 'Team Bot', status: 'active', visibility: 'org', addPolicy: 'anyone' },
    ],
    ...extra,
  };
}

function listFor(ctx, auth, visibleOwnerAgentIds = []) {
  return listInstallableSpaceAgentsForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: auth,
    externalOrgId: fake.ORG,
    spaceRef: 'space-a',
    visibleOwnerAgentIds,
  });
}

function refsOf(rows) {
  return rows.map((row) => row.agentRef).sort();
}

test('a personal agent is listed to its creator once active, and no other member sees it', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(orgTables());
  // Ana sees her active agent and the org agent. Cara's agent is foreign to her, and drafts never appear.
  assert.deepEqual(refsOf(await listFor(ctx, ANA)), ['agent_mine', 'agent_org']);
  // Ana's agent is foreign to Cara, so Cara does not see it without a Control clearance.
  assert.deepEqual(refsOf(await listFor(ctx, CARA)), ['agent_cara', 'agent_org']);
});

test('a foreign personal agent is listed to a colleague only when Control cleared its subject id', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(orgTables());
  assert.deepEqual(refsOf(await listFor(ctx, CARA, ['agent-agent_mine'])), ['agent_cara', 'agent_mine', 'agent_org']);
});

test('drafts are never listed, not even to a subject Control cleared or to their own creator', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(orgTables());
  const cleared = ['agent-agent_mine_draft', 'agent-agent_cara_draft'];
  assert.deepEqual(refsOf(await listFor(ctx, CARA, cleared)), ['agent_cara', 'agent_org']);
  assert.deepEqual(refsOf(await listFor(ctx, ANA, cleared)), ['agent_mine', 'agent_org']);
});

function binding(bindingRef, agentId) {
  return {
    _id: `binding_${bindingRef}`,
    bindingRef,
    spaceRef: 'space-a',
    externalOrgId: fake.ORG,
    agentId,
    subjectId: `agent-${agentId}`,
    status: 'active',
    projectionVersion: 1,
    updatedAt: 1000,
  };
}

test('agentInstallationsForOrgForGateway hides a foreign personal agent and keeps the caller\'s own and org agents', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(
    orgTables({
      spaces: [{ _id: 'space_row_a', spaceRef: 'space-a', externalOrgId: fake.ORG, name: 'Team Room', kind: 'room', lifecycle: 'active' }],
      spaceAgentBindings: [binding('sab_mine', 'agent_mine'), binding('sab_cara', 'agent_cara'), binding('sab_org', 'agent_org')],
    }),
  );
  const installedFor = async (auth) =>
    (
      await agentInstallationsForOrgForGateway._handler(ctx, {
        serviceKey: fake.SERVICE_KEY,
        externalAuthId: auth,
        externalOrgId: fake.ORG,
      })
    )
      .map((row) => row.bindingRef)
      .sort();
  assert.deepEqual(await installedFor(ANA), ['sab_mine', 'sab_org']);
  assert.deepEqual(await installedFor(CARA), ['sab_cara', 'sab_org']);
});

test('createPersonalAgentForGateway makes one draft per clientRequestId and replays the same one', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = fake.makeCtx(orgTables({ agents: [] }));
  const args = {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: ANA,
    externalOrgId: fake.ORG,
    name: 'Meeting Helper',
    clientRequestId: 'create-request-0001',
  };
  const first = await createPersonalAgentForGateway._handler(ctx, args);
  assert.equal(first.status, 'draft');
  assert.equal(first.replayed, false);
  assert.equal(log.inserts.length, 1);
  const stored = tables.agents.find((row) => row.clientRequestId === 'create-request-0001');
  assert.equal(first.agentRef, stored._id);
  assert.equal(stored.visibility, 'owner');
  assert.equal(stored.addPolicy, 'owner_only');
  assert.equal(stored.createdBy, 'user_row');

  const replay = await createPersonalAgentForGateway._handler(ctx, args);
  assert.deepEqual(replay, { agentRef: first.agentRef, subjectId: first.subjectId, status: 'draft', replayed: true });
  assert.equal(log.inserts.length, 1, 'a replay writes nothing');

  const next = await createPersonalAgentForGateway._handler(ctx, { ...args, clientRequestId: 'create-request-0002' });
  assert.equal(next.replayed, false);
  assert.notEqual(next.agentRef, first.agentRef);
  assert.equal(log.inserts.length, 2);
});

test('setPrimaryAgentForGateway accepts only an agent- subject id, or null to clear it', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = fake.makeCtx(orgTables());
  const setPrimary = (agentSubjectId) =>
    setPrimaryAgentForGateway._handler(ctx, {
      serviceKey: fake.SERVICE_KEY,
      externalAuthId: ANA,
      externalOrgId: fake.ORG,
      agentSubjectId,
    });
  for (const bad of ['svc-1', 'agentx', 'user-1']) {
    await fake.rejectsWith(setPrimary(bad), 'agentSubjectId must be an agent id');
  }
  assert.equal(log.patches.length, 0, 'a refused id writes nothing');
  assert.deepEqual(await setPrimary('agent-agent_mine'), { primarySubjectId: 'agent-agent_mine' });
  assert.equal(tables.users.find((row) => row._id === 'user_row').primaryAgentSubjectId, 'agent-agent_mine');
  assert.deepEqual(await setPrimary(null), { primarySubjectId: null });
  assert.equal('primaryAgentSubjectId' in tables.users.find((row) => row._id === 'user_row'), false);
});
