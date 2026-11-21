const assert = require('node:assert/strict');
const test = require('node:test');
const { getFunctionName } = require('convex/server');

const fake = require('./helpers/fake-convex.cjs');
const spaces = require('../convex/spaces.ts');

/**
 * P2-app-03-rel1: a claim must still reach a legacy outbox row, one stored before `deliveryState` and
 * `nextAttemptAt` were required (`convex/schema/spaces.ts`, `convex/schema/spaceAudience.ts`). The old
 * full scan selected such a row as pending and due at its `createdAt`. Every bounded read of P2-app-03
 * fixes the state first, so a row with no `deliveryState` was never selected and stayed in the outbox.
 * A row with no `nextAttemptAt` is also read by an explicit `eq(undefined)`, so the claim does not depend
 * on how a missing bound sorts within a state. These tests pin the reads that find both kinds of row,
 * under the same window bound. The fake matches a missing field against `undefined` for `eq` (its `lte`
 * never matches one) and takes rows in creation order, so each test creates its rows in the order the
 * window must read them.
 */

const NOW = 10_000;
const CLAIM_SCAN_LIMIT = 20;
const CLAIM_SCAN_WINDOWS = 4;

function spaceRow(spaceRef) {
  return {
    spaceRef,
    externalOrgId: fake.ORG,
    kind: 'room',
    ownerExternalAuthId: 'user_a',
    createdByExternalAuthId: 'user_a',
    lifecycle: 'pending_registration',
    lifecycleRevision: 1,
    createdAt: 0,
    updatedAt: 0,
  };
}

function audienceSnapshot(audienceRef, spaceRef) {
  return {
    audienceRef,
    audienceHash: `sha256:${'0'.repeat(64)}`,
    controlState: 'pending',
    externalOrgId: fake.ORG,
    recipientExternalAuthIds: ['user_a'],
    revision: 1,
    spaceRef,
    state: 'active',
    createdAt: 0,
    updatedAt: 0,
  };
}

// A registration row as it was stored before its delivery fields were required: identity and createdAt only.
function legacyLifecycleEvent(spaceRef, createdAt) {
  return {
    eventId: `space:${spaceRef}:lifecycle:1`,
    spaceRef,
    externalOrgId: fake.ORG,
    lifecycle: 'pending_registration',
    revision: 1,
    createdAt,
  };
}

// An audience row as it was stored before its delivery fields were required: identity and createdAt only.
function legacyAudienceEvent(spaceRef, audienceRef, createdAt) {
  return {
    eventId: `space:${spaceRef}:recipient-audience:1:register`,
    audienceRef,
    spaceRef,
    externalOrgId: fake.ORG,
    revision: 1,
    createdAt,
  };
}

// A current registration row, due at nextAttemptAt (created at nextAttemptAt unless createdAt is given).
function dueLifecycleEvent(spaceRef, { nextAttemptAt, createdAt = nextAttemptAt, deliveryState = 'pending' }) {
  return { ...legacyLifecycleEvent(spaceRef, createdAt), nextAttemptAt, deliveryState, deliveryAttempts: 0 };
}

// A current audience row, due at nextAttemptAt (created at nextAttemptAt unless createdAt is given).
function dueAudienceEvent(spaceRef, audienceRef, { nextAttemptAt, createdAt = nextAttemptAt, deliveryState = 'pending' }) {
  return { ...legacyAudienceEvent(spaceRef, audienceRef, createdAt), nextAttemptAt, deliveryState, deliveryAttempts: 0 };
}

function claimRegistration(ctx, { now = NOW, workerId = 'worker-1' } = {}) {
  return spaces.claimNextRegistration._handler(ctx, { workerId, now });
}

