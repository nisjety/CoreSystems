const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const {
  assertLegacyWriteAllowed,
  canonicalDefinitionBody,
  canonicalJson,
  definitionPersona,
  nextVersionNumber,
  projectAgentVersion,
  versionDigest,
} = require('../convex/agents.ts');
const { agentTables } = require('../convex/schema/agents.ts');
const { agentDefinitionVersionTables } = require('../convex/schema/agentDefinitionVersions.ts');
const { spaceTables } = require('../convex/schema/spaces.ts');
const schema = require('../convex/schema.ts').default;

/**
 * EP-6 S7: versioned agent definitions. This package has no harness that runs
 * `query`/`mutation` handlers against a mocked database, so the decisions are tested
 * as the pure functions the handlers call (the same pattern as space-agent-registry),
 * and the wiring is pinned by reading the source.
 */

const convexDirectory = path.resolve(__dirname, '..', 'convex');
const ORG_A = 'org-a';
const ORG_B = 'org-b';
const CAP_TICKET = 'cap.tool.ticket.create';

function body(overrides) {
  return {
    name: 'Support Bot',
    systemPrompt: 'Answer tickets politely.',
    model: 'claude-sonnet-5-5',
    toolCapabilityIds: [CAP_TICKET],
    autonomyCap: 'read_only',
    knowledgeBindings: { scope: 'selected', documentIds: ['doc-1'], includeQnA: true },
    ...overrides,
  };
}

function versionRow(overrides) {
  return {
    ...canonicalDefinitionBody(body()),
    orgId: ORG_A,
    version: 2,
    digest: `sha256:${'0'.repeat(64)}`,
    publishedBy: 'subject-1',
    publishedAt: 1000,
    ...overrides,
  };
}

