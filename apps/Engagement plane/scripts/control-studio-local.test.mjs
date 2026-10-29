import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const studioRoot = fileURLToPath(new URL('../', import.meta.url));
const controlCompose = fileURLToPath(new URL('../../Control Plane/docker-compose.yml', import.meta.url));
const localOverride = fileURLToPath(new URL('../docker-compose.auth-marketing.local.yml', import.meta.url));
const composeFiles = [controlCompose, localOverride];
const requiredNames = composeFiles.flatMap((file) =>
  [...readFileSync(file, 'utf8').matchAll(/\$\{([A-Z_0-9]+):\?/g)].map((match) => match[1]));
// No provider files, private environment values or live Docker daemon are needed.
const syntheticEnvironment = Object.fromEntries(requiredNames.map((name) => [name, `synthetic-${name.toLowerCase()}`]));

function render({ override = true, extraPlanes = false } = {}) {
  const args = ['compose', '--env-file', '/dev/null', '-f', controlCompose];
  if (override) args.push('-f', localOverride);
  if (extraPlanes) args.push('--profile', 'control-extra-planes');
  args.push('config', '--no-env-resolution', '--format', 'json');
  const result = spawnSync('docker', args, {
    cwd: studioRoot,
    env: { PATH: process.env.PATH, ...syntheticEnvironment },
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, 'synthetic Compose configuration must render');
  return JSON.parse(result.stdout);
}

test('Studio local Control stack retains all six Control cores', () => {
  const { services } = render();
  for (const core of ['auth', 'user', 'org', 'billing', 'session', 'audit']) {
    assert.ok(services[`${core}-core`], `${core}-core must remain enabled`);
  }
  assert.ok(services['audit-nats-provisioner'], 'local Control stream provisioner must remain enabled');
});

test('Studio local Control does not retry disabled Application or Model planes', () => {
  const { services } = render();
  assert.equal(services['session-core'].environment.CONVEX_URL, '');
  assert.equal(services['session-core'].environment.CONVEX_SERVICE_KEY, '');
  assert.equal(services['audit-core'].environment.AUDIT_EXTRA_NATS_BUSES, '[]');
  assert.equal(services['audit-extra-nats-provisioner'], undefined);
});

test('local Audit authority and protected Auth registry remain unchanged', () => {
  const base = render({ override: false }).services;
  const local = render().services;
  for (const key of ['NATS_URL', 'NATS_USER', 'NATS_PASSWORD', 'AUDIT_CORE_SERVICE_CREDENTIALS']) {
    assert.equal(local['audit-core'].environment[key], base['audit-core'].environment[key]);
  }
  assert.deepEqual(local['audit-nats-provisioner'], base['audit-nats-provisioner']);
  assert.equal(local['auth-core'].environment.PLANE_SERVICE_PRINCIPALS_JSON, undefined);
  assert.equal(local['auth-core'].environment.PLANE_SERVICE_PRINCIPALS_FILE, '/run/secrets/marketing_auth_plane_principals');
});

test('foreign broker provisioner remains available only by explicit opt-in', () => {
  const provisioner = render({ extraPlanes: true }).services['audit-extra-nats-provisioner'];
  assert.deepEqual(provisioner.profiles, ['control-extra-planes']);
  assert.ok(provisioner.environment.AUDIT_NATS_PROVISION_BUSES.includes('model-nats'));
  assert.ok(provisioner.environment.AUDIT_NATS_PROVISION_BUSES.includes('application-nats'));
});
