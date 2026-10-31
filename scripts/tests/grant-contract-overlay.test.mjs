// The test-only contract overlay (apps/Ingestion Plane/docker-compose.grant-contract.yml)
// must leave every production hardening key of quarry-edge (and of every other
// service) exactly as the production overlay renders it. Plan 6.4 A: "the contract
// overlay only sets env and volumes and removes no production hardening key".
//
// Run from the repo root:
//   node --test scripts/tests/grant-contract-overlay.test.mjs
//
// It renders the Ingestion Plane base file, the base plus the production overlay,
// and base plus production plus the contract overlay with `docker compose config`
// (render only, no daemon, no container; dummy values for every required variable)
// and diffs the last two. The diff is a pure function (hardeningViolations) that the
// tests below also drive with planted mutations, so the check cannot pass vacuously.
// Without a `docker compose` the render tests are skipped, unless
// REQUIRE_COMPOSE_RENDER=1 (CI sets it), which makes a missing compose a failure.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const PLANE = join(ROOT, 'apps', 'Ingestion Plane');
const BASE = 'docker-compose.yml';
const PRODUCTION = 'docker-compose.production.yml';
const CONTRACT = 'docker-compose.grant-contract.yml';
const read = (file) => readFileSync(join(PLANE, file), 'utf8').replace(/\r\n/g, '\n');

/** Environment keys the contract overlay may change, and only these. */
export const OVERRIDABLE_ENV = new Set(['QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL']);

/**
 * Environment keys that are production hardening and must read exactly this in
 * the contract render of quarry-edge (the values are the production overlay's).
 */
export const HARDENING_ENV = {
  ENVIRONMENT: 'production',
  QUARRY_BROWSER_NO_SANDBOX: '0',
  QUARRY_EDGE_AUTH_DEV_BYPASS: '0',
  QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS: '0',
  QUARRY_EDGE__DURABLE_EVENT_HISTORY: '1',
  QUARRY_EDGE__ARTIFACT_ROOT: '/app/data/artifacts',
};

const sorted = (list) => [...list].sort();
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/** A volume or tmpfs entry as a stable string. */
const mountKey = (mount) =>
  typeof mount === 'string' ? mount : `${mount.type ?? ''}:${mount.source ?? ''}:${mount.target}`;

/**
 * What the contract render lost or changed relative to the production render.
 * `production` and `contract` are `docker compose config --format json` outputs.
 * Rules, per service of the production render:
 *   - the service exists in the contract render;
 *   - every key except `environment` and `volumes` is deep-equal (read_only, cap_drop,
 *     cap_add, security_opt, tmpfs, init, ports, privileged, user, networks, image,
 *     build, healthcheck, command, depends_on and the rest);
 *   - every production environment key keeps its value, except OVERRIDABLE_ENV;
 *   - every production volume is still mounted, same source and target (adding is allowed);
 *   - for quarry-edge, every HARDENING_ENV key reads its production value.
 * Top-level named volumes and networks may only be added to.
 */
export function hardeningViolations(production, contract) {
  const problems = [];
  for (const [name, service] of Object.entries(production.services ?? {})) {
    const other = contract.services?.[name];
    if (!other) {
      problems.push(`${name}: the service is gone`);
      continue;
    }
    const keys = new Set([...Object.keys(service), ...Object.keys(other)]);
    for (const key of keys) {
      if (key === 'environment' || key === 'volumes') continue;
      if (!same(service[key], other[key])) {
        problems.push(
          `${name}: ${key} changed from ${JSON.stringify(service[key])} to ${JSON.stringify(other[key])}`,
        );
      }
    }
    for (const [variable, value] of Object.entries(service.environment ?? {})) {
      if (OVERRIDABLE_ENV.has(variable)) continue;
      if (!(variable in (other.environment ?? {}))) {
        problems.push(`${name}: environment ${variable} was removed`);
      } else if (other.environment[variable] !== value) {
        problems.push(`${name}: environment ${variable} changed`);
      }
    }
    const kept = new Set((other.volumes ?? []).map(mountKey));
    for (const volume of service.volumes ?? []) {
      if (!kept.has(mountKey(volume))) problems.push(`${name}: volume ${mountKey(volume)} was removed`);
    }
    if (name === 'quarry-edge') {
      for (const [variable, value] of Object.entries(HARDENING_ENV)) {
        if (String(other.environment?.[variable]) !== value) {
          problems.push(`quarry-edge: hardening variable ${variable} is not ${JSON.stringify(value)}`);
        }
      }
    }
  }
  for (const section of ['volumes', 'networks']) {
    for (const name of Object.keys(production[section] ?? {})) {
      if (!(name in (contract[section] ?? {}))) problems.push(`top-level ${section} ${name} is gone`);
    }
  }
  return problems;
}

