const assert = require('node:assert/strict');
const test = require('node:test');
const { getFunctionName } = require('convex/server');

const fake = require('./helpers/fake-convex.cjs');
const spaces = require('../convex/spaces.ts');

/**
 * R-app-04: the Space registration and recipient-audience claims read a bounded window of their
 * outbox through `by_delivery_state_and_next_attempt`, never the whole table. A due row whose Space
 * or audience snapshot is missing is closed with a typed reason, and the claims behind it still run.
 * The fake ignores index order and takes rows in creation order, so each test creates its rows in
 * the order the window must read them.
 */

const NOW = 10_000;
// spaces.ts reads this many rows of each claimable state per window, and at most this many windows per claim.
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

function lifecycleEvent(spaceRef, { nextAttemptAt, createdAt = nextAttemptAt, deliveryState = 'pending' } = {}) {
  return {
    eventId: `space:${spaceRef}:lifecycle:1`,
    spaceRef,
    externalOrgId: fake.ORG,
    lifecycle: 'pending_registration',
    revision: 1,
    createdAt,
    nextAttemptAt,
    deliveryState,
    deliveryAttempts: 0,
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

function audienceEvent(spaceRef, audienceRef, { nextAttemptAt, createdAt = nextAttemptAt, deliveryState = 'pending' } = {}) {
  return {
    eventId: `space:${spaceRef}:recipient-audience:1:register`,
    audienceRef,
    spaceRef,
    externalOrgId: fake.ORG,
    revision: 1,
    createdAt,
    nextAttemptAt,
    deliveryState,
    deliveryAttempts: 0,
  };
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

test('a stale registration head is closed with its reason, and the next due Space is claimed in the same call', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_b')],
    spaceLifecycleEvents: [
      lifecycleEvent('space_gone', { nextAttemptAt: 100 }),
      lifecycleEvent('space_b', { nextAttemptAt: 200 }),
    ],
  });

  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.eventId, 'space:space_b:lifecycle:1');
  assert.equal(claimed.space.kind, 'room');

  const stale = tables.spaceLifecycleEvents.find((row) => row.spaceRef === 'space_gone');
  assert.equal(stale.deliveryState, 'closed');
  assert.equal(stale.lastDeliveryError, 'space_missing');
  assert.equal(stale.leaseOwner, undefined);
  const live = tables.spaceLifecycleEvents.find((row) => row.spaceRef === 'space_b');
  assert.equal(live.deliveryState, 'claimed');
  assert.equal(live.leaseOwner, 'worker-1');

  assert.deepEqual(log.scheduled, [], 'a claim that found its row schedules nothing');
  assert.equal(warn.mock.callCount(), 1, 'the closed row is logged once');
  const payload = warn.mock.calls[0].arguments[1];
  assert.deepEqual(Object.keys(payload).sort(), ['audienceRef', 'eventId', 'reason', 'scope', 'spaceRef']);
  assert.equal(payload.scope, 'registration');
  assert.equal(payload.reason, 'space_missing');
  assert.equal(payload.eventId, 'space:space_gone:lifecycle:1');
  assert.equal(payload.spaceRef, 'space_gone');
});

test('a stale audience head is closed with its reason, and the next due audience is claimed in the same call', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const { ctx, tables, log } = fake.makeCtx({
    spaceRecipientAudiences: [audienceSnapshot('aud_b', 'space_b')],
    spaceRecipientAudienceEvents: [
      audienceEvent('space_a', 'aud_missing', { nextAttemptAt: 100 }),
      audienceEvent('space_b', 'aud_b', { nextAttemptAt: 200 }),
    ],
  });

  const claimed = await claimAudience(ctx);
  assert.equal(claimed.audience.audienceRef, 'aud_b');
  assert.equal(claimed.eventId, 'space:space_b:recipient-audience:1:register');

  const stale = tables.spaceRecipientAudienceEvents.find((row) => row.audienceRef === 'aud_missing');
  assert.equal(stale.deliveryState, 'closed');
  assert.equal(stale.lastDeliveryError, 'audience_snapshot_missing');
  const live = tables.spaceRecipientAudienceEvents.find((row) => row.audienceRef === 'aud_b');
  assert.equal(live.deliveryState, 'claimed');

  assert.deepEqual(log.scheduled, []);
  assert.equal(warn.mock.callCount(), 1);
  const payload = warn.mock.calls[0].arguments[1];
  assert.deepEqual(Object.keys(payload).sort(), ['audienceRef', 'eventId', 'reason', 'scope', 'spaceRef']);
  assert.equal(payload.scope, 'audience');
  assert.equal(payload.reason, 'audience_snapshot_missing');
  assert.equal(payload.audienceRef, 'aud_missing');
});