test('a published version is never rewritten: no code path replaces, deletes or patches a version row', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'agents.ts'), 'utf8');
  assert.doesNotMatch(source, /ctx\.db\.(replace|delete)\(/);
  const patchTargets = [...source.matchAll(/ctx\.db\.patch\(\s*([\w.]+)/g)].map((match) => match[1]);
  assert.ok(patchTargets.length > 0, 'expected agent patches to be found');
  for (const target of patchTargets) {
    assert.ok(
      ['agentId', 'args.agentId'].includes(target),
      `ctx.db.patch targets ${target}, which is not the agent row`,
    );
  }
  assert.equal(
    [...source.matchAll(/ctx\.db\.insert\(\s*"agentDefinitionVersions"/g)].length,
    1,
    'only publishForGateway writes a version row, once',
  );
});

test('the digest is stable across key order and tool order, and moves with the body', async () => {
  const canonical = canonicalDefinitionBody(body({ toolCapabilityIds: [CAP_TICKET, 'cap.browser.open'] }));
  const digest = await versionDigest(canonical);
  assert.match(digest, /^sha256:[0-9a-f]{64}$/);

  const reversedKeys = canonicalDefinitionBody({
    knowledgeBindings: { includeQnA: true, documentIds: ['doc-1'], scope: 'selected' },
    autonomyCap: 'read_only',
    toolCapabilityIds: ['cap.browser.open', CAP_TICKET, CAP_TICKET],
    model: 'claude-sonnet-5-5',
    systemPrompt: 'Answer tickets politely.',
    name: 'Support Bot',
  });
  assert.equal(await versionDigest(reversedKeys), digest);
  assert.notEqual(
    await versionDigest(canonicalDefinitionBody(body({ systemPrompt: 'Answer rudely.' }))),
    digest,
    'a changed instruction must change the digest',
  );
  assert.equal(canonicalJson({ b: 1, a: { d: 2, c: [3] } }), '{"a":{"c":[3],"d":2},"b":1}');
  assert.equal(canonicalJson({ a: undefined, b: 1 }), '{"b":1}');
});

test('a draft is validated and canonicalised before it is stored or published', () => {
  const trimmed = canonicalDefinitionBody(body({ name: '  Support Bot  ', model: ' claude-sonnet-5-5 ' }));
  assert.equal(trimmed.name, 'Support Bot');
  assert.equal(trimmed.model, 'claude-sonnet-5-5');
  assert.deepEqual(
    canonicalDefinitionBody(body({ toolCapabilityIds: ['cap.z', 'cap.a', 'cap.a'] })).toolCapabilityIds,
    ['cap.a', 'cap.z'],
  );
  assert.equal(
    'knowledgeBindings' in canonicalDefinitionBody(body({ knowledgeBindings: undefined })),
    false,
    'an absent knowledge binding stays absent',
  );

  assert.throws(() => canonicalDefinitionBody(body({ name: 'S' })), /agent_name_invalid/);
  assert.throws(() => canonicalDefinitionBody(body({ name: 'x'.repeat(61) })), /agent_name_invalid/);
  assert.throws(() => canonicalDefinitionBody(body({ model: '   ' })), /agent_model_invalid/);
  assert.throws(
    () => canonicalDefinitionBody(body({ systemPrompt: 'x'.repeat(4001) })),
    /agent_system_prompt_too_long/,
  );
  assert.equal(canonicalDefinitionBody(body({ systemPrompt: 'x'.repeat(4000) })).systemPrompt.length, 4000);
  assert.throws(() => canonicalDefinitionBody(body({ autonomyCap: 'lockdown' })), /agent_autonomy_cap_invalid/);
  assert.throws(
    () => canonicalDefinitionBody(body({ toolCapabilityIds: ['Bad Tool'] })),
    /agent_tool_capability_invalid/,
  );
  const tooMany = Array.from({ length: 65 }, (_, index) => `cap.tool.n${index}`);
  assert.throws(
    () => canonicalDefinitionBody(body({ toolCapabilityIds: tooMany })),
    /agent_too_many_tool_capabilities/,
  );
});

test('an agent without a published version keeps the legacy persona and stays writable', () => {
  const legacy = { name: 'Legacy', systemPrompt: 'Old words', orgId: ORG_A };
  assert.deepEqual(definitionPersona(legacy, null), { name: 'Legacy', systemPrompt: 'Old words' });
  assert.deepEqual(
    definitionPersona({ name: 'Bare', orgId: ORG_A }, null),
    { name: 'Bare', systemPrompt: null },
  );
  assert.doesNotThrow(() =>
    assertLegacyWriteAllowed(legacy, { systemPrompt: 'New words', tools: ['legacy_tool'] }),
  );
});

test('a versioned agent serves its published version and refuses direct prompt and tool writes', () => {
  const versioned = { name: 'Legacy name', systemPrompt: 'stale', orgId: ORG_A, publishedVersion: 2 };
  const row = versionRow({});
  assert.deepEqual(definitionPersona(versioned, row), {
    name: 'Support Bot',
    systemPrompt: 'Answer tickets politely.',
    version: 2,
    digest: row.digest,
    toolCapabilityIds: [CAP_TICKET],
    autonomyCap: 'read_only',
  });
  assert.throws(
    () => assertLegacyWriteAllowed(versioned, { systemPrompt: 'x' }),
    /agent_versioned_field_refused/,
  );
  assert.throws(() => assertLegacyWriteAllowed(versioned, { tools: [] }), /agent_versioned_field_refused/);
  assert.doesNotThrow(() => assertLegacyWriteAllowed(versioned, { name: 'Renamed' }));
});

test('a pointer whose version cannot be read serves no persona, never the legacy prompt', () => {
  const versioned = { name: 'Legacy name', systemPrompt: 'stale', orgId: ORG_A, publishedVersion: 2 };
  assert.equal(definitionPersona(versioned, null), null);
  assert.equal(definitionPersona(versioned, versionRow({ version: 1 })), null);
});

test('a version of another organisation is refused on read', () => {
  const row = versionRow({});
  assert.equal(projectAgentVersion(row, ORG_B), null);
  assert.equal(projectAgentVersion(row, ORG_A).version, 2);
  const versionedInOtherOrg = { name: 'Legacy name', systemPrompt: 'stale', orgId: ORG_B, publishedVersion: 2 };
  assert.equal(definitionPersona(versionedInOtherOrg, row), null);
});

test('version numbers run from 1 and are never reused', () => {
  assert.equal(nextVersionNumber(null), 1);
  assert.equal(nextVersionNumber({ version: 4 }), 5);
});

test('every new gateway function checks the service key and gateway membership', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'agents.ts'), 'utf8');
  for (const name of [
    'saveDraftForGateway',
    'publishForGateway',
    'listVersionsForGateway',
    'getVersionForGateway',
    'builderDefinitionForGateway',
  ]) {
    const start = source.indexOf(`export const ${name} = `);
    assert.notEqual(start, -1, `${name} is not exported`);
    const next = source.indexOf('\nexport const ', start + 1);
    const block = source.slice(start, next === -1 ? source.length : next);
    assert.match(block, /assertServiceKey\(args\.serviceKey\)/, name);
    assert.match(block, /requireGatewayMember\(ctx, args\.externalAuthId, args\.externalOrgId\)/, name);
  }
  assert.match(source, /assertLegacyWriteAllowed\(agent, args\);/);
});