function claimAudience(ctx, { now = NOW, workerId = 'worker-1' } = {}) {
  return spaces.claimNextRecipientAudienceRegistration._handler(ctx, { workerId, now });
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

/** Counts every row a handler reads: each take, collect, first or unique returns rows to scan. */
function countRowsRead(ctx) {
  const read = { rows: 0 };
  const query = ctx.db.query;
  ctx.db.query = (table) => {
    const inner = query(table);
    const counted = {
      withIndex(name, build) {
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
      async take(n) {
        const rows = await inner.take(n);
        read.rows += rows.length;
        return rows;
      },
      async collect() {
        const rows = await inner.collect();
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
  return read;
}

/** Emulates a read that returns each row twice, as a bound-only range does when it also matches a row missing the bounded field. */
function returnsEveryRowTwice(ctx) {
  const query = ctx.db.query;
  ctx.db.query = (table) => {
    const api = query(table);
    const take = api.take;
    api.take = async (n) => (await take(n)).flatMap((row) => [row, row]);
    return api;
  };
}

test('a legacy registration row is claimed as the old full scan claimed it: pending, due at its createdAt', async () => {
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_legacy')],
    spaceLifecycleEvents: [legacyLifecycleEvent('space_legacy', 50)],
  });

  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.eventId, 'space:space_legacy:lifecycle:1');
  assert.equal(claimed.space.kind, 'room');

  const row = tables.spaceLifecycleEvents[0];
  assert.equal(row.deliveryState, 'claimed');
  assert.equal(row.leaseOwner, 'worker-1');
  assert.equal(row.deliveryAttempts, 1);
  assert.equal(row.nextAttemptAt, NOW);
  assert.equal(row.leaseExpiresAt, NOW + 30_000);
  assert.equal(await claimRegistration(ctx, { workerId: 'worker-2' }), null, 'the claim holds a live lease');
  assert.deepEqual(log.scheduled, []);
});

test('a legacy audience row is claimed as the old full scan claimed it: pending, due at its createdAt', async () => {
  const { ctx, tables, log } = fake.makeCtx({
    spaceRecipientAudiences: [audienceSnapshot('aud_legacy', 'space_legacy')],
    spaceRecipientAudienceEvents: [legacyAudienceEvent('space_legacy', 'aud_legacy', 50)],
  });

  const claimed = await claimAudience(ctx);
  assert.equal(claimed.audience.audienceRef, 'aud_legacy');
  assert.equal(claimed.eventId, 'space:space_legacy:recipient-audience:1:register');

  const row = tables.spaceRecipientAudienceEvents[0];
  assert.equal(row.deliveryState, 'claimed');
  assert.equal(row.leaseOwner, 'worker-1');
  assert.equal(await claimAudience(ctx, { workerId: 'worker-2' }), null, 'the claim holds a live lease');
  assert.deepEqual(log.scheduled, []);
});

test('a row with a delivery state but no nextAttemptAt is claimed when its createdAt is due, in each claimable state', async () => {
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_pending'), spaceRow('space_failed'), spaceRow('space_claimed')],
    spaceLifecycleEvents: [
      { ...legacyLifecycleEvent('space_pending', 50), deliveryState: 'pending' },
      { ...legacyLifecycleEvent('space_failed', 60), deliveryState: 'failed' },
      { ...legacyLifecycleEvent('space_claimed', 70), deliveryState: 'claimed', leaseOwner: 'worker-0', leaseExpiresAt: 5_000 },
    ],
  });

  const order = [];
  for (let claim = 0; claim < 3; claim += 1) {
    order.push((await claimRegistration(ctx)).event.spaceRef);
  }
  assert.deepEqual(order, ['space_pending', 'space_failed', 'space_claimed'], 'the claimable states are claimed in createdAt order');
  assert.equal(await claimRegistration(ctx), null, 'every row is now under a live lease');
  assert.ok(tables.spaceLifecycleEvents.every((row) => row.deliveryState === 'claimed'));
});

test('a row with a nextAttemptAt but no delivery state is claimed when that time is due', async () => {
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_timed')],
    spaceLifecycleEvents: [{ ...legacyLifecycleEvent('space_timed', 40), nextAttemptAt: 45 }],
  });

  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.spaceRef, 'space_timed');
  assert.equal(tables.spaceLifecycleEvents[0].deliveryState, 'claimed');
});

