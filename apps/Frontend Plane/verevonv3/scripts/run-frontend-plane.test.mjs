import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, chmodSync, symlinkSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { readSelectedEnvironment, frontendEnvironment, readMarketingDeployment, readAnbudDeployment, run } from './run-frontend-plane.mjs'

function fixture(run) {
  const directory = mkdtempSync(join(tmpdir(), 'frontend-plane-test-'))
  const file = join(directory, 'credentials')
  writeFileSync(file, 'SELECTED=synthetic-secret\nIGNORED=not-projected\n', { mode: 0o600 })
  try { run({ directory, file }) } finally { rmSync(directory, { recursive: true }) }
}

test('reads only selected literal values without shell evaluation', () => fixture(({ file }) => {
  assert.deepEqual(readSelectedEnvironment(file, ['SELECTED']), { SELECTED: 'synthetic-secret' })
}))
test('marketing scope and key come only from protected Studio deployment inputs', () => fixture(({ directory }) => {
  const root = join(directory, 'Engagement plane'); const secrets = join(root, '.local', 'secrets');
  mkdirSync(secrets, { recursive: true, mode: 0o700 });
  assert.deepEqual(readMarketingDeployment(root), {});
  writeFileSync(join(root, '.local', 'marketing-integration.json'), JSON.stringify({version:1, organizationId:'org-one',googleConnectionId:'connection-one'}), {mode:0o600});
  assert.throws(() => readMarketingDeployment(root), /protected/);
  writeFileSync(join(secrets, 'marketing_internal_api_key'), 'synthetic-marketing-only-key\n', {mode:0o600});
  assert.deepEqual(readMarketingDeployment(root), {MARKETING_CORE_SERVICE_KEY:'synthetic-marketing-only-key', MARKETING_CORE_ORGANIZATION_ID:'org-one'});
  writeFileSync(join(root, '.local', 'marketing-integration.json'), JSON.stringify({version:1,organizationId:'org-two/../org-one'}));
  assert.throws(() => readMarketingDeployment(root), /scope/);
}))
test('anbud core key comes only from the protected Studio secret', () => fixture(({ directory }) => {
  const root = join(directory, 'Engagement plane'); const secrets = join(root, '.local', 'secrets');
  mkdirSync(secrets, { recursive: true, mode: 0o700 });
  assert.deepEqual(readAnbudDeployment(root), {});
  writeFileSync(join(secrets, 'anbud_internal_api_key'), 'synthetic-anbud-key-0000000000\n', { mode: 0o600 });
  assert.deepEqual(readAnbudDeployment(root), { ANBUD_API_KEY: 'synthetic-anbud-key-0000000000' });
  writeFileSync(join(secrets, 'anbud_internal_api_key'), 'short\n', { mode: 0o600 });
  assert.throws(() => readAnbudDeployment(root), error => error.message === 'Invalid protected anbud deployment credential');
}))
test('rejects readable credential files and symlinks', () => fixture(({ file, directory }) => {
  chmodSync(file, 0o644)
  assert.throws(() => readSelectedEnvironment(file, ['SELECTED']), /protected/)
  chmodSync(file, 0o600)
  const link = join(directory, 'link')
  symlinkSync(file, link)
  assert.throws(() => readSelectedEnvironment(link, ['SELECTED']), /protected/)
}))
test('rejects duplicates and interpolation without printing values', () => fixture(({ file }) => {
  for (const content of ['SELECTED=first\nSELECTED=second', 'SELECTED=${SHELL_SECRET}', 'SELECTED="quoted"']) {
    writeFileSync(file, content)
    assert.throws(() => readSelectedEnvironment(file, ['SELECTED']), error => !error.message.includes(content))
  }
}))
test('canonical control values win over ambient and local values', () => {
  const result = frontendEnvironment({ ambient: { PATH: '/bin', SESSION_CORE_SERVICE_TOKEN: 'wrong', PRIVATE_OTHER: 'hidden' },
    local: { INTERNAL_API_KEY: 'local-internal', SESSION_CORE_SERVICE_TOKEN: 'stale' },
    control: { SESSION_CORE_SERVICE_TOKEN: 'canonical' }, ingestion: {} })
  assert.equal(result.SESSION_CORE_SERVICE_TOKEN, 'canonical')
  assert.equal(result.INTERNAL_API_KEY, 'local-internal')
  assert.equal(result.PRIVATE_OTHER, undefined)
  assert.equal(result.PATH, '/bin')
})
test('security policy cannot inherit bypasses, remote public origin or cache credentials', () => {
  const result = frontendEnvironment({ ambient: { ALLOW_DEV_AUTH_BYPASS: '1', VITE_ALLOW_DEV_AUTH_BYPASS: 'true', GATEWAY_CACHE_REDIS_URL: 'remote' }, local: {}, control: {}, ingestion: {} })
  assert.equal(result.ALLOW_DEV_AUTH_BYPASS, '0')
  assert.equal(result.VITE_ALLOW_DEV_AUTH_BYPASS, 'false')
  assert.equal(result.CONTROL_PLANE_ENFORCEMENT, 'strict')
  assert.equal(result.VEREVON_BIND_ADDRESS, '127.0.0.1')
  assert.equal(result.GATEWAY_CACHE_REDIS_URL, undefined)
})

