const assert = require('node:assert/strict');
const test = require('node:test');

// The Convex copy of the legal-hold classification for a Model erase refusal: the exact code with
// `details.retryable: false`, and nothing weaker. The verevonv3 BFF copy (gateway spaces/memory.rs `hold_answer`,
// test `a_legal_hold_needs_the_code_and_its_retryable_false`) pins the same table, so the Convex side and the
// gateway classify a hold the same way (V3d-bff-2). Test fixtures only.
const { isModelLegalHoldRefusal } = require('../convex/spaceDeletionModel.ts');

const HOLD = 'erasure_blocked_legal_hold';
const OUTAGE = 'erasure_hold_check_unavailable';

test('a legal hold is the code with details.retryable false', () => {
  const held = { error: { code: HOLD, message: 'upstream-detail', details: { retryable: false } } };
  assert.equal(isModelLegalHoldRefusal(held), true);
});

test('the code alone, a flag that is not the boolean false, or the outage code is not a hold', () => {
  const notHolds = [
    { error: { code: HOLD, message: 'upstream-detail' } },
    { error: { code: HOLD, details: {} } },
    { error: { code: HOLD, details: { retryable: 'false' } } },
    { error: { code: HOLD, details: { retryable: true } } },
    { error: { code: HOLD, details: false } },
    { error: { code: OUTAGE, details: { retryable: true } } },
    { error: { code: OUTAGE, details: { retryable: false } } },
    { error: HOLD },
    { error: null },
    null,
    [HOLD],
    'erasure_blocked_legal_hold',
  ];
  for (const body of notHolds) {
    assert.equal(isModelLegalHoldRefusal(body), false, JSON.stringify(body));
  }
});
