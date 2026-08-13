const assert = require('node:assert/strict');
const test = require('node:test');

const {
  acknowledgeSpaceLifecycleDelivery,
  canClaimSpaceLifecycleDelivery,
  claimSpaceLifecycleDelivery,
  controlRegistrationResourceRef,
  createSpaceLifecycleEvent,
  releaseSpaceLifecycleDelivery,
  transitionSpaceLifecycle,
} = require('../convex/spaceLifecycle.ts');
const {
  canonicalRecipientSubjectIds,
  recipientAudienceHash,
  spaceRecipientAudienceRef,
} = require('../convex/spaceAudience.ts');
const { receiptStatus } = require('../convex/spaceDeletionAuthorization.ts');
const { modelDeletionReceipt } = require('../convex/spaceDeletionModel.ts');
const { dataDeletionReceipt } = require('../convex/spaceDeletionData.ts');
const { ingestionDeletionReceipt } = require('../convex/spaceDeletionIngestion.ts');
const {
  aggregateDeletionReceipts,
  assertReceiptTransition,
  receiptDeadlineHasElapsed,
} = require('../convex/spaceDeletionReceipts.ts');

test('a new Space starts pending Control registration with a stable first event', () => {
  const event = createSpaceLifecycleEvent({
    externalOrgId: 'org_a',
    spaceRef: 'space_a',
    lifecycle: 'pending_registration',
    revision: 1,
  });

  assert.deepEqual(event, {
    eventId: 'space:space_a:lifecycle:1',
    externalOrgId: 'org_a',
    lifecycle: 'pending_registration',
    revision: 1,
    spaceRef: 'space_a',
  });
});

test('Space lifecycle advances monotonically and rejects unsafe transitions', () => {
  assert.deepEqual(
    transitionSpaceLifecycle({ lifecycle: 'pending_registration', revision: 1 }, 'active'),
    { lifecycle: 'active', revision: 2 },
  );
  assert.deepEqual(
    transitionSpaceLifecycle({ lifecycle: 'active', revision: 8 }, 'deleting'),
    { lifecycle: 'deleting', revision: 9 },
  );
  assert.throws(
    () => transitionSpaceLifecycle({ lifecycle: 'deleted', revision: 9 }, 'active'),
    /cannot transition a deleted Space/i,
  );
  assert.throws(
    () => transitionSpaceLifecycle({ lifecycle: 'pending_registration', revision: 1 }, 'suspended'),
    /invalid Space lifecycle transition/i,
  );
});

test('Space lifecycle events reject incomplete identity and invalid revisions', () => {
  assert.throws(
    () => createSpaceLifecycleEvent({
      externalOrgId: '',
      spaceRef: 'space_a',
      lifecycle: 'active',
      revision: 1,
    }),
    /identity is required/i,
  );
  assert.throws(
    () => createSpaceLifecycleEvent({
      externalOrgId: 'org_a',
      spaceRef: 'space_a',
      lifecycle: 'active',
      revision: 0,
    }),
    /positive safe integer/i,
  );
});

test('Space lifecycle delivery claims are lease-fenced and retry without a storm', () => {
  const pending = {
    attempts: 0,
    nextAttemptAt: 100,
    state: 'pending',
  };
  assert.equal(canClaimSpaceLifecycleDelivery(pending, 100), true);
  const claimed = claimSpaceLifecycleDelivery(pending, 'worker-a', 100, 30_000);
  assert.deepEqual(claimed, {
    attempts: 1,
    leaseExpiresAt: 30_100,
    leaseOwner: 'worker-a',
    nextAttemptAt: 100,
    state: 'claimed',
  });
  assert.equal(canClaimSpaceLifecycleDelivery(claimed, 30_099), false);
  assert.equal(canClaimSpaceLifecycleDelivery(claimed, 30_100), true);
  assert.throws(
    () => releaseSpaceLifecycleDelivery(claimed, 'worker-b', 200),
    /active delivery worker/i,
  );
  assert.deepEqual(releaseSpaceLifecycleDelivery(claimed, 'worker-a', 200), {
    attempts: 1,
    nextAttemptAt: 2_200,
    state: 'failed',
  });
});

test('only the lease holder may acknowledge a Space lifecycle delivery', () => {
  const claimed = claimSpaceLifecycleDelivery(
    { attempts: 2, nextAttemptAt: 10, state: 'failed' },
    'worker-a',
    20,
    100,
  );
  assert.throws(
    () => acknowledgeSpaceLifecycleDelivery(claimed, 'worker-b', 21),
    /active delivery worker/i,
  );
  assert.deepEqual(acknowledgeSpaceLifecycleDelivery(claimed, 'worker-a', 21), {
    attempts: 3,
    nextAttemptAt: 21,
    state: 'acknowledged',
  });
});

test('a rejected delivery is terminal and cannot be reclaimed', () => {
  const rejected = {
    attempts: 1,
    nextAttemptAt: 1,
    state: 'rejected',
  };
  assert.equal(canClaimSpaceLifecycleDelivery(rejected, 1_000_000), false);
});

test('only a matching Control receipt may activate a claimed Space registration', () => {
  assert.equal(
    controlRegistrationResourceRef({ data: { space_ref: 'space_a' } }, 'space_a'),
    'space_a',
  );
  assert.throws(
    () => controlRegistrationResourceRef({ data: { space_ref: 'space_other' } }, 'space_a'),
    /did not match/i,
  );
  assert.throws(
    () => controlRegistrationResourceRef({ data: { SpaceRef: 'space_a' } }, 'space_a'),
    /did not match/i,
  );
});