test('agentPersonaForGateway answers through definitionPersona, reading the pointer first', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'spaceAgents.ts'), 'utf8');
  assert.match(source, /definitionPersona\(agent, published\)/);
  assert.doesNotMatch(source, /return \{ name: agent\.name, systemPrompt: agent\.systemPrompt \?\? null \};/);
});

test('the version table stores authored configuration only', () => {
  const fields = Object.keys(agentDefinitionVersionTables.agentDefinitionVersions.validator.fields).sort();
  assert.deepEqual(fields, [
    'agentId',
    'autonomyCap',
    'digest',
    'knowledgeBindings',
    'model',
    'name',
    'orgId',
    'publishedAt',
    'publishedBy',
    'systemPrompt',
    'toolCapabilityIds',
    'version',
  ]);
});

test('agents carry the pointer and the draft, bindings carry the pinned version, and the schema composes the table', () => {
  const agentFields = Object.keys(agentTables.agents.validator.fields);
  assert.ok(agentFields.includes('publishedVersion'));
  assert.ok(agentFields.includes('draft'));
  assert.ok(Object.keys(spaceTables.spaceAgentBindings.validator.fields).includes('pinnedVersion'));
  assert.ok(schema.tables.agentDefinitionVersions, 'schema.ts composes agentDefinitionVersions');
});

test('the builder read answers null for another organisation and derives the origin Space of a room-born agent', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'agents.ts'), 'utf8');
  const start = source.indexOf('export const builderDefinitionForGateway = ');
  assert.notEqual(start, -1, 'builderDefinitionForGateway is not exported');
  const block = source.slice(start);
  // Another organisation's agent reads exactly like an unknown id.
  assert.match(block, /if \(!agent \|\| agent\.orgId !== organization\._id\) return null;/);
  // A created agent has a creator and no origin; a room-born one (no createdBy) has its first
  // binding's Space, matched on the shared creation timestamp, and no creator.
  assert.match(block, /const origin = agent\.createdBy\s*\?\s*null/);
  assert.match(block, /binding\.agentId === args\.agentId && binding\.createdAt === agent\.createdAt/);
  assert.match(block, /creatorExternalAuthId: creator\?\.externalAuthId \?\? null/);
  assert.match(block, /originSpaceRef: origin\?\.spaceRef \?\? null/);
});

// ---------------------------------------------------------------------------
// W3-EP-6-S7-convex (D3-249, OD-4 b) and W3-EP-6-S9-pin: handlers driven through
// the in-memory ctx in test/helpers/fake-convex.cjs.
// ---------------------------------------------------------------------------

