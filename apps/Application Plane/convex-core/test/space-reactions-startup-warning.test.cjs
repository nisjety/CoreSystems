const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

/**
 * R-reg-10: startup.sh logs one line when SPACE_REACTIONS holds a former spelling, because Convex and the gateway read
 * only the exact literal true. The block is marked in startup.sh and run here under a POSIX shell, with the environment
 * the container gives it. A host with no POSIX shell on PATH skips the run tests.
 */

const STARTUP = path.join(__dirname, '..', 'startup.sh');
const BEGIN = '# BEGIN space-reactions-former-spelling';
const END = '# END space-reactions-former-spelling';

const FORMER_SPELLINGS = ['1', 'on', 'yes', 'enabled', 'TRUE', 'True', 'ON', 'Yes', ' true ', '\ttrue\t'];
const NO_WARNING = ['true', 'false', '0', '', 'truex', 'onward', 'disabled', undefined];

function startupText() {
  return fs.readFileSync(STARTUP, 'utf8');
}

function markedBlock() {
  const text = startupText();
  const from = text.indexOf(BEGIN);
  const to = text.indexOf(END);
  assert.ok(from >= 0 && to > from, 'startup.sh marks the SPACE_REACTIONS warning block');
  return text.slice(from, to + END.length);
}

/** Runs the block with SPACE_REACTIONS set to `value` (unset when undefined). Returns its output lines, or null with no POSIX shell. */
function runBlock(value) {
  const env = { ...process.env };
  if (value === undefined) delete env.SPACE_REACTIONS;
  else env.SPACE_REACTIONS = value;
  const result = spawnSync('sh', ['-c', markedBlock()], { env, encoding: 'utf8' });
  if (result.error && result.error.code === 'ENOENT') return null;
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.split('\n').filter((line) => line.length > 0);
}

test('a former spelling of SPACE_REACTIONS logs exactly one line that says it reads off now', async (t) => {
  for (const value of FORMER_SPELLINGS) {
    const lines = runBlock(value);
    if (lines === null) {
      t.skip('no POSIX shell on PATH');
      return;
    }
    assert.equal(lines.length, 1, `one line for ${JSON.stringify(value)}`);
    assert.match(lines[0], /SPACE_REACTIONS .*reads off now; only the exact literal true turns it on\./, JSON.stringify(value));
  }
});

test('the exact literal true, a false value, an empty or unset value and look-alike words log nothing', async (t) => {
  for (const value of NO_WARNING) {
    const lines = runBlock(value);
    if (lines === null) {
      t.skip('no POSIX shell on PATH');
      return;
    }
    assert.deepEqual(lines, [], JSON.stringify(value));
  }
});

test('startup.sh defaults SPACE_REACTIONS to the word false, and the warning follows its set_convex_env line', () => {
  const text = startupText();
  assert.match(text, /set_convex_env "SPACE_REACTIONS" "\$\{SPACE_REACTIONS:-false\}"/);
  assert.ok(text.indexOf(BEGIN) > text.indexOf('set_convex_env "SPACE_REACTIONS"'), 'the warning block comes after the set line');
});
