const assert = require('node:assert/strict');
const test = require('node:test');

const {
  AGENT_ACTIVITY_STALE_MS,
  STATUS_MAX_LIFE_MS,
  STATUS_STATES,
  STATUS_TEXT_MAX_CHARS,
  agentChipState,
  checkStatusInput,
  isMemberState,
  statusView,
} = require('../convex/spaceMemberStatusRules.ts');

const NOW = 1_800_000_000_000;

function reject(input) {
  const result = checkStatusInput(input, NOW);
  assert.equal(result.ok, false, JSON.stringify(result));
  return result.reason;
}

test('the four states are accepted and nothing else is', () => {
  assert.deepEqual([...STATUS_STATES], ['available', 'away', 'dnd', 'offline']);
  for (const state of STATUS_STATES) {
    assert.deepEqual(checkStatusInput({ state }, NOW), { ok: true, state, text: null, expiresAt: null });
    assert.equal(isMemberState(state), true);
  }
  for (const state of ['busy', 'DND', '', undefined, null, 3, 'available ']) {
    assert.equal(reject({ state }), 'invalid_state', String(state));
    assert.equal(isMemberState(state), false);
  }
});

test('text is NFC-normalised, whitespace-collapsed and trimmed', () => {
  // "e" + combining acute becomes one code point.
  const result = checkStatusInput({ state: 'away', text: '  café \t at \r\n lunch  ' }, NOW);
  assert.equal(result.ok, true);
  assert.equal(result.text, 'café at lunch');
});

test('empty or blank text means no text; absent and null text too', () => {
  for (const text of ['', '   ', '\t\n', undefined, null]) {
    const result = checkStatusInput({ state: 'available', text }, NOW);
    assert.equal(result.ok, true);
    assert.equal(result.text, null, JSON.stringify(text));
  }
});

test('text is capped at 80 code points, not UTF-16 units', () => {
  assert.equal(STATUS_TEXT_MAX_CHARS, 80);
  assert.equal(checkStatusInput({ state: 'away', text: 'x'.repeat(80) }, NOW).ok, true);
  assert.equal(reject({ state: 'away', text: 'x'.repeat(81) }), 'text_too_long');
  // 80 astral emoji are 160 UTF-16 units but 80 code points: allowed. 81: refused.
  assert.equal(checkStatusInput({ state: 'away', text: '\u{1F600}'.repeat(80) }, NOW).ok, true);
  assert.equal(reject({ state: 'away', text: '\u{1F600}'.repeat(81) }), 'text_too_long');
  // The cap applies after collapsing whitespace.
  assert.equal(checkStatusInput({ state: 'away', text: `${'x '.repeat(40)}`.trimEnd() + '     ' }, NOW).ok, true);
});

test('a non-string text is refused', () => {
  for (const text of [5, true, {}, []]) {
    assert.equal(reject({ state: 'away', text }), 'text_not_a_string');
  }
});

test('bidirectional overrides and isolates are refused', () => {
  for (const mark of ['‪', '‫', '‬', '‭', '‮', '⁦', '⁧', '⁨', '⁩']) {
    assert.equal(reject({ state: 'away', text: `ab${mark}cd` }), 'text_bidi_override', mark.codePointAt(0).toString(16));
  }
});

test('control characters are refused, but tab, LF and CR are collapsed to a space', () => {
  for (const ch of ['\u0000', '\u0007', '\u000b', '\u000c', '\u001b', '\u007f', '\u0085', '\u009f']) {
    assert.equal(reject({ state: 'away', text: `a${ch}b` }), 'text_control_character', ch.codePointAt(0).toString(16));
  }
  const ok = checkStatusInput({ state: 'away', text: 'a\tb\nc\rd' }, NOW);
  assert.equal(ok.ok, true);
  assert.equal(ok.text, 'a b c d');
});

