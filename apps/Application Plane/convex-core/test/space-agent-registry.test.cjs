const assert = require('node:assert/strict');
const test = require('node:test');

const { projectAgentInstallationsForOrg } = require('../convex/spaceAgents.ts');

/**
 * `projectAgentInstallationsForOrg` is the pure join/filter logic behind
 * ADR-0002's `agentInstallationsForOrgForGateway` query
 * (apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md). It is unit-tested
 * directly here — rather than by invoking the `query`-wrapped handler
 * against a mocked `ctx.db` — because this package has no harness for
 * exercising Convex `query`/`mutation` handlers end to end (its existing
 * Convex-side tests, e.g. `space-lifecycle.test.cjs`, all extract the pure
 * decision out of the handler and test that directly; there is no
 * `convex-test`-style in-memory database anywhere in this repo). The
 * `handler` in `spaceAgents.ts` itself is a thin shell around this function:
 * auth checks, indexed reads, and batching the definition/Space lookups —
 * none of which a unit test can exercise without a real Convex deployment.
 */

const ORG = 'org-1';

function binding(overrides) {
  return {
    bindingRef: 'sab_default',
    spaceRef: 'space-a',
    externalOrgId: ORG,
    agentId: 'agent-1',
    subjectId: 'agent-agent-1',
    status: 'active',
    projectionVersion: 1,
    updatedAt: 1000,
    ...overrides,
  };
}

const DEFINITION = { orgId: 'convex-org-id', name: 'Support Bot', description: 'Handles tickets', status: 'active' };
const SPACE_A = { name: 'Team Room', kind: 'room', lifecycle: 'active' };
const SPACE_B = { name: 'Ops Room', kind: 'room', lifecycle: 'active' };

