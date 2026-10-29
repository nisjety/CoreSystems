import assert from 'node:assert/strict';
import test from 'node:test';
import { existsSync } from 'node:fs';

const implementation = new URL('./engagement-runtime.mjs', import.meta.url);
test('canonical runtime implementation exists', () => assert.ok(existsSync(implementation)));
test('full runtime uses canonical owners, never Model or production frontend', async () => {
  const { runtimePlan } = await import(implementation);
  const plan = runtimePlan('up');
  assert.deepEqual(plan.map((step) => step.owner), ['control', 'frontend', 'ingestion', 'engagement']);
  assert.ok(plan[0].args.includes('scripts/auth-marketing-local.sh'));
  assert.ok(plan[1].args.some((arg) => arg.endsWith('verevonv3/scripts/run-frontend-plane.mjs')));
  assert.ok(plan[2].args.some((arg) => arg.endsWith('Ingestion Plane/scripts/run-engagement-ingestion.sh')));
  assert.ok(plan[3].args.includes('scripts/engagement-compose.sh'));
  assert.equal(plan[3].env.STUDIO_ANBUD_ENABLED, '1');
  assert.equal(plan[3].env.STUDIO_PLANE_LINKS, '1');
  assert.equal(plan[3].env.STUDIO_WEB_VITALS_ENABLED, '1');
  assert.equal(plan[2].env.INTEGRATION_LOCAL_WEB_VITALS, '1');
  assert.equal(plan[2].env.INTEGRATION_LOCAL_MICROSOFT, '1');
  assert.equal(plan[2].env.INTEGRATION_LOCAL_TRENDS, '1');
  assert.equal(plan[2].env.INTEGRATION_LOCAL_CRUX, '1');
  assert.equal(plan[2].env.INTEGRATION_LOCAL_SITE_SOURCES, '1');
  assert.equal(plan[2].env.INTEGRATION_LOCAL_ALGOLIA_OAUTH, '1');
  for (const i of [0, 2, 3]) assert.equal(plan[i].env.SHARED_NATS_ENABLED, '1');
  for (const step of plan) {
    assert.ok(!JSON.stringify(step).includes('Model Plane'));
    assert.ok(!step.args.some((arg) => /prune|--remove-orphans|--volumes|^-v$/.test(arg)));
  }
});
test('all runtime commands keep the retired standalone demo stopped', async () => {
  const { runtimePlan } = await import(implementation);
  for (const command of ['up', 'config', 'status']) {
    const plan = runtimePlan(command);
    assert.ok(plan.every((step) => step.owner !== 'demo'));
    assert.ok(!JSON.stringify(plan).includes('docker-compose.studio-demo.yml'));
    assert.ok(!JSON.stringify(plan).includes('docker-compose.studio-pulse.studio.yml'));
    assert.ok(!JSON.stringify(plan).includes('5174'));
  }
});
test('configuration and status are non-mutating plans', async () => {
  const { runtimePlan } = await import(implementation);
  for (const command of ['config', 'status']) {
    for (const step of runtimePlan(command)) {
      assert.ok(!step.args.some((arg) => ['up', 'down', 'rm', 'build'].includes(arg)));
    }
  }
});
test('unsupported/destructive commands are rejected', async () => {
  const { runtimePlan } = await import(implementation);
  for (const command of ['down', 'prune', 'rm', '--help', '']) {
    assert.throws(() => runtimePlan(command), /up, config, status/);
  }
});
test('runner preserves order and stops after a failed owner', async () => {
  const { runRuntime } = await import(implementation);
  const calls = [];
  runRuntime('status', (command, args, options) => {
    calls.push({ command, args, options });
    return { status: 0 };
  });
  assert.equal(calls.length, 4);
  assert.equal(calls[3].options.env.STUDIO_ANBUD_ENABLED, '1');
  let count = 0;
  assert.throws(() => runRuntime('up', () => ({ status: ++count === 2 ? 1 : 0 })), /frontend up failed/);
  assert.equal(count, 2);
  assert.throws(() => runRuntime('config', () => ({ error: new Error('sensitive child details') })), /control config failed/);
});
