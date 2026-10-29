import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { chmodSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { provisionMarketingPrincipal } from './provision-marketing-principal.mjs'

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'marketing-principal-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const secretsDirectory = join(root, 'secrets')
  mkdirSync(secretsDirectory, { mode: 0o700 })
  const sourceEnvFile = join(secretsDirectory, 'control_plane_compose')
  const original = { existing: { credential: 'a'.repeat(64), audiences: ['ingestion'], orgIds: ['another-org'], scopes: ['integration:read'] } }
  writeFileSync(sourceEnvFile, `PLANE_SERVICE_PRINCIPALS_JSON=${JSON.stringify(original)}\n`, { mode: 0o600 })
  return { root, original, options: { organizationId: 'real-org-123', secretsDirectory, sourceEnvFile } }
}

test('provisions fixed tenant read access, preserves registry, and keeps secrets private', (t) => {
  const { original, options } = fixture(t)
  const result = provisionMarketingPrincipal(options)
  const key = readFileSync(result.keyFile, 'utf8')
  const registry = JSON.parse(readFileSync(result.registryFile, 'utf8'))
  assert.match(key, /^[a-f0-9]{64}$/)
  assert.deepEqual(registry.existing, original.existing)
  assert.deepEqual(registry['marketing-core'], {
    credential: key, audiences: ['ingestion'], orgIds: ['real-org-123'],
    scopes: ['integration:read'], scopesByAudience: { ingestion: ['integration:read'] },
    retentionByAudience: { ingestion: 'persistent' },
  })
  for (const file of [result.keyFile, result.registryFile]) assert.equal(lstatSync(file).mode & 0o777, 0o600)
  assert.equal(result.created, true)
  assert.equal(JSON.stringify(result).includes(key), false)
})

test('idempotent second run never rotates existing key or registry', (t) => {
  const { options } = fixture(t)
  const first = provisionMarketingPrincipal(options)
  const before = [readFileSync(first.keyFile), readFileSync(first.registryFile)]
  const second = provisionMarketingPrincipal(options)
  assert.equal(second.created, false)
  assert.deepEqual(readFileSync(second.keyFile), before[0])
  assert.deepEqual(readFileSync(second.registryFile), before[1])
})

test('refuses an existing principal for a different org without changing files', (t) => {
  const { options } = fixture(t)
  const result = provisionMarketingPrincipal(options)
  const before = readFileSync(result.registryFile)
  assert.throws(() => provisionMarketingPrincipal({ ...options, organizationId: 'other-org' }), /existing Marketing principal/)
  assert.deepEqual(readFileSync(result.registryFile), before)
})

test('recovers a previously installed key without rotating it', (t) => {
  const { options } = fixture(t)
  const keyFile = join(options.secretsDirectory, 'marketing_plane_service_key')
  writeFileSync(keyFile, 'b'.repeat(64), { mode: 0o600 })
  const result = provisionMarketingPrincipal(options)
  assert.equal(readFileSync(result.keyFile, 'utf8'), 'b'.repeat(64))
})

test('rejects unsafe permissions, symlinks, and malformed registries', (t) => {
  const { options, original } = fixture(t)
  chmodSync(options.sourceEnvFile, 0o644)
  assert.throws(() => provisionMarketingPrincipal(options), /private regular file/)
  chmodSync(options.sourceEnvFile, 0o600)
  chmodSync(options.secretsDirectory, 0o755)
  assert.throws(() => provisionMarketingPrincipal(options), /private secrets directory/)
  chmodSync(options.secretsDirectory, 0o700)
  const link = join(options.secretsDirectory, 'source-link')
  symlinkSync(options.sourceEnvFile, link)
  assert.throws(() => provisionMarketingPrincipal({ ...options, sourceEnvFile: link }), /private regular file/)
  for (const content of ['PLANE_SERVICE_PRINCIPALS_JSON=not-json\n', 'PLANE_SERVICE_PRINCIPALS_JSON=[]\n', 'PLANE_SERVICE_PRINCIPALS_JSON={"bad":{"credential":"secret"}}\n', `PLANE_SERVICE_PRINCIPALS_JSON=${JSON.stringify(original)}\nPLANE_SERVICE_PRINCIPALS_JSON={}\n`]) {
    writeFileSync(options.sourceEnvFile, content)
    assert.throws(() => provisionMarketingPrincipal(options), /registry/)
  }
})

test('rejects unsafe organization IDs before writing credentials', (t) => {
  const { options } = fixture(t)
  for (const organizationId of ['', '../org', 'org id', 'org\nheader']) {
    assert.throws(() => provisionMarketingPrincipal({ ...options, organizationId }), /organization ID/)
  }
})

