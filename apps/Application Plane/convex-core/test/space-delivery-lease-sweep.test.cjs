const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

// The adapters and the registration workers read these at import time. The values are test fixtures.
process.env.MODEL_SPACE_DELETION_URL = 'http://model.invalid/api/v1/internal/space-deletion/threads';
process.env.CONTROL_SPACE_DELETION_MODEL_BEARER = 'model-bearer-aaaaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_SESSION_BEARER = 'session-bearer-aaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_DELETION_CAPABILITY_BEARER = 'capability-bearer-aaaaaaaaaaaaaaaaaa';
process.env.DATA_SPACE_DELETION_URL = 'http://data.invalid/api/v1/internal/space-deletion/bindings';
process.env.CONTROL_SPACE_DELETION_DATA_BEARER = 'data-bearer-aaaaaaaaaaaaaaaaaaaaaaaaaa';
process.env.INGESTION_SPACE_DELETION_URL = 'http://ingestion.invalid/api/v1/internal/space-deletion/jobs';
process.env.CONTROL_SPACE_DELETION_INGESTION_BEARER = 'ingestion-bearer-aaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_REGISTRATION_URL = 'http://control.invalid/space-registration';
process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN = 'lifecycle-token-aaaaaaaaaaaaaaaaaaaaaaaa';
process.env.CONTROL_SPACE_AUDIENCE_URL = 'http://control.invalid/space-audience';
process.env.APPLICATION_SPACE_AUDIENCE_TOKEN = 'audience-token-aaaaaaaaaaaaaaaaaaaaaaaa';

const { getFunctionName } = require('convex/server');
const spaces = require('../convex/spaces.ts');
const crons = require('../convex/crons.ts').default;
const modelAdapter = require('../convex/spaceDeletionModel.ts');
const dataAdapter = require('../convex/spaceDeletionData.ts');
const ingestionAdapter = require('../convex/spaceDeletionIngestion.ts');
const registrationWorker = require('../convex/spaceRegistration.ts');
const audienceWorker = require('../convex/spaceAudienceRegistration.ts');
const fake = require('./helpers/fake-convex.cjs');

// The clock every test reads. A lease that ends at NOW is exactly due.
const NOW = 1_000_000;
const ORG = 'org_a';
const OWNER_PLANES = ['application', 'control', 'data', 'ingestion', 'model', 'infra'];
const SWITCH_VALUES_NOT_ON = [undefined, '', 'false', '0', '1', 'TRUE', 'True', ' true', 'true ', 'yes'];

// The module each scheduled function name resolves to.
const WORKERS = {
  spaceAudienceRegistration: audienceWorker,
  spaceDeletionData: dataAdapter,
  spaceDeletionIngestion: ingestionAdapter,
  spaceDeletionModel: modelAdapter,
  spaceRegistration: registrationWorker,
};

function requestIdOf(spaceRef) {
  return `space:${spaceRef}:deletion:idem-${spaceRef}`;
}

function eventIdOf(spaceRef, owner) {
  return `space-deletion:${requestIdOf(spaceRef)}:owner:${owner}`;
}

/** The delivery columns an outbox row carries, under the names its table uses. */
function deliveryColumns(claim) {
  return {
    deliveryState: claim.state,
    deliveryAttempts: claim.attempts,
    nextAttemptAt: claim.nextAttemptAt,
    leaseOwner: claim.leaseOwner,
    leaseExpiresAt: claim.leaseExpiresAt,
  };
}

/** A claim whose worker died: its lease ended before NOW. */
function expiredClaim() {
  return { state: 'claimed', attempts: 1, leaseOwner: 'dead-worker', leaseExpiresAt: NOW - 1_000, nextAttemptAt: NOW - 31_000 };
}

/** A claim whose worker is still inside its call: its lease runs past NOW. */
function liveClaim() {
  return { state: 'claimed', attempts: 1, leaseOwner: 'live-worker', leaseExpiresAt: NOW + 25_000, nextAttemptAt: NOW - 5_000 };
}

/**
 * One authorized deletion of `spaceRef` in `deleting`, its owner delivery row for `owner` carrying `claim`, and one receipt per
 * owner plane. The owner's receipt starts `pending`; every other plane's receipt is already `succeeded`.
 */
