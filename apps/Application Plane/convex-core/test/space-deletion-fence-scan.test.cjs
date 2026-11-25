const assert = require('node:assert/strict');
const test = require('node:test');

const fake = require('./helpers/fake-convex.cjs');
const spaces = require('../convex/spaces.ts');
const schema = require('../convex/schema.ts').default;

/**
 * P2-app-03-rel1: the deletion fence purges one Space's recipient audiences and audience outbox rows
 * through the by-space index `by_space_and_revision`, never by collecting a whole table and filtering
 * it. The fake ignores index names and filters rows on the eq conditions, so these tests count what
 * the fence reads: a fence that scans a whole table reads every other Space's rows too, and records
 * no index at all.
 */

const NOW = 10_000;
const ORG = 'org_a';
const FENCED = 'space_a';
const OTHER_SPACE_COUNT = 300;

function requestIdOf(spaceRef) {
  return `space:${spaceRef}:deletion:idem-${spaceRef}`;
}

function authorizationEventIdOf(spaceRef) {
  return `${requestIdOf(spaceRef)}:authorize`;
}

// One personal deletion intent pending Control's authorization, for a live Space.
function pendingDeletion(spaceRef) {
  const requestId = requestIdOf(spaceRef);
  return {
    spaceDeletionRequests: [{
      requestId, idempotencyKey: `idem-${spaceRef}`, spaceRef, externalOrgId: ORG,
      ownerExternalAuthId: 'user_a', state: 'pending_authorization', createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionAuthorizationEvents: [{
      eventId: authorizationEventIdOf(spaceRef), requestId,
      deliveryState: 'pending', deliveryAttempts: 0, nextAttemptAt: 1_000, createdAt: 0,
    }],
    spaces: [{
      spaceRef, externalOrgId: ORG, kind: 'personal', ownerExternalAuthId: 'user_a',
      lifecycle: 'active', lifecycleRevision: 4,
    }],
  };
}

function audienceRow(spaceRef, revision, state) {
  return {
    audienceRef: `aud_${spaceRef}_${revision}`,
    audienceHash: `sha256:${'0'.repeat(64)}`,
    controlState: 'pending',
    externalOrgId: ORG,
    recipientExternalAuthIds: ['user_a'],
    revision,
    spaceRef,
    state,
    createdAt: 0,
    updatedAt: 0,
  };
}

function audienceEventRow(spaceRef, revision, audienceRef = `aud_${spaceRef}_${revision}`) {
  return {
    eventId: `space:${spaceRef}:recipient-audience:${revision}:register`,
    audienceRef,
    spaceRef,
    externalOrgId: ORG,
    revision,
    createdAt: 0,
    nextAttemptAt: 0,
    deliveryState: 'pending',
    deliveryAttempts: 0,
  };
}

// The fenced Space has a superseded and an active snapshot, and an outbox row per revision. The
// last row is closed because its snapshot is gone. Every row of the Space goes, whatever its state.
function fencedSpaceRecipients() {
  return {
    spaceRecipientAudiences: [audienceRow(FENCED, 1, 'superseded'), audienceRow(FENCED, 2, 'active')],
    spaceRecipientAudienceEvents: [
      audienceEventRow(FENCED, 1),
      audienceEventRow(FENCED, 2),
      {
        ...audienceEventRow(FENCED, 3, 'aud_space_a_gone'),
        deliveryState: 'closed',
        lastDeliveryError: 'audience_snapshot_missing',
      },
    ],
  };
}

// Other Spaces' recipient rows. `space_a2` starts with the fenced ref and must survive the fence.
function otherSpaceRecipients() {
  const spaceRefs = [
    'space_a2',
    ...Array.from({ length: OTHER_SPACE_COUNT }, (_, index) => `space_other_${index}`),
  ];
  return {
    spaceRecipientAudiences: spaceRefs.map((spaceRef) => audienceRow(spaceRef, 1, 'active')),
    spaceRecipientAudienceEvents: spaceRefs.map((spaceRef) => audienceEventRow(spaceRef, 1)),
  };
}

function mergeTables(...parts) {
  const tables = {};
  for (const part of parts) {
    for (const [table, rows] of Object.entries(part)) {
      tables[table] = [...(tables[table] ?? []), ...rows];
    }
  }
  return tables;
}

/**
 * Wraps ctx.db.query so each read records its table, the indexes it went through and the rows it
 * returned. A whole-table collect() goes through no index.
 */
function recordReads(ctx) {
  const reads = [];
  const query = ctx.db.query;
  ctx.db.query = (table) => {
    const inner = query(table);
    const read = { table, indexes: [], rows: 0 };
    reads.push(read);
    const counted = {
      withIndex(name, build) {
        read.indexes.push(name);
        inner.withIndex(name, build);
        return counted;
      },
      order(direction) {
        inner.order(direction);
        return counted;
      },
      filter() {
        return counted;
      },
      async collect() {
        const rows = await inner.collect();
        read.rows += rows.length;
        return rows;
      },
      async take(n) {
        const rows = await inner.take(n);
        read.rows += rows.length;
        return rows;
      },
      async first() {
        const row = await inner.first();
        read.rows += row ? 1 : 0;
        return row;
      },
      async unique() {
        const row = await inner.unique();
        read.rows += row ? 1 : 0;
        return row;
      },
    };
    return counted;
  };
  return reads;
}

function rowsReadFrom(reads, table) {
  return reads.filter((read) => read.table === table).reduce((total, read) => total + read.rows, 0);
}

function indexesReadFrom(reads, table) {
  return reads.filter((read) => read.table === table).flatMap((read) => read.indexes);
}

function rowsOfSpace(rows, spaceRef) {
  return rows.filter((row) => row.spaceRef === spaceRef).length;
}

async function claimAuthorization(handle) {
  const claimed = await spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
  assert.ok(claimed, 'the authorization event is claimed');
}

function acknowledgeAuthorized(handle) {
  return spaces.acknowledgeDeletionAuthorizationClaim._handler(handle.ctx, {
    eventId: authorizationEventIdOf(FENCED), workerId: 'worker-1', now: NOW, status: 'authorized',
  });
}

test('the fence reads only the fenced Space\'s recipient rows, through by_space_and_revision', async () => {
  const handle = fake.makeCtx(mergeTables(pendingDeletion(FENCED), fencedSpaceRecipients(), otherSpaceRecipients()));
  await claimAuthorization(handle);
  const reads = recordReads(handle.ctx);

  assert.deepEqual(await acknowledgeAuthorized(handle), { state: 'authorized' });
  assert.deepEqual(indexesReadFrom(reads, 'spaceRecipientAudiences'), ['by_space_and_revision']);
  assert.deepEqual(indexesReadFrom(reads, 'spaceRecipientAudienceEvents'), ['by_space_and_revision']);
  assert.equal(rowsReadFrom(reads, 'spaceRecipientAudiences'), 2, 'the two snapshots of the fenced Space, not every Space\'s');
  assert.equal(rowsReadFrom(reads, 'spaceRecipientAudienceEvents'), 3, 'the three outbox rows of the fenced Space, not every Space\'s');
});

test('the fence deletes every recipient row of the fenced Space, whatever its state, and no other Space\'s', async () => {
  const handle = fake.makeCtx(mergeTables(pendingDeletion(FENCED), fencedSpaceRecipients(), otherSpaceRecipients()));
  await claimAuthorization(handle);

  assert.deepEqual(await acknowledgeAuthorized(handle), { state: 'authorized' });
  assert.equal(rowsOfSpace(handle.tables.spaceRecipientAudiences, FENCED), 0);
  assert.equal(rowsOfSpace(handle.tables.spaceRecipientAudienceEvents, FENCED), 0);
  assert.equal(handle.tables.spaceRecipientAudiences.length, OTHER_SPACE_COUNT + 1, 'every other Space keeps its snapshot');
  assert.equal(handle.tables.spaceRecipientAudienceEvents.length, OTHER_SPACE_COUNT + 1, 'every other Space keeps its outbox row');
  assert.equal(rowsOfSpace(handle.tables.spaceRecipientAudiences, 'space_a2'), 1, 'a Space whose ref only starts with the fenced ref keeps its snapshot');
  assert.equal(rowsOfSpace(handle.tables.spaceRecipientAudienceEvents, 'space_a2'), 1, 'and its outbox row');
  assert.equal(handle.tables.spaces.find((row) => row.spaceRef === FENCED).lifecycle, 'deleting');
});

test('a Space with no recipient rows reads none of the other Spaces\' rows', async () => {
  const handle = fake.makeCtx(mergeTables(pendingDeletion(FENCED), otherSpaceRecipients()));
  await claimAuthorization(handle);
  const reads = recordReads(handle.ctx);

  assert.deepEqual(await acknowledgeAuthorized(handle), { state: 'authorized' });
  assert.equal(rowsReadFrom(reads, 'spaceRecipientAudiences'), 0);
  assert.equal(rowsReadFrom(reads, 'spaceRecipientAudienceEvents'), 0);
  assert.equal(handle.tables.spaceRecipientAudiences.length, OTHER_SPACE_COUNT + 1);
  assert.equal(handle.tables.spaceRecipientAudienceEvents.length, OTHER_SPACE_COUNT + 1);
});

test('both recipient tables define by_space_and_revision on spaceRef, the index the fence reads', () => {
  for (const table of ['spaceRecipientAudiences', 'spaceRecipientAudienceEvents']) {
    const index = schema.tables[table].indexes.find((candidate) => candidate.indexDescriptor === 'by_space_and_revision');
    assert.ok(index, `${table} defines by_space_and_revision`);
    assert.deepEqual(index.fields, ['spaceRef', 'revision'], table);
  }
});
