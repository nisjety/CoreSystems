const assert = require('node:assert/strict');
const test = require('node:test');
const { getFunctionName } = require('convex/server');

const { confirmSpaceAgentMembershipForGateway } = require('../convex/spaceAgents.ts');
const fake = require('./helpers/fake-convex.cjs');

/**
 * WS17-S4 confirm leg (`confirmSpaceAgentMembershipForGateway`). After the room roster is applied at Control,
 * the Space's pending bindings become active only if Control skipped no unadmitted agent. When
 * `data.skipped_unadmitted` is non-empty the leg answers rejected and never runs markSpaceBindingsActive.
 * A recording ctx captures every mutation, and fetch is stubbed, so Control is never reached.
 */

const SPACE = 'space-a';
const MEMBERS_QUERY = 'spaceAgents:serviceMembersForSpace';
const MARK_ACTIVE = 'spaceAgents:markSpaceBindingsActive';

function recordingCtx() {
  const mutations = [];
  const ctx = {
    async runQuery(ref) {
      assert.equal(getFunctionName(ref), MEMBERS_QUERY);
      return [{ subjectId: 'agent-agent-1' }];
    },
    async runMutation(ref, args) {
      mutations.push({ name: getFunctionName(ref), args });
    },
  };
  return { ctx, mutations };
}

/** Stub the Control roster PUT with one reply body. Restored when the test ends. */
function stubControlReply(t, body) {
  const original = globalThis.fetch;
  globalThis.fetch = async () => ({ ok: true, status: 200, json: async () => body });
  t.after(() => {
    globalThis.fetch = original;
  });
}

function useLifecycleConfig(t) {
  const keys = ['CONTROL_SPACE_MEMBERSHIP_BASE_URL', 'APPLICATION_SPACE_LIFECYCLE_TOKEN'];
  const prior = keys.map((key) => process.env[key]);
  process.env.CONTROL_SPACE_MEMBERSHIP_BASE_URL = 'https://control.test/api/spaces';
  process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN = 'lifecycle-token-aaaaaaaaaaaaaaaaaaaa';
  t.after(() => {
    keys.forEach((key, index) => {
      if (prior[index] === undefined) delete process.env[key];
      else process.env[key] = prior[index];
    });
  });
}

function confirm(ctx) {
  return confirmSpaceAgentMembershipForGateway._handler(ctx, {
    serviceKey: fake.SERVICE_KEY,
    externalOrgId: fake.ORG,
    spaceRef: SPACE,
  });
}

test('a roster Control reports with a skipped unadmitted agent is rejected and never marks bindings active', async (t) => {
  fake.withServiceKey(t);
  useLifecycleConfig(t);
  stubControlReply(t, { data: { skipped_unadmitted: ['agent-agent-9'] } });
  const { ctx, mutations } = recordingCtx();
  assert.deepEqual(await confirm(ctx), { status: 'rejected', httpStatus: 200 });
  assert.deepEqual(mutations, [], 'markSpaceBindingsActive must not run for a roster that skipped an agent');
});

test('the same roster with nothing skipped is applied, and it marks the pending bindings active', async (t) => {
  fake.withServiceKey(t);
  useLifecycleConfig(t);
  stubControlReply(t, { data: { skipped_unadmitted: [] } });
  const { ctx, mutations } = recordingCtx();
  assert.deepEqual(await confirm(ctx), { status: 'applied' });
  assert.deepEqual(mutations, [{ name: MARK_ACTIVE, args: { spaceRef: SPACE, externalOrgId: fake.ORG } }]);
});
