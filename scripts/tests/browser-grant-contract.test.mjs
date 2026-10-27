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
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';
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

// WS6.E (R-6 ledger): the budget claim is 40-44 on every message that carries it. 45-49 stay reserved.
const BUDGET_NUMBERS = [40, 41, 42, 43, 44];
const BUDGET_FIELDS = [
  { name: 'budget_micro_usd', type: 'int64' },
  { name: 'budget_max_steps', type: 'int32' },
  { name: 'budget_max_runtime_s', type: 'int32' },
  { name: 'budget_source', type: 'string' },
  { name: 'budget_minted_at_unix', type: 'int64' },
];
// WS8.C (R-6 ledger): 50-59 are the human-browse / attach / renew allocation. 54-59 stay reserved.
const WS8_RESERVED = [54, 55, 56, 57, 58, 59];
const BUDGET_MESSAGES = [
  'AcquireGrantRequest',
  'AcquireGrantResponse',
  'ValidateGrantResponse',
  'DeriveRunGrantRequest',
  'DeriveRunGrantResponse',
];

test('upstream proto: grant messages keep their field numbers and add kind/zdr/owner_id', () => {
  const messages = parseProto(read(UPSTREAM_PROTO));

  const request = messages.get('AcquireGrantRequest');
  assert.deepEqual(numbers(request), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, ...BUDGET_NUMBERS, 50]);
  assert.deepEqual(request.get(10), { name: 'zdr', type: 'bool', repeated: false });

  const issued = messages.get('AcquireGrantResponse');
  assert.deepEqual(numbers(issued), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, ...BUDGET_NUMBERS, 50, 51, 52]);
  assert.deepEqual(issued.get(10), { name: 'kind', type: 'string', repeated: false });
  assert.deepEqual(issued.get(11), { name: 'zdr', type: 'bool', repeated: false });

  const validated = messages.get('ValidateGrantResponse');
  assert.deepEqual(numbers(validated), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, ...BUDGET_NUMBERS, 50, 51, 52, 53]);
  // D1-001: the key the grant was issued under, so a consumer can bind a run grant to its run.
  assert.deepEqual(validated.get(13), { name: "session_key", type: "string", repeated: false });
  assert.deepEqual(validated.get(10), { name: 'kind', type: 'string', repeated: false });
  assert.deepEqual(validated.get(11), { name: 'owner_id', type: 'string', repeated: false });
  assert.deepEqual(validated.get(12), { name: 'zdr', type: 'bool', repeated: false });
});

test('upstream proto: the budget claim is fields 40-44 on every message that carries it', () => {
  const messages = parseProto(read(UPSTREAM_PROTO));
  for (const name of BUDGET_MESSAGES) {
    const fields = messages.get(name);
    assert.ok(fields, `${name} is missing`);
    BUDGET_NUMBERS.forEach((number, index) => {
      assert.deepEqual(
        fields.get(number),
        { ...BUDGET_FIELDS[index], repeated: false },
        `${name}.${BUDGET_FIELDS[index].name} must be field ${number}`,
      );
    });
    // 45-49 are reserved for follow-ons: nothing may take them silently.
    for (const number of [45, 46, 47, 48, 49]) {
      assert.equal(fields.has(number), false, `${name}: field ${number} is reserved by the ledger`);
    }
  }
});

test('upstream proto: DeriveRunGrant request and response pin their field numbers', () => {
  const proto = read(UPSTREAM_PROTO);
  const messages = parseProto(proto);

  const request = messages.get('DeriveRunGrantRequest');
  assert.ok(request, 'DeriveRunGrantRequest is missing');
  assert.deepEqual(numbers(request), [1, 2, 3, 4, 5, 6, ...BUDGET_NUMBERS, 50]);
  assert.deepEqual(request.get(1), { name: 'parent_grant_id', type: 'string', repeated: false });
  assert.deepEqual(request.get(2), { name: 'session_key', type: 'string', repeated: false });
  assert.deepEqual(request.get(3), { name: 'org_id', type: 'string', repeated: false });
  assert.deepEqual(request.get(4), { name: 'allowed_domains', type: 'string', repeated: true });
  assert.deepEqual(request.get(5), { name: 'purpose', type: 'string', repeated: false });
  assert.deepEqual(request.get(6), { name: 'ttl_seconds', type: 'int32', repeated: false });

  const response = messages.get('DeriveRunGrantResponse');
  assert.ok(response, 'DeriveRunGrantResponse is missing');
  assert.deepEqual(numbers(response), [1, 2, 3, 4, 5, 6, 7, ...BUDGET_NUMBERS, 50]);
  assert.deepEqual(response.get(1), { name: 'grant_id', type: 'string', repeated: false });
  assert.deepEqual(response.get(2), { name: 'endpoint', type: 'string', repeated: false });
  assert.deepEqual(response.get(3), { name: 'expires_at', type: 'google.protobuf.Timestamp', repeated: false });
  assert.deepEqual(response.get(4), { name: 'allowed_domains', type: 'string', repeated: true });
  assert.deepEqual(response.get(5), { name: 'parent_grant_id', type: 'string', repeated: false });
  assert.deepEqual(response.get(6), { name: 'kind', type: 'string', repeated: false });
  assert.deepEqual(response.get(7), { name: 'zdr', type: 'bool', repeated: false });

  // The rpc itself: a rename or a signature change breaks every caller.
  assert.match(
    proto.replace(/\/\/.*$/gm, ''),
    /rpc\s+DeriveRunGrant\s*\(\s*DeriveRunGrantRequest\s*\)\s*returns\s*\(\s*DeriveRunGrantResponse\s*\)/,
  );
});