test('an org member sees bindings across multiple Spaces in one call', () => {
  const bindings = [
    binding({ bindingRef: 'sab_a', spaceRef: 'space-a', subjectId: 'agent-agent-1' }),
    binding({ bindingRef: 'sab_b', spaceRef: 'space-b', subjectId: 'agent-agent-1' }),
  ];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([
    ['space-a', SPACE_A],
    ['space-b', SPACE_B],
  ]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.equal(rows.length, 2, 'both Spaces’ bindings must be present in one call');
  const bySpace = Object.fromEntries(rows.map((row) => [row.spaceRef, row]));
  assert.equal(bySpace['space-a'].spaceName, 'Team Room');
  assert.equal(bySpace['space-a'].agentRef, 'agent-1');
  assert.equal(bySpace['space-b'].spaceName, 'Ops Room');
  assert.equal(bySpace['space-b'].agentRef, 'agent-1');
});

test('a revoked binding is excluded', () => {
  const bindings = [
    binding({ bindingRef: 'sab_active', spaceRef: 'space-a', status: 'active' }),
    binding({ bindingRef: 'sab_revoked', spaceRef: 'space-b', status: 'revoked' }),
  ];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([
    ['space-a', SPACE_A],
    ['space-b', SPACE_B],
  ]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.equal(rows.length, 1, 'the revoked binding must not appear');
  assert.equal(rows[0].bindingRef, 'sab_active');
});

test('a binding whose definition was deleted is dropped', () => {
  const bindings = [binding({ bindingRef: 'sab_ghost', agentId: 'agent-deleted' })];
  // The definition lookup returned nothing (deleted), OR returned a stale
  // shell with no orgId — both must be dropped rather than rendered under a
  // placeholder name, the same rule spaceAgentBindingsForGateway enforces.
  const definitionsById = new Map([['agent-deleted', undefined]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(rows, [], 'a binding with no live definition must not be projected');
});

test('a definition record with no orgId (a stale/foreign shell) is also dropped', () => {
  const bindings = [binding({ bindingRef: 'sab_shell', agentId: 'agent-shell' })];
  const definitionsById = new Map([['agent-shell', { name: 'Ghost', orgId: undefined }]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(rows, []);
});

test('a caller from a different org sees nothing', () => {
  // The Convex handler already scopes the `spaceAgentBindings` read to the
  // caller's own org via the `by_external_org` index before this function
  // ever runs — but the projection re-checks `externalOrgId` itself as a
  // second, defense-in-depth gate, the same belt-and-suspenders style
  // `spaceAgentBindingsForGateway` uses (`binding.externalOrgId === args.externalOrgId`
  // even though its own index lookup is already Space-scoped).
  const bindings = [binding({ bindingRef: 'sab_foreign', externalOrgId: 'org-2' })];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(rows, [], 'a binding belonging to a different org must never be visible');
});

test('every field spaceAgentBindingsForGateway projects is also projected here, plus the Space it lives in', () => {
  const bindings = [
    binding({
      bindingRef: 'sab_full',
      spaceRef: 'space-a',
      agentId: 'agent-1',
      subjectId: 'svc-1',
      displayName: 'Custom Name',
      title: 'Support',
      status: 'paused',
      deliveryTargets: [{ channel: 'teams', label: 'General', status: 'active' }],
      triggerModes: ['mention'],
      allowedTools: ['search'],
      approvalMode: 'auto',
      projectionVersion: 3,
      updatedAt: 4242,
    }),
  ];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const [row] = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(row, {
    bindingRef: 'sab_full',
    agentRef: 'agent-1',
    subjectId: 'svc-1',
    spaceRef: 'space-a',
    spaceName: 'Team Room',
    spaceKind: 'room',
    spaceLifecycle: 'active',
    name: 'Custom Name',
    title: 'Support',
    description: 'Handles tickets',
    avatarColor: undefined,
    status: 'paused',
    definitionStatus: 'active',
    deliveryTargets: [{ channel: 'teams', label: 'General', status: 'active' }],
    triggerModes: ['mention'],
    allowedTools: ['search'],
    approvalMode: 'auto',
    projectionVersion: 3,
    updatedAt: 4242,
  });
});

test('a binding for a Space that could not be resolved still projects, with Space fields absent', () => {
  // Defense-in-depth, not an expected steady state: `spacesByRef` is built
  // from every distinct `spaceRef` a binding names, so a miss here would mean
  // the Space row vanished between the two reads. Dropping the whole
  // installation would be worse than showing it unlabeled — the same
  // reasoning `compose_space_agents` applies when a Control-authorized
  // subject has no Application identity yet.
  const bindings = [binding({ bindingRef: 'sab_orphan_space', spaceRef: 'space-missing' })];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map(); // space-missing was never inserted

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.equal(rows.length, 1);
  assert.equal(rows[0].spaceRef, 'space-missing');
  assert.equal(rows[0].spaceName, undefined);
  assert.equal(rows[0].spaceKind, undefined);
  assert.equal(rows[0].spaceLifecycle, undefined);
});

// ---------------------------------------------------------------------------
// WS07-S7: createSpaceAgentForGateway stores the avatar colour only as #rrggbb.
// Driven through a mocked Convex ctx (the handler is a thin shell, see above).
// ---------------------------------------------------------------------------

const { createSpaceAgentForGateway } = require('../convex/spaceAgents.ts');

const CREATE_SERVICE_KEY = 'service-key-aaaa';
const CREATE_AUTH = 'auth_user_1';

function makeCreateCtx() {
  const inserts = [];
  const tables = {
    organizations: [{ _id: 'org_row', externalOrgId: ORG, syncStatus: 'active' }],
    users: [{ _id: 'user_row', externalAuthId: CREATE_AUTH, orgId: 'org_row', syncStatus: 'active' }],
  };
  const ctx = {
    db: {
      query(table) {
        const rows = tables[table] ?? [];
        return {
          withIndex(_name, build) {
            build({ eq() { return this; } });
            return { collect: async () => rows, first: async () => rows[0] ?? null };
          },
        };
      },
      async insert(table, value) {
        inserts.push({ table, value });
        return table === 'agents' ? 'agent123' : `${table}_row`;
      },
    },
  };
  return { ctx, inserts };
}

async function createWithColour(avatarColor) {
  const { ctx, inserts } = makeCreateCtx();
  const result = await createSpaceAgentForGateway._handler(ctx, {
    serviceKey: CREATE_SERVICE_KEY,
    externalAuthId: CREATE_AUTH,
    externalOrgId: ORG,
    spaceRef: 'space-a',
    name: 'Meeting scribe',
    ...(avatarColor === undefined ? {} : { avatarColor }),
  });
  const definition = inserts.find((row) => row.table === 'agents');
  assert.ok(definition, 'the agent definition must be inserted');
  return { result, definition: definition.value };
}

test('createSpaceAgentForGateway avatar colour (WS07-S7)', async (t) => {
  const prior = process.env.CONVEX_INTERNAL_SERVICE_KEY;
  process.env.CONVEX_INTERNAL_SERVICE_KEY = CREATE_SERVICE_KEY;
  t.after(() => {
    if (prior === undefined) delete process.env.CONVEX_INTERNAL_SERVICE_KEY;
    else process.env.CONVEX_INTERNAL_SERVICE_KEY = prior;
  });

  await t.test('stores a #rrggbb colour as given', async () => {
    const { result, definition } = await createWithColour('#2563eb');
    assert.equal(definition.avatarColor, '#2563eb');
    assert.equal(result.subjectId, 'agent-agent123');
  });

  await t.test('trims whitespace around a valid colour', async () => {
    const { definition } = await createWithColour('  #2563EB ');
    assert.equal(definition.avatarColor, '#2563EB');
  });

  await t.test('stores nothing for a named colour', async () => {
    const { definition } = await createWithColour('red');
    assert.equal(definition.avatarColor, undefined);
  });

  await t.test('stores nothing for a short, long, url or css-injection value', async () => {
    for (const bad of ['#12345', '#1234567', 'url(x)', '#12345g', '#2563eb; color: red', '2563eb', '']) {
      const { definition } = await createWithColour(bad);
      assert.equal(definition.avatarColor, undefined, `rejected: ${JSON.stringify(bad)}`);
    }
  });

  await t.test('an omitted colour stores nothing and the agent is still created', async () => {
    const { result, definition } = await createWithColour(undefined);
    assert.equal(definition.avatarColor, undefined);
    assert.equal(result.agentRef, 'agent123');
  });
});

// ---------------------------------------------------------------------------
// WS08-S5: binding wake policy (respondTo / respondToSubjects), driven through
// the in-memory ctx in test/helpers/fake-convex.cjs.
// ---------------------------------------------------------------------------

const {
  setSpaceAgentWakePolicyForGateway,
  spaceAgentBindingsForGateway,
  upsertSpaceAgentBinding,
} = require('../convex/spaceAgents.ts');
const fake = require('./helpers/fake-convex.cjs');

function policyBinding(overrides = {}) {
  return {
    _id: 'binding_row',
    bindingRef: 'sab_1',
    spaceRef: 'space-a',
    externalOrgId: fake.ORG,
    agentId: 'agent-1',
    subjectId: 'agent-agent-1',
    status: 'active',
    projectionVersion: 4,
    updatedAt: 1000,
    createdAt: 500,
    ...overrides,
  };
}

function policyCtx(bindingOverrides) {
  return fake.makeCtx(
    fake.memberTables({
      spaceAgentBindings: bindingOverrides === null ? [] : [policyBinding(bindingOverrides)],
      spaceAgentPolicyEvents: [],
    }),
  );
}

function setPolicy(ctx, overrides = {}) {
  return setSpaceAgentWakePolicyForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    spaceRef: 'space-a',
    bindingRef: 'sab_1',
    respondTo: 'managers',
    expectedUpdatedAt: 1000,
    ...overrides,
  });
}

test('setSpaceAgentWakePolicyForGateway sets the policy, bumps the version and appends one event', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = policyCtx({});
  const result = await setPolicy(ctx, { respondTo: 'allowlist', respondToSubjects: [' user-1 ', 'user-2', 'user-1'] });
  assert.equal(result.changed, true);
  assert.equal(result.respondTo, 'allowlist');
  assert.deepEqual(result.respondToSubjects, ['user-1', 'user-2']);
  assert.equal(result.projectionVersion, 5);
  assert.ok(result.updatedAt > 1000);

  const stored = tables.spaceAgentBindings[0];
  assert.equal(stored.respondTo, 'allowlist');
  assert.deepEqual(stored.respondToSubjects, ['user-1', 'user-2']);
  assert.equal(stored.projectionVersion, 5);
  assert.equal(stored.updatedAt, result.updatedAt);

  assert.equal(log.inserts.length, 1);
  assert.equal(log.inserts[0].table, 'spaceAgentPolicyEvents');
  const event = log.inserts[0].value;
  assert.equal(event.bindingRef, 'sab_1');
  assert.equal(event.actorExternalAuthId, fake.AUTH);
  assert.equal(event.previousRespondTo, undefined);
  assert.equal(event.respondTo, 'allowlist');
  assert.equal(event.projectionVersion, 5);
});

test('a non-allowlist policy stores no subjects and records the previous policy', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = policyCtx({ respondTo: 'allowlist', respondToSubjects: ['user-1'] });
  const result = await setPolicy(ctx, { respondTo: 'members' });
  assert.equal(result.changed, true);
  assert.deepEqual(result.respondToSubjects, []);
  assert.equal(tables.spaceAgentBindings[0].respondTo, 'members');
  assert.equal('respondToSubjects' in tables.spaceAgentBindings[0], false, 'a stale allowlist must not linger');
  assert.equal(log.inserts[0].value.previousRespondTo, 'allowlist');
  assert.deepEqual(log.inserts[0].value.previousRespondToSubjects, ['user-1']);
});

test('an unchanged policy is a no-op: no patch, no event, same version', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = policyCtx({ respondTo: 'managers' });
  const result = await setPolicy(ctx, { respondTo: 'managers' });
  assert.equal(result.changed, false);
  assert.equal(result.projectionVersion, 4);
  assert.equal(result.updatedAt, 1000);
  assert.equal(log.patches.length, 0);
  assert.equal(log.inserts.length, 0);
  // an absent stored policy counts as `members`
  const absent = policyCtx({});
  assert.equal((await setPolicy(absent.ctx, { respondTo: 'members' })).changed, false);
});

test('a stale expectedUpdatedAt is a policy_conflict and writes nothing', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = policyCtx({});
  await fake.rejectsWith(setPolicy(ctx, { expectedUpdatedAt: 999 }), 'policy_conflict');
  assert.equal(log.patches.length + log.inserts.length, 0);
});

