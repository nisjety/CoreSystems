// L1 contract checks for the browser grant, across the planes that share it.
//
// Pure file checks (no services, no network), run from the repo root:
//   node --test scripts/tests/browser-grant-contract.test.mjs
//
// What breaks without them: the proto is vendored into Quarry by hand, the
// grant is minted in exactly one place, and the broker's reachability is a
// compose detail. Each of those drifted silently before (see the plan,
// WS1.A1). The registry and manifest rows for the Quarry credential are
// asserted in WS1.A2.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (path) => readFileSync(join(ROOT, path), 'utf8').replace(/\r\n/g, '\n');

const UPSTREAM_PROTO = 'apps/Model Plane/proto/model_plane/v1/browser.proto';
const QUARRY_PROTO = 'apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/proto/browser.proto';

/** message name -> Map(field number -> { name, type, repeated }) */
function parseProto(text) {
  const bare = text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
  const messages = new Map();
  for (const message of bare.matchAll(/message\s+(\w+)\s*\{([^}]*)\}/g)) {
    const fields = new Map();
    for (const field of message[2].matchAll(/(repeated\s+)?([\w.]+)\s+(\w+)\s*=\s*(\d+)\s*;/g)) {
      const number = Number(field[4]);
      assert.ok(!fields.has(number), `${message[1]}: field number ${number} is used twice`);
      fields.set(number, { name: field[3], type: field[2], repeated: Boolean(field[1]) });
    }
    messages.set(message[1], fields);
  }
  return messages;
}

const numbers = (fields) => [...fields.keys()].sort((a, b) => a - b);

