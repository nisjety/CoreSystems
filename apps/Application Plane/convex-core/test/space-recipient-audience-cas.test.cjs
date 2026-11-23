const assert = require('node:assert/strict');
const test = require('node:test');
const { getFunctionName } = require('convex/server');

const { recipientAudienceHash } = require('../convex/spaceAudience.ts');
const { replaceRecipientAudience } = require('../convex/spaces.ts');
const { makeCtx, rejectsWith } = require('./helpers/fake-convex.cjs');

// The compare-and-set in replaceRecipientAudience, driven through its handler
// against the in-memory ctx. Two concurrent writers at the same base revision
// are serialised by Convex's OCC on a real deployment; that race is a live check
// and is not simulated here.

const SPACE_REF = 'space_ref_cas';
const EXT_ORG = 'org_ext_cas';

function audienceRow(overrides = {}) {
  return {
    _id: 'audience_rev_1',
    audienceHash: 'sha256:previous',
    audienceRef: `space:${SPACE_REF}:recipient-audience:1`,
    controlState: 'acknowledged',
    recipientExternalAuthIds: ['alice'],
    revision: 1,
    spaceRef: SPACE_REF,
    state: 'active',
    updatedAt: 1,
    ...overrides,
  };
}

function fixture({ members = ['alice', 'bob'], previous, lifecycle = 'active', orgSyncStatus = 'active', extraUsers = [] } = {}) {
  return makeCtx({
    spaces: [{ _id: 'space_row', spaceRef: SPACE_REF, externalOrgId: EXT_ORG, lifecycle }],
    organizations: [{ _id: 'org_row', externalOrgId: EXT_ORG, syncStatus: orgSyncStatus }],
    users: [
      ...members.map((externalAuthId) => ({
        _id: `user_${externalAuthId}`,
        externalAuthId,
        orgId: 'org_row',
        syncStatus: 'active',
      })),
      ...extraUsers,
    ],
    spaceRecipientAudiences: previous ? [previous] : [],
    spaceRecipientAudienceEvents: [],
  });
}

function assertNoWrites(log) {
  assert.equal(log.inserts.length, 0, 'no insert');
  assert.equal(log.patches.length, 0, 'no patch');
  assert.equal(log.deletes.length, 0, 'no delete');
  assert.equal(log.scheduled.length, 0, 'no scheduled delivery');
}

test('the first registration writes revision 1 as active with a pending event and one delivery', async () => {
  const { ctx, tables, log } = fixture();
  const result = await replaceRecipientAudience._handler(ctx, {
    spaceRef: SPACE_REF,
    recipientExternalAuthIds: ['bob', 'alice'],
  });

  assert.equal(result.revision, 1);
  assert.equal(result.spaceRef, SPACE_REF);
  assert.equal(result.audienceRef, `space:${SPACE_REF}:recipient-audience:1`);
  assert.equal(result.audienceHash, await recipientAudienceHash(['alice', 'bob']));

  assert.equal(tables.spaceRecipientAudiences.length, 1);
  const [row] = tables.spaceRecipientAudiences;
  assert.equal(row.state, 'active');
  assert.equal(row.controlState, 'pending');
  assert.deepEqual(row.recipientExternalAuthIds, ['alice', 'bob']);

  assert.equal(tables.spaceRecipientAudienceEvents.length, 1);
  assert.equal(tables.spaceRecipientAudienceEvents[0].eventId, `space:${SPACE_REF}:recipient-audience:1:register`);
  assert.equal(tables.spaceRecipientAudienceEvents[0].deliveryState, 'pending');

  assert.equal(log.patches.length, 0, 'nothing is superseded on the first registration');
  assert.equal(log.scheduled.length, 1);
  assert.equal(getFunctionName(log.scheduled[0].fn), 'spaceAudienceRegistration:deliverOne');
});

