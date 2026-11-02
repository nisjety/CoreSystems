const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const schema = require('../convex/schema.ts').default;

// The 38 tables that existed in convex/schema.ts before the P-CX1 split. The
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

test('the pre-split list really has 38 distinct tables', () => {
  assert.equal(PRE_SPLIT_TABLES.length, 38);
  assert.equal(new Set(PRE_SPLIT_TABLES).size, 38);
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