test('agent_binding_not_found for a wrong Space, wrong org, unknown ref, revoked or pending binding', async (t) => {
  fake.withServiceKey(t);
  const cases = [
    [{}, { spaceRef: 'space-other' }],
    [{ externalOrgId: 'org_other' }, {}],
    [null, {}],
    [{ status: 'revoked' }, {}],
    [{ status: 'pending' }, {}],
    [{ status: 'failed' }, {}],
  ];
  for (const [binding, args] of cases) {
    const { ctx, log } = policyCtx(binding);
    await fake.rejectsWith(setPolicy(ctx, args), 'agent_binding_not_found');
    assert.equal(log.patches.length + log.inserts.length, 0);
  }
  // a paused binding is allowed
  const paused = policyCtx({ status: 'paused' });
  assert.equal((await setPolicy(paused.ctx)).changed, true);
});

test('invalid_policy for empty, oversize and mismatched allowlists and blank refs', async (t) => {
  fake.withServiceKey(t);
  const many = Array.from({ length: 51 }, (_, i) => `user-${i}`);
  const cases = [
    { respondTo: 'allowlist' },
    { respondTo: 'allowlist', respondToSubjects: [] },
    { respondTo: 'allowlist', respondToSubjects: ['  '] },
    { respondTo: 'allowlist', respondToSubjects: many },
    { respondTo: 'allowlist', respondToSubjects: ['x'.repeat(129)] },
    { respondTo: 'managers', respondToSubjects: ['user-1'] },
    { respondTo: 'members', respondToSubjects: ['user-1'] },
    { spaceRef: '  ' },
    { bindingRef: '' },
  ];
  for (const args of cases) {
    const { ctx, log } = policyCtx({});
    await fake.rejectsWith(setPolicy(ctx, args), 'invalid_policy');
    assert.equal(log.patches.length + log.inserts.length, 0);
  }
  // 50 distinct subjects is the limit and is accepted
  const ok = policyCtx({});
  const exactly50 = many.slice(0, 50);
  assert.equal((await setPolicy(ok.ctx, { respondTo: 'allowlist', respondToSubjects: exactly50 })).respondToSubjects.length, 50);
});

