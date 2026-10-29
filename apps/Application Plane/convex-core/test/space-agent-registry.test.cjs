const assert = require('node:assert/strict');
const test = require('node:test');

const { projectAgentInstallationsForOrg } = require('../convex/spaceAgents.ts');

/**
 * `projectAgentInstallationsForOrg` is the pure join/filter logic behind
 * ADR-0002's `agentInstallationsForOrgForGateway` query
 * (apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md). It is unit-tested
 * directly here — rather than by invoking the `query`-wrapped handler
 * against a mocked `ctx.db` — because this package has no harness for
 * exercising Convex `query`/`mutation` handlers end to end (its existing
 * Convex-side tests, e.g. `space-lifecycle.test.cjs`, all extract the pure
 * decision out of the handler and test that directly; there is no
 * `convex-test`-style in-memory database anywhere in this repo). The
 * `handler` in `spaceAgents.ts` itself is a thin shell around this function:
 * auth checks, indexed reads, and batching the definition/Space lookups —
 * none of which a unit test can exercise without a real Convex deployment.
 */

const ORG = 'org-1';

function binding(overrides) {
  return {
    bindingRef: 'sab_default',
    spaceRef: 'space-a',
    externalOrgId: ORG,
    agentId: 'agent-1',
    subjectId: 'agent-agent-1',
    status: 'active',
    projectionVersion: 1,
    updatedAt: 1000,
    ...overrides,
  };
}

const DEFINITION = { orgId: 'convex-org-id', name: 'Support Bot', description: 'Handles tickets', status: 'active' };
const SPACE_A = { name: 'Team Room', kind: 'room', lifecycle: 'active' };
const SPACE_B = { name: 'Ops Room', kind: 'room', lifecycle: 'active' };

test('an org member sees bindings across multiple Spaces in one call', () => {
  const bindings = [
    binding({ bindingRef: 'sab_a', spaceRef: 'space-a', subjectId: 'agent-agent-1' }),
    binding({ bindingRef: 'sab_b', spaceRef: 'space-b', subjectId: 'agent-agent-1' }),
  ];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([
    ['space-a', SPACE_A],
    ['space-b', SPACE_B],
  ]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.equal(rows.length, 2, 'both Spaces’ bindings must be present in one call');
  const bySpace = Object.fromEntries(rows.map((row) => [row.spaceRef, row]));
  assert.equal(bySpace['space-a'].spaceName, 'Team Room');
  assert.equal(bySpace['space-a'].agentRef, 'agent-1');
  assert.equal(bySpace['space-b'].spaceName, 'Ops Room');
  assert.equal(bySpace['space-b'].agentRef, 'agent-1');
});

test('a revoked binding is excluded', () => {
  const bindings = [
    binding({ bindingRef: 'sab_active', spaceRef: 'space-a', status: 'active' }),
    binding({ bindingRef: 'sab_revoked', spaceRef: 'space-b', status: 'revoked' }),
  ];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([
    ['space-a', SPACE_A],
    ['space-b', SPACE_B],
  ]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.equal(rows.length, 1, 'the revoked binding must not appear');
  assert.equal(rows[0].bindingRef, 'sab_active');
});

test('a binding whose definition was deleted is dropped', () => {
  const bindings = [binding({ bindingRef: 'sab_ghost', agentId: 'agent-deleted' })];
  // The definition lookup returned nothing (deleted), OR returned a stale
  // shell with no orgId — both must be dropped rather than rendered under a
  // placeholder name, the same rule spaceAgentBindingsForGateway enforces.
  const definitionsById = new Map([['agent-deleted', undefined]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(rows, [], 'a binding with no live definition must not be projected');
});

test('a definition record with no orgId (a stale/foreign shell) is also dropped', () => {
  const bindings = [binding({ bindingRef: 'sab_shell', agentId: 'agent-shell' })];
  const definitionsById = new Map([['agent-shell', { name: 'Ghost', orgId: undefined }]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(rows, []);
});

test('a caller from a different org sees nothing', () => {
  // The Convex handler already scopes the `spaceAgentBindings` read to the
  // caller's own org via the `by_external_org` index before this function
  // ever runs — but the projection re-checks `externalOrgId` itself as a
  // second, defense-in-depth gate, the same belt-and-suspenders style
  // `spaceAgentBindingsForGateway` uses (`binding.externalOrgId === args.externalOrgId`
  // even though its own index lookup is already Space-scoped).
  const bindings = [binding({ bindingRef: 'sab_foreign', externalOrgId: 'org-2' })];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(rows, [], 'a binding belonging to a different org must never be visible');
});

test('every field spaceAgentBindingsForGateway projects is also projected here, plus the Space it lives in', () => {
  const bindings = [
    binding({
      bindingRef: 'sab_full',
      spaceRef: 'space-a',
      agentId: 'agent-1',
      subjectId: 'svc-1',
      displayName: 'Custom Name',
      title: 'Support',
      status: 'paused',
      deliveryTargets: [{ channel: 'teams', label: 'General', status: 'active' }],
      triggerModes: ['mention'],
      allowedTools: ['search'],
      approvalMode: 'auto',
      projectionVersion: 3,
      updatedAt: 4242,
    }),
  ];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map([['space-a', SPACE_A]]);

  const [row] = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.deepEqual(row, {
    bindingRef: 'sab_full',
    agentRef: 'agent-1',
    subjectId: 'svc-1',
    spaceRef: 'space-a',
    spaceName: 'Team Room',
    spaceKind: 'room',
    spaceLifecycle: 'active',
    name: 'Custom Name',
    title: 'Support',
    description: 'Handles tickets',
    avatarColor: undefined,
    status: 'paused',
    definitionStatus: 'active',
    deliveryTargets: [{ channel: 'teams', label: 'General', status: 'active' }],
    triggerModes: ['mention'],
    allowedTools: ['search'],
    approvalMode: 'auto',
    projectionVersion: 3,
    updatedAt: 4242,
  });
});

test('a binding for a Space that could not be resolved still projects, with Space fields absent', () => {
  // Defense-in-depth, not an expected steady state: `spacesByRef` is built
  // from every distinct `spaceRef` a binding names, so a miss here would mean
  // the Space row vanished between the two reads. Dropping the whole
  // installation would be worse than showing it unlabeled — the same
  // reasoning `compose_space_agents` applies when a Control-authorized
  // subject has no Application identity yet.
  const bindings = [binding({ bindingRef: 'sab_orphan_space', spaceRef: 'space-missing' })];
  const definitionsById = new Map([['agent-1', DEFINITION]]);
  const spacesByRef = new Map(); // space-missing was never inserted

  const rows = projectAgentInstallationsForOrg(bindings, definitionsById, spacesByRef, ORG);

  assert.equal(rows.length, 1);
  assert.equal(rows[0].spaceRef, 'space-missing');
  assert.equal(rows[0].spaceName, undefined);
  assert.equal(rows[0].spaceKind, undefined);
  assert.equal(rows[0].spaceLifecycle, undefined);
});
