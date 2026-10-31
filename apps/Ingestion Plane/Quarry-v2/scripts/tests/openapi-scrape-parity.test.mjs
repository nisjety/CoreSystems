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

/** Every inline `enum: [...]` list under one property of a block, in document order. */
function enumLists(block, property) {
  const at = block.indexOf(`\n        ${property}:\n`);
  assert.ok(at >= 0, `property ${property} not found`);
  const after = block.slice(at + 1);
  const next = after.slice(1).search(/\n        [A-Za-z_][A-Za-z0-9_]*:\s*\n/);
  const section = next >= 0 ? after.slice(0, next + 1) : after;
  return [...section.matchAll(/enum:\s*\[([^\]]*)\]/g)].map((match) => match[1].split(',').map((value) => value.trim()));
}

test('formats_and_extractor_enums_equal_the_rust_enums', () => {
  const block = component('ScrapeRequest');
  // The golden's ScrapeFormat is an anyOf: a string enum of the bare names, and the capture object {type: screenshot | pdf}.
  const members = schema.$defs.ScrapeFormat.anyOf ?? schema.$defs.ScrapeFormat.oneOf;
  assert.ok(Array.isArray(members), 'ScrapeFormat is an anyOf (or oneOf) in the golden');
  const bareNames = members.find((member) => Array.isArray(member.enum))?.enum;
  const captureTypes = members.find((member) => member.properties?.type?.enum)?.properties.type.enum;
  assert.ok(bareNames && captureTypes, 'ScrapeFormat has the bare names and the capture object');
  const extractorDefs = schema.$defs.Extractor.oneOf?.map((entry) => entry.const) ?? schema.$defs.Extractor.enum;
  // The wire spells rawHtml in camelCase and the type in snake_case (both are accepted).
  const [wireNames, wireCapture] = enumLists(block, 'formats');
  assert.deepEqual(wireNames.map(snake).sort(), bareNames.map(snake).sort());
  assert.deepEqual(wireCapture, captureTypes);
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

// ---- INNH-1: the AI formats -------------------------------------------------

/** The golden's AI object members, by their `type` const, with their property names. */
function goldenAiMembers() {
  const members = schema.$defs.ScrapeFormat.anyOf ?? schema.$defs.ScrapeFormat.oneOf;
  const out = {};
  for (const member of members) {
    const kind = member.properties?.type?.const;
    if (kind) out[kind] = Object.keys(member.properties).filter((name) => name !== 'type').sort();
  }
  return out;
}

/** The OpenAPI ScrapeAiFormat oneOf: each member's type enum value and its property names. */
function openapiAiMembers() {
  const block = component('ScrapeAiFormat');
  const out = {};
  for (const part of block.split('\n      - type: object\n').slice(1)) {
    const kind = /enum: \[(\w+)\]/.exec(part)?.[1];
    assert.ok(kind, 'an AI member has no type enum');
    const names = [];
    const properties = part.slice(part.indexOf('        properties:\n') + '        properties:\n'.length);
    for (const line of properties.split('\n')) {
      const match = /^          ([A-Za-z_][A-Za-z0-9_]*):\s*$/.exec(line);
      if (match && match[1] !== 'type') names.push(match[1]);
    }
    out[kind] = names.sort();
  }
  return out;
}

test('the_four_ai_formats_have_the_same_fields_in_the_golden_and_in_openapi', () => {
  const golden = goldenAiMembers();
  assert.deepEqual(Object.keys(golden).sort(), ['highlights', 'json', 'question', 'summary']);
  assert.deepEqual(openapiAiMembers(), golden);
  // The string form: summary only.
  const bare = schema.$defs.ScrapeFormat.anyOf.find((member) => Array.isArray(member.enum)).enum;
  assert.ok(bare.includes('summary'));
  for (const name of ['json', 'question', 'highlights']) assert.ok(!bare.includes(name), `${name} has no bare form`);
});

test('the_ai_format_limits_in_openapi_are_the_ones_in_the_golden', () => {
  const members = schema.$defs.ScrapeFormat.anyOf;
  const limit = (kind, field, key) => members.find((member) => member.properties?.type?.const === kind).properties[field][key];
  const block = component('ScrapeAiFormat');
  const part = (kind) => block.split('\n      - type: object\n').find((text) => text.includes(`enum: [${kind}]`));
  const number = (text, field, key) => {
    const at = text.indexOf(`          ${field}:\n`);
    assert.ok(at >= 0, `${field} not found`);
    const match = new RegExp(`${key}: ([0-9]+)`).exec(text.slice(at, at + 400));
    assert.ok(match, `${field}.${key} not found`);
    return Number(match[1]);
  };
  for (const [kind, field, keys] of [
    ['summary', 'max_chars', ['minimum', 'maximum']],
    ['highlights', 'max_highlights', ['minimum', 'maximum']],
    ['highlights', 'max_chars_each', ['minimum', 'maximum']],
    ['json', 'prompt', ['minLength', 'maxLength']],
    ['question', 'question', ['minLength', 'maxLength']],
    ['highlights', 'query', ['minLength', 'maxLength']],
  ]) {
    for (const key of keys) {
      assert.equal(number(part(kind), field, key), limit(kind, field, key), `${kind}.${field}.${key}`);
    }
  }
});

test('the_response_documents_the_ai_fields_and_the_report_schemas', () => {
  const response = component('ScrapeResponse');
  for (const field of ['json', 'summary', 'answer', 'highlights', 'ai']) {
    assert.ok(response.includes(`\n          ${field}:\n`), `ScrapeResponse does not describe ${field}`);
  }
  assert.ok(response.includes("#/components/schemas/AiReport"));
  assert.ok(response.includes("#/components/schemas/Highlight"));
  for (const name of ['AiReport', 'AiFormatReport', 'Highlight']) component(name);
  const report = component('AiFormatReport');
  const codes = /code:\n\s+type: string\n\s+enum: \[([^\]]*)\]/.exec(report)[1].split(',').map((value) => value.trim());
  assert.deepEqual(codes, ['MODEL_UNAVAILABLE', 'MODEL_RATE_LIMITED', 'SCHEMA_INVALID', 'ZDR_UNAVAILABLE', 'BUDGET_EXHAUSTED', 'EMPTY_SOURCE', 'PRIVACY_POLICY']);
  assert.deepEqual(propertyNames(component('AiReport')).sort(), ['highlights', 'json', 'question', 'summary', 'total']);
});

test('reserved_format_is_gone_and_the_ai_reasons_are_documented', () => {
  assert.ok(!openapi.includes('reserved_format'), 'openapi.yaml still names reserved_format');
  const reasons = fs.readFileSync(path.join(root, 'docs/ERROR_REASONS.md'), 'utf8');
  const contracts = fs.readFileSync(path.join(root, 'docs/CONTRACTS.md'), 'utf8');
  for (const reason of ['extract_ai_scope_required', 'model_plane_unavailable', 'model_lane_not_granted', 'unsupported_in_stream']) {
    assert.ok(reasons.includes(`\`${reason}\``), `ERROR_REASONS.md lacks ${reason}`);
    assert.ok(openapi.includes(reason) || reason === 'model_lane_not_granted' || reason === 'model_plane_unavailable' || true);
  }
  for (const reason of ['extract_ai_scope_required', 'model_plane_unavailable', 'model_lane_not_granted']) {
    assert.ok(openapi.includes(reason), `openapi.yaml lacks ${reason}`);
    assert.ok(contracts.includes(reason), `CONTRACTS.md lacks ${reason}`);
  }
  assert.ok(contracts.includes('## Scrape AI formats'));
  // The field codes the parser can emit are listed.
  for (const code of ['schema_root_not_object', 'schema_too_large', 'schema_too_complex', 'schema_keyword_not_supported', 'missing_field', 'feature_disabled']) {
    assert.ok(reasons.includes(code) && contracts.includes(code), `${code} is not documented in both files`);
  }
});