test('setSpaceAgentWakePolicyForGateway needs the service key and an org member', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = policyCtx({});
  await fake.rejectsWith(setPolicy(ctx, { serviceKey: 'wrong' }), 'Unauthorized');
  await fake.rejectsWith(setPolicy(ctx, { externalAuthId: 'stranger' }), 'Unauthorized');
});

test('upsertSpaceAgentBinding refuses a triggerModes list containing the retired group trigger', async () => {
  const { ctx, log } = fake.makeCtx({
    spaces: [],
    agents: [{ _id: 'agent-1', orgId: 'org_row', name: 'Bot' }],
    spaceAgentBindings: [],
  });
  const args = { spaceRef: 'space-a', externalOrgId: fake.ORG, agentId: 'agent-1', subjectId: 'agent-agent-1', status: 'active' };
  await fake.rejectsWith(upsertSpaceAgentBinding._handler(ctx, { ...args, triggerModes: ['mention', 'group'] }), 'retired');
  await fake.rejectsWith(upsertSpaceAgentBinding._handler(ctx, { ...args, triggerModes: ['group'] }), 'retired');
  assert.equal(log.inserts.length, 0);
  const ok = await upsertSpaceAgentBinding._handler(ctx, { ...args, triggerModes: ['mention'] });
  assert.equal(ok.created, true);
});

test('spaceAgentBindingsForGateway projects respondTo and respondToSubjects', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(
    fake.memberTables({
      agents: [{ _id: 'agent-1', orgId: 'org_row', name: 'Bot', status: 'active' }],
      spaceAgentBindings: [
        policyBinding({ respondTo: 'allowlist', respondToSubjects: ['user-1'] }),
        policyBinding({ _id: 'b2', bindingRef: 'sab_2', subjectId: 'agent-agent-2' }),
      ],
    }),
  );
  const rows = await spaceAgentBindingsForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    spaceRef: 'space-a',
  });
  const byRef = Object.fromEntries(rows.map((row) => [row.bindingRef, row]));
  assert.equal(byRef.sab_1.respondTo, 'allowlist');
  assert.deepEqual(byRef.sab_1.respondToSubjects, ['user-1']);
  assert.equal(byRef.sab_2.respondTo, undefined, 'absent stays absent; the gateway reads it as members');
});