const fake = require('./helpers/fake-convex.cjs');
const agentsModule = require('../convex/agents.ts');
const { setSpaceAgentBindingPinForGateway } = require('../convex/spaceAgents.ts');

function agentRow(overrides = {}) {
  return {
    _id: 'agent-1',
    orgId: 'org_row',
    name: 'Legacy name',
    model: 'legacy-model',
    systemPrompt: 'Legacy prompt',
    tools: ['legacy_tool'],
    knowledgeBindings: { scope: 'all', includeQnA: false },
    status: 'active',
    draft: body({ name: '  Published Bot ', model: ' claude-opus-5-5 ' }),
    ...overrides,
  };
}

function publish(ctx, overrides = {}) {
  return agentsModule.publishForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    agentId: 'agent-1',
    ...overrides,
  });
}

test('publish mirrors the published name, model and knowledge bindings onto the agent row', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(
    fake.memberTables({ agents: [agentRow()], agentDefinitionVersions: [] }),
  );
  const result = await publish(ctx);
  assert.equal(result.version, 1);

  const agent = tables.agents[0];
  assert.equal(agent.publishedVersion, 1);
  // The canonical (trimmed) values of the version, not the raw draft.
  assert.equal(agent.name, 'Published Bot');
  assert.equal(agent.model, 'claude-opus-5-5');
  assert.deepEqual(agent.knowledgeBindings, { scope: 'selected', documentIds: ['doc-1'], includeQnA: true });
  // systemPrompt and tools are served from the version, never mirrored onto the row.
  assert.equal(agent.systemPrompt, 'Legacy prompt');
  assert.deepEqual(agent.tools, ['legacy_tool']);

  const versions = tables.agentDefinitionVersions;
  assert.equal(versions.length, 1);
  assert.equal(versions[0].name, agent.name);
  assert.equal(versions[0].model, agent.model);
  assert.deepEqual(versions[0].knowledgeBindings, agent.knowledgeBindings);
});

test('publishing a version without knowledge bindings clears the row binding', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(
    fake.memberTables({
      agents: [agentRow({ draft: body({ knowledgeBindings: undefined }) })],
      agentDefinitionVersions: [],
    }),
  );
  await publish(ctx);
  assert.equal('knowledgeBindings' in tables.agents[0], false, 'the row must follow the version, not keep the old scope');
});

test('a second publish takes the next number and the row follows the newest version', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = fake.makeCtx(
    fake.memberTables({ agents: [agentRow()], agentDefinitionVersions: [] }),
  );
  await publish(ctx);
  tables.agents[0].draft = body({ name: 'Second Bot', model: 'claude-haiku-5-5' });
  const second = await publish(ctx);
  assert.equal(second.version, 2);
  assert.equal(tables.agents[0].publishedVersion, 2);
  assert.equal(tables.agents[0].name, 'Second Bot');
  assert.equal(tables.agents[0].model, 'claude-haiku-5-5');
  assert.equal(tables.agentDefinitionVersions[0].name, 'Published Bot', 'version 1 is never rewritten');
});

test('publish refuses an agent of another organisation and writes nothing', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = fake.makeCtx(
    fake.memberTables({ agents: [agentRow({ orgId: 'other_org_row' })], agentDefinitionVersions: [] }),
  );
  await fake.rejectsWith(publish(ctx), 'Agent not found');
  assert.equal(log.inserts.length, 0);
  assert.equal(log.patches.length, 0);
  assert.equal(tables.agents[0].name, 'Legacy name');
});

test('publish refuses a wrong service key before any read', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = fake.makeCtx(
    fake.memberTables({ agents: [agentRow()], agentDefinitionVersions: [] }),
  );
  await assert.rejects(publish(ctx, { serviceKey: 'wrong-key' }));
  assert.equal(log.inserts.length, 0);
  assert.equal(log.patches.length, 0);
});

