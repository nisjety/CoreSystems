// Loader tests for the model entitlement refusal golden (WS10-S1):
//
//   apps/Model Plane/api/contracts/model-entitlement-refusals.json
//
// Run:  node --test scripts/tests/model-entitlement-contract-schema.test.mjs
//
// It never skips: an absent golden is a failure. Each mutation test changes an in-memory clone
// and asserts the SPECIFIC finding, so a validator that rejects everything cannot pass them.

import test from 'node:test';
import assert from 'node:assert/strict';

import {
  CODES,
  REASONS,
  loadGoldens,
  validateAgainstMatrix,
  validateAll,
  validateBytes,
  validateRefusals,
} from './model-entitlement-contract-schema.mjs';

const { refusals, matrix } = loadGoldens();
const clone = (value) => structuredClone(value);

function assertFinding(list, pattern) {
  assert.ok(
    list.some((finding) => pattern.test(finding)),
    `expected a finding matching ${pattern}; got:\n  ${list.join('\n  ') || '(none)'}`,
  );
}

test('the committed golden is well formed and agrees with the matrix', () => {
  assert.deepEqual(validateAll({ refusals, matrix }), []);
});

test('the golden has exactly the contract codes and reasons', () => {
  assert.deepEqual(refusals.codes.map((entry) => entry.code), CODES);
  assert.deepEqual(refusals.reasons.map((entry) => entry.reason), REASONS);
});

test('an unknown top-level field is refused', () => {
  const doc = clone(refusals);
  doc.surprise = true;
  assertFinding(validateRefusals(doc), /unknown field 'surprise'/);
});

test('a code outside the contract is refused', () => {
  const doc = clone(refusals);
  doc.codes[0].code = 'model_forbidden';
  assertFinding(validateRefusals(doc), /not one of model_not_entitled/);
  assertFinding(validateRefusals(doc), /missing code 'model_not_entitled'/);
});

test('a code listed twice is refused', () => {
  const doc = clone(refusals);
  doc.codes.push(clone(doc.codes[0]));
  assertFinding(validateRefusals(doc), /listed twice/);
});

test('retryable must agree with the retry action', () => {
  const doc = clone(refusals);
  doc.codes[0].retryable = true;
  assertFinding(validateRefusals(doc), /retryable and action 'retry' must agree/);
});

test('the unavailable code cannot become a verdict on a model', () => {
  const doc = clone(refusals);
  const unavailable = doc.codes.find((entry) => entry.code === 'model_entitlement_unavailable');
  unavailable.takes_reason = true;
  assertFinding(validateRefusals(doc), /must be retryable, 503 and carry no reason/);
});

test('a refusal body that carries more than a model id and a reason is refused', () => {
  const doc = clone(refusals);
  doc.wire.details.push('prompt');
  assertFinding(validateRefusals(doc), /exactly \["model","reason"\]/);
  const body = clone(refusals);
  body.wire.json_body.error.push('content');
  assertFinding(validateRefusals(body), /carries no content/);
});

test('a sentence with a placeholder the wire does not fill is refused', () => {
  const doc = clone(refusals);
  doc.reasons[0].message_en += ' See {policy}.';
  assertFinding(validateRefusals(doc), /placeholder the wire does not fill/);
});

test('names_model must say whether the sentence holds {model}', () => {
  const doc = clone(refusals);
  doc.reasons[0].names_model = !doc.reasons[0].names_model;
  assertFinding(validateRefusals(doc), /names_model must say/);
});

test('a missing reason is refused', () => {
  const doc = clone(refusals);
  doc.reasons.pop();
  assertFinding(validateRefusals(doc), /missing reason/);
});

test('a matrix vocabulary that drifts from the refusal reasons is refused', () => {
  const m = clone(matrix);
  m.vocabulary.reasons.push('too_expensive');
  assertFinding(validateAgainstMatrix(refusals, m), /differs from the refusal reasons/);
});

test('a matrix refusal row with another status than the catalogue is refused', () => {
  const m = clone(matrix);
  const row = m.rows.find((r) => r.kind === 'refusal');
  row.expect.http_status = 400;
  assertFinding(validateAgainstMatrix(refusals, m), /http_status 400 differs/);
});

test('a matrix refusal row that lets a content key into the body is refused', () => {
  const m = clone(matrix);
  const row = m.rows.find((r) => r.kind === 'refusal');
  row.expect.body_keys.push('prompt');
  assertFinding(validateAgainstMatrix(refusals, m), /body_keys must be exactly/);
});

test('a refusing availability row that disagrees on retry is refused', () => {
  const m = clone(matrix);
  const row = m.rows.find((r) => r.kind === 'availability' && r.expect.decision === 'refuse');
  row.expect.retryable = false;
  assertFinding(validateAgainstMatrix(refusals, m), /retryable false differs/);
});

test('byte rules: CR, BOM and a missing or doubled final newline are refused', () => {
  assert.deepEqual(validateBytes('{}\n', 'x'), []);
  assertFinding(validateBytes('{}\r\n', 'x'), /CR line endings/);
  assertFinding(validateBytes('﻿{}\n', 'x'), /byte order mark/);
  assertFinding(validateBytes('{}', 'x'), /exactly one newline/);
  assertFinding(validateBytes('{}\n\n', 'x'), /exactly one newline/);
});