test('CLI reports only paths/status and suppresses secret values in parse failures', (t) => {
  const { options } = fixture(t)
  const script = new URL('./provision-marketing-principal.mjs', import.meta.url)
  const args = [fileURLToPath(script), '--org', options.organizationId, '--secrets-dir', options.secretsDirectory, '--source-env', options.sourceEnvFile]
  const result = spawnSync(process.execPath, args, { encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr)
  const key = readFileSync(join(options.secretsDirectory, 'marketing_plane_service_key'), 'utf8')
  assert.equal(result.stdout.includes(key), false)
  const secretMarker = 'secret-must-never-be-printed'
  writeFileSync(options.sourceEnvFile, `PLANE_SERVICE_PRINCIPALS_JSON=${secretMarker}\n`)
  const failure = spawnSync(process.execPath, args, { encoding: 'utf8' })
  assert.notEqual(failure.status, 0)
  assert.equal((failure.stdout + failure.stderr).includes(secretMarker), false)
})

test('rejects source principal conflicts and unsafe existing output files', (t) => {
  const { options, original } = fixture(t)
  const sourceMarketing = { credential: 'c'.repeat(64), audiences: ['ingestion'], orgIds: ['other-org'], scopes: ['integration:read'] }
  writeFileSync(options.sourceEnvFile, `PLANE_SERVICE_PRINCIPALS_JSON=${JSON.stringify({ ...original, 'marketing-core': sourceMarketing })}\n`)
  assert.throws(() => provisionMarketingPrincipal(options), /existing Marketing principal/)
  writeFileSync(options.sourceEnvFile, `PLANE_SERVICE_PRINCIPALS_JSON=${JSON.stringify(original)}\n`)
  const result = provisionMarketingPrincipal(options)
  chmodSync(result.keyFile, 0o644)
  assert.throws(() => provisionMarketingPrincipal(options), /private regular file/)
  chmodSync(result.keyFile, 0o600)
  chmodSync(result.registryFile, 0o644)
  assert.throws(() => provisionMarketingPrincipal(options), /private regular file/)
})

test('Auth overlay clears inline registry and mounts it through the privilege-dropping handoff', (t) => {
  const { root, options } = fixture(t)
  const result = provisionMarketingPrincipal(options)
  const base = join(root, 'base.yml')
  writeFileSync(base, 'services:\n  auth-core:\n    image: synthetic-auth\n    environment:\n      PLANE_SERVICE_PRINCIPALS_JSON: synthetic-inline\n')
  const overlay = fileURLToPath(new URL('../docker-compose.auth-marketing.local.yml', import.meta.url))
  const composed = spawnSync('docker', ['compose', '-f', base, '-f', overlay, 'config', '--format', 'json'], {
    encoding: 'utf8', env: { ...process.env, STUDIO_MARKETING_AUTH_PRINCIPALS_FILE: result.registryFile },
  })
  assert.equal(composed.status, 0, composed.stderr)
  const rendered = JSON.parse(composed.stdout)
  const auth = rendered.services['auth-core']
  assert.equal(auth.user, 'root')
  assert.deepEqual(auth.cap_drop, ['ALL'])
  assert.deepEqual(auth.cap_add, ['CHOWN', 'FOWNER', 'DAC_OVERRIDE', 'SETUID', 'SETGID'])
  assert.equal(auth.environment.PLANE_SERVICE_PRINCIPALS_JSON, undefined)
  assert.equal(auth.environment.PLANE_SERVICE_PRINCIPALS_FILE, '/run/secrets/marketing_auth_plane_principals')
  assert.deepEqual(auth.tmpfs, ['/run/control-secrets:rw,noexec,nosuid,nodev,size=2m,mode=0710'])
  assert.equal(auth.secrets[0].target, 'marketing_auth_plane_principals')
  assert.equal(rendered.secrets.studio_marketing_auth_plane_principals.file, result.registryFile)
})

test('launcher selects stable protected sources without exposing credentials or invoking Docker', (t) => {
  const { root, options } = fixture(t)
  const result = provisionMarketingPrincipal(options)
  const bin = join(root, 'bin')
  mkdirSync(bin)
  const reporter = `#!${process.execPath}\nconsole.log(JSON.stringify({args:process.argv.slice(2),source:process.env.CONTROL_PLANE_SECRETS_FILE,registry:process.env.STUDIO_MARKETING_AUTH_PRINCIPALS_FILE}))\n`
  writeFileSync(join(bin, 'bash'), reporter, { mode: 0o700 })
  const launcher = fileURLToPath(new URL('./auth-marketing-local.sh', import.meta.url))
  const launched = spawnSync('sh', [launcher, 'config', '--quiet'], {
    encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, STUDIO_LOCAL_SECRETS_DIR: options.secretsDirectory },
  })
  assert.equal(launched.status, 0, launched.stderr)
  const report = JSON.parse(launched.stdout)
  assert.equal(report.source, options.sourceEnvFile)
  assert.equal(report.registry, result.registryFile)
  assert.deepEqual(report.args.slice(-2), ['config', '--quiet'])
  assert.equal(report.args[1], '-f')
  assert.equal(launched.stdout.includes(readFileSync(result.keyFile, 'utf8')), false)
  chmodSync(result.registryFile, 0o644)
  const refused = spawnSync('sh', [launcher, 'config', '--quiet'], {
    encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, STUDIO_LOCAL_SECRETS_DIR: options.secretsDirectory },
  })
  assert.notEqual(refused.status, 0)
  assert.match(refused.stderr, /mode-0600/)
})
