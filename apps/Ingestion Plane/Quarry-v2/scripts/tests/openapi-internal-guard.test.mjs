// Guard for decision 3.13 (OPENAPI-INTERNAL, 2026-10-28): the /v1/internal/* routes live only in
// docs/openapi-internal.yaml. That file is repository documentation: it is never served, and no SDK
// is generated from it. The public spec, the SDK generator input and the tracked SDK files must
// carry no internal route or schema. The internal file must describe exactly the internal routes
// the edge registers.
//
//   node --test scripts/tests/openapi-internal-guard.test.mjs

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { parseYamlSubset } from '../yaml-subset.mjs';
import { documentedPaths, registeredPaths } from '../openapi-routes.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const read = (rel) => fs.readFileSync(path.join(root, rel), 'utf8').replace(/\r\n/g, '\n');
const publicText = read('docs/openapi.yaml');
const internalText = read('docs/openapi-internal.yaml');

// Schemas that only the internal run_page reaches. A public path must never reach them.
const INTERNAL_ONLY_SCHEMAS = [
  'CrawlContext',
  'CrawlScope',
  'CrawlReport',
  'CrawlDenials',
  'CrawlDenialSample',
  'CrawlRobotsDenial',
];

// The parser returns a node tree; this turns it into plain objects.
function toPlain(node) {
  if (node === null || node === undefined) return null;
  if (node.t === 's') return node.v;
  if (node.t === 'l') return node.items.map(toPlain);
  if (node.t === 'm') {
    const out = {};
    for (const key of node.keys) out[key] = node.values ? toPlain(node.values.get(key)) : null;
    return out;
  }
  return node;
}

const parseSpec = (text) => toPlain(parseYamlSubset(text));

const HTTP_METHODS = ['get', 'put', 'post', 'delete', 'options', 'head', 'patch', 'trace'];

// Every schema name a value refers to, through $ref, anywhere in the value.
function refNames(value, out = new Set()) {
  if (value && typeof value === 'object') {
    if (Array.isArray(value)) {
      for (const item of value) refNames(item, out);
    } else {
      for (const [key, inner] of Object.entries(value)) {
        if (key === '$ref' && typeof inner === 'string' && inner.startsWith('#/components/schemas/')) {
          out.add(inner.slice('#/components/schemas/'.length));
        } else {
          refNames(inner, out);
        }
      }
    }
  }
  return out;
}

// Every schema name reachable from the given values, through the components.
function reachableSchemas(seeds, schemas) {
  const seen = new Set();
  const queue = [...seeds];
  while (queue.length > 0) {
    const name = queue.shift();
    if (seen.has(name) || !schemas[name]) continue;
    seen.add(name);
    for (const next of refNames(schemas[name])) queue.push(next);
  }
  return seen;
}

// Walks a directory tree, returning the files under it whose path matches the test, skipping build output.
function filesUnder(rel, test) {
  const found = [];
  const start = path.join(root, rel);
  if (!fs.existsSync(start)) return found;
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === 'target' || entry.name === 'node_modules' || entry.name === '.git') continue;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (test(full)) found.push(full);
    }
  };
  if (fs.statSync(start).isDirectory()) walk(start);
  else if (test(start)) found.push(start);
  return found;
}

test('the public spec has no /v1/internal path and no operation tagged internal', () => {
  const spec = parseSpec(publicText);
  const internalPaths = Object.keys(spec.paths ?? {}).filter((p) => p.startsWith('/v1/internal'));
  assert.deepEqual(internalPaths, [], `internal paths in docs/openapi.yaml: ${internalPaths.join(', ')}`);
  const tagged = [];
  for (const [route, item] of Object.entries(spec.paths ?? {})) {
    for (const method of HTTP_METHODS) {
      const op = item?.[method];
      if (op && Array.isArray(op.tags) && op.tags.includes('internal')) tagged.push(`${method.toUpperCase()} ${route}`);
    }
  }
  assert.deepEqual(tagged, [], `operations tagged internal in docs/openapi.yaml: ${tagged.join(', ')}`);
  assert.ok(!(spec.tags ?? []).some((tag) => tag.name === 'internal'), 'the internal tag is not in docs/openapi.yaml');
});

test('the public spec defines no Internal schema and no public path reaches the internal-only crawl schemas', () => {
  const spec = parseSpec(publicText);
  const schemas = spec.components?.schemas ?? {};
  const internalNamed = Object.keys(schemas).filter((name) => name.startsWith('Internal'));
  assert.deepEqual(internalNamed, [], `Internal schemas in docs/openapi.yaml: ${internalNamed.join(', ')}`);
  const reached = reachableSchemas(refNames(spec.paths ?? {}), schemas);
  const reachedByPublic = INTERNAL_ONLY_SCHEMAS.filter((name) => reached.has(name));
  assert.deepEqual(reachedByPublic, [], `public paths reach internal-only schemas: ${reachedByPublic.join(', ')}`);
  const defined = INTERNAL_ONLY_SCHEMAS.filter((name) => schemas[name] !== undefined);
  assert.deepEqual(defined, [], `internal-only schemas defined in docs/openapi.yaml: ${defined.join(', ')}`);
});

test('docs/openapi-internal.yaml parses as one YAML document with no duplicate keys', () => {
  try {
    parseYamlSubset(internalText);
  } catch (error) {
    assert.fail(`docs/openapi-internal.yaml does not parse as one YAML document: ${error.message}`);
  }
});

