const assert = require('node:assert/strict');
const test = require('node:test');

const { setSpaceAgentCanvasToolsForGateway, spaceAgentBindingsForGateway } = require('../convex/spaceAgents.ts');
const fake = require('./helpers/fake-convex.cjs');

/**
 * W3-DEC-CANVAS (D2): the per-binding switch for the room canvas tools, written by
 * `setSpaceAgentCanvasToolsForGateway`. The gateway has already checked that the caller is the Space's owner
 * or a manager. These tests cover the Convex gate behind it: the binding must belong to this Space and
 * organization and be active, one real change bumps projectionVersion once, an unchanged value is a no-op,
 * and the self-API switch is never touched. Driven through the in-memory ctx in test/helpers/fake-convex.cjs.
 */

const NOT_FOUND = 'agent_binding_not_found';
const NOT_ACTIVE = 'agent_binding_not_active';

function binding(overrides = {}) {
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

function bindingCtx(bindingOverrides) {
  return fake.makeCtx(
    fake.memberTables({
      agents: [{ _id: 'agent-1', orgId: 'org_row', name: 'Bot', status: 'active' }],
      spaceAgentBindings: bindingOverrides === null ? [] : [binding(bindingOverrides)],
    }),
  );
}

function setCanvas(ctx, overrides = {}) {
  return setSpaceAgentCanvasToolsForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    spaceRef: 'space-a',
    bindingRef: 'sab_1',
    enabled: true,
    ...overrides,
  });
}

/** The Convex error text is the stable code the gateway maps, so the whole message must match. */
async function rejectsWithCode(promise, code) {
  await assert.rejects(promise, (error) => {
    assert.equal(error.message, code);
    return true;
  });
}

test('turning the canvas switch on bumps projectionVersion once and patches only its own fields', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = bindingCtx({ selfApiEnabled: true });
  const result = await setCanvas(ctx, { enabled: true });
  assert.deepEqual(result, { bindingRef: 'sab_1', canvasToolsEnabled: true, projectionVersion: 5, changed: true });
  assert.equal(log.patches.length, 1);
  assert.deepEqual(Object.keys(log.patches[0].value).sort(), ['canvasToolsEnabled', 'projectionVersion']);
  const stored = tables.spaceAgentBindings[0];
  assert.equal(stored.canvasToolsEnabled, true);
  assert.equal(stored.projectionVersion, 5);
  assert.equal(stored.selfApiEnabled, true, 'the self-API switch must be left as it was');
  assert.equal(log.inserts.length, 0);
});

test('each real change bumps projectionVersion once, and an unchanged value is a no-op', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = bindingCtx({});
  // an absent stored value counts as off, so switching it off is already the state
  assert.deepEqual(await setCanvas(ctx, { enabled: false }), {
    bindingRef: 'sab_1',
    canvasToolsEnabled: false,
    projectionVersion: 4,
    changed: false,
  });
  assert.equal((await setCanvas(ctx, { enabled: true })).projectionVersion, 5);
  assert.deepEqual(await setCanvas(ctx, { enabled: true }), {
    bindingRef: 'sab_1',
    canvasToolsEnabled: true,
    projectionVersion: 5,
    changed: false,
  });
  assert.equal((await setCanvas(ctx, { enabled: false })).projectionVersion, 6);
  assert.equal(log.patches.length, 2, 'one patch per real change and none for the no-ops');
});

test('a paused, pending, revoked or failed binding answers agent_binding_not_active in both directions', async (t) => {
  fake.withServiceKey(t);
  for (const status of ['paused', 'pending', 'revoked', 'failed']) {
    for (const enabled of [true, false]) {
      const { ctx, tables, log } = bindingCtx({ status });
      await rejectsWithCode(setCanvas(ctx, { enabled }), NOT_ACTIVE);
      assert.equal(log.patches.length, 0, `${status}/${enabled}: nothing is patched`);
      assert.equal(tables.spaceAgentBindings[0].canvasToolsEnabled, undefined, `${status}/${enabled}: switch stays absent`);
    }
  }
});

test('a binding of another Space, of another organization, or no binding answers agent_binding_not_found', async (t) => {
  fake.withServiceKey(t);
  for (const overrides of [{ spaceRef: 'space-b' }, { externalOrgId: 'org_other' }, null]) {
    const { ctx, log } = bindingCtx(overrides);
    await rejectsWithCode(setCanvas(ctx, { enabled: true }), NOT_FOUND);
    assert.equal(log.patches.length, 0);
  }
});

test('the projection reports canvasToolsEnabled as false while the switch is absent', async (t) => {
  fake.withServiceKey(t);
  const { ctx } = fake.makeCtx(
    fake.memberTables({
      agents: [{ _id: 'agent-1', orgId: 'org_row', name: 'Bot', status: 'active' }],
      spaceAgentBindings: [
        binding({ _id: 'binding_absent', bindingRef: 'sab_absent' }),
        binding({ _id: 'binding_on', bindingRef: 'sab_on', subjectId: 'agent-agent-2', canvasToolsEnabled: true }),
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
  assert.equal(byRef.sab_absent.canvasToolsEnabled, false);
  assert.equal(byRef.sab_on.canvasToolsEnabled, true);
});

test('the canvas switch needs the service key and an org member, and writes nothing without them', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = bindingCtx({});
  await rejectsWithCode(setCanvas(ctx, { serviceKey: 'wrong' }), 'Unauthorized');
  await rejectsWithCode(setCanvas(ctx, { externalAuthId: 'stranger' }), 'Unauthorized');
  assert.equal(log.patches.length, 0);
});
