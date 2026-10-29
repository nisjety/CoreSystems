import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, mkdirSync, chmodSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('./shared-nats-leaf-entrypoint.sh', import.meta.url));
const preflight = fileURLToPath(new URL('./shared-nats-preflight.sh', import.meta.url));
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'studio-leaf-test-'));
  mkdirSync(join(dir, 'bin'));
  const base = join(dir, 'base.conf');
  const leaf = join(dir, 'leaf.conf');
  const secret = join(dir, 'secret');
  const output = join(dir, 'runtime.conf');
  writeFileSync(base, 'jetstream: true\nstore_dir: /data/jetstream\nauthorization { users: [] }\n');
  writeFileSync(leaf, 'jetstream { domain: STUDIO; store_dir: /data/jetstream }\nleafnodes { remotes: [{ url: $NATS_SHARED_LEAF_URL }] }\n');
  writeFileSync(secret, 'a'.repeat(64), { mode: 0o600 });
  writeFileSync(join(dir, 'bin', 'nats-server'), '#!/bin/sh\nprintf "%s\\n" "$@" > "$TEST_ARGS"\n', { mode: 0o700 });
  const env = { ...process.env, PATH: `${join(dir, 'bin')}:${process.env.PATH}`, NATS_LEAF_DOMAIN: 'STUDIO', NATS_LEAF_SECRET_FILE: secret, NATS_LEAF_BASE_CONFIG: base, NATS_LEAF_CONFIG: leaf, NATS_LEAF_RUNTIME_CONFIG: output, TEST_ARGS: join(dir, 'args') };
  return { dir, base, leaf, secret, output, env, run: () => spawnSync('sh', [script], { env, encoding: 'utf8' }), cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

test('preserves scoped ACLs, local storage and uses no secret in config/arguments', () => {
  const f = fixture();
  try {
    const result = f.run();
    assert.equal(result.status, 0, result.stderr);
    const config = readFileSync(f.output, 'utf8');
    assert.match(config, /authorization \{ users: \[\] \}/);
    assert.match(config, /domain: STUDIO/);
    assert.doesNotMatch(config, /jetstream: true/);
    assert.doesNotMatch(config + readFileSync(join(f.dir, 'args'), 'utf8') + result.stdout + result.stderr, /a{64}/);
    const check = spawnSync('sh', [preflight, f.secret], { encoding: 'utf8' });
    assert.equal(check.status, 0, check.stderr);
  } finally { f.cleanup(); }
});

test('accepts a protected 64-hex deployment secret with one trailing LF', () => {
  const f = fixture();
  try {
    writeFileSync(f.secret, 'a'.repeat(64) + '\n');
    const result = f.run();
    assert.equal(result.status, 0, result.stderr);
    const check = spawnSync('sh', [preflight, f.secret], { encoding: 'utf8' });
    assert.equal(check.status, 0, check.stderr);
  } finally { f.cleanup(); }
});

for (const scenario of ['missing', 'world-readable', 'symlink', 'invalid', 'newline', 'domain', 'base-drift']) {
  test(`fails closed for ${scenario}`, () => {
    const f = fixture();
    try {
      if (scenario === 'missing') rmSync(f.secret);
      if (scenario === 'world-readable') chmodSync(f.secret, 0o644);
      if (scenario === 'symlink') { const target = join(f.dir, 'target'); writeFileSync(target, 'a'.repeat(64), { mode: 0o600 }); rmSync(f.secret); symlinkSync(target, f.secret); }
      if (scenario === 'invalid') writeFileSync(f.secret, 'not-a-safe-password');
      if (scenario === 'newline') writeFileSync(f.secret, 'a'.repeat(63) + '\n');
      if (scenario === 'domain') f.env.NATS_LEAF_DOMAIN = 'CONTROL';
      if (scenario === 'base-drift') writeFileSync(f.base, 'jetstream { enabled: true }\n');
      const result = f.run();
      assert.notEqual(result.status, 0);
      assert.doesNotMatch(result.stdout + result.stderr, /a{64}/);
      if (!['domain', 'base-drift'].includes(scenario)) {
        const check = spawnSync('sh', [preflight, f.secret], { encoding: 'utf8' });
        assert.notEqual(check.status, 0);
        assert.doesNotMatch(check.stdout + check.stderr, /a{64}/);
      }
    } finally { f.cleanup(); }
  });
}
