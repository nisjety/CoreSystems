const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const schema = require('../convex/schema.ts').default;

// The tables that existed in convex/schema.ts before the P-CX1 split (38 of them;
// `spaceMemberGrants` was retired on purpose by WS04-S12, which leaves 37). The
// composed schema must keep every one of them; later slices may add tables, so
// this is a subset check, not an equality check.
const PRE_SPLIT_TABLES = [
  'organizations',
  'organizationTombstones',
  'users',
  'membershipTombstones',
  'reconciliationNonces',
  'conversations',
  'messages',
  'jobs',
  'presence',
  'webhooks',
  'plannerDocuments',
  'plannerDocumentStates',
  'auditLog',
  'agents',
  'agentRuns',
  'controlSessions',
  'ingestJobs',
  'projects',
  'spaces',
  'spaceAgentBindings',
  'spaceLifecycleEvents',
  'spaceDeletionRequests',
  'spaceDeletionAuthorizationEvents',
  'spaceDeletionOwnerReceipts',
  'spaceDeletionOwnerDeliveryEvents',
  'spaceReadMarkers',
  'spaceRecipientAudiences',
  'spaceRecipientAudienceEvents',
  'knowledgeQnA',
  'conversationProjectionEvents',
  'conversationInboxItems',
  'conversationProjectedMessages',
  'conversationPresence',
  'spacePresence',
  'conversationAiActions',
  'searchThreads',
  'searchTurns',
];

const SCHEMA_DIR = path.join(__dirname, '..', 'convex', 'schema');
const featureFiles = fs
  .readdirSync(SCHEMA_DIR)
  .filter((name) => name.endsWith('.ts'))
  .sort();

function tableRecordsOf(file) {
  const exported = require(path.join(SCHEMA_DIR, file));
  return Object.entries(exported).filter(([name]) => name.endsWith('Tables'));
}

test('the pre-split list has 37 distinct tables (38 minus the retired spaceMemberGrants)', () => {
  assert.equal(PRE_SPLIT_TABLES.length, 37);
  assert.equal(new Set(PRE_SPLIT_TABLES).size, 37);
});

test('WS04-S12: the composed schema no longer has spaceMemberGrants', () => {
  assert.equal(Object.keys(schema.tables).includes('spaceMemberGrants'), false);
});

test('WS12-S2: the composed schema carries spaceDmHides beside the pre-split tables', () => {
  const composed = Object.keys(schema.tables);
  assert.ok(composed.includes('spaceDmHides'));
  assert.equal(PRE_SPLIT_TABLES.includes('spaceDmHides'), false);
});

const ATTENTION_TABLES = [
  'spaceRoomNotifyPrefs',
  'spaceNotifyUserSettings',
  'spaceThreadFollows',
  'spaceAttentionItems',
  'spaceAttentionEvents',
  'spaceAttentionBudget',
  'spaceMemberStatus',
  'spaceAgentActivity',
  'spaceReminders',
];

test('WS11-S3: the composed schema lists the 9 attention tables', () => {
  assert.equal(new Set(ATTENTION_TABLES).size, 9);
  const composed = Object.keys(schema.tables);
  assert.deepEqual(ATTENTION_TABLES.filter((name) => !composed.includes(name)), []);
});

test('WS11-S3: spaceReadMarkers has by_org_and_subject beside by_space_and_subject', () => {
  const indexes = schema.tables.spaceReadMarkers.indexes.map((index) => [index.indexDescriptor, index.fields]);
  assert.deepEqual(indexes.find(([name]) => name === 'by_org_and_subject'), [
    'by_org_and_subject',
    ['externalOrgId', 'externalAuthId'],
  ]);
  assert.ok(indexes.some(([name]) => name === 'by_space_and_subject'));
});

test('the composed schema still exports every pre-split table name', () => {
  const composed = Object.keys(schema.tables);
  const missing = PRE_SPLIT_TABLES.filter((name) => !composed.includes(name));
  assert.deepEqual(missing, []);
});

test('every convex/schema/*.ts exports a non-empty table record', () => {
  assert.ok(featureFiles.length >= 9, `expected at least 9 feature files, found ${featureFiles.length}`);
  for (const file of featureFiles) {
    const records = tableRecordsOf(file);
    assert.ok(records.length >= 1, `${file} exports no *Tables record`);
    for (const [exportName, record] of records) {
      assert.ok(record && typeof record === 'object', `${file}:${exportName} is not an object`);
      const names = Object.keys(record);
      assert.ok(names.length > 0, `${file}:${exportName} defines no tables`);
      for (const name of names) {
        assert.equal(typeof record[name]?.validator, 'object', `${file}:${exportName}.${name} is not a table definition`);
      }
    }
  }
});

test('no table is defined in two feature files, and the composition drops none', () => {
  const seen = new Map();
  for (const file of featureFiles) {
    for (const [, record] of tableRecordsOf(file)) {
      for (const name of Object.keys(record)) {
        assert.ok(!seen.has(name), `${name} is defined in both ${seen.get(name)} and ${file}`);
        seen.set(name, file);
      }
    }
  }
  assert.deepEqual(Object.keys(schema.tables).sort(), [...seen.keys()].sort());
});

test('schema.ts only composes the feature groups and defines no table itself', () => {
  const source = fs.readFileSync(path.join(__dirname, '..', 'convex', 'schema.ts'), 'utf8');
  assert.equal(/defineTable\s*\(/.test(source), false);
  for (const file of featureFiles) {
    assert.match(source, new RegExp(`from "\\./schema/${file.replace(/\.ts$/, '')}"`), `schema.ts does not import ${file}`);
  }
});