test('runner uses only canonical root Compose with no credentials in arguments', () => fixture(({ directory }) => {
  const root = join(directory, 'Frontend Plane', 'verevonv3')
  const secrets = join(directory, 'Engagement plane', '.local', 'secrets')
  mkdirSync(root, { recursive: true })
  mkdirSync(secrets, { recursive: true, mode: 0o700 })
  const localNames = ['INTERNAL_API_KEY', 'CONVERSATION_GATEWAY_SERVICE_TOKEN', 'NOTIFICATION_GATEWAY_SERVICE_TOKEN', 'VEREVON_NATS_TOKEN']
  const controlNames = ['SESSION_CORE_SERVICE_TOKEN', 'USER_CORE_GATEWAY_TOKEN', 'GATEWAY_AUDIT_CORE_SERVICE_TOKEN', 'GATEWAY_BILLING_CORE_SERVICE_TOKEN', 'GATEWAY_ORG_CORE_SERVICE_TOKEN', 'VEREVON_GATEWAY_GDPR_NATS_PASSWORD']
  writeFileSync(join(root, '.env'), localNames.map(name => `${name}=synthetic-local-${name}`).join('\n'), { mode: 0o600 })
  writeFileSync(join(secrets, 'control_plane_compose'), controlNames.map(name => `${name}=synthetic-control-${name}`).join('\n'), { mode: 0o600 })
  for (const plane of ['studio', 'control', 'ingestion']) writeFileSync(join(secrets, `shared_nats_${plane}_leaf_password`), `synthetic-${plane}-credential-at-least-32-characters`, { mode: 0o600 })
  let called = false
  const spawn = (binary, args, config) => {
    called = true
    assert.equal(binary, 'docker')
    assert.deepEqual(args, ['compose', '--env-file', '/dev/null', '-f', join(root, 'docker-compose.yml'), 'config', '--quiet'])
    assert.equal(config.env.SESSION_CORE_SERVICE_TOKEN, 'synthetic-control-SESSION_CORE_SERVICE_TOKEN')
    assert.equal(config.env.ALLOW_DEV_AUTH_BYPASS, '0')
    assert.equal(config.env.ANBUD_API_KEY, undefined)
    assert.equal(config.env.VEREVON_NATS_TOKEN, JSON.stringify('synthetic-local-VEREVON_NATS_TOKEN'))
    for (const plane of ['studio', 'control', 'ingestion']) {
      assert.equal(config.env[`NATS_${plane.toUpperCase()}_LEAF_PASSWORD`], JSON.stringify(`synthetic-${plane}-credential-at-least-32-characters`))
    }
    assert.equal(args.some(value => value.includes('synthetic-')), false)
    return { status: 0 }
  }
  assert.equal(run(['config', '--quiet'], { root, spawn }), 0)
  assert.equal(called, true)
  for (const args of [[], ['config'], ['up', '-f', 'other.yml'], ['--env-file', 'other'], ['up', '--env-file=other'], ['up', '-fother']]) assert.throws(() => run(args, { root, spawn }))
  assert.throws(() => run(['build'], { root, spawn: () => ({ error: new Error('synthetic') }) }), /could not be started/)
}))