test('a replacement at the current revision supersedes it and writes the next revision', async () => {
  const { ctx, tables, log } = fixture({ previous: audienceRow() });
  const result = await replaceRecipientAudience._handler(ctx, {
    spaceRef: SPACE_REF,
    recipientExternalAuthIds: ['alice', 'bob'],
    expectedRevision: 1,
  });

  assert.equal(result.revision, 2);
  const byRevision = Object.fromEntries(tables.spaceRecipientAudiences.map((row) => [row.revision, row]));
  assert.equal(byRevision[1].state, 'superseded');
  assert.equal(byRevision[2].state, 'active');
  assert.equal(byRevision[2].controlState, 'pending');
  assert.deepEqual(byRevision[2].recipientExternalAuthIds, ['alice', 'bob']);

  assert.equal(log.patches.length, 1, 'only the previous revision is patched');
  assert.equal(log.patches[0].id, 'audience_rev_1');
  assert.equal(log.inserts.filter((insert) => insert.table === 'spaceRecipientAudiences').length, 1);
  assert.equal(tables.spaceRecipientAudienceEvents[0].eventId, `space:${SPACE_REF}:recipient-audience:2:register`);
});

test('a stale or ahead expectedRevision is a revision conflict and writes nothing', async () => {
  for (const expectedRevision of [0, 2]) {
    const { ctx, log } = fixture({ previous: audienceRow() });
    await rejectsWith(
      replaceRecipientAudience._handler(ctx, {
        spaceRef: SPACE_REF,
        recipientExternalAuthIds: ['alice', 'bob'],
        expectedRevision,
      }),
      'recipient audience revision conflict',
    );
    assertNoWrites(log);
  }
});

test('a replacement without expectedRevision is refused while an audience is active', async () => {
  const { ctx, log } = fixture({ previous: audienceRow() });
  await rejectsWith(
    replaceRecipientAudience._handler(ctx, { spaceRef: SPACE_REF, recipientExternalAuthIds: ['alice', 'bob'] }),
    'recipient audience replacement requires expected revision',
  );
  assertNoWrites(log);
});

test('a first registration cannot claim a base revision: no audience yet is a conflict', async () => {
  const { ctx, log } = fixture();
  await rejectsWith(
    replaceRecipientAudience._handler(ctx, {
      spaceRef: SPACE_REF,
      recipientExternalAuthIds: ['alice'],
      expectedRevision: 0,
    }),
    'recipient audience revision conflict',
  );
  assertNoWrites(log);
});

test('a recipient without an active Application membership is refused before any write', async () => {
  const outsider = fixture({ members: ['alice'] });
  await rejectsWith(
    replaceRecipientAudience._handler(outsider.ctx, {
      spaceRef: SPACE_REF,
      recipientExternalAuthIds: ['alice', 'mallory'],
    }),
    'recipient is not an active Application organization member',
  );
  assertNoWrites(outsider.log);

  const removed = fixture({
    members: ['alice'],
    extraUsers: [{ _id: 'user_carol_deleted', externalAuthId: 'carol', orgId: 'org_row', syncStatus: 'deleted' }],
  });
  await rejectsWith(
    replaceRecipientAudience._handler(removed.ctx, {
      spaceRef: SPACE_REF,
      recipientExternalAuthIds: ['alice', 'carol'],
    }),
    'recipient is not an active Application organization member',
  );
  assertNoWrites(removed.log);
});

test('an inactive Space, a deleted organization, an empty or a duplicate list are refused with no write', async () => {
  const inactive = fixture({ lifecycle: 'deleting' });
  await rejectsWith(
    replaceRecipientAudience._handler(inactive.ctx, { spaceRef: SPACE_REF, recipientExternalAuthIds: ['alice'] }),
    'active Space required',
  );
  assertNoWrites(inactive.log);

  const deletedOrg = fixture({ orgSyncStatus: 'deleted' });
  await rejectsWith(
    replaceRecipientAudience._handler(deletedOrg.ctx, { spaceRef: SPACE_REF, recipientExternalAuthIds: ['alice'] }),
    'Organization not found',
  );
  assertNoWrites(deletedOrg.log);

  const empty = fixture();
  await rejectsWith(
    replaceRecipientAudience._handler(empty.ctx, { spaceRef: SPACE_REF, recipientExternalAuthIds: [] }),
    'recipient audience must not be empty',
  );
  assertNoWrites(empty.log);

  const duplicate = fixture();
  await rejectsWith(
    replaceRecipientAudience._handler(duplicate.ctx, { spaceRef: SPACE_REF, recipientExternalAuthIds: ['alice', 'alice'] }),
    'recipient audience contains a duplicate subject',
  );
  assertNoWrites(duplicate.log);
});