test('upstream proto: grant messages keep their field numbers and add kind/zdr/owner_id', () => {
  const messages = parseProto(read(UPSTREAM_PROTO));

  const request = messages.get('AcquireGrantRequest');
  assert.deepEqual(numbers(request), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  assert.deepEqual(request.get(10), { name: 'zdr', type: 'bool', repeated: false });

  const issued = messages.get('AcquireGrantResponse');
  assert.deepEqual(numbers(issued), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
  assert.deepEqual(issued.get(10), { name: 'kind', type: 'string', repeated: false });
  assert.deepEqual(issued.get(11), { name: 'zdr', type: 'bool', repeated: false });

  const validated = messages.get('ValidateGrantResponse');
  assert.deepEqual(numbers(validated), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
  assert.deepEqual(validated.get(10), { name: 'kind', type: 'string', repeated: false });
  assert.deepEqual(validated.get(11), { name: 'owner_id', type: 'string', repeated: false });
  assert.deepEqual(validated.get(12), { name: 'zdr', type: 'bool', repeated: false });
});

test('Quarry vendored proto is a subset of the upstream one (same name, number, type)', () => {
  const upstream = parseProto(read(UPSTREAM_PROTO));
  const vendored = parseProto(read(QUARRY_PROTO));
  assert.ok(vendored.size > 0, 'parsed no messages from the vendored proto');
  for (const [message, fields] of vendored) {
    const source = upstream.get(message);
    assert.ok(source, `${message} is vendored but does not exist upstream`);
    for (const [number, field] of fields) {
      assert.deepEqual(
        source.get(number),
        field,
        `${message}.${field.name} (= ${number}) differs from the upstream contract`,
      );
    }
  }
});

test('Quarry reads kind, owner_id and zdr from ValidateGrantResponse', () => {
  const validated = parseProto(read(QUARRY_PROTO)).get('ValidateGrantResponse');
  assert.deepEqual(
    [10, 11, 12].map((number) => validated.get(number)?.name),
    ['kind', 'owner_id', 'zdr'],
  );
});

function trackedFiles(pattern) {
  const out = execFileSync('git', ['ls-files', '-z', '--', pattern], { cwd: ROOT, encoding: 'utf8' });
  return out.split('\0').filter(Boolean);
}

const isTestPath = (path) =>
  /(^|\/)tests?\//.test(path) || /_test\.(go|rs)$/.test(path) || /(^|\/)testdata\//.test(path);

test('single minting path: only the model-gateway grant route calls AcquireGrant', () => {
  // Quarry's legacy `GrpcGrantValidator::acquire` has no caller; WS1.B deletes it.
  const allowedRust = new Set([
    'apps/Model Plane/rust/services/model-gateway/src/browser_grants.rs',
    'apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/grpc/browser_broker.rs',
  ]);
  const rustHits = [];
  for (const path of trackedFiles('*.rs')) {
    if (isTestPath(path) || path.includes('/.claude/') || path.includes('/target/')) continue;
    let text = read(path);
    const testsAt = text.indexOf('#[cfg(test)]');
    if (testsAt >= 0) text = text.slice(0, testsAt);
    if (/\.acquire_grant\s*\(/.test(text)) rustHits.push(path);
  }
  assert.deepEqual(
    rustHits.filter((path) => !allowedRust.has(path)),
    [],
    'a new caller mints browser grants outside the model-gateway grant route',
  );
  assert.ok(
    rustHits.includes('apps/Model Plane/rust/services/model-gateway/src/browser_grants.rs'),
    'the model-gateway grant route no longer calls acquire_grant: this guard is stale',
  );

  const goHits = [];
  for (const path of trackedFiles('*.go')) {
    if (isTestPath(path) || path.includes('/gen/') || path.includes('/.claude/')) continue;
    if (/\.AcquireGrant\s*\(/.test(read(path))) goHits.push(path);
  }
  assert.deepEqual(goHits, [], 'a Go service calls BrowserBroker.AcquireGrant');
});

test('no principal in the registry holds browser:write', () => {
  const registry = JSON.parse(read('apps/Control Plane/config/plane-service-principals.json'));
  for (const [principal, entry] of Object.entries(registry)) {
    assert.ok(!(entry.scopes ?? []).includes('browser:write'), `${principal} holds browser:write`);
    for (const [audience, scopes] of Object.entries(entry.scopesByAudience ?? {})) {
      assert.ok(!scopes.includes('browser:write'), `${principal} holds browser:write on ${audience}`);
    }
  }
});

function composeService(text, name) {
  const start = text.indexOf(`\n  ${name}:\n`);
  assert.ok(start >= 0, `service ${name} not found`);
  const rest = text.slice(start + 1);
  const next = rest.slice(1).search(/\n {2}[A-Za-z0-9_-]+:\n/);
  return next < 0 ? rest : rest.slice(0, next + 1);
}

test('the broker is reachable on inter-plane-bus under the names its clients use', () => {
  const compose = read('apps/Model Plane/deploy/docker-compose.yml');
  const broker = composeService(compose, 'browser-broker');
  assert.match(broker, /inter-plane-bus:\s*\n\s+aliases:\s*\n\s+- model-plane-browser-broker-1\s*\n\s+- browser-broker/);
  assert.match(broker, /\n\s+default: \{\}/, 'the broker keeps its own default network');
  assert.match(broker, /BROWSER_BROKER_GRANT_TTL_SECONDS: \$\{BROWSER_BROKER_GRANT_TTL_SECONDS:-900\}/);
  assert.match(broker, /BROWSER_BROKER_MAX_SESSION_DOMAINS: \$\{BROWSER_BROKER_MAX_SESSION_DOMAINS:-8\}/);
  assert.match(
    broker,
    /BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER: \$\{BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER:-20\}/,
  );
  // The model-gateway reaches the broker by the same alias.
  assert.match(compose, /BROWSER_BROKER_ADDR: http:\/\/model-plane-browser-broker-1:9095/);
});

test('the broker defaults in compose match the Go defaults', () => {
  const config = read('apps/Model Plane/go/services/browser-broker/internal/server/config.go');
  assert.match(config, /DefaultGrantTTL = 15 \* time\.Minute/);
  assert.match(config, /DefaultMaxActiveGrantsPerOwner = 20/);
  assert.match(read('apps/Model Plane/go/services/browser-broker/internal/grant/policy.go'), /DefaultMaxSessionDomains = 8/);
});