test('a legacy row is not claimed before its createdAt, its due time', async () => {
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_future')],
    spaceLifecycleEvents: [legacyLifecycleEvent('space_future', NOW + 1)],
  });

  assert.equal(await claimRegistration(ctx), null);
  assert.equal(tables.spaceLifecycleEvents[0].deliveryState, undefined, 'the row is left as it was stored');
  assert.deepEqual(log.patches, []);
});

test('legacy and dated rows are claimed in one order: the earliest due time first, a missing time read as createdAt', async () => {
  const { ctx } = fake.makeCtx({
    spaces: [spaceRow('space_a'), spaceRow('space_b'), spaceRow('space_c')],
    spaceLifecycleEvents: [
      legacyLifecycleEvent('space_a', 30),
      dueLifecycleEvent('space_b', { nextAttemptAt: 10, createdAt: 5 }),
      legacyLifecycleEvent('space_c', 20),
    ],
  });

  const order = [];
  for (let claim = 0; claim < 3; claim += 1) {
    order.push((await claimRegistration(ctx)).event.spaceRef);
  }
  assert.deepEqual(order, ['space_b', 'space_c', 'space_a']);
  assert.equal(await claimRegistration(ctx), null, 'every row is now under a live lease');
});

test('a legacy registration row whose Space is missing is closed with its reason, and the claim goes on to the next row', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_b')],
    spaceLifecycleEvents: [
      legacyLifecycleEvent('space_gone', 5),
      dueLifecycleEvent('space_b', { nextAttemptAt: 200 }),
    ],
  });

  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.spaceRef, 'space_b');

  const stale = tables.spaceLifecycleEvents.find((row) => row.spaceRef === 'space_gone');
  assert.equal(stale.deliveryState, 'closed');
  assert.equal(stale.lastDeliveryError, 'space_missing');
  assert.deepEqual(log.scheduled, []);
  assert.equal(warn.mock.callCount(), 1, 'the closed row is logged once');
  const payload = warn.mock.calls[0].arguments[1];
  assert.equal(payload.scope, 'registration');
  assert.equal(payload.reason, 'space_missing');
  assert.equal(payload.eventId, 'space:space_gone:lifecycle:1');
});

test('a legacy audience row whose snapshot is missing is closed with its reason, and the claim goes on to the next row', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables, log } = fake.makeCtx({
    spaceRecipientAudiences: [audienceSnapshot('aud_b', 'space_b')],
    spaceRecipientAudienceEvents: [
      legacyAudienceEvent('space_gone', 'aud_gone', 5),
      dueAudienceEvent('space_b', 'aud_b', { nextAttemptAt: 200 }),
    ],
  });

  const claimed = await claimAudience(ctx);
  assert.equal(claimed.audience.audienceRef, 'aud_b');

  const stale = tables.spaceRecipientAudienceEvents.find((row) => row.audienceRef === 'aud_gone');
  assert.equal(stale.deliveryState, 'closed');
  assert.equal(stale.lastDeliveryError, 'audience_snapshot_missing');
  assert.deepEqual(log.scheduled, []);
  assert.equal(warn.mock.callCount(), 1, 'the closed row is logged once');
  const payload = warn.mock.calls[0].arguments[1];
  assert.equal(payload.scope, 'audience');
  assert.equal(payload.reason, 'audience_snapshot_missing');
  assert.equal(payload.audienceRef, 'aud_gone');
});

test('rows in a state the claim never selects are not read, even with no nextAttemptAt', async () => {
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_a'), spaceRow('space_b'), spaceRow('space_c')],
    spaceLifecycleEvents: [
      { ...legacyLifecycleEvent('space_a', 5), deliveryState: 'acknowledged' },
      { ...legacyLifecycleEvent('space_b', 6), deliveryState: 'closed' },
      { ...legacyLifecycleEvent('space_c', 7), deliveryState: 'rejected' },
    ],
    spaceRecipientAudienceEvents: [
      { ...legacyAudienceEvent('space_a', 'aud_a', 5), deliveryState: 'acknowledged' },
      { ...legacyAudienceEvent('space_b', 'aud_b', 6), deliveryState: 'closed' },
      { ...legacyAudienceEvent('space_c', 'aud_c', 7), deliveryState: 'rejected' },
    ],
  });

  assert.equal(await claimRegistration(ctx), null);
  assert.equal(await claimAudience(ctx), null);
  assert.deepEqual(tables.spaceLifecycleEvents.map((row) => row.deliveryState), ['acknowledged', 'closed', 'rejected']);
  assert.deepEqual(tables.spaceRecipientAudienceEvents.map((row) => row.deliveryState), ['acknowledged', 'closed', 'rejected']);
  assert.deepEqual(log.patches, []);
  assert.deepEqual(log.scheduled, []);
});

