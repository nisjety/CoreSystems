// docs/openapi.yaml describes every scrape option the Rust type has, with the
// same enum values. The golden docs/schemas/scrape-options.schema.json is
// generated from the type (crates/quarry-core/tests/scrape_schema.rs), so this
// catches an option added to the code and not to the OpenAPI file, or the other
// way round.
//
//   node --test scripts/tests/openapi-scrape-parity.test.mjs

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const schema = JSON.parse(fs.readFileSync(path.join(root, 'docs/schemas/scrape-options.schema.json'), 'utf8'));
const openapi = fs.readFileSync(path.join(root, 'docs/openapi.yaml'), 'utf8').replace(/\r\n/g, '\n');

/** The text of one component schema: from `    Name:` to the next component. */
function component(name) {
  const start = openapi.indexOf(`\n    ${name}:\n`);
  assert.ok(start >= 0, `components.schemas.${name} not found`);
  const rest = openapi.slice(start + 1);
  const end = rest.slice(1).search(/\n    [A-Za-z_][A-Za-z0-9_]*:\n/);
  return end >= 0 ? rest.slice(0, end + 1) : rest;
}

/** Property names at the property indent of a component's `properties:` block. */
function propertyNames(block) {
  const at = block.indexOf('\n      properties:\n');
  assert.ok(at >= 0, 'no properties block');
  const names = [];
  for (const line of block.slice(at + '\n      properties:\n'.length).split('\n')) {
    if (/^      \S/.test(line)) break; // the next key of the component
    const match = /^        ([A-Za-z_][A-Za-z0-9_]*):\s*$/.exec(line);
    if (match) names.push(match[1]);
  }
  return names;
}

/** `enum: [a, b]` values found after a property name inside a block. */
function enumValues(block, property) {
  const at = block.indexOf(`\n        ${property}:\n`);
  assert.ok(at >= 0, `property ${property} not found`);
  const after = block.slice(at + 1);
  const next = after.slice(1).search(/\n        [A-Za-z_][A-Za-z0-9_]*:\s*\n/);
  const section = next >= 0 ? after.slice(0, next + 1) : after;
  const match = /enum:\s*\[([^\]]*)\]/.exec(section);
  assert.ok(match, `no inline enum under ${property}`);
  return match[1].split(',').map((value) => value.trim());
}

const snake = (value) => value.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);

test('every_scrape_option_in_the_rust_type_is_a_property_of_ScrapeRequest', () => {
  const documented = propertyNames(component('ScrapeRequest'));
  for (const option of Object.keys(schema.properties)) {
    assert.ok(documented.includes(option), `openapi.yaml ScrapeRequest is missing the option ${option}`);
  }
});

test('the_request_keys_the_edge_reads_are_documented_too', () => {
  const documented = propertyNames(component('ScrapeRequest'));
  for (const key of ['url', 'prev_fingerprint', 'cache', 'zdr', 'privacy', 'signals', 'ingest', 'org_id', 'render', 'prefer_http3']) {
    assert.ok(documented.includes(key), `ScrapeRequest does not describe ${key}`);
  }
});

test('formats_and_extractor_enums_equal_the_rust_enums', () => {
  const block = component('ScrapeRequest');
  const formatsDefs = schema.$defs.ScrapeFormat.oneOf?.map((entry) => entry.const) ?? schema.$defs.ScrapeFormat.enum;
  const extractorDefs = schema.$defs.Extractor.oneOf?.map((entry) => entry.const) ?? schema.$defs.Extractor.enum;
  // The wire spells rawHtml in camelCase and the type in snake_case (both are accepted).
  assert.deepEqual(enumValues(block, 'formats').map(snake).sort(), [...formatsDefs].sort());
  assert.deepEqual(enumValues(block, 'extractor').map(snake).sort(), [...extractorDefs].sort());
});

test('the_response_documents_the_additive_scrape_fields', () => {
  const response = component('ScrapeResponse');
  for (const field of ['markdown', 'html', 'raw_html', 'truncated', 'omitted', 'effective_options', 'warnings']) {
    assert.ok(response.includes(`\n          ${field}:\n`), `ScrapeResponse does not describe ${field}`);
  }
});

test('the_scrape_by_id_route_is_documented', () => {
  assert.ok(openapi.includes('\n  /v1/scrape/{id}:\n'));
  assert.ok(openapi.includes('operationId: getScrape'));
});