test('recipient audiences are canonical, opaque, and revision-addressed', async () => {
  assert.deepEqual(canonicalRecipientSubjectIds([' user-2 ', 'user-1']), ['user-1', 'user-2']);
  const forward = await recipientAudienceHash(['user-2', 'user-1']);
  const reverse = await recipientAudienceHash(['user-1', 'user-2']);
  assert.equal(forward, reverse);
  assert.equal(forward, 'sha256:cb3fb473b339cd4581363ee94743b61c591b83c7c14af8f7722b6d04ed0dd4da');
  assert.match(forward, /^sha256:[a-f0-9]{64}$/);
  assert.equal(spaceRecipientAudienceRef('space_a', 3), 'space:space_a:recipient-audience:3');
  assert.throws(() => canonicalRecipientSubjectIds(['user-1', 'user-1']), /duplicate/i);
});

test('deletion authorization receipt is bound to the exact durable request', () => {
  assert.equal(receiptStatus({ data: { request_id: 'delete-1', status: 'authorized' } }, 'delete-1'), 'authorized');
  assert.equal(receiptStatus({ data: { request_id: 'delete-1', status: 'blocked_legal_hold' } }, 'delete-1'), 'blocked_legal_hold');
  assert.equal(receiptStatus({ data: { request_id: 'other', status: 'authorized' } }, 'delete-1'), null);
  assert.equal(receiptStatus({ data: { request_id: 'delete-1', status: 'forged' } }, 'delete-1'), null);
});

test('Model deletion receipt is exact-request bound and cannot manufacture success', () => {
  assert.deepEqual(
    modelDeletionReceipt({ request_id: 'delete-1', owner_plane: 'model', owner_outcome: 'partial', remaining_work: ['external_letta_memory_deletion'] }, 'delete-1'),
    { status: 'partial', receiptRef: 'model:delete-1', detail: 'Model owner receipt: external_letta_memory_deletion' },
  );
  assert.equal(modelDeletionReceipt({ request_id: 'other', owner_plane: 'model', owner_outcome: 'succeeded' }, 'delete-1'), null);
  assert.equal(modelDeletionReceipt({ request_id: 'delete-1', owner_plane: 'data', owner_outcome: 'succeeded' }, 'delete-1'), null);
  assert.equal(modelDeletionReceipt({ request_id: 'delete-1', owner_plane: 'model', owner_outcome: 'forged' }, 'delete-1'), null);
});

test('Data deletion receipt is exact-request and owner bound', () => {
  assert.deepEqual(
    dataDeletionReceipt({ request_id: 'delete-1', owner_plane: 'data', owner_outcome: 'partial', remaining_work: 'resource-owner purge required' }, 'delete-1'),
    { status: 'partial', receiptRef: 'data:delete-1', detail: 'Data owner receipt: resource-owner purge required' },
  );
  assert.equal(dataDeletionReceipt({ request_id: 'delete-1', owner_plane: 'model', owner_outcome: 'succeeded' }, 'delete-1'), null);
  assert.equal(dataDeletionReceipt({ request_id: 'other', owner_plane: 'data', owner_outcome: 'succeeded' }, 'delete-1'), null);
});

test('Ingestion deletion receipt is exact-request and owner bound', () => {
  assert.deepEqual(
    ingestionDeletionReceipt({ request_id: 'delete-1', owner_plane: 'ingestion', owner_outcome: 'partial', remaining_work: 'connector owners remain' }, 'delete-1'),
    { status: 'partial', receiptRef: 'ingestion:delete-1', detail: 'Ingestion owner receipt: connector owners remain' },
  );
  assert.equal(ingestionDeletionReceipt({ request_id: 'delete-1', owner_plane: 'data', owner_outcome: 'partial' }, 'delete-1'), null);
});

test('deletion receipt aggregation never infers success from missing or uncertain owners', () => {
  assert.equal(aggregateDeletionReceipts([]), 'pending');
  assert.equal(aggregateDeletionReceipts([{ ownerPlane: 'model', status: 'pending' }]), 'pending');
  assert.equal(aggregateDeletionReceipts([
    { ownerPlane: 'model', status: 'pending' },
    { ownerPlane: 'control', status: 'blocked_legal_hold' },
  ]), 'blocked_legal_hold');
  assert.equal(aggregateDeletionReceipts([{ ownerPlane: 'model', status: 'unknown' }]), 'unknown');
  assert.equal(aggregateDeletionReceipts([
    { ownerPlane: 'model', status: 'succeeded' },
    { ownerPlane: 'data', status: 'failed' },
  ]), 'partial');
  assert.equal(aggregateDeletionReceipts([
    { ownerPlane: 'model', status: 'succeeded' },
    { ownerPlane: 'data', status: 'succeeded' },
  ]), 'succeeded');
  assert.throws(() => assertReceiptTransition('succeeded', 'failed'), /invalid/i);
  assert.doesNotThrow(() => assertReceiptTransition('unknown', 'succeeded'));
  assert.equal(receiptDeadlineHasElapsed('pending', 100, 99), false);
  assert.equal(receiptDeadlineHasElapsed('pending', 100, 100), true);
  assert.equal(receiptDeadlineHasElapsed('succeeded', 100, 101), false);
});

test('only a pending Application deletion receipt can become the local purge success', () => {
  assert.doesNotThrow(() => assertReceiptTransition('pending', 'succeeded'));
  assert.throws(() => assertReceiptTransition('blocked_legal_hold', 'succeeded'), /invalid/i);
  assert.throws(() => assertReceiptTransition('succeeded', 'partial'), /invalid/i);
});
