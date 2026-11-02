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

// --- WS11-S6 / S9 / S10 / S11, WS12-S8, WS15-S12 indexes ---------------------------------------

function indexFields(table, name) {
  const found = schema.tables[table].indexes.find((index) => index.indexDescriptor === name);
  return found ? found.fields : undefined;
}

test('WS11-S6: spaceRoomNotifyPrefs and spaceReminders list by_space (Space erasure)', () => {
  assert.deepEqual(indexFields('spaceRoomNotifyPrefs', 'by_space'), ['spaceRef']);
  assert.deepEqual(indexFields('spaceReminders', 'by_space'), ['spaceRef']);
});

test('WS11-S10: spaceMemberStatus has by_expiry beside by_org_and_subject', () => {
  assert.deepEqual(indexFields('spaceMemberStatus', 'by_expiry'), ['expiresAt']);
  assert.deepEqual(indexFields('spaceMemberStatus', 'by_org_and_subject'), ['externalOrgId', 'externalAuthId']);
});

test('WS11-S11: spaceReminders has the indexes the create, heal and erase paths read', () => {
  assert.deepEqual(indexFields('spaceReminders', 'by_subject_and_state'), ['externalAuthId', 'state']);
  assert.deepEqual(indexFields('spaceReminders', 'by_due'), ['state', 'remindAt']);
});

test('WS12-S8: spaceReactions is composed with the indexes the reactions API reads', () => {
  assert.ok(Object.keys(schema.tables).includes('spaceReactions'));
  assert.deepEqual(indexFields('spaceReactions', 'by_space_thread'), ['spaceRef', 'threadId']);
  assert.deepEqual(indexFields('spaceReactions', 'by_message_subject_emoji')?.[0], 'messageId');
});

test('WS15-S12: spaceRoomNotices is composed with by_space_and_created and by_created', () => {
  assert.ok(Object.keys(schema.tables).includes('spaceRoomNotices'));
  assert.deepEqual(indexFields('spaceRoomNotices', 'by_space_and_created'), ['spaceRef', 'createdAt']);
  assert.deepEqual(indexFields('spaceRoomNotices', 'by_created'), ['createdAt']);
});

test('WS11-S6: every index the attention ingest and its erasure paths name exists with the field they filter on', () => {
  const expected = [
    ['spaceAttentionItems', 'by_thread', 'threadId'],
    ['spaceAttentionItems', 'by_space', 'spaceRef'],
    ['spaceAttentionItems', 'by_expiry', 'expiresAt'],
    ['spaceAttentionItems', 'by_recipient_state_updated', 'recipientAuthId'],
    ['spaceAttentionItems', 'by_recipient_coalesce', 'recipientAuthId'],
    ['spaceThreadFollows', 'by_thread', 'threadId'],
    ['spaceThreadFollows', 'by_space', 'spaceRef'],
    ['spaceThreadFollows', 'by_subject_and_thread', 'externalAuthId'],
    ['spaceAttentionEvents', 'by_event_id', 'eventId'],
    ['spaceAttentionEvents', 'by_created', 'createdAt'],
    ['spaceAttentionBudget', 'by_recipient', 'recipientAuthId'],
    ['spaceAgentActivity', 'by_space_and_agent', 'spaceRef'],
    ['spaceRoomNotifyPrefs', 'by_subject_and_space', 'externalAuthId'],
    ['spaceRoomNotifyPrefs', 'by_subject', 'externalOrgId'],
    ['spaceNotifyUserSettings', 'by_subject', 'externalOrgId'],
    ['spaceReadMarkers', 'by_space_and_subject', 'spaceRef'],
    ['spaceReadMarkers', 'by_org_and_subject', 'externalOrgId'],
    ['spacePresence', 'by_space', 'spaceRef'],
    ['spacePresence', 'by_space_and_subject', 'spaceRef'],
  ];
  const missing = [];
  for (const [table, name, first] of expected) {
    const fields = indexFields(table, name);
    if (!fields || fields[0] !== first) missing.push(`${table}.${name}`);
  }
  assert.deepEqual(missing, []);
});