function deletion(spaceRef, owner, claim) {
  const requestId = requestIdOf(spaceRef);
  return {
    spaces: [{
      _id: `space_${spaceRef}`, spaceRef, externalOrgId: ORG, kind: 'personal', ownerExternalAuthId: 'user_a',
      lifecycle: 'deleting', lifecycleRevision: 4, createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionRequests: [{
      _id: `request_${spaceRef}`, requestId, idempotencyKey: `idem-${spaceRef}`, spaceRef, externalOrgId: ORG,
      ownerExternalAuthId: 'user_a', state: 'authorized', createdAt: 0, updatedAt: 0,
    }],
    spaceDeletionOwnerDeliveryEvents: [{
      _id: `owner_event_${spaceRef}_${owner}`, eventId: eventIdOf(spaceRef, owner), requestId, ownerPlane: owner,
      createdAt: 0, ...deliveryColumns(claim),
    }],
    spaceDeletionOwnerReceipts: OWNER_PLANES.map((ownerPlane) => ({
      _id: `receipt_${spaceRef}_${ownerPlane}`, requestId, ownerPlane,
      status: ownerPlane === owner ? 'pending' : 'succeeded', deadlineAt: NOW + 7 * 24 * 60 * 60 * 1000, updatedAt: 0,
    })),
  };
}

/** One Space awaiting Control's registration, its lifecycle event carrying `claim`. */
function registration(spaceRef, claim) {
  return {
    spaces: [{
      _id: `space_${spaceRef}`, spaceRef, externalOrgId: ORG, kind: 'personal', name: 'Personal Space',
      ownerExternalAuthId: 'user_a', createdByExternalAuthId: 'user_a', lifecycle: 'pending_registration',
      lifecycleRevision: 1, createdAt: 0, updatedAt: 0,
    }],
    spaceLifecycleEvents: [{
      _id: `lifecycle_${spaceRef}`, eventId: `space:${spaceRef}:lifecycle:1`, spaceRef, externalOrgId: ORG,
      lifecycle: 'pending_registration', revision: 1, createdAt: 0, ...deliveryColumns(claim),
    }],
  };
}

/** One recipient audience awaiting Control's registration, its audience event carrying `claim`. */
function audience(spaceRef, claim) {
  const audienceRef = `audience_${spaceRef}`;
  return {
    spaceRecipientAudiences: [{
      _id: `snapshot_${spaceRef}`, audienceRef, audienceHash: `sha256:${spaceRef}`, controlState: 'pending',
      externalOrgId: ORG, recipientExternalAuthIds: ['user_a'], revision: 1, spaceRef, state: 'active',
      createdAt: 0, updatedAt: 0,
    }],
    spaceRecipientAudienceEvents: [{
      _id: `audience_event_${spaceRef}`, eventId: `space:${spaceRef}:recipient-audience:1:register`, audienceRef,
      externalOrgId: ORG, revision: 1, spaceRef, createdAt: 0, ...deliveryColumns(claim),
    }],
  };
}

function jsonResponse(status, payload) {
  return new Response(JSON.stringify(payload), { status, headers: { 'content-type': 'application/json' } });
}

/**
 * One owner's deletion outbox. Each owner gets its own Space, so the three owners never share a request. Its `answer` is the
 * owner's exact 200 receipt, and `outcome` reads the owner's receipt after the run.
 */
function ownerOutbox(owner, spaceRef, worker, prefix) {
  return {
    name: `${owner} owner deletion`,
    worker,
    prefix,
    table: 'spaceDeletionOwnerDeliveryEvents',
    eventId: eventIdOf(spaceRef, owner),
    tables: (claim) => deletion(spaceRef, owner, claim),
    answer: () => jsonResponse(200, { request_id: requestIdOf(spaceRef), owner_plane: owner, owner_outcome: 'succeeded' }),
    outcome: (handle) => handle.tables.spaceDeletionOwnerReceipts
      .find((row) => row.requestId === requestIdOf(spaceRef) && row.ownerPlane === owner).status,
    settled: 'succeeded',
    notClaimed: ['pending', 'failed', 'acknowledged', 'blocked_legal_hold', 'failed_permanent', 'closed'],
  };
}

const REGISTRATION = {
  name: 'Space registration',
  worker: 'spaceRegistration',
  prefix: 'space-registration',
  table: 'spaceLifecycleEvents',
  eventId: 'space:space_r:lifecycle:1',
  tables: (claim) => registration('space_r', claim),
  answer: () => jsonResponse(201, { data: { space_ref: 'space_r' } }),
  outcome: (handle) => handle.tables.spaces.find((row) => row.spaceRef === 'space_r').lifecycle,
  settled: 'active',
  notClaimed: ['pending', 'failed', 'acknowledged', 'rejected'],
};

const AUDIENCE = {
  name: 'recipient-audience registration',
  worker: 'spaceAudienceRegistration',
  prefix: 'space-audience-registration',
  table: 'spaceRecipientAudienceEvents',
  eventId: 'space:space_s:recipient-audience:1:register',
  tables: (claim) => audience('space_s', claim),
  answer: () => jsonResponse(201, {
    data: { space_ref: 'space_s', audience_ref: 'audience_space_s', audience_hash: 'sha256:space_s', revision: 1 },
  }),
  outcome: (handle) => handle.tables.spaceRecipientAudiences.find((row) => row.spaceRef === 'space_s').controlState,
  settled: 'acknowledged',
  notClaimed: ['pending', 'failed', 'acknowledged', 'rejected'],
};

/** Every outbox the sweep re-drives. */
const OUTBOXES = [
  ownerOutbox('model', 'space_m', 'spaceDeletionModel', 'space-deletion-model'),
  ownerOutbox('data', 'space_d', 'spaceDeletionData', 'space-deletion-data'),
  ownerOutbox('ingestion', 'space_i', 'spaceDeletionIngestion', 'space-deletion-ingestion'),
  REGISTRATION,
  AUDIENCE,
];

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

/**
 * A ctx whose runMutation and runQuery run the real spaces.ts handlers against the same in-memory tables. Every query the
 * sweep makes is counted in `handle.queries`.
 */
function harness(tables) {
  const handle = fake.makeCtx(tables, {
    runMutation(fn, args) {
      const [moduleName, exportName] = getFunctionName(fn).split(':');
      assert.equal(moduleName, 'spaces', `unexpected mutation ${getFunctionName(fn)}`);
      return spaces[exportName]._handler(handle.ctx, args);
    },
  });
  handle.queries = 0;
  handle.ctx.runQuery = async (fn, args) => {
    handle.queries += 1;
    const [moduleName, exportName] = getFunctionName(fn).split(':');
    assert.equal(moduleName, 'spaces', `unexpected query ${getFunctionName(fn)}`);
    return spaces[exportName]._handler(handle.ctx, args);
  };
  return handle;
}

function sweep(handle) {
  return spaces.sweepExpiredDeliveryLeases._handler(handle.ctx, {});
}

function eventRow(handle, outbox) {
  return handle.tables[outbox.table].find((row) => row.eventId === outbox.eventId);
}

function scheduledNames(log) {
  return log.scheduled.map((entry) => getFunctionName(entry.fn));
}

/** Runs every scheduled delivery, as the scheduler would, and returns the results in order. */
async function runScheduled(handle) {
  const results = [];
  for (const entry of handle.log.scheduled.splice(0)) {
    const [moduleName, exportName] = getFunctionName(entry.fn).split(':');
    assert.ok(WORKERS[moduleName], `unexpected scheduled ${getFunctionName(entry.fn)}`);
    results.push(await WORKERS[moduleName][exportName]._handler(handle.ctx, entry.args));
  }
  return results;
}

/** Answers every call a worker makes with `reply()` while `body` runs, and returns the requests sent. */
async function withReply(reply, body) {
  const sent = [];
  const realFetch = global.fetch;
  global.fetch = async (url, init) => {
    sent.push({ url, init });
    return reply();
  };
  try {
    await body();
  } finally {
    global.fetch = realFetch;
  }
  return sent;
}

/** Sets SPACE_DELIVERY_LEASE_SWEEP for one test (`undefined` removes it), restored afterwards. */
function setSwitch(t, value) {
  const prior = process.env.SPACE_DELIVERY_LEASE_SWEEP;
  t.after(() => {
    if (prior === undefined) delete process.env.SPACE_DELIVERY_LEASE_SWEEP;
    else process.env.SPACE_DELIVERY_LEASE_SWEEP = prior;
  });
  if (value === undefined) delete process.env.SPACE_DELIVERY_LEASE_SWEEP;
  else process.env.SPACE_DELIVERY_LEASE_SWEEP = value;
}

/** Fixes the clock the sweep and its claims read, and silences the sweep's warning for the test. */
function quiet(t) {
  t.mock.method(Date, 'now', () => NOW);
  return t.mock.method(console, 'warn', () => {});
}

for (const outbox of OUTBOXES) {
  test(`${outbox.name}: an expired claim is re-driven, and the run the sweep schedules holds and acknowledges it`, async (t) => {
    const warn = quiet(t);
    setSwitch(t, 'true');
    const handle = harness(tablesOf(outbox.tables(expiredClaim())));

    assert.deepEqual(await sweep(handle), { status: 'swept', scheduled: 1 });
    assert.deepEqual(handle.log.mutations, [], 'the sweep writes nothing itself');
    assert.deepEqual(handle.log.patches, [], 'the sweep writes nothing itself');
    assert.deepEqual(scheduledNames(handle.log), [`${outbox.worker}:deliverOne`]);
    const workerId = handle.log.scheduled[0].args.workerId;
    assert.ok(workerId.startsWith(`${outbox.prefix}:${outbox.eventId}:`), workerId);
    assert.equal(warn.mock.callCount(), 1);
    assert.deepEqual(Object.keys(warn.mock.calls[0].arguments[1]), ['scheduled'], 'the warning carries a count only');

    let leaseOwnerWhenAsked;
    const sent = await withReply(() => {
      leaseOwnerWhenAsked = eventRow(handle, outbox).leaseOwner;
      return outbox.answer();
    }, async () => {
      assert.equal((await runScheduled(handle)).length, 1);
    });
    assert.equal(sent.length, 1, 'the re-driven run asks once');
    assert.equal(leaseOwnerWhenAsked, workerId, 'the re-driven run holds the claim before it asks');
    assert.equal(eventRow(handle, outbox).deliveryState, 'acknowledged');
    assert.equal(outbox.outcome(handle), outbox.settled);
  });

  test(`${outbox.name}: a claim whose lease is still live is not re-driven, and the sweep leaves it alone`, async (t) => {
    quiet(t);
    setSwitch(t, 'true');
    const handle = harness(tablesOf(outbox.tables(liveClaim())));

    assert.deepEqual(await sweep(handle), { status: 'swept', scheduled: 0 });
    assert.deepEqual(scheduledNames(handle.log), [], 'nothing is scheduled for a live lease');
    assert.equal(eventRow(handle, outbox).deliveryState, 'claimed');
    assert.equal(eventRow(handle, outbox).leaseOwner, 'live-worker');
  });

  test(`${outbox.name}: a lease counts as expired from the instant it ends, and not one millisecond earlier`, async (t) => {
    quiet(t);
    setSwitch(t, 'true');
    const endsNow = harness(tablesOf(outbox.tables({ ...expiredClaim(), leaseExpiresAt: NOW })));
    assert.deepEqual(await sweep(endsNow), { status: 'swept', scheduled: 1 });
    const endsLater = harness(tablesOf(outbox.tables({ ...expiredClaim(), leaseExpiresAt: NOW + 1 })));
    assert.deepEqual(await sweep(endsLater), { status: 'swept', scheduled: 0 });
  });

  test(`${outbox.name}: a row that is not claimed is never re-driven, even with an old lease left on it`, async (t) => {
    quiet(t);
    setSwitch(t, 'true');
    for (const state of outbox.notClaimed) {
      const handle = harness(tablesOf(outbox.tables({ ...expiredClaim(), state })));
      assert.deepEqual(await sweep(handle), { status: 'swept', scheduled: 0 }, state);
      assert.deepEqual(scheduledNames(handle.log), [], state);
      assert.equal(eventRow(handle, outbox).deliveryState, state);
      assert.equal(eventRow(handle, outbox).leaseOwner, 'dead-worker', state);
    }
  });

  test(`${outbox.name}: with the switch off, the sweep reads and schedules nothing for any value but the literal true`, async (t) => {
    quiet(t);
    setSwitch(t, undefined);
    for (const value of SWITCH_VALUES_NOT_ON) {
      if (value === undefined) delete process.env.SPACE_DELIVERY_LEASE_SWEEP;
      else process.env.SPACE_DELIVERY_LEASE_SWEEP = value;
      const handle = harness(tablesOf(outbox.tables(expiredClaim())));

      assert.deepEqual(await sweep(handle), { status: 'disabled', scheduled: 0 }, String(value));
      assert.equal(handle.queries, 0, `${JSON.stringify(value)} reads no lease`);
      assert.deepEqual(scheduledNames(handle.log), [], `${JSON.stringify(value)} schedules nothing`);
      assert.equal(eventRow(handle, outbox).deliveryState, 'claimed', `${JSON.stringify(value)} leaves the claim alone`);
      assert.equal(eventRow(handle, outbox).leaseOwner, 'dead-worker', `${JSON.stringify(value)} leaves the lease alone`);
    }
  });
}

test('one sweep re-drives at most twenty expired claims of one outbox, the longest-expired first, and the rest wait for a later run', async (t) => {
  quiet(t);
  setSwitch(t, 'true');
  const parts = [];
  for (let index = 0; index < 25; index += 1) {
    parts.push(deletion(`space_${index}`, 'model', {
      state: 'claimed', attempts: 1, leaseOwner: `dead-${index}`,
      leaseExpiresAt: NOW - 90_000 + index, nextAttemptAt: NOW - 120_000 + index,
    }));
  }
  const handle = harness(tablesOf(...parts));

  assert.deepEqual(await sweep(handle), { status: 'swept', scheduled: 20 });
  const workerIds = handle.log.scheduled.map((entry) => entry.args.workerId);
  assert.equal(workerIds.length, 20);
  assert.ok(workerIds[0].startsWith(`space-deletion-model:${eventIdOf('space_0', 'model')}:`), 'the longest-expired lease is re-driven first');
  assert.ok(workerIds[19].startsWith(`space-deletion-model:${eventIdOf('space_19', 'model')}:`));
  assert.equal(
    workerIds.some((workerId) => workerId.includes(`${eventIdOf('space_20', 'model')}:`)),
    false,
    'the twenty-first waits for a later run',
  );
});

test('one sweep covers every outbox: an expired claim of each owner, of registration and of recipient audience is re-driven in one pass', async (t) => {
  quiet(t);
  setSwitch(t, 'true');
  const handle = harness(tablesOf(...OUTBOXES.map((outbox) => outbox.tables(expiredClaim()))));

  assert.deepEqual(await sweep(handle), { status: 'swept', scheduled: 5 });
  assert.deepEqual(scheduledNames(handle.log), [
    'spaceDeletionModel:deliverOne',
    'spaceDeletionData:deliverOne',
    'spaceDeletionIngestion:deliverOne',
    'spaceRegistration:deliverOne',
    'spaceAudienceRegistration:deliverOne',
  ]);
});

test('a sweep that finds nothing expired schedules nothing and reports zero', async (t) => {
  quiet(t);
  setSwitch(t, 'true');
  const handle = harness(tablesOf(...OUTBOXES.map((outbox) => outbox.tables(liveClaim()))));

  assert.deepEqual(await sweep(handle), { status: 'swept', scheduled: 0 });
  assert.deepEqual(handle.log.scheduled, []);
  assert.equal(handle.queries, 5, 'each owner, registration and audience outbox is read once');
});

test('crons.ts registers the delivery lease sweep every five minutes against spaces.sweepExpiredDeliveryLeases', () => {
  const job = crons.crons['space delivery lease sweep'];
  assert.ok(job, 'cron "space delivery lease sweep" is not registered');
  assert.equal(job.name, 'spaces:sweepExpiredDeliveryLeases');
  assert.deepEqual(job.schedule, { minutes: 5, type: 'interval' });
  assert.deepEqual(job.args, [{}]);
});

// P2-app-02-rel1: SPACE_DELIVERY_LEASE_SWEEP is wired like its siblings wherever a switch lives. startup.sh hands it to Convex;
// the convex-gateway block of the convex-core compose passes it with the off default 0; and the flag registry holds one dark row
// that reads only the literal true and names spaces.ts as its reader. The compose line and the registry row are shared files
// that the lead applies, so the compose and registry tests below fail until that is done.
const CONVEX_CORE = path.join(__dirname, '..');
const FLAG_REGISTRY = path.join(CONVEX_CORE, '..', '..', 'Control Plane', 'flag-registry', 'flag-registry.json');

function readTextFile(file) {
  return fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n');
}

/** The lines of one service block in docker-compose.yml, from its key up to the next service key. */
function composeServiceLines(compose, service) {
  const lines = compose.split('\n');
  const start = lines.findIndex((line) => line.trimEnd() === `  ${service}:`);
  assert.ok(start >= 0, `docker-compose.yml declares the ${service} service`);
  const next = lines.findIndex((line, index) => index > start && /^ {2}[A-Za-z0-9_-]+:\s*$/.test(line));
  return lines.slice(start + 1, next < 0 ? lines.length : next).map((line) => line.trimEnd());
}

test('startup.sh hands SPACE_DELIVERY_LEASE_SWEEP to Convex, defaulting to the word false', () => {
  const startup = readTextFile(path.join(CONVEX_CORE, 'startup.sh'));
  assert.ok(
    startup.includes('set_convex_env "SPACE_DELIVERY_LEASE_SWEEP" "${SPACE_DELIVERY_LEASE_SWEEP:-false}"'),
    'startup.sh passes the switch to Convex with the off default false',
  );
});

test('the convex-gateway block of the convex-core compose passes SPACE_DELIVERY_LEASE_SWEEP once, with the off default false', () => {
  const compose = readTextFile(path.join(CONVEX_CORE, 'docker-compose.yml'));
  const lines = composeServiceLines(compose, 'convex-gateway').filter((line) => /^ {6}SPACE_DELIVERY_LEASE_SWEEP:/.test(line));
  assert.deepEqual(lines, ['      SPACE_DELIVERY_LEASE_SWEEP: ${SPACE_DELIVERY_LEASE_SWEEP:-false}']);
});

test('the flag registry holds one dark switch row for SPACE_DELIVERY_LEASE_SWEEP that reads only the literal true and names spaces.ts', () => {
  const registry = JSON.parse(readTextFile(FLAG_REGISTRY));
  const rows = registry.flags.filter((flag) => flag.name === 'SPACE_DELIVERY_LEASE_SWEEP');
  assert.equal(rows.length, 1, 'one row, so a second apply of the shared edit cannot duplicate it');
  const [row] = rows;
  assert.deepEqual(
    { kind: row.kind, owner: row.owner, source: row.source, default: row.default, onLiteral: row.onLiteral, stage: row.stage, codePath: row.codePath },
    {
      kind: 'switch',
      owner: 'application-plane',
      source: 'code',
      default: 'false',
      onLiteral: 'true',
      stage: 'dark',
      codePath: 'apps/Application Plane/convex-core/convex/spaces.ts',
    },
  );
  assert.match(row.purpose, /\bliteral true\b/, 'the purpose names the literal that turns the switch on');
  assert.ok(row.purpose.length <= 240, 'the purpose fits one registry line');
  for (const key of ['declarations', 'codeDefault', 'driftReason', 'onDefaultReason', 'onLiteralAnyCase', 'devOnly']) {
    assert.equal(row[key], undefined, `a code switch row carries no ${key}`);
  }
  assert.ok(
    readTextFile(path.join(CONVEX_CORE, 'convex', 'spaces.ts')).includes('process.env.SPACE_DELIVERY_LEASE_SWEEP === "true"'),
    'the codePath reads the switch as the literal true',
  );
});
