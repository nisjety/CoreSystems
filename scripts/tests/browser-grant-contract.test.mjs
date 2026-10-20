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

test('upstream proto: DeriveRunGrant request and response pin their field numbers', () => {
  const proto = read(UPSTREAM_PROTO);
  const messages = parseProto(proto);

  const request = messages.get('DeriveRunGrantRequest');
  assert.ok(request, 'DeriveRunGrantRequest is missing');
  assert.deepEqual(numbers(request), [1, 2, 3, 4, 5, 6]);
  assert.deepEqual(request.get(1), { name: 'parent_grant_id', type: 'string', repeated: false });
  assert.deepEqual(request.get(2), { name: 'session_key', type: 'string', repeated: false });
  assert.deepEqual(request.get(3), { name: 'org_id', type: 'string', repeated: false });
  assert.deepEqual(request.get(4), { name: 'allowed_domains', type: 'string', repeated: true });
  assert.deepEqual(request.get(5), { name: 'purpose', type: 'string', repeated: false });
  assert.deepEqual(request.get(6), { name: 'ttl_seconds', type: 'int32', repeated: false });

  const response = messages.get('DeriveRunGrantResponse');
  assert.ok(response, 'DeriveRunGrantResponse is missing');
  assert.deepEqual(numbers(response), [1, 2, 3, 4, 5, 6, 7]);
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

test('the generated Go code carries the same DeriveRunGrant fields as the proto', () => {
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
  for (const name of ['DeriveRunGrantRequest', 'DeriveRunGrantResponse']) {
    const fromProto = new Map([...proto.get(name)].map(([number, field]) => [number, field.name]));
    assert.ok(fromProto.size > 0, `${name} parsed empty from the proto`);
    assert.deepEqual(goFields(name), fromProto, `${name}: browser.pb.go is stale against browser.proto`);
  }
  assert.match(
    read('apps/Model Plane/go/gen/model_plane/v1/browser_grpc.pb.go'),
    /BrowserBroker_DeriveRunGrant_FullMethodName\s*=\s*"\/model_plane\.v1\.BrowserBroker\/DeriveRunGrant"/,
  );
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