test('a claim reads one bounded window of legacy registration rows, not the whole outbox', async () => {
  const { ctx } = fake.makeCtx({
    spaces: Array.from({ length: 300 }, (_, index) => spaceRow(`space_${index}`)),
    spaceLifecycleEvents: Array.from({ length: 300 }, (_, index) => legacyLifecycleEvent(`space_${index}`, 100 + index)),
  });
  const read = countRowsRead(ctx);
  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.spaceRef, 'space_0');
  // One window of the legacy rows (20 rows), then the claimed Space's own row.
  assert.ok(read.rows <= CLAIM_SCAN_LIMIT + 1, `a claim read ${read.rows} rows of a 300-row legacy outbox`);
});

test('a claim reads one bounded window of legacy audience rows, not the whole outbox', async () => {
  const { ctx } = fake.makeCtx({
    spaceRecipientAudiences: Array.from({ length: 300 }, (_, index) => audienceSnapshot(`aud_${index}`, `space_${index}`)),
    spaceRecipientAudienceEvents: Array.from({ length: 300 }, (_, index) =>
      legacyAudienceEvent(`space_${index}`, `aud_${index}`, 100 + index)),
  });
  const read = countRowsRead(ctx);
  const claimed = await claimAudience(ctx);
  assert.equal(claimed.audience.audienceRef, 'aud_0');
  // One window of the legacy rows (20 rows), then the claimed row's own snapshot.
  assert.ok(read.rows <= CLAIM_SCAN_LIMIT + 1, `a claim read ${read.rows} rows of a 300-row legacy outbox`);
});

test('a long run of stale legacy rows is closed one bounded run at a time, and the next run reaches the Space behind them', async (t) => {
  t.mock.method(console, 'warn', () => {});
  const staleRows = Array.from({ length: 100 }, (_, index) => legacyLifecycleEvent(`gone_${index}`, 100 + index));
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_live')],
    spaceLifecycleEvents: [...staleRows, legacyLifecycleEvent('space_live', 500)],
  });
  const closedCount = () => tables.spaceLifecycleEvents.filter((row) => row.deliveryState === 'closed').length;

  const read = countRowsRead(ctx);
  const first = await claimRegistration(ctx);
  assert.equal(first, null, 'the window bound is reached before the live row');
  assert.equal(read.rows, CLAIM_SCAN_LIMIT * CLAIM_SCAN_WINDOWS, 'one window of legacy rows per scan, and no Space row (every Space is missing)');
  assert.equal(closedCount(), CLAIM_SCAN_LIMIT * CLAIM_SCAN_WINDOWS);
  assert.deepEqual(scheduledNames(log), ['spaceRegistration:deliverOne'], 'the claim schedules the next run');

  const second = await claimRegistration(ctx, { workerId: 'worker-2' });
  assert.equal(second.event.spaceRef, 'space_live');
  assert.equal(closedCount(), staleRows.length, 'every stale legacy row is closed by the two runs');
});

test('a row that a read returns twice is closed or claimed once', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables } = fake.makeCtx({
    spaces: [spaceRow('space_b')],
    spaceLifecycleEvents: [
      legacyLifecycleEvent('space_gone', 5),
      dueLifecycleEvent('space_b', { nextAttemptAt: 200 }),
    ],
  });
  returnsEveryRowTwice(ctx);

  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.spaceRef, 'space_b');
  assert.equal(tables.spaceLifecycleEvents.find((row) => row.spaceRef === 'space_gone').deliveryState, 'closed');
  assert.equal(warn.mock.callCount(), 1, 'the stale row returned twice is closed and logged once');
});
