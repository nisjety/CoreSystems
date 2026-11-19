const assert = require('node:assert/strict');
const test = require('node:test');

// The authorization claim reads only the Application tables; no adapter is loaded here.
const { getFunctionName } = require('convex/server');
const spaces = require('../convex/spaces.ts');
const fake = require('./helpers/fake-convex.cjs');

const NOW = 10_000;
const ORG = 'org_a';

function requestIdOf(spaceRef) {
  return `space:${spaceRef}:deletion:idem-${spaceRef}`;
}

function authorizationEventIdOf(spaceRef) {
  return `${requestIdOf(spaceRef)}:authorize`;
}

/** One personal deletion intent (its request row) and its Control authorization event. */
function authorization(spaceRef, {
  requestState = 'pending_authorization',
  state = 'pending',
  nextAttemptAt = 1_000,
  attempts = 0,
} = {}) {
  const requestId = requestIdOf(spaceRef);
  return {
    spaceDeletionRequests: [{
      _id: `request_${spaceRef}`, requestId, idempotencyKey: `idem-${spaceRef}`, spaceRef,
      externalOrgId: ORG, ownerExternalAuthId: 'user_a', state: requestState, createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionAuthorizationEvents: [{
      _id: `auth_${spaceRef}`, eventId: authorizationEventIdOf(spaceRef), requestId,
      deliveryState: state, deliveryAttempts: attempts, nextAttemptAt, createdAt: 0,
    }],
  };
}

/** Concatenates table fragments into one set of tables. */
function tablesOf(...parts) {
  const tables = {};
  for (const part of parts) {
    for (const [table, rows] of Object.entries(part)) {
      tables[table] = [...(tables[table] ?? []), ...rows];
    }
  }
  return tables;
}

/** A ctx whose runMutation runs the real spaces.ts handlers against the same in-memory tables. */
function harness(tables) {
  const handle = fake.makeCtx(tables, {
    runMutation(fn, args) {
      const [moduleName, exportName] = getFunctionName(fn).split(':');
      assert.equal(moduleName, 'spaces', `unexpected mutation ${getFunctionName(fn)}`);
      return spaces[exportName]._handler(handle.ctx, args);
    },
  });
  return handle;
}

function eventOf(tables, spaceRef) {
  return tables.spaceDeletionAuthorizationEvents.find((row) => row.eventId === authorizationEventIdOf(spaceRef));
}

/** Counts the rows each read of one table returns, through the same ctx the handler uses. */
function countRowsRead(handle, table) {
  const counts = [];
  const realQuery = handle.ctx.db.query;
  handle.ctx.db.query = (name) => {
    const query = realQuery(name);
    if (name === table) {
      for (const method of ['collect', 'take', 'first']) {
        const real = query[method];
        query[method] = async (...args) => {
          const result = await real.apply(query, args);
          counts.push(Array.isArray(result) ? result.length : (result ? 1 : 0));
          return result;
        };
      }
    }
    return query;
  };
  return () => counts.reduce((sum, count) => sum + count, 0);
}

test('a stale authorization event is closed by the claim, and the next pending authorization is claimed', async () => {
  const handle = harness(tablesOf(
    authorization('space_a', { requestState: 'rejected', nextAttemptAt: 1_000 }),
    authorization('space_b', { nextAttemptAt: 2_000 }),
  ));

  const claimed = await spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
  assert.equal(claimed.eventId, authorizationEventIdOf('space_b'));
  assert.equal(claimed.request.requestId, requestIdOf('space_b'));
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'closed');
  assert.equal(eventOf(handle.tables, 'space_a').lastDeliveryError, 'deletion_not_live');
  assert.equal(eventOf(handle.tables, 'space_a').leaseOwner, undefined);
  assert.equal(eventOf(handle.tables, 'space_b').deliveryState, 'claimed');
});

test('the authorization claim rejects an invalid claim time, even on an empty queue', async () => {
  const handle = harness(tablesOf(authorization('space_a', { nextAttemptAt: 1_000 })));
  for (const now of [0, -1, 1.5, Number.NaN]) {
    await assert.rejects(
      spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now }),
      /claim time is invalid/,
      String(now),
    );
  }
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'pending', 'a rejected claim writes nothing');
});

test('a closed authorization event is never claimed again, and an empty queue claims nothing', async () => {
  const handle = harness(tablesOf(
    authorization('space_a', { requestState: 'rejected', state: 'closed', nextAttemptAt: 1_000 }),
  ));
  assert.equal(await spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now: NOW }), null);
  assert.equal(eventOf(handle.tables, 'space_a').deliveryState, 'closed');
});

test('an authorization event whose request is missing is closed with a warning that carries ids only, and the live one is claimed', async (t) => {
  const warn = t.mock.method(console, 'warn', () => {});
  const handle = harness(tablesOf(
    {
      spaceDeletionAuthorizationEvents: [{
        _id: 'auth_space_gone', eventId: authorizationEventIdOf('space_gone'), requestId: requestIdOf('space_gone'),
        deliveryState: 'pending', deliveryAttempts: 0, nextAttemptAt: 1_000, createdAt: 0,
      }],
    },
    authorization('space_b', { nextAttemptAt: 2_000 }),
  ));

  const claimed = await spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
  assert.equal(claimed.eventId, authorizationEventIdOf('space_b'));
  assert.equal(eventOf(handle.tables, 'space_gone').deliveryState, 'closed');
  assert.equal(eventOf(handle.tables, 'space_gone').lastDeliveryError, 'deletion_not_live');
  assert.equal(warn.mock.callCount(), 1);
  const payload = warn.mock.calls[0].arguments[1];
  assert.deepEqual(Object.keys(payload).sort(), ['eventId', 'reason', 'requestId', 'scope']);
  assert.equal(payload.reason, 'deletion_not_live');
  assert.equal(payload.scope, 'authorization');
  assert.equal(payload.eventId, authorizationEventIdOf('space_gone'));
});

test('the authorization claim reads only the due, claimable events and never the acknowledged history', async () => {
  const history = [];
  for (let index = 0; index < 40; index += 1) {
    history.push({
      _id: `auth_old_${index}`, eventId: `old-${index}:authorize`, requestId: `old-${index}`,
      deliveryState: 'acknowledged', deliveryAttempts: 1, nextAttemptAt: 100 + index, createdAt: 0,
    });
  }
  const handle = harness(tablesOf(
    { spaceDeletionAuthorizationEvents: history },
    authorization('space_b', { nextAttemptAt: 2_000 }),
  ));
  const rowsRead = countRowsRead(handle, 'spaceDeletionAuthorizationEvents');

  const claimed = await spaces.claimNextDeletionAuthorization._handler(handle.ctx, { workerId: 'worker-1', now: NOW });
  assert.equal(claimed.eventId, authorizationEventIdOf('space_b'));
  assert.equal(rowsRead(), 1, 'only the one due pending event is read');
});
