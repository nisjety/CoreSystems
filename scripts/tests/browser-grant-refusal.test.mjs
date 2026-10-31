// The classifier behind legs B and C of tests/e2e/browser-grants-prod-contract.spec.ts:
// only the known pre-WS1.B refusal (the session start, 403, "browser grant_id is
// required") may read as the expected failure. Run with: node --test scripts/tests/browser-grant-refusal.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const helper = pathToFileURL(
  join(ROOT, 'apps', 'Frontend Plane', 'verevonv3', 'tests', 'e2e', 'browser-grants-prod-contract-refusal.ts'),
).href;
const { classifyKnownRefusal, knownRefusalFromEnv, NO_GRANT_MESSAGE, AUTHENTICATION_REFUSAL } = await import(helper);

const known = knownRefusalFromEnv({});
const seen = (over = {}) => ({ step: 'session', status: 403, message: NO_GRANT_MESSAGE, ...over });

test('the defaults pin one status and one message', () => {
  assert.deepEqual(known, { statuses: [403], messages: ['browser grant_id is required'] });
});

test('the missing-grant refusal at the session start is the known failure', () => {
  assert.deepEqual(classifyKnownRefusal(seen(), known), { known: true });
});

test('no recorded answer is not the known failure', () => {
  assert.equal(classifyKnownRefusal(undefined, known).known, false);
});

test('a 401 is never the known failure, even when the override lists it and the message matches', () => {
  const widened = knownRefusalFromEnv({ CONTRACT_BC_EXPECTED_STATUSES: '401,403' });
  const verdict = classifyKnownRefusal(seen({ status: 401 }), widened);
  assert.equal(verdict.known, false);
  assert.match(verdict.reason, /authentication/);
});

test('authentication messages are never the known failure, whatever the status', () => {
  for (const message of [
    'Authentication required',
    'unauthorized',
    'Please sign in',
    'invalid token',
    'session expired',
    'CSRF origin mismatch',
    'missing credentials',
  ]) {
    assert.match(message, AUTHENTICATION_REFUSAL, message);
    for (const status of [401, 403]) {
      assert.equal(classifyKnownRefusal(seen({ status, message }), knownRefusalFromEnv({ CONTRACT_BC_EXPECTED_MESSAGES: message })).known, false, status + ' ' + message);
    }
  }
});

test('a 403 with a different message is not the known failure', () => {
  const verdict = classifyKnownRefusal(seen({ message: 'browser grant is inactive or expired' }), known);
  assert.equal(verdict.known, false);
  assert.match(verdict.reason, /missing-grant/);
});

test('another status with the right message is not the known failure', () => {
  for (const status of [400, 404, 409, 500, 502, 503]) {
    assert.equal(classifyKnownRefusal(seen({ status }), known).known, false, String(status));
  }
});

test('a refusal after the session was made (the run step) is not the known failure', () => {
  assert.equal(classifyKnownRefusal(seen({ step: 'run' }), known).known, false);
});

test('the message must match word for word, not by substring', () => {
  assert.equal(classifyKnownRefusal(seen({ message: NO_GRANT_MESSAGE + ' (and more)' }), known).known, false);
  assert.equal(classifyKnownRefusal(seen({ message: '' }), known).known, false);
});

test('the env override widens the statuses and the messages, and only those', () => {
  const widened = knownRefusalFromEnv({
    CONTRACT_BC_EXPECTED_STATUSES: '403, 409',
    CONTRACT_BC_EXPECTED_MESSAGES: 'browser grant_id is required|browser grant is required for every action',
  });
  assert.deepEqual(widened.statuses, [403, 409]);
  assert.equal(classifyKnownRefusal(seen({ status: 409, message: 'browser grant is required for every action' }), widened).known, true);
  assert.equal(classifyKnownRefusal(seen({ status: 500 }), widened).known, false);
});
