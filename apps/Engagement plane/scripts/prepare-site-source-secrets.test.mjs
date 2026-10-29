import assert from 'node:assert/strict';
import { mkdtemp, mkdir, lstat, readFile, writeFile, chmod, symlink, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const implementation = new URL('./prepare-site-source-secrets.mjs', import.meta.url);
test('provisions private empty keys and inactive bindings without overwriting keys', async () => {
  const { prepareSiteSourceSecrets } = await import(implementation);
  const dir = await mkdtemp(join(tmpdir(), 'site-sources-'));
  try {
    const secrets = join(dir, 'secrets');
    await mkdir(secrets, { mode: 0o700 });
    await prepareSiteSourceSecrets(secrets);
    const cli = spawnSync(process.execPath, [fileURLToPath(implementation), secrets], { encoding: 'utf8' });
    assert.equal(cli.status, 0);
    assert.match(cli.stdout, /preserved/);
    for (const name of ['integration_cookiebot_data_api_key', 'integration_algolia_analytics_api_key']) {
      assert.equal((await lstat(join(secrets, name))).mode & 0o777, 0o600);
      assert.equal(await readFile(join(secrets, name), 'utf8'), '');
    }
    assert.deepEqual(JSON.parse(await readFile(join(secrets, 'integration_marketing_providers_bindings.json'), 'utf8')), { version: 1, installations: [] });
    await writeFile(join(secrets, 'integration_cookiebot_data_api_key'), 'synthetic-kept-secret');
    await prepareSiteSourceSecrets(secrets);
    assert.equal(await readFile(join(secrets, 'integration_cookiebot_data_api_key'), 'utf8'), 'synthetic-kept-secret');
  } finally { await rm(dir, { recursive: true, force: true }); }
});
test('rejects public directory, symlinks and existing public files without leaking contents', async () => {
  const { prepareSiteSourceSecrets } = await import(implementation);
  const dir = await mkdtemp(join(tmpdir(), 'site-sources-'));
  try {
    const secrets = join(dir, 'secrets');
    await mkdir(secrets, { mode: 0o755 });
    await assert.rejects(prepareSiteSourceSecrets(secrets), /private/);
    await chmod(secrets, 0o700);
    await symlink('/does-not-exist', join(secrets, 'integration_cookiebot_data_api_key'));
    await assert.rejects(prepareSiteSourceSecrets(secrets), /private/);
    await rm(join(secrets, 'integration_cookiebot_data_api_key'));
    await writeFile(join(secrets, 'integration_cookiebot_data_api_key'), 'do-not-leak', { mode: 0o644 });
    await assert.rejects(prepareSiteSourceSecrets(secrets), /private/);
    await assert.rejects(prepareSiteSourceSecrets('/'), /private/);
    const cli = spawnSync(process.execPath, [fileURLToPath(implementation), secrets], { encoding: 'utf8' });
    assert.equal(cli.status, 1);
    assert.ok(!`${cli.stdout}${cli.stderr}`.includes('do-not-leak'));
    assert.equal(spawnSync(process.execPath, [fileURLToPath(implementation), secrets, 'unexpected']).status, 1);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