// ---- rendering --------------------------------------------------------------------

function composeAvailable() {
  try {
    execFileSync('docker', ['compose', 'version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

const COMPOSE = composeAvailable();
if (!COMPOSE && process.env.REQUIRE_COMPOSE_RENDER === '1') {
  throw new Error('REQUIRE_COMPOSE_RENDER=1 but `docker compose` is not available');
}

/** Dummy values for every `${NAME:?...}` the given files require (never real secrets). */
function dummyEnvFile(files, dir) {
  const names = new Set();
  for (const file of files) {
    for (const match of read(file).matchAll(/\$\{([A-Za-z0-9_]+):\?/g)) names.add(match[1]);
  }
  const path = join(dir, 'dummy.env');
  writeFileSync(path, [...names].sort().map((n) => `${n}=dummy-${n.toLowerCase()}-0123456789abcdef`).join('\n') + '\n');
  return path;
}

/** `docker compose config --format json` of the given files, as an object. */
function render(files, dir) {
  const args = ['compose', '-p', 'grant-contract-render', '--env-file', dummyEnvFile([BASE, PRODUCTION, CONTRACT], dir)];
  for (const file of files) args.push('-f', file);
  args.push('config', '--format', 'json', '--no-env-resolution');
  const out = execFileSync('docker', args, { cwd: PLANE, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return JSON.parse(out);
}

const withRenders = (fn) => {
  const dir = mkdtempSync(join(tmpdir(), 'grant-contract-'));
  try {
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

const renderTest = COMPOSE ? test : (name) => test(name, { skip: 'docker compose is not available' }, () => {});

// ---- the overlay file -------------------------------------------------------------

test('the contract overlay sets environment only on quarry-edge and uses no reset or override tag', () => {
  const text = read(CONTRACT);
  const code = text.replace(/^\s*#.*$/gm, '');
  assert.doesNotMatch(code, /!reset|!override/, 'a YAML tag can drop a production key');
  // Top-level keys: only `services`.
  assert.deepEqual([...code.matchAll(/^([A-Za-z_][\w-]*):/gm)].map((m) => m[1]), ['services']);
  // The services it touches, and the keys under each (indent 4): environment (and volumes) only.
  assert.deepEqual([...code.matchAll(/^ {2}([\w.-]+):\s*$/gm)].map((m) => m[1]), ['quarry-edge']);
  const keys = [...code.matchAll(/^ {4}([\w-]+):/gm)].map((m) => m[1]);
  assert.ok(keys.length > 0 && keys.every((k) => k === 'environment' || k === 'volumes'), `keys: ${keys}`);
  // The plan's line: the validator is the broker's gRPC address.
  assert.match(code, /QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: http:\/\/browser-broker:9095/);
  assert.match(code, /QUARRY_EDGE__REQUIRE_BROWSER_GRANTS: "1"/);
});

test('the production overlay requires the validator URL and the base file leaves it empty', () => {
  assert.match(
    read(PRODUCTION),
    /QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: \$\{QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL:\?[^}]+\}/,
  );
  assert.match(
    read(BASE),
    /QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: \$\{QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL:-\}/,
    'the base compose carries the key, empty outside production',
  );
});

// ---- the diff, against planted mutations ------------------------------------------

/** A production render in miniature: the keys the diff must notice losing. */
const sample = () => ({
  services: {
    'quarry-edge': {
      image: 'edge',
      read_only: true,
      cap_drop: ['ALL'],
      init: true,
      tmpfs: ['/tmp:rw,nosuid,size=1g', '/var/lib/quarry/chromium:rw,nosuid,size=512m,uid=65532,gid=65532,mode=0700'],
      security_opt: ['seccomp=./Quarry-v2/deploy/compose/seccomp-chromium.json'],
      environment: {
        ...HARDENING_ENV,
        QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: 'http://elsewhere:1',
      },
      volumes: [{ type: 'volume', source: 'quarry-edge-artifacts', target: '/app/data/artifacts' }],
    },
    'quarry-control': { read_only: true, cap_drop: ['ALL'], environment: { ENVIRONMENT: 'production' } },
  },
  volumes: { 'quarry-edge-artifacts': {} },
  networks: { 'inter-plane-bus': {} },
});

test('the diff accepts the overlay shape: an overridden validator URL and added env and volumes', () => {
  const contract = sample();
  contract.services['quarry-edge'].environment.QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL = 'http://browser-broker:9095';
  contract.services['quarry-edge'].environment.QUARRY_EDGE__REQUIRE_BROWSER_GRANTS = '1';
  contract.services['quarry-edge'].volumes.push({ type: 'volume', source: 'extra', target: '/extra' });
  assert.deepEqual(hardeningViolations(sample(), contract), []);
});

test('the diff fails when the contract render loses or weakens a hardening key', () => {
  const mutations = {
    'read_only removed': (c) => delete c.services['quarry-edge'].read_only,
    'read_only false': (c) => (c.services['quarry-edge'].read_only = false),
    'cap_drop removed': (c) => delete c.services['quarry-edge'].cap_drop,
    'cap_add appears': (c) => (c.services['quarry-edge'].cap_add = ['SYS_ADMIN']),
    'a tmpfs entry removed': (c) => c.services['quarry-edge'].tmpfs.pop(),
    'security_opt removed': (c) => delete c.services['quarry-edge'].security_opt,
    'init removed': (c) => delete c.services['quarry-edge'].init,
    'a port is published': (c) => (c.services['quarry-edge'].ports = [{ target: 8082, published: '8082' }]),
    'privileged': (c) => (c.services['quarry-edge'].privileged = true),
    'ENVIRONMENT is not production': (c) => (c.services['quarry-edge'].environment.ENVIRONMENT = 'dev'),
    'NO_SANDBOX turned on': (c) => (c.services['quarry-edge'].environment.QUARRY_BROWSER_NO_SANDBOX = '1'),
    'auth dev bypass turned on': (c) => (c.services['quarry-edge'].environment.QUARRY_EDGE_AUTH_DEV_BYPASS = '1'),
    'cross-plane dev bypass removed': (c) => delete c.services['quarry-edge'].environment.QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS,
    'durable history off': (c) => (c.services['quarry-edge'].environment.QUARRY_EDGE__DURABLE_EVENT_HISTORY = '0'),
    'the artifact volume removed': (c) => (c.services['quarry-edge'].volumes = []),
    'the artifact volume re-sourced': (c) => (c.services['quarry-edge'].volumes[0].source = 'host-path'),
    'another service loses read_only': (c) => delete c.services['quarry-control'].read_only,
    'a service is gone': (c) => delete c.services['quarry-control'],
    'a named volume is gone': (c) => delete c.volumes['quarry-edge-artifacts'],
  };
  for (const [what, mutate] of Object.entries(mutations)) {
    const contract = sample();
    mutate(contract);
    assert.notDeepEqual(hardeningViolations(sample(), contract), [], `not noticed: ${what}`);
  }
  assert.deepEqual(hardeningViolations(sample(), sample()), [], 'the control: no mutation, no violation');
});

// ---- the real renders --------------------------------------------------------------

renderTest('the production render has the hardening the diff protects (the check is not vacuous)', () => {
  withRenders((dir) => {
    const production = render([BASE, PRODUCTION], dir);
    const edge = production.services['quarry-edge'];
    assert.equal(edge.read_only, true);
    assert.deepEqual(edge.cap_drop, ['ALL']);
    assert.ok(edge.tmpfs.some((entry) => entry.startsWith('/var/lib/quarry/chromium:')), 'the Chromium scratch mount');
    assert.equal(edge.ports ?? undefined, undefined, 'production publishes no port');
    for (const [variable, value] of Object.entries(HARDENING_ENV)) {
      assert.equal(String(edge.environment[variable]), value, variable);
    }
    assert.ok(
      (edge.volumes ?? []).some((v) => v.target === '/app/data/artifacts' && v.type === 'volume'),
      'the artifact root is on a named volume',
    );
    assert.ok(edge.environment.QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL, 'the validator URL is required and set');
  });
});

renderTest('base, production and the contract overlay render; the overlay loses no production hardening key', () => {
  withRenders((dir) => {
    render([BASE], dir); // the base renders on its own
    const production = render([BASE, PRODUCTION], dir);
    const contract = render([BASE, PRODUCTION, CONTRACT], dir);
    assert.deepEqual(hardeningViolations(production, contract), []);
    const edge = contract.services['quarry-edge'];
    assert.equal(edge.environment.QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL, 'http://browser-broker:9095');
    assert.equal(String(edge.environment.QUARRY_EDGE__REQUIRE_BROWSER_GRANTS), '1');
    assert.deepEqual(sorted(Object.keys(production.services)), sorted(Object.keys(contract.services)));
  });
});

renderTest('the contract overlay without the production overlay would NOT pass: the diff sees production in it', () => {
  // The control for the order of the files: base plus contract alone is not the production
  // posture, and the diff must say so (this is what a wrongly ordered wrapper would run).
  withRenders((dir) => {
    const production = render([BASE, PRODUCTION], dir);
    const wrong = render([BASE, CONTRACT], dir);
    assert.notDeepEqual(hardeningViolations(production, wrong), []);
  });
});