test('the registration claim order is unchanged: the earliest nextAttemptAt first, then the earliest createdAt', async () => {
  const { ctx } = fake.makeCtx({
    spaces: [spaceRow('space_a'), spaceRow('space_b'), spaceRow('space_c')],
    spaceLifecycleEvents: [
      lifecycleEvent('space_a', { nextAttemptAt: 30, createdAt: 1 }),
      lifecycleEvent('space_b', { nextAttemptAt: 10, createdAt: 5 }),
      lifecycleEvent('space_c', { nextAttemptAt: 10, createdAt: 2 }),
    ],
  });

  const order = [];
  for (let claim = 0; claim < 3; claim += 1) {
    order.push((await claimRegistration(ctx)).event.spaceRef);
  }
  assert.deepEqual(order, ['space_c', 'space_b', 'space_a']);
  assert.equal(await claimRegistration(ctx), null, 'every row is now under a live lease');
});

test('the audience claim order is unchanged: the earliest nextAttemptAt first, then the earliest createdAt', async () => {
  const { ctx } = fake.makeCtx({
    spaceRecipientAudiences: [
      audienceSnapshot('aud_a', 'space_a'),
      audienceSnapshot('aud_b', 'space_b'),
      audienceSnapshot('aud_c', 'space_c'),
    ],
    spaceRecipientAudienceEvents: [
      audienceEvent('space_a', 'aud_a', { nextAttemptAt: 30, createdAt: 1 }),
      audienceEvent('space_b', 'aud_b', { nextAttemptAt: 10, createdAt: 5 }),
      audienceEvent('space_c', 'aud_c', { nextAttemptAt: 10, createdAt: 2 }),
    ],
  });

  const order = [];
  for (let claim = 0; claim < 3; claim += 1) {
    order.push((await claimAudience(ctx)).audience.audienceRef);
  }
  assert.deepEqual(order, ['aud_c', 'aud_b', 'aud_a']);
  assert.equal(await claimAudience(ctx), null, 'every row is now under a live lease');
});

test('a claim reads one bounded window of each outbox, not the whole table', async () => {
  const { ctx } = fake.makeCtx({
    spaces: Array.from({ length: 300 }, (_, index) => spaceRow(`space_${index}`)),
    spaceLifecycleEvents: Array.from({ length: 300 }, (_, index) =>
      lifecycleEvent(`space_${index}`, { nextAttemptAt: 100 + index })),
  });
  const read = countRowsRead(ctx);
  const claimed = await claimRegistration(ctx);
  assert.equal(claimed.event.spaceRef, 'space_0');
  // One window of the pending state (20 rows), then the claimed Space's own row.
  assert.ok(read.rows <= CLAIM_SCAN_LIMIT + 1, `a claim read ${read.rows} rows of a 300-row outbox`);
});

test('an audience claim reads one bounded window of a 300-row outbox', async () => {
  const { ctx } = fake.makeCtx({
    spaceRecipientAudiences: Array.from({ length: 300 }, (_, index) => audienceSnapshot(`aud_${index}`, `space_${index}`)),
    spaceRecipientAudienceEvents: Array.from({ length: 300 }, (_, index) =>
      audienceEvent(`space_${index}`, `aud_${index}`, { nextAttemptAt: 100 + index })),
  });
  const read = countRowsRead(ctx);
  const claimed = await claimAudience(ctx);
  assert.equal(claimed.audience.audienceRef, 'aud_0');
  // One window of the pending state (20 rows), then the claimed row's own snapshot.
  assert.ok(read.rows <= CLAIM_SCAN_LIMIT + 1, `a claim read ${read.rows} rows of a 300-row outbox`);
});

