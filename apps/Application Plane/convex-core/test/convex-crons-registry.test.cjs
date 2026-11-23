const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const crons = require('../convex/crons.ts').default;

const convexDirectory = path.resolve(__dirname, '..', 'convex');

// Every scheduled job in crons.ts. Each one runs an internal function, which a
// browser can never call; the wrapper is the one the function must be exported
// with. The live cron run (docker restart convex-gateway, then the first tick)
// is a live check and is not covered here.
const JOBS = [
  { name: 'space org-room roster reconcile', schedule: { minutes: 10 }, module: 'spaceMembershipSync', fn: 'syncAllOrganizationRooms', wrapper: 'internalAction' },
  { name: 'space audience reconcile', schedule: { minutes: 10 }, module: 'spaceAudienceReconcile', fn: 'reconcileAllRoomAudiences', wrapper: 'internalAction' },
  { name: 'space sidebar orphan sweep', schedule: { hours: 6 }, module: 'spaceSidebar', fn: 'sweepOrphans', wrapper: 'internalMutation' },
  { name: 'space attention sweep', schedule: { hours: 1 }, module: 'spaceAttentionIngest', fn: 'sweep', wrapper: 'internalMutation' },
  { name: 'space member status sweep', schedule: { hours: 1 }, module: 'spaceMemberStatus', fn: 'sweepExpired', wrapper: 'internalMutation' },
  { name: 'space reminder heal', schedule: { minutes: 5 }, module: 'spaceReminders', fn: 'healReminders', wrapper: 'internalMutation' },
  { name: 'space room notice prune', schedule: { hours: 1 }, module: 'spaceRoomNotices', fn: 'pruneRoomNotices', wrapper: 'internalMutation' },
];

test('crons.ts registers exactly the scheduled jobs listed here', () => {
  assert.deepEqual(Object.keys(crons.crons).sort(), JOBS.map((job) => job.name).sort());
});

for (const job of JOBS) {
  test(`cron "${job.name}" runs ${job.module}.${job.fn} on its interval`, () => {
    const registered = crons.crons[job.name];
    assert.ok(registered, `cron "${job.name}" is not registered`);
    assert.equal(registered.name, `${job.module}:${job.fn}`);
    assert.deepEqual(registered.args, [{}]);
    assert.deepEqual(registered.schedule, { type: 'interval', ...job.schedule });
  });

  test(`cron "${job.name}" targets a function exported as ${job.wrapper}`, () => {
    const source = fs.readFileSync(path.join(convexDirectory, `${job.module}.ts`), 'utf8');
    assert.match(
      source,
      new RegExp(`export const ${job.fn} = ${job.wrapper}\\(`),
      `${job.module}.ts must export ${job.fn} as ${job.wrapper}`,
    );
  });
}
