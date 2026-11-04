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