test('the internal spec documents exactly the /v1/internal routes the edge registers', () => {
  const registered = registeredPaths(root).paths.filter((route) => route.startsWith('/v1/internal/'));
  const documented = documentedPaths(internalText).filter((route) => route.startsWith('/v1/internal/'));
  assert.ok(registered.length >= 9, `only ${registered.length} internal routes registered`);
  assert.deepEqual(
    [...documented].sort(),
    [...registered].sort(),
    'docs/openapi-internal.yaml and the registered /v1/internal routes differ',
  );
  const outside = documentedPaths(internalText).filter((route) => !route.startsWith('/v1/internal/'));
  assert.deepEqual(outside, [], `the internal spec documents a path outside /v1/internal: ${outside.join(', ')}`);
});

test('every internal operation has a unique operationId and the internal tag', () => {
  const spec = parseSpec(internalText);
  const ids = [];
  for (const [route, item] of Object.entries(spec.paths ?? {})) {
    for (const method of HTTP_METHODS) {
      const op = item?.[method];
      if (!op) continue;
      assert.ok(Array.isArray(op.tags) && op.tags.includes('internal'), `${method.toUpperCase()} ${route} is not tagged internal`);
      assert.ok(typeof op.operationId === 'string' && op.operationId.length > 0, `${method.toUpperCase()} ${route} has no operationId`);
      ids.push(op.operationId);
    }
  }
  assert.equal(new Set(ids).size, ids.length, `duplicate operationId in the internal spec: ${ids.join(', ')}`);
});

test('every $ref in the internal spec resolves to a schema, a parameter or a security scheme', () => {
  const spec = parseSpec(internalText);
  const components = spec.components ?? {};
  const schemas = new Set(Object.keys(components.schemas ?? {}));
  const parameters = new Set(Object.keys(components.parameters ?? {}));
  const unresolved = [];
  const walk = (value) => {
    if (value && typeof value === 'object') {
      for (const [key, inner] of Object.entries(value)) {
        if (key === '$ref' && typeof inner === 'string') {
          const known =
            (inner.startsWith('#/components/schemas/') && schemas.has(inner.slice('#/components/schemas/'.length))) ||
            (inner.startsWith('#/components/parameters/') && parameters.has(inner.slice('#/components/parameters/'.length)));
          if (!known) unresolved.push(inner);
        } else {
          walk(inner);
        }
      }
    }
  };
  walk(spec);
  assert.deepEqual(unresolved, [], `unresolved refs in docs/openapi-internal.yaml: ${unresolved.join(', ')}`);
  const schemesUsed = new Set();
  for (const item of Object.values(spec.paths ?? {})) {
    for (const method of HTTP_METHODS) {
      for (const requirement of item?.[method]?.security ?? []) {
        for (const name of Object.keys(requirement ?? {})) schemesUsed.add(name);
      }
    }
  }
  const declared = new Set(Object.keys(components.securitySchemes ?? {}));
  const undeclared = [...schemesUsed].filter((name) => !declared.has(name));
  assert.deepEqual(undeclared, [], `security schemes used but not declared: ${undeclared.join(', ')}`);
});

test('no code, Dockerfile or script loads docs/openapi-internal.yaml (it is repository documentation only)', () => {
  const hits = [];
  const scanned = [
    ...filesUnder('crates', (file) => file.endsWith('.rs')),
    ...filesUnder('services', (file) => file.endsWith('.go') || file.endsWith('.ts') || file.endsWith('.mjs')),
    ...filesUnder('deploy', () => true),
    ...filesUnder('sdks/generate.sh', () => true),
    ...filesUnder('Makefile', () => true),
  ];
  for (const file of scanned) {
    if (fs.readFileSync(file, 'utf8').includes('openapi-internal')) hits.push(path.relative(root, file).replaceAll('\\', '/'));
  }
  assert.deepEqual(hits, [], `these files refer to openapi-internal: ${hits.join(', ')}`);
});

test('the SDK generator reads only the public spec', () => {
  const generator = read('sdks/generate.sh');
  assert.ok(generator.includes('SPEC="/local/docs/openapi.yaml"'), 'generate.sh names docs/openapi.yaml as its spec');
  assert.ok(!generator.includes('openapi-internal'), 'generate.sh does not name the internal spec');
});

test('the SDK files that are tracked or generated carry no internal API', () => {
  const typeScriptDocs = filesUnder('sdks/typescript/docs', () => true).map((file) => path.basename(file));
  const internalDocs = typeScriptDocs.filter((name) => /^(InternalApi|InternalRunPage)/.test(name));
  assert.deepEqual(internalDocs, [], `internal SDK docs: ${internalDocs.join(', ')}`);

  // The generated trees are gitignored; a regeneration does not delete stale files, so check what is on disk.
  const generated = [
    ...filesUnder('sdks/typescript/src', (file) => /internal/i.test(path.basename(file))),
    ...filesUnder('sdks/python/quarry_client', (file) => /internal/i.test(path.basename(file))),
    ...filesUnder('sdks/python/docs', (file) => /^internal/i.test(path.basename(file))),
    ...filesUnder('sdks/python/test', (file) => /internal/i.test(path.basename(file))),
  ].map((file) => path.relative(root, file).replaceAll('\\', '/'));
  assert.deepEqual(generated, [], `generated internal SDK files still on disk: ${generated.join(', ')}`);

  const pythonTests = filesUnder('sdks/python/tests', (file) => file.endsWith('.py'));
  const pythonHits = pythonTests.filter((file) => /InternalApi|InternalRunPage|internal_api|internal_run_page/.test(fs.readFileSync(file, 'utf8')));
  assert.deepEqual(pythonHits.map((file) => path.relative(root, file)), [], 'the Python smoke tests do not exercise the internal API');
});