test('the generated Go code carries the same grant fields as the proto, budget claim included', () => {
  // The broker's wire contract is the generated code, not the .proto text: a
  // proto edit without a regeneration leaves them apart. Each struct tag reads
  // protobuf:"bytes,<number>,<opt|rep>,name=<proto name>,...".
  const proto = parseProto(read(UPSTREAM_PROTO));
  const generated = read('apps/Model Plane/go/gen/model_plane/v1/browser.pb.go');
  const goFields = (name) => {
    const match = generated.match(new RegExp(`type ${name} struct \\{([\\s\\S]*?)\\n\\}`));
    assert.ok(match, `${name} is not in browser.pb.go: regenerate it`);
    return new Map(
      [...match[1].matchAll(/protobuf:"[a-z0-9]+,(\d+),(?:opt|rep),name=(\w+)/g)].map((m) => [Number(m[1]), m[2]]),
    );
  };
  for (const name of [...BUDGET_MESSAGES, 'RenewGrantRequest', 'RenewGrantResponse']) {
    const fromProto = new Map([...proto.get(name)].map(([number, field]) => [number, field.name]));
    assert.ok(fromProto.size > 0, `${name} parsed empty from the proto`);
    assert.deepEqual(goFields(name), fromProto, `${name}: browser.pb.go is stale against browser.proto`);
  }
  assert.match(
    read('apps/Model Plane/go/gen/model_plane/v1/browser_grpc.pb.go'),
    /BrowserBroker_DeriveRunGrant_FullMethodName\s*=\s*"\/model_plane\.v1\.BrowserBroker\/DeriveRunGrant"/,
  );
  assert.match(
    read('apps/Model Plane/go/gen/model_plane/v1/browser_grpc.pb.go'),
    /BrowserBroker_RenewGrant_FullMethodName\s*=\s*"\/model_plane\.v1\.BrowserBroker\/RenewGrant"/,
    'RenewGrant is not in the generated service: regenerate browser_grpc.pb.go',
  );
});

// WS8.C (R-6: 50-59). The numbers and names are the wire contract with Quarry (WS8.B), the model-gateway and the
// gateway; the 54-59 band is held for follow-ons exactly like 45-49 of the budget claim.
test('upstream proto: WS8.C pins fields 50-53, RenewGrant, and reserves 54-59', () => {
  const proto = read(UPSTREAM_PROTO);
  const messages = parseProto(proto);
  const string = (name) => ({ name, type: 'string', repeated: false });
  const timestamp = (name) => ({ name, type: 'google.protobuf.Timestamp', repeated: false });

  assert.deepEqual(messages.get('AcquireGrantRequest').get(50), string('grant_kind'));
  const issued = messages.get('AcquireGrantResponse');
  assert.deepEqual(issued.get(50), string('policy_mode'));
  assert.deepEqual(issued.get(51), { name: 'denied_domains', type: 'string', repeated: true });
  assert.deepEqual(issued.get(52), timestamp('absolute_expires_at'));
  const validated = messages.get('ValidateGrantResponse');
  assert.deepEqual(validated.get(50), string('policy_mode'));
  assert.deepEqual(validated.get(51), { name: 'denied_domains', type: 'string', repeated: true });
  assert.deepEqual(validated.get(52), timestamp('absolute_expires_at'));
  assert.deepEqual(validated.get(53), string('resource_id'));
  assert.deepEqual(messages.get('DeriveRunGrantRequest').get(50), string('resource_id'));
  assert.deepEqual(messages.get('DeriveRunGrantResponse').get(50), string('resource_id'));

  const renew = messages.get('RenewGrantRequest');
  assert.ok(renew, 'RenewGrantRequest is missing');
  assert.deepEqual(numbers(renew), [1, 2]);
  assert.deepEqual(renew.get(1), string('grant_id'));
  assert.deepEqual(renew.get(2), string('org_id'));
  const renewed = messages.get('RenewGrantResponse');
  assert.ok(renewed, 'RenewGrantResponse is missing');
  assert.deepEqual(numbers(renewed), [1, 2]);
  assert.deepEqual(renewed.get(1), timestamp('expires_at'));
  assert.deepEqual(renewed.get(2), timestamp('absolute_expires_at'));
  assert.match(
    proto.replace(/\/\/.*$/gm, ''),
    /rpc\s+RenewGrant\s*\(\s*RenewGrantRequest\s*\)\s*returns\s*\(\s*RenewGrantResponse\s*\)/,
  );

  for (const [name, fields] of messages) {
    for (const number of WS8_RESERVED) {
      assert.equal(fields.has(number), false, `${name}: field ${number} is reserved by the ledger`);
    }
  }
});

// Invariant I3: a browser grant belongs to a person and an organization; it never names a Space or a team.
test('no browser grant message carries a space_id or team_id field', () => {
  for (const [name, fields] of parseProto(read(UPSTREAM_PROTO))) {
    for (const [number, field] of fields) {
      assert.doesNotMatch(field.name, /^(space|team)_?id$/i, `${name}.${field.name} (= ${number}) breaks invariant I3`);
    }
  }
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

// WS8.B vendors 50-53 into Quarry by hand (there is no sync tool). The subset test above fails if it picks a
// different number or type; this one only checks that, once vendored, the names are the contract's.
test('when Quarry vendors the WS8.C fields of ValidateGrantResponse they are the upstream names', () => {
  const validated = parseProto(read(QUARRY_PROTO)).get('ValidateGrantResponse');
  const expected = new Map([
    [50, 'policy_mode'],
    [51, 'denied_domains'],
    [52, 'absolute_expires_at'],
    [53, 'resource_id'],
  ]);
  for (const [number, name] of expected) {
    const field = validated.get(number);
    if (field) assert.equal(field.name, name, `vendored field ${number} must be ${name}`);
  }
});

test('Quarry reads the budget claim from ValidateGrantResponse', () => {
  const validated = parseProto(read(QUARRY_PROTO)).get('ValidateGrantResponse');
  assert.deepEqual(
    BUDGET_NUMBERS.map((number) => validated.get(number)?.name),
    BUDGET_FIELDS.map((field) => field.name),
  );
});

function trackedFiles(pattern) {
  const out = execFileSync('git', ['ls-files', '-z', '--', pattern], { cwd: ROOT, encoding: 'utf8' });
  return out.split('\0').filter(Boolean);
}

/**
 * End of the Rust item that starts at `from` (after its attributes): the index
 * just past its closing `}` or its terminating `;`. Skips comments, string,
 * raw-string and char literals so braces inside them do not count.
 */
function rustItemEnd(text, from) {
  let depth = 0; // () and [] nesting: a `;` inside them does not end the item
  let braces = 0;
  let i = from;
  while (i < text.length) {
    const c = text[i];
    const next = text[i + 1];
    if (c === '/' && next === '/') {
      const eol = text.indexOf('\n', i);
      i = eol < 0 ? text.length : eol;
    } else if (c === '/' && next === '*') {
      let nest = 1;
      i += 2;
      while (i < text.length && nest > 0) {
        if (text[i] === '/' && text[i + 1] === '*') (nest += 1), (i += 2);
        else if (text[i] === '*' && text[i + 1] === '/') (nest -= 1), (i += 2);
        else i += 1;
      }
    } else if (c === 'r' && /^r#*"/.test(text.slice(i, i + 40)) && !/[\w]/.test(text[i - 1] ?? ' ')) {
      const hashes = /^r(#*)"/.exec(text.slice(i, i + 40))[1];
      const close = text.indexOf('"' + hashes, i + 2 + hashes.length);
      i = close < 0 ? text.length : close + 1 + hashes.length;
    } else if (c === '"') {
      i += 1;
      while (i < text.length && text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
      i += 1;
    } else if (c === "'") {
      if (next === '\\') {
        const close = text.indexOf("'", i + 3);
        i = close < 0 ? text.length : close + 1;
      } else if (text[i + 2] === "'") {
        i += 3; // a char literal such as '{'
      } else {
        i += 1; // a lifetime
      }
    } else {
      if (c === '(' || c === '[') depth += 1;
      else if (c === ')' || c === ']') depth -= 1;
      else if (c === '{') braces += 1;
      else if (c === '}') {
        braces -= 1;
        if (braces === 0 && depth === 0) return i + 1;
      } else if (c === ';' && braces === 0 && depth === 0) return i + 1;
      i += 1;
    }
  }
  return text.length;
}

/**
 * The file without its `#[cfg(test)]` items (a `mod tests` block, a test-only
 * `use`, a test helper fn), found by brace matching. Production code after a
 * test module, or after a test-only import near the top, stays in.
 */
function stripTestCode(text) {
  // `#[cfg(test)]` and `#[cfg(all(test, ...))]`, alone on a line or followed by its item on the same line.
  const attribute = /^[ \t]*#\[cfg\((?:test|all\(test,[^\]\n]*\))\)\][ \t]*/gm;
  let out = '';
  let last = 0;
  let match;
  while ((match = attribute.exec(text)) !== null) {
    if (match.index < last) continue;
    out += text.slice(last, match.index);
    const end = rustItemEnd(text, match.index + match[0].length);
    last = end;
    attribute.lastIndex = end;
  }
  return out + text.slice(last);
}

const isTestPath = (path) =>
  /(^|\/)tests?\//.test(path) || /_test\.(go|rs)$/.test(path) || /(^|\/)testdata\//.test(path);

test('single minting path: only the model-gateway grant route calls AcquireGrant', () => {
  // WS1.B deleted Quarry's legacy `GrpcGrantValidator::acquire`: Quarry only
  // validates grants, so the allow-list is the one model-gateway route.
  const allowedRust = new Set(['apps/Model Plane/rust/services/model-gateway/src/browser_grants.rs']);
  const rustHits = [];
  for (const path of trackedFiles('*.rs')) {
    if (isTestPath(path) || path.includes('/.claude/') || path.includes('/target/')) continue;
    if (/\.acquire_grant\s*\(/.test(stripTestCode(read(path)))) rustHits.push(path);
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

// ---- compose network membership (no YAML dependency: the files are regular enough) ----

/** Names of the services of one compose file text. */
function composeServiceNames(text) {
  const padded = '\n' + text;
  const start = padded.indexOf('\nservices:\n');
  if (start < 0) return []; // an overlay without services
  const rest = padded.slice(start + '\nservices:\n'.length);
  const end = rest.search(/\n[A-Za-z]/); // the next top-level key
  const body = end < 0 ? rest : rest.slice(0, end);
  return [...body.matchAll(/^ {2}([A-Za-z0-9_.-]+):[ \t]*(?:#.*)?$/gm)].map((m) => m[1]);
}

/**
 * The networks one service block joins: list form ("- name"), map form ("name:" or "name: {}"),
 * and flow form ("networks: [a, b]"). Comments and nested options (aliases) are ignored.
 */
function serviceNetworks(block) {
  const flow = block.match(/\n {4}networks:[ \t]*\[([^\]]*)\]/);
  if (flow) return flow[1].split(',').map((n) => n.trim()).filter(Boolean);
  const lines = block.split('\n');
  const at = lines.findIndex((line) => /^ {4}networks:[ \t]*(?:#.*)?$/.test(line));
  if (at < 0) return [];
  const names = [];
  for (const line of lines.slice(at + 1)) {
    if (/^\s*(?:#.*)?$/.test(line)) continue;
    const indent = line.match(/^ */)[0].length;
    if (indent <= 4) break;
    if (indent !== 6) continue; // options of a network entry, such as aliases
    const entry = line.match(/^ {6}(?:- ([A-Za-z0-9_.-]+)|([A-Za-z0-9_.-]+):)[ \t]*(?:\{\})?[ \t]*(?:#.*)?$/);
    if (entry) names.push(entry[1] ?? entry[2]);
  }
  return names;
}

/** Services (across the given compose texts) that are on the network, sorted. */
function networkMembers(texts, network) {
  const members = new Set();
  for (const text of texts) {
    for (const name of composeServiceNames(text)) {
      if (serviceNetworks(composeService(text, name)).includes(network)) members.add(name);
    }
  }
  return [...members].sort();
}

/** Every docker-compose*.yml of a plane directory (base file and overlays), as texts. */
const composeTexts = (dir) =>
  readdirSync(join(ROOT, dir))
    .filter((file) => /^docker-compose.*\.ya?ml$/.test(file))
    .map((file) => read(dir + '/' + file));

// The ephemeral-store flag as a YAML key of an environment block (indented, at line start).
const EPHEMERAL_FLAG = /^\s+BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT:/m;

test('compose network membership: the parser sees list, map and flow forms, and the flag guard fires', () => {
  const sample = [
    'name: x', 'services:',
    '  a:', '    image: i', '    networks:', '      - one', '      - two # c',
    '  b:', '    networks:', '      one: {}', '      two:', '        aliases:', '          - zed',
    '  c:', '    networks: [two, three]',
    '  d:', '    image: j',
    'volumes: {}', '',
  ].join('\n');
  assert.deepEqual(composeServiceNames(sample), ['a', 'b', 'c', 'd']);
  assert.deepEqual(serviceNetworks(composeService(sample, 'a')), ['one', 'two']);
  assert.deepEqual(serviceNetworks(composeService(sample, 'b')), ['one', 'two']);
  assert.deepEqual(serviceNetworks(composeService(sample, 'c')), ['two', 'three']);
  assert.deepEqual(serviceNetworks(composeService(sample, 'd')), []);
  assert.deepEqual(networkMembers([sample], 'one'), ['a', 'b']);
  assert.deepEqual(networkMembers([sample], 'three'), ['c']);
  // The ephemeral-flag guard must match an indented environment key (it once had "s+" for "\s+" and never fired).
  assert.match('    environment:\n      BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT: "true"\n', EPHEMERAL_FLAG);
  assert.doesNotMatch('      # BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT=true is dev only\n', EPHEMERAL_FLAG);
});

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

test('the broker uses a dedicated database and role on Quarry\'s Postgres, reached over a Postgres-only network', () => {
  const compose = read('apps/Model Plane/deploy/docker-compose.yml');
  const broker = composeService(compose, 'browser-broker');
  // Dedicated role + database on ingestion-postgres: never quarry_edge / quarry_v2, never the postgres superuser.
  assert.match(
    broker,
    /DATABASE_URL: postgresql:\/\/browser_broker:\$\{BROWSER_BROKER_PG_PASSWORD:\?[^}]+\}@ingestion-postgres:5432\/browser_broker\n/,
  );
  assert.doesNotMatch(broker, EPHEMERAL_FLAG, 'the base file must not enable the ephemeral store');
  // The broker joins the dedicated network, declared external in Model Plane and owned by the Ingestion compose.
  assert.match(broker, /\n\s+browser-broker-db: \{\}/);
  assert.match(compose, /\n {2}browser-broker-db:\n {4}external: true/);
  const ingestion = read('apps/Ingestion Plane/docker-compose.yml');
  const postgres = composeService(ingestion, 'postgres');
  assert.match(postgres, /container_name: ingestion-postgres/);
  assert.match(postgres, /networks:\n(?:\s+(?:#.*|- ingestion-net)\n)+\s+- browser-broker-db\n/);
  assert.match(ingestion, /\n {2}browser-broker-db:\n {4}name: browser-broker-db\n {4}driver: bridge/);
  // Membership, counted across the base file and every overlay of each plane, in list, map and flow form:
  // only ingestion-postgres on the Ingestion side, only the broker on the Model Plane side, and the broker
  // is never on ingestion-net (that would reach Quarry's own services).
  assert.deepEqual(
    networkMembers(composeTexts('apps/Ingestion Plane'), 'browser-broker-db'),
    ['postgres'],
    'only the postgres service of the Ingestion compose files may join browser-broker-db',
  );
  assert.deepEqual(
    networkMembers(composeTexts('apps/Model Plane/deploy'), 'browser-broker-db'),
    ['browser-broker'],
    'only browser-broker may join browser-broker-db in the Model Plane compose files',
  );
  assert.ok(!serviceNetworks(broker).includes('ingestion-net'), 'browser-broker must not join ingestion-net');
  assert.deepEqual(
    networkMembers(composeTexts('apps/Model Plane/deploy'), 'ingestion-net'),
    [],
    'no Model Plane service joins ingestion-net',
  );
  // The password is documented, and a signed release may carry it.
  assert.match(read('apps/Model Plane/deploy/.env.example'), /^BROWSER_BROKER_PG_PASSWORD=$/m);
  assert.match(read('apps/Model Plane/scripts/release-artifact.sh'), /\bBROWSER_BROKER_PG_PASSWORD\b/);
  // The root bootstrap provisions it (a compose variable that is required but never minted breaks
  // scripts/tests/bootstrap_runtime_environment.test.sh), in the Model Plane env file.
  assert.match(
    read('scripts/bootstrap_runtime_environment.sh'),
    /ensure_secret "\$MODEL_ENV" BROWSER_BROKER_PG_PASSWORD >\/dev\/null/,
  );
});

test('the broker defaults in compose match the Go defaults', () => {
  const config = read('apps/Model Plane/go/services/browser-broker/internal/server/config.go');
  assert.match(config, /DefaultGrantTTL = 15 \* time\.Minute/);
  assert.match(config, /DefaultMaxActiveGrantsPerOwner = 20/);
  assert.match(read('apps/Model Plane/go/services/browser-broker/internal/grant/policy.go'), /DefaultMaxSessionDomains = 8/);
  assert.match(config, /DefaultMaxRunDomains = grant\.DefaultMaxRunDomains/);
  assert.match(read('apps/Model Plane/go/services/browser-broker/internal/grant/grant.go'), /DefaultMaxRunDomains = 8/);
  const brokerCompose = read('apps/Model Plane/deploy/docker-compose.yml');
  assert.match(brokerCompose, /BROWSER_BROKER_MAX_RUN_DOMAINS: \$\{BROWSER_BROKER_MAX_RUN_DOMAINS:-8\}/);
  assert.match(brokerCompose, /BROWSER_BROKER_GRANT_TTL_SECONDS: \$\{BROWSER_BROKER_GRANT_TTL_SECONDS:-900\}/);
});

// WS8.C: human_browse is a development convenience and a production decision. The broker's own default is off, the
// dev compose turns it on, the production overlay turns it off again, and the absolute cap in compose is the Go one.
test('human_browse is off by default in the broker and in the production overlay, on in the dev compose', () => {
  const config = read('apps/Model Plane/go/services/browser-broker/internal/server/config.go');
  assert.match(config, /BROWSER_BROKER_HUMAN_BROWSE_ENABLED/);
  assert.match(config, /BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS/);
  assert.match(config, /BROWSER_BROKER_DENY_SUFFIXES/);
  assert.match(config, /BROWSER_BROKER_MAX_ATTACH_DOMAINS/);
  // No HumanBrowseEnabled in DefaultConfig: the zero value is off.
  const defaults = config.match(/func DefaultConfig\(\) Config \{([\s\S]*?)\n\}/);
  assert.ok(defaults, 'DefaultConfig not found');
  assert.doesNotMatch(defaults[1], /HumanBrowseEnabled/);
  assert.match(read('apps/Model Plane/go/services/browser-broker/internal/grant/humanbrowse.go'), /DefaultHumanAbsoluteTTL = 4 \* time\.Hour/);
  assert.match(read('apps/Model Plane/go/services/browser-broker/internal/grant/humanbrowse.go'), /DefaultMaxAttachDomains = 8/);

  const dev = composeService(read('apps/Model Plane/deploy/docker-compose.yml'), 'browser-broker');
  assert.match(dev, /BROWSER_BROKER_HUMAN_BROWSE_ENABLED: \$\{BROWSER_BROKER_HUMAN_BROWSE_ENABLED:-1\}/);
  assert.match(dev, /BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS: \$\{BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS:-14400\}/);
  assert.match(dev, /BROWSER_BROKER_DENY_SUFFIXES: \$\{BROWSER_BROKER_DENY_SUFFIXES:-\}/);
  assert.match(dev, /BROWSER_BROKER_MAX_ATTACH_DOMAINS: \$\{BROWSER_BROKER_MAX_ATTACH_DOMAINS:-8\}/);

  const production = composeService(read('apps/Model Plane/deploy/docker-compose.production.yml'), 'browser-broker');
  assert.match(production, /BROWSER_BROKER_HUMAN_BROWSE_ENABLED: \$\{BROWSER_BROKER_HUMAN_BROWSE_ENABLED:-0\}/);
});

// WS8.C migration 0004 (R-3 override: the design said 0002). It is re-applied on every broker start.
test('migration 0004 is idempotent, has a down migration, and adds the three policy columns', () => {
  const dir = 'apps/Model Plane/go/services/browser-broker/migrations';
  const names = readdirSync(join(ROOT, dir));
  assert.ok(names.includes('0004_browser_grants_policy.up.sql'), '0004 up is missing');
  assert.ok(names.includes('0004_browser_grants_policy.down.sql'), '0004 down is missing');
  const up = read(`${dir}/0004_browser_grants_policy.up.sql`).replace(/--.*$/gm, '');
  for (const column of ['policy_mode', 'denied_domains', 'resource_id']) {
    assert.match(up, new RegExp(`ADD COLUMN IF NOT EXISTS ${column}\\b`), `${column} must be added with IF NOT EXISTS`);
  }
  assert.doesNotMatch(up, /\bADD COLUMN (?!IF NOT EXISTS)/i, 'a non-idempotent ADD COLUMN');
  assert.doesNotMatch(up, /\bCREATE (UNIQUE )?INDEX (?!IF NOT EXISTS)/i, 'a non-idempotent CREATE INDEX');
  // Every ADD CONSTRAINT sits behind an existence check (the DO block), never bare.
  const bare = up.replace(/DO \$\$[\s\S]*?\$\$;/g, '');
  assert.doesNotMatch(bare, /ADD CONSTRAINT/i, 'a bare ADD CONSTRAINT would fail on the second start');
  assert.match(up, /browser_grants_attach_idempotency_idx/);
  const down = read(`${dir}/0004_browser_grants_policy.down.sql`).replace(/--.*$/gm, '');
  for (const column of ['policy_mode', 'denied_domains', 'resource_id']) {
    assert.match(down, new RegExp(`DROP COLUMN IF EXISTS ${column}\\b`));
  }
  // The z_ id CHECK of 0001 survives: 0004 neither drops nor replaces it.
  assert.doesNotMatch(up + down, /browser_grants_no_zdr_id_check/);
});

test('the gateway reads BROWSER_GRANT_ENFORCEMENT from compose with an empty default, so the code picks the mode per environment', () => {
  const compose = read('apps/Frontend Plane/verevonv3/docker-compose.yml');
  assert.match(compose, /^\s+BROWSER_GRANT_ENFORCEMENT: \$\{BROWSER_GRANT_ENFORCEMENT:-\}\s*$/m);
  // The base file must not hard-code a mode: the production overlay would inherit it.
  assert.doesNotMatch(compose, /BROWSER_GRANT_ENFORCEMENT: \$\{BROWSER_GRANT_ENFORCEMENT:-(off|permissive|strict)\}/);
});

// WS1.C: Zero Data Retention grants are never stored. The table has no zdr column and its
// id CHECK refuses the "z_" prefix, so a routing bug fails the insert instead of persisting.
test('the grants migration has no zdr column and keeps the z_ id CHECK', () => {
  const sql = read('apps/Model Plane/go/services/browser-broker/migrations/0001_browser_grants.up.sql')
    .replace(/--.*$/gm, '');
  const table = sql.match(/CREATE TABLE IF NOT EXISTS browser_grants\s*\(([\s\S]*?)\n\);/);
  assert.ok(table, 'browser_grants table definition not found');
  const columns = [...table[1].matchAll(/^\s{4}([a-z_]+)\s+(?:TEXT|TIMESTAMPTZ)/gm)].map((m) => m[1]);
  assert.ok(columns.length >= 10, `expected the grant columns, parsed ${columns.length}`);
  assert.ok(!columns.some((name) => /zdr/i.test(name)), `a zdr column exists: ${columns.join(', ')}`);
  assert.match(table[1], /CONSTRAINT browser_grants_no_zdr_id_check CHECK \(id NOT LIKE 'z\\_%'\)/);
  // No later migration may add one either.
  const dir = 'apps/Model Plane/go/services/browser-broker/migrations';
  for (const name of readdirSync(join(ROOT, dir)).filter((n) => n.endsWith('.up.sql'))) {
    const text = read(`${dir}/${name}`).replace(/--.*$/gm, '');
    assert.ok(!/ADD COLUMN[^;]*\bzdr\b/i.test(text), `${name} adds a zdr column`);
    assert.ok(!/DROP CONSTRAINT[^;]*browser_grants_no_zdr_id_check/i.test(text), `${name} drops the z_ id CHECK`);
  }
});

/** The file's production code: every `#[cfg(test)]` item removed by brace matching. */
const productionPart = stripTestCode;

test('the test-code stripper removes only cfg(test) items and keeps production code after them', () => {
  const planted = 'self.broker.derive_run_grant(request).await';
  const early = `#[cfg(test)]
use crate::fixtures::Thing;

fn production_a() {}

#[cfg(test)]
mod tests {
    fn helper() { let s = "}"; let c = '}'; /* } */ let r = r#"}"#; let l: &'static str = "x"; }
    #[test]
    fn t() { assert_eq!(1, 1); }
}

pub async fn production_after_the_test_module() {
    ${planted};
}
`;
  const stripped = stripTestCode(early);
  assert.match(stripped, /production_a/);
  assert.ok(stripped.includes(planted), 'production code after an early cfg(test) item and after the test module is kept');
  assert.doesNotMatch(stripped, /mod tests|fixtures::Thing|helper|assert_eq/);

  // A call inside the test module is not production code.
  const inTests = `fn p() {}\n#[cfg(test)]\nmod tests {\n    fn t() { ${planted}; }\n}\n`;
  assert.ok(!stripTestCode(inTests).includes(planted));

  // A test-only fn with a `;` inside its signature and a gated impl.
  const gatedFn = `#[cfg(test)]\nfn fixture(x: [u8; 3]) -> u8 { x[0] }\nfn q() { ${planted}; }\n`;
  const stripped2 = stripTestCode(gatedFn);
  assert.ok(stripped2.includes(planted) && !stripped2.includes('fixture'));
  assert.equal(stripTestCode('fn r() {}\n'), 'fn r() {}\n');

  // A same-line gated item and a cfg(all(test, ...)) item are stripped as well.
  const sameLine = `#[cfg(test)] mod helpers;\nfn a() { ${planted}; }\n#[cfg(all(test, feature = "x"))]\nmod gated { fn t() { ${planted}; } }\nfn b() {}\n`;
  const stripped3 = stripTestCode(sameLine);
  assert.ok(stripped3.includes('fn a()') && stripped3.includes('fn b()'));
  assert.ok(!stripped3.includes('helpers') && !stripped3.includes('gated'));
  assert.equal(stripped3.split(planted).length - 1, 1, 'only the production call is left');
});

test('the legacy Quarry acquire is gone and Quarry has no grant-minting client call', () => {
  const quarry = read('apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/grpc/browser_broker.rs');
  const production = productionPart(quarry);
  assert.doesNotMatch(production, /AcquireGrantRequest|AcquiredGrant|\.acquire_grant\s*\(|fn acquire\b/);
});

test('DeriveRunGrant has one caller: the model-gateway browser run route', () => {
  const hits = [];
  for (const path of trackedFiles('*.rs')) {
    if (isTestPath(path) || path.includes('/.claude/') || path.includes('/target/')) continue;
    if (/\.derive_run_grant\s*\(/.test(stripTestCode(read(path)))) hits.push(path);
  }
  assert.deepEqual(hits, ['apps/Model Plane/rust/services/model-gateway/src/browser_run.rs']);
  const goHits = [];
  for (const path of trackedFiles('*.go')) {
    if (isTestPath(path) || path.includes('/gen/') || path.includes('/.claude/')) continue;
    if (/\.DeriveRunGrant\s*\(/.test(read(path))) goHits.push(path);
  }
  assert.deepEqual(goHits, [], 'a Go service calls BrowserBroker.DeriveRunGrant');
});

test('the Verevon gateway never mints or invents a browser grant', () => {
  const gatewayRoot = 'apps/Frontend Plane/verevonv3/apps/gateway/src';
  for (const path of trackedFiles(`${gatewayRoot}/*.rs`)) {
    const text = productionPart(read(path));
    assert.doesNotMatch(text, /AcquireGrant|acquire_grant|DeriveRunGrant|derive_run_grant/, `${path} calls the broker directly`);
    assert.doesNotMatch(text, /"session:|format!\(\s*"session:/, `${path} invents a "session:<id>" grant string`);
  }
  // The browser routes send the delegated aud=browser-broker credential.
  const browser = read(`${gatewayRoot}/domains/browser.rs`);
  const production = productionPart(browser);
  assert.match(production, /browser_broker_token\(/, 'browser.rs no longer mints the browser-broker audience token');
  assert.match(production, /"x-browser-authorization"/, 'browser.rs no longer forwards x-browser-authorization');
});

test('the gateway and model-gateway spell the Idempotency-Key header the same way', () => {
  const gateway = productionPart(read('apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs'));
  const modelGateway = productionPart(
    read('apps/Model Plane/rust/services/model-gateway/src/browser_run.rs'),
  );
  // The gateway forwards the header under a literal; model-gateway reads it
  // through one constant. A rename on either side makes retries silently
  // create new runs, so both must name the same lowercase header.
  assert.match(gateway, /\("idempotency-key", key\)/, 'the gateway no longer forwards idempotency-key');
  assert.match(
    modelGateway,
    /const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";/,
    'model-gateway reads a different header than the gateway forwards',
  );
});

test('the test-code stripper keeps real production code in the files the reviewers found cut short', () => {
  const upstream = stripTestCode(read('apps/Frontend Plane/verevonv3/apps/gateway/src/upstream.rs'));
  assert.match(upstream, /fn proxy_artifact_bytes/);
  assert.doesNotMatch(upstream, /\nmod tests\b/);
  const browser = stripTestCode(read('apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs'));
  assert.ok(browser.length > 100_000, 'browser.rs is scanned past its first cfg(test) item');
});

// ---- WS1.A2: the credential Quarry validates grants with ----

test('quarry-edge holds browser:read on browser-broker and nothing else there, in the zdr retention lane', () => {
  const registry = JSON.parse(read('apps/Control Plane/config/plane-service-principals.json'));
  const edge = registry['quarry-edge'];
  assert.ok(edge.audiences.includes('browser-broker'), 'quarry-edge may mint for browser-broker');
  assert.deepEqual(edge.scopesByAudience['browser-broker'], ['browser:read']);
  assert.equal(edge.retentionByAudience['browser-broker'], 'zdr');
  assert.ok(edge.scopes.includes('browser:read'), 'the top-level scopes are the union of the per-audience ones');
  // Nobody else holds anything on the broker audience: Quarry is its only service caller.
  const holders = Object.entries(registry)
    .filter(([, entry]) => 'browser-broker' in (entry.scopesByAudience ?? {}))
    .map(([name]) => name);
  assert.deepEqual(holders, ['quarry-edge']);
});

test('the browser-grant-validate manifest row is granted, and the vendored copy is the same bytes as its pin', () => {
  const manifestPath = 'apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json';
  const vendoredPath = 'apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json';
  const manifest = JSON.parse(read(manifestPath));
  const row = manifest.rows.find((candidate) => candidate.name === 'browser-grant-validate');
  assert.ok(row, 'the manifest has the browser-grant-validate row');
  assert.equal(row.granted, true);
  assert.equal(row.audience, 'browser-broker');
  assert.equal(row.mint_path, '/api/browser-broker/internal-token');
  assert.deepEqual(row.scopes, ['browser:read']);
  assert.equal(row.pending, undefined, 'a granted row is not pending');
  // The auth-core copy is byte-identical (LF), and both pins say the same digest.
  assert.equal(read(vendoredPath), read(manifestPath));
  const digest = createHash('sha256').update(read(manifestPath)).digest('hex');
  for (const path of [manifestPath, vendoredPath]) {
    assert.match(read(path + '.sha256'), new RegExp('^' + digest), path + ' is pinned at the manifest\'s digest');
  }
});

test('every agent-route call site asks the broker for the run\'s organization, never the org-blind validate', () => {
  const dir = 'apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes';
  const files = readdirSync(join(ROOT, dir)).filter((name) => name.endsWith('.rs') && name !== 'tests.rs');
  let orgAware = 0;
  for (const name of files) {
    const text = read(dir + '/' + name);
    assert.doesNotMatch(
      text,
      /grant_validator\s*\.\s*validate\s*\(/,
      name + ' calls the org-blind grant_validator.validate',
    );
    orgAware += (text.match(/grant_validator\s*\.\s*validate_for_org\s*\(/g) ?? []).length;
  }
  assert.ok(orgAware >= 3, 'the start, the per-action check and the sensitive approval each call validate_for_org (found ' + orgAware + ')');
});

test('main builds the production grant validator with the service tokens, through the function the tests drive', () => {
  const edge = 'apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/';
  const main = productionPart(read(edge + 'src/main.rs'));
  // main hands the shared provider (the production credential) to build_grpc_validator ...
  assert.match(
    main,
    /grant_boot\s*::\s*build_grpc_validator\s*\(\s*[\w.&]+\s*,\s*cross_plane_tokens(?:\s*\.\s*clone\s*\(\s*\))?\s*,/,
    'main must pass cross_plane_tokens to build_grpc_validator',
  );
  assert.doesNotMatch(main, /GrpcGrantValidator::connect/, 'main must not build the validator by hand');
  // ... which attaches them, and the production test builds its validator the same way.
  const boot = read(edge + 'src/grant_boot.rs');
  assert.match(boot, /\(\s*Some\(\s*tokens\s*\)\s*,\s*_\s*\)\s*=>\s*validator\s*\.\s*with_service_tokens\(\s*tokens\s*\)/);
  assert.match(read(edge + 'tests/browser_grants_prod.rs'), /grant_boot::build_grpc_validator\(/);
});

test('cross_plane_tokens in main is bound once, from QUARRY_EDGE__QUARRY_SERVICE_API_KEY, and never rebound or shadowed', () => {
  const main = productionPart(read('apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs'));
  // Every way Rust can introduce the name: a let pattern (plain, mut, typed, tuple or struct destructuring,
  // let-else, if/while let), a match or closure pattern, a for pattern, a function parameter.
  const LET = /\blet\b[^=;]*?\bcross_plane_tokens\b/g;
  const PATTERN = /(?:\bSome|\bOk|\bErr)\s*\(\s*(?:mut\s+)?cross_plane_tokens\s*\)|\|[^|;]*\bcross_plane_tokens\b[^|;]*\||\bfor\b[^;{]*?\bcross_plane_tokens\b[^;{]*?\bin\b|\bfn\s+\w+[^)]*\bcross_plane_tokens\s*:/g;
  const bindings = [...(main.match(LET) ?? []), ...(main.match(PATTERN) ?? [])];
  assert.equal(bindings.length, 1, 'cross_plane_tokens must be bound exactly once (found ' + bindings.length + ': ' + JSON.stringify(bindings) + '): a second binding would shadow the production credential');
  assert.doesNotMatch(main, /\bcross_plane_tokens\s*=[^=]/, 'cross_plane_tokens must never be reassigned');
  // destructuring assignment: (cross_plane_tokens, _) = ...
  assert.doesNotMatch(main, /\(\s*(?:[\w.&]+\s*,\s*)*cross_plane_tokens\s*(?:,\s*[\w.&]+\s*)*\)\s*=(?![=>])/, 'cross_plane_tokens must never be reassigned by destructuring');
  const at = main.search(/\blet\s+(?:mut\s+)?cross_plane_tokens\b/);
  assert.ok(at >= 0, 'the single binding must be a plain let cross_plane_tokens');
  // The binding is the match over the configured service credential that builds the Auth Core provider.
  const binding = main.slice(at, main.indexOf(';', main.indexOf('None => None', at)) + 1);
  assert.match(binding, /match\s+cfg\s*\.\s*quarry_service_api_key\s*\.\s*as_deref\s*\(\s*\)/, 'the credential is read from cfg.quarry_service_api_key');
  assert.match(
    binding,
    /Some\(\s*credential\s*\)\s*=>\s*Some\(\s*Arc::new\(\s*quarry_runtime::service_tokens::AuthCoreServiceTokenProvider::new\(\s*&cfg\s*\.\s*auth_core_url\s*,\s*credential\s*,?\s*\)\?\s*,?\s*\)\s*,?\s*\)/,
    'a configured credential builds the Auth Core provider from cfg.auth_core_url',
  );
  assert.match(binding, /None\s*=>\s*None/, 'only an unset credential leaves the provider empty');
  // QUARRY_EDGE__QUARRY_SERVICE_API_KEY reaches cfg.quarry_service_api_key: the config crate reads the QUARRY_EDGE
  // prefix with a double-underscore separator, the field name is the lower-cased remainder, and no serde rename
  // moves it to another key. The compose files set exactly that variable.
  const config = read('apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs');
  assert.match(
    config,
    /config::Environment::with_prefix\(\s*"QUARRY_EDGE"\s*\)\s*\.separator\(\s*"__"\s*\)/,
    'EdgeConfig::from_env must read the QUARRY_EDGE prefix with the __ separator',
  );
  const field = config.match(/((?:[ \t]*(?:#\[[^\]]*\]|\/\/[^\n]*)\n)*)[ \t]*pub\s+quarry_service_api_key\s*:\s*Option<String>\s*,/);
  assert.ok(field, 'EdgeConfig must declare pub quarry_service_api_key: Option<String>');
  assert.doesNotMatch(field[1], /rename|alias/, 'quarry_service_api_key must not be renamed or aliased away from QUARRY_EDGE__QUARRY_SERVICE_API_KEY');
  for (const file of ['docker-compose.yml', 'docker-compose.production.yml']) {
    assert.match(
      read('apps/Ingestion Plane/' + file),
      /^\s+QUARRY_EDGE__QUARRY_SERVICE_API_KEY:\s*\$\{QUARRY_SERVICE_API_KEY:\?/m,
      file + ' must set QUARRY_EDGE__QUARRY_SERVICE_API_KEY from the required QUARRY_SERVICE_API_KEY',
    );
  }
});

test('the production overlay requires the broker address and the contract overlay pins it', () => {
  const production = read('apps/Ingestion Plane/docker-compose.production.yml');
  assert.match(
    production,
    /QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: \$\{QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL:\?/,
  );
  const contract = read('apps/Ingestion Plane/docker-compose.grant-contract.yml');
  assert.match(contract, /QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: http:\/\/browser-broker:9095/);
  // The hardening comparison itself is scripts/tests/grant-contract-overlay.test.mjs (it renders compose).
  assert.doesNotMatch(contract.replace(/^\s*#.*$/gm, ''), /read_only|cap_drop|cap_add|tmpfs|security_opt|privileged|ports:|!reset|!override/);
});