test('an expiry must be a finite future number within seven days', () => {
  assert.equal(STATUS_MAX_LIFE_MS, 7 * 24 * 60 * 60 * 1000);
  assert.equal(reject({ state: 'dnd', expiresAt: NOW }), 'expiry_in_past');
  assert.equal(reject({ state: 'dnd', expiresAt: NOW - 1 }), 'expiry_in_past');
  assert.equal(reject({ state: 'dnd', expiresAt: NOW + STATUS_MAX_LIFE_MS + 1 }), 'expiry_too_far');
  for (const expiresAt of ['soon', NaN, Infinity, true, {}]) {
    assert.equal(reject({ state: 'dnd', expiresAt }), 'invalid_expiry', String(expiresAt));
  }
  const edge = checkStatusInput({ state: 'dnd', expiresAt: NOW + STATUS_MAX_LIFE_MS }, NOW);
  assert.equal(edge.ok, true);
  assert.equal(edge.expiresAt, NOW + STATUS_MAX_LIFE_MS);
  const soon = checkStatusInput({ state: 'dnd', expiresAt: NOW + 1000.9 }, NOW);
  assert.equal(soon.expiresAt, NOW + 1000, 'the expiry is floored');
  assert.equal(checkStatusInput({ state: 'dnd', expiresAt: null }, NOW).expiresAt, null);
});

test('statusView: a live row is returned with its text, an expired row reads as cleared', () => {
  const row = { state: 'away', text: 'lunch', expiresAt: NOW + 1000, updatedAt: 5 };
  assert.deepEqual(statusView(row, NOW, true), { state: 'away', text: 'lunch', expiresAt: NOW + 1000, updatedAt: 5 });
  assert.equal(statusView({ ...row, expiresAt: NOW }, NOW, true), null, 'expiresAt == now is expired');
  assert.equal(statusView({ ...row, expiresAt: NOW - 1 }, NOW, true), null);
  assert.deepEqual(statusView({ state: 'dnd', updatedAt: 1 }, NOW, true), {
    state: 'dnd',
    text: null,
    expiresAt: null,
    updatedAt: 1,
  });
});

test('statusView: includeText=false drops the text and keeps the state', () => {
  const view = statusView({ state: 'away', text: 'private note', updatedAt: 9 }, NOW, false);
  assert.deepEqual(view, { state: 'away', text: null, expiresAt: null, updatedAt: 9 });
});

test('statusView: no row, an unknown state or an empty text give null / null text', () => {
  assert.equal(statusView(null, NOW, true), null);
  assert.equal(statusView(undefined, NOW, true), null);
  assert.equal(statusView({ state: 'busy', updatedAt: 1 }, NOW, true), null);
  assert.equal(statusView({ state: 'away', text: '', updatedAt: 1 }, NOW, true).text, null);
  assert.equal(statusView({ state: 'away', text: null, updatedAt: 1 }, NOW, true).text, null);
});

test('agentChipState: working and waiting rows are shown while fresh', () => {
  assert.equal(agentChipState({ state: 'working', updatedAt: NOW - 1000 }, NOW), 'working');
  assert.equal(agentChipState({ state: 'waiting', updatedAt: NOW - 1000 }, NOW), 'waiting');
  assert.equal(agentChipState({ state: 'idle', updatedAt: NOW }, NOW), 'idle');
});

test('agentChipState: no row, an unknown state or a row older than 20 minutes is idle', () => {
  assert.equal(AGENT_ACTIVITY_STALE_MS, 20 * 60 * 1000);
  assert.equal(agentChipState(null, NOW), 'idle');
  assert.equal(agentChipState(undefined, NOW), 'idle');
  assert.equal(agentChipState({ state: 'thinking', updatedAt: NOW }, NOW), 'idle');
  assert.equal(agentChipState({ state: 'working', updatedAt: NOW - AGENT_ACTIVITY_STALE_MS }, NOW), 'working', 'exactly 20 min is still fresh');
  assert.equal(agentChipState({ state: 'working', updatedAt: NOW - AGENT_ACTIVITY_STALE_MS - 1 }, NOW), 'idle');
  assert.equal(agentChipState({ state: 'waiting', updatedAt: NOW - 3 * AGENT_ACTIVITY_STALE_MS }, NOW), 'idle');
});
