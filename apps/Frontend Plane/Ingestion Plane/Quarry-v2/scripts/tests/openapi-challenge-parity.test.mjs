// docs/openapi.yaml describes ChallengeSignal with the same fields, the same
// required list, the same defaults and the same enum values as the Rust types.
// The golden docs/schemas/challenge-signal.schema.json is generated from the
// types (crates/quarry-core/tests/challenge_schema.rs), so this catches a
// variant added to ChallengeKind, ChallengeStrength, BlockReason or
// ChallengeRecovery and not to the OpenAPI file, or the other way round.
//
//   node --test scripts/tests/openapi-challenge-parity.test.mjs

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const schema = JSON.parse(fs.readFileSync(path.join(root, 'docs/schemas/challenge-signal.schema.json'), 'utf8'));
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

/** The lines of one property of a component, from its name to the next property. */
function propertySection(block, property) {
  const at = block.indexOf(`\n        ${property}:\n`);
  assert.ok(at >= 0, `property ${property} not found`);
  const after = block.slice(at + 1);
  const next = after.slice(1).search(/\n        [A-Za-z_][A-Za-z0-9_]*:\s*\n/);
  return next >= 0 ? after.slice(0, next + 1) : after;
}

/** `enum: [a, b]` values of a property. */
function enumValues(block, property) {
  const match = /enum:\s*\[([^\]]*)\]/.exec(propertySection(block, property));
  assert.ok(match, `no inline enum under ${property}`);
  return match[1].split(',').map((value) => value.trim());
}

/** The `required:` list of a component. */
function requiredNames(block) {
  const at = block.indexOf('\n      required:\n');
  assert.ok(at >= 0, 'no required block');
  const names = [];
  for (const line of block.slice(at + '\n      required:\n'.length).split('\n')) {
    const match = /^      - (\S+)\s*$/.exec(line);
    if (!match) break;
    names.push(match[1]);
  }
  return names;
}

/**
 * The values of an enum def of the golden. schemars writes a variant with a doc comment as a
 * `oneOf` entry with a `const` and one without as an entry with a one-element `enum`, so a def
 * can mix both.
 */
function goldenValues(def) {
  const entry = schema.$defs[def];
  assert.ok(entry, `$defs.${def} is missing from the golden`);
  const values = entry.oneOf ? entry.oneOf.flatMap((variant) => ('const' in variant ? [variant.const] : variant.enum)) : entry.enum;
  assert.ok(Array.isArray(values) && values.length > 0, `${def} has no values`);
  return [...values].sort();
}

const signal = () => component('ChallengeSignal');

test('the_challenge_signal_has_the_same_fields_in_openapi_and_in_the_rust_type', () => {
  assert.deepEqual(propertyNames(signal()).sort(), Object.keys(schema.properties).sort());
});

test('the_required_fields_are_the_same', () => {
  assert.deepEqual(requiredNames(signal()).sort(), [...schema.required].sort());
});

for (const [property, def] of [
  ['kind', 'ChallengeKind'],
  ['strength', 'ChallengeStrength'],
  ['block_reason', 'BlockReason'],
  ['recovery', 'ChallengeRecovery'],
]) {
  test(`the_${property}_enum_equals_the_rust_enum_${def}`, () => {
    assert.deepEqual(enumValues(signal(), property).sort(), goldenValues(def));
  });
}

test('the_defaults_of_an_older_payload_are_the_same', () => {
  // A signal from before the taxonomy has no strength or recovery. The Rust
  // type reads that as strong with nothing to recover from; the OpenAPI file
  // must say the same, or a generated client reads it differently.
  for (const property of ['strength', 'recovery']) {
    const rust = schema.properties[property]?.default;
    assert.ok(rust, `the golden has no default for ${property}`);
    const documented = /\n          default:\s*(\S+)/.exec(propertySection(signal(), property));
    assert.ok(documented, `openapi.yaml gives no default for ${property}`);
    assert.equal(documented[1], rust, `${property} default`);
  }
});

test('block_reason_is_documented_as_optional_and_nullable', () => {
  assert.ok(!requiredNames(signal()).includes('block_reason'));
  assert.match(propertySection(signal(), 'block_reason'), /nullable:\s*true/);
});
