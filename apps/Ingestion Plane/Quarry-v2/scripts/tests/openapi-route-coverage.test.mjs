// Every route the edge registers is in docs/openapi.yaml, in docs/openapi-internal.yaml (the
// /v1/internal/* routes; repo docs only, decision 3.13) or on the allowlist with a reason, and the
// allowlist holds nothing that is documented or gone.
//
//   node --test scripts/tests/openapi-route-coverage.test.mjs
//
// `ROUTE_BASELINE_UPDATE=1` rewrites the pinned counts and the stale entries of
// the allowlist after a deliberate change; it never adds an undocumented route
// (that needs a reason, written by hand).

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { documentedPaths, normalize, productionPart, registeredPaths, routesIn, routeSourceFiles } from '../openapi-routes.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const allowlistPath = path.join(root, 'scripts/openapi-route-allowlist.json');
const openapi = fs.readFileSync(path.join(root, 'docs/openapi.yaml'), 'utf8');
const openapiInternal = fs.readFileSync(path.join(root, 'docs/openapi-internal.yaml'), 'utf8');

// The paths both files document. The internal file is never served and never an SDK source.
function documentedEverywhere() {
  return [...documentedPaths(openapi), ...documentedPaths(openapiInternal)];
}

function readAllowlist() {
  return JSON.parse(fs.readFileSync(allowlistPath, 'utf8'));
}

test('route_scan_ignores_cfg_test_modules', () => {
  const source = `
    pub fn router() -> Router { Router::new().route("/v1/real", get(h)).route(
        "/v1/multi/:line", post(h)) }
    #[cfg(test)]
    mod tests {
        fn t() { Router::new().route("/only-in-tests", get(h)); }
    }
  `;
  assert.deepEqual(routesIn(source), ['/v1/real', '/v1/multi/:line']);
});

test('route_scan_follows_the_agent_routes_split', () => {
  const files = routeSourceFiles(root).map((file) => path.relative(root, file).replaceAll('\\', '/'));
  assert.ok(files.includes('crates/quarry-edge/src/routes.rs'));
  assert.ok(
    files.some((file) => file.startsWith('crates/quarry-edge/src/agent_routes/')) ||
      files.includes('crates/quarry-edge/src/agent_routes.rs'),
    `no agent routes file in ${files.join(', ')}`,
  );
  const { paths } = registeredPaths(root);
  assert.ok(paths.includes('/v1/agent/runs'), 'the agent runs route is found');
});

test('normalize_makes_the_two_spellings_equal', () => {
  assert.equal(normalize('/v1/agent/runs/:run_id/tabs/:tab_id'), normalize('/v1/agent/runs/{id}/tabs/{tab}'));
  assert.equal(normalize('/v1/:kind/jobs'), '/v1/{}/jobs');
});

test('test_only_routes_in_auth_rs_are_not_scanned', () => {
  const auth = fs.readFileSync(path.join(root, 'crates/quarry-edge/src/auth.rs'), 'utf8');
  assert.ok(routesIn(auth).length === 0 || !routesIn(auth).some((route) => route === '/v1/other'));
  assert.ok(!routeSourceFiles(root).some((file) => file.endsWith('auth.rs')));
});

test('a_routes_found_floor_guards_against_an_empty_extraction', () => {
  const { paths, registrations } = registeredPaths(root);
  assert.ok(registrations >= 80, `only ${registrations} route registrations found`);
  assert.ok(paths.length >= 60, `only ${paths.length} distinct paths found`);
});

test('every_registered_route_is_documented_or_allowlisted_with_a_reason', () => {
  const { paths } = registeredPaths(root);
  const documented = new Set(documentedEverywhere());
  const allowlist = readAllowlist();
  const allowed = new Map(allowlist.routes.map((entry) => [normalize(entry.path), entry]));

  for (const entry of allowlist.routes) {
    assert.ok(
      typeof entry.reason === 'string' && entry.reason.trim().length >= 10,
      `allowlist entry ${entry.path} needs a real reason`,
    );
  }
  const undocumented = paths.filter((route) => !documented.has(route) && !allowed.has(route));
  assert.deepEqual(
    undocumented,
    [],
    `routes registered but neither in docs/openapi.yaml nor docs/openapi-internal.yaml nor on scripts/openapi-route-allowlist.json:\n  ${undocumented.join('\n  ')}`,
  );
});

test('the_allowlist_holds_no_stale_entries', () => {
  const { paths } = registeredPaths(root);
  const registered = new Set(paths);
  const documented = new Set(documentedEverywhere());
  const stale = [];
  for (const entry of readAllowlist().routes) {
    const route = normalize(entry.path);
    if (!registered.has(route)) stale.push(`${entry.path} (no longer registered)`);
    else if (documented.has(route)) stale.push(`${entry.path} (now documented: remove it)`);
  }
  assert.deepEqual(stale, [], `stale allowlist entries:\n  ${stale.join('\n  ')}`);
});

test('every_documented_path_is_still_a_route', () => {
  const registered = new Set(registeredPaths(root).paths);
  const gone = documentedEverywhere().filter((route) => !registered.has(route));
  assert.deepEqual(gone, [], `documented in docs/openapi.yaml or docs/openapi-internal.yaml but not registered:\n  ${gone.join('\n  ')}`);
});

test('the_route_count_is_pinned', () => {
  const { registrations } = registeredPaths(root);
  const allowlist = readAllowlist();
  if (process.env.ROUTE_BASELINE_UPDATE === '1') {
    allowlist.route_registrations = registrations;
    fs.writeFileSync(allowlistPath, `${JSON.stringify(allowlist, null, 2)}\n`);
    return;
  }
  assert.equal(
    registrations,
    allowlist.route_registrations,
    'the number of .route(...) registrations changed: document or allowlist the new route, then ROUTE_BASELINE_UPDATE=1',
  );
});

test('productionPart_cuts_at_the_test_module_only', () => {
  assert.equal(productionPart('a\n#[cfg(test)]\nmod tests {}\n'), 'a');
  assert.equal(productionPart('a\n#[cfg(test)]\nfn helper() {}\n'), 'a\n#[cfg(test)]\nfn helper() {}\n');
});