test('a long run of stale registration rows is closed one bounded run at a time, and the next run reaches the Space behind them', async (t) => {
  t.mock.method(console, 'warn', () => {});
  const staleRows = Array.from({ length: 100 }, (_, index) => lifecycleEvent(`gone_${index}`, { nextAttemptAt: 100 + index }));
  const { ctx, tables, log } = fake.makeCtx({
    spaces: [spaceRow('space_live')],
    spaceLifecycleEvents: [...staleRows, lifecycleEvent('space_live', { nextAttemptAt: 500 })],
  });
  const closedCount = () => tables.spaceLifecycleEvents.filter((row) => row.deliveryState === 'closed').length;

  const read = countRowsRead(ctx);
  const first = await claimRegistration(ctx);
  assert.equal(first, null, 'the window bound is reached before the live row');
  assert.equal(read.rows, CLAIM_SCAN_LIMIT * CLAIM_SCAN_WINDOWS, 'one window of rows per scan, and no Space row (every Space is missing)');
  assert.equal(closedCount(), CLAIM_SCAN_LIMIT * CLAIM_SCAN_WINDOWS);
  assert.deepEqual(scheduledNames(log), ['spaceRegistration:deliverOne'], 'the claim schedules the next run');
  assert.equal(tables.spaceLifecycleEvents.find((row) => row.spaceRef === 'space_live').deliveryState, 'pending');

  const second = await claimRegistration(ctx, { workerId: 'worker-2' });
  assert.equal(second.event.spaceRef, 'space_live');
  assert.equal(closedCount(), staleRows.length, 'every stale row is closed by the two runs');
});

test('a long run of stale audience rows is closed one bounded run at a time, and the next run reaches the audience behind them', async (t) => {
  t.mock.method(console, 'warn', () => {});
  const staleRows = Array.from({ length: 100 }, (_, index) => audienceEvent(`space_gone_${index}`, `aud_gone_${index}`, { nextAttemptAt: 100 + index }));
  const { ctx, tables, log } = fake.makeCtx({
    spaceRecipientAudiences: [audienceSnapshot('aud_live', 'space_live')],
    spaceRecipientAudienceEvents: [...staleRows, audienceEvent('space_live', 'aud_live', { nextAttemptAt: 500 })],
  });
  const closedCount = () => tables.spaceRecipientAudienceEvents.filter((row) => row.deliveryState === 'closed').length;

  const read = countRowsRead(ctx);
  const first = await claimAudience(ctx);
  assert.equal(first, null, 'the window bound is reached before the live audience');
  assert.equal(read.rows, CLAIM_SCAN_LIMIT * CLAIM_SCAN_WINDOWS, 'one window of rows per scan, and no snapshot row (every snapshot is missing)');
  assert.equal(closedCount(), CLAIM_SCAN_LIMIT * CLAIM_SCAN_WINDOWS);
  assert.deepEqual(scheduledNames(log), ['spaceAudienceRegistration:deliverOne'], 'the claim schedules the next run');

  const second = await claimAudience(ctx, { workerId: 'worker-2' });
  assert.equal(second.audience.audienceRef, 'aud_live');
  assert.equal(closedCount(), staleRows.length, 'every stale row is closed by the two runs');
});

test('a claim with a non-positive time is refused before any row is read', async () => {
  const { ctx } = fake.makeCtx({
    spaces: [spaceRow('space_a')],
    spaceLifecycleEvents: [lifecycleEvent('space_a', { nextAttemptAt: 1 })],
    spaceRecipientAudienceEvents: [],
  });
  await assert.rejects(claimRegistration(ctx, { now: 0 }), /claim time is invalid/);
  await assert.rejects(claimAudience(ctx, { now: -1 }), /claim time is invalid/);
});