test('the knowledge-binding and retrieval-config writers are internal mutations (D3-243)', () => {
  for (const name of ['updateKnowledgeBindings', 'updateRetrievalConfig']) {
    const fn = agentsModule[name];
    assert.ok(fn, `${name} is exported`);
    assert.equal(fn.isMutation, true, `${name} is a mutation`);
    assert.equal(fn.isInternal, true, `${name} must be internal`);
    assert.notEqual(fn.isPublic, true, `${name} must not be public`);
  }
});

function pinBinding(overrides = {}) {
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

function pinCtx(bindingOverrides = {}, versions = [{ agentId: 'agent-1', version: 1, orgId: 'org_row' }]) {
  return fake.makeCtx(
    fake.memberTables({
      spaceAgentBindings: [pinBinding(bindingOverrides)],
      agentDefinitionVersions: versions,
    }),
  );
}

function setPin(ctx, overrides = {}) {
  return setSpaceAgentBindingPinForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalAuthId: fake.AUTH,
    externalOrgId: fake.ORG,
    spaceRef: 'space-a',
    bindingRef: 'sab_1',
    ...overrides,
  });
}

test('pinning an unknown version is refused and the binding is untouched', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables, log } = pinCtx();
  await fake.rejectsWith(setPin(ctx, { pinnedVersion: 7 }), 'agent_version_not_found');
  assert.equal(log.patches.length, 0);
  assert.equal(tables.spaceAgentBindings[0].pinnedVersion, undefined);
  assert.equal(tables.spaceAgentBindings[0].projectionVersion, 4);
});

test('pinning a version of a different agent is refused', async (t) => {
  fake.withServiceKey(t);
  const { ctx, log } = pinCtx({}, [{ agentId: 'agent-2', version: 1, orgId: 'org_row' }]);
  await fake.rejectsWith(setPin(ctx, { pinnedVersion: 1 }), 'agent_version_not_found');
  assert.equal(log.patches.length, 0);
});

test('pinning an existing version sets the pin and bumps the projection; absent clears it', async (t) => {
  fake.withServiceKey(t);
  const { ctx, tables } = pinCtx();
  const set = await setPin(ctx, { pinnedVersion: 1 });
  assert.equal(set.changed, true);
  assert.equal(set.pinnedVersion, 1);
  assert.equal(set.projectionVersion, 5);
  assert.equal(tables.spaceAgentBindings[0].pinnedVersion, 1);

  const same = await setPin(ctx, { pinnedVersion: 1 });
  assert.equal(same.changed, false);
  assert.equal(tables.spaceAgentBindings[0].projectionVersion, 5);

  const cleared = await setPin(ctx, {});
  assert.equal(cleared.changed, true);
  assert.equal(cleared.pinnedVersion, null);
  assert.equal(cleared.projectionVersion, 6);
  assert.equal('pinnedVersion' in tables.spaceAgentBindings[0], false);
});

test('a pin is refused for a binding of another Space, another org or a removed binding', async (t) => {
  fake.withServiceKey(t);
  for (const overrides of [{ spaceRef: 'space-b' }, { externalOrgId: 'other_org' }, { status: 'removed' }]) {
    const { ctx, log } = pinCtx(overrides);
    await fake.rejectsWith(setPin(ctx, { pinnedVersion: 1 }), 'agent_binding_not_found');
    assert.equal(log.patches.length, 0, JSON.stringify(overrides));
  }
});

test('a pin that is not a positive integer is refused', async (t) => {
  fake.withServiceKey(t);
  for (const bad of [0, -1, 1.5, Number.NaN]) {
    const { ctx, log } = pinCtx();
    await fake.rejectsWith(setPin(ctx, { pinnedVersion: bad }), 'invalid_policy');
    assert.equal(log.patches.length, 0);
  }
});
