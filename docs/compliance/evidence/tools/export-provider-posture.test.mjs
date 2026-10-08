// Tests for export-provider-posture.sh. Run: node --test docs/compliance/evidence/tools/*.test.mjs
//
// The property under test: the script never prints a value outside its allow-list.
// Every fixture value that is not allow-listed is a distinctive canary, and the
// assertion is that no canary (and no fragment of one) appears anywhere in the
// output, on stdout or stderr.
//
// Needs `bash` and `awk` on PATH (Git Bash on Windows, any Linux or macOS). The
// tests are skipped, loudly, when bash is missing rather than passing by default.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const HERE = path.dirname(fileURLToPath(import.meta.url))
const SCRIPT = path.join(HERE, 'export-provider-posture.sh').split(path.sep).join('/')

const probe = spawnSync('bash', ['-c', 'command -v awk >/dev/null && echo ok'], { encoding: 'utf8' })
const HAVE_BASH = probe.status === 0 && probe.stdout.trim() === 'ok'
const maybe = HAVE_BASH ? test : test.skip

function run(envText, extraArgs = []) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'posture-'))
  const file = path.join(dir, 'fixture.env')
  fs.writeFileSync(file, envText)
  const result = spawnSync('bash', [SCRIPT, '--env-file', file.split(path.sep).join('/'), ...extraArgs], {
    encoding: 'utf8',
  })
  fs.rmSync(dir, { recursive: true, force: true })
  return { status: result.status, out: result.stdout, err: result.stderr, all: `${result.stdout}\n${result.stderr}` }
}

function rows(out) {
  const found = new Map()
  for (const line of out.split('\n')) {
    if (line.startsWith('#') || line.length === 0) continue
    const [name, state, shown] = line.split('\t')
    found.set(name, { state, shown })
  }
  return found
}

// Names and values below are fixtures. Every CANARY must stay out of the output.
const FIXTURE = [
  '# a comment, ignored',
  '',
  'AZURE_OPENAI_API_KEY=CANARY_KEY_AZURE_1234567890',
  'ANTHROPIC_API_KEY="CANARY_QUOTED_KEY"',
  "OPENAI_API_KEY='CANARY_SINGLE_QUOTED'",
  'export COHERE_API_KEY=CANARY_EXPORTED_KEY',
  'AZURE_OPENAI_ENDPOINT=https://admin:CANARY_URL_PASSWORD@core-ai-rg.cognitiveservices.azure.com:443/openai?api-key=CANARY_QUERY_KEY#CANARY_FRAGMENT',
  'AZURE_COHERE_ENDPOINT=HTTPS://Core-AI-RG.services.ai.azure.com/models',
  'RERANK_ENDPOINT=CANARY_BARE_TOKEN_IN_ENDPOINT',
  'AZURE_SPEECH_ENDPOINT=//CANARY_NO_SCHEME_HOST/path',
  'OPENAI_API_BASE=https://api.openai.com/v1',
  'COHERE_URL=https://api.cohere.ai/v1/rerank',
  'AZURE_OPENAI_REGION=swedencentral',
  'AZURE_ANTHROPIC_REGION=',
  'AZURE_TRANSLATOR_REGION=Sweden Central',
  'AZURE_OPENAI_DEPLOYMENT_TYPE=DataZoneStandard',
  'MODEL_PLANE_RESIDENCY=swedencentral',
  'MODEL_PLANE_ALLOW_NON_EU_TTS=TRUE',
  'MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT=false',
  // Ends in _RESIDENCY AND starts with MODEL_PLANE_ALLOW_: must be held to the boolean shape.
  'MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY=CANARY_NOT_A_BOOLEAN',
  'AZURE_PASTED_REGION=CANARY_sk-abcdefghijklmnopqrstuvwxyz0123456789',
  'AZURE_SPACEY_REGION=CANARY has spaces @ and = signs',
  'AZURE_DIGITS_REGION=abc12345678',
  'AZURE_LONG_DEPLOYMENT_TYPE=abcdefghijklmnopqrstuvwxyzabcdefgh',
  'INFERENCE_PROVIDER_ORDER=azure,anthropic,openai,cohere,openai-codex-subscription',
  'GENERATION_MODEL=azure/gpt-4o-mini',
  'EMBEDDING_PROVIDER=cohere',
  'CODEX_SUBSCRIPTION_TRANSPORT=app-server',
  'EMBEDDING_MODEL=CANARYabcdefghijklmnopqrstuvwxyz0123456789',
  'NOVU_SECRET_KEY=',
  'STRIPE_API_KEY=CANARY_STRIPE_SECRET',
  'TWILIO_AUTH_TOKEN=CANARY_TWILIO_TOKEN',
  'DATABASE_URL=postgres://postgres:CANARY_DB_PASSWORD@db:5432/session_core',
  'JWT_SECRET=CANARY_JWT_SECRET',
  'PATH=/usr/bin:CANARY_PATH_ENTRY',
  'lowercase_var=CANARY_LOWERCASE',
  'AZURE_OPENAI_ZDR_DIGEST=CANARY_DIGEST_0123456789abcdef0123456789abcdef',
  'NOT_A_LINE_WITH_EQUALS_CANARY_TEXT',
  '',
].join('\n')

const CANARIES = [
  'CANARY', 'sk-abcdefghijklmnopqrstuvwxyz', 'admin', 'api-key=', 'postgres://', 'a_comment',
]

maybe('no canary value is ever printed, on stdout or stderr', () => {
  const result = run(FIXTURE)
  assert.equal(result.status, 0)
  for (const canary of CANARIES) {
    if (canary === 'a_comment') continue
    assert.ok(!result.all.includes(canary), `leaked ${JSON.stringify(canary)} in:\n${result.all}`)
  }
  // Credentials, ports, paths and query strings of an endpoint are gone.
  for (const fragment of ['admin', ':443', '/openai', 'api-key', 'password', 'PASSWORD', 'FRAGMENT']) {
    assert.ok(!result.all.includes(fragment), `leaked endpoint fragment ${JSON.stringify(fragment)}`)
  }
})

maybe('only the allow-listed values are shown, in the right shape', () => {
  const found = rows(run(FIXTURE).out)
  const expected = {
    AZURE_OPENAI_REGION: ['set', 'swedencentral'],
    AZURE_ANTHROPIC_REGION: ['empty', '-'],
    AZURE_TRANSLATOR_REGION: ['set', 'Sweden Central'],
    AZURE_OPENAI_DEPLOYMENT_TYPE: ['set', 'DataZoneStandard'],
    MODEL_PLANE_RESIDENCY: ['set', 'swedencentral'],
    MODEL_PLANE_ALLOW_NON_EU_TTS: ['set', 'true'],
    MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT: ['set', 'false'],
    MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY: ['set', '[withheld: unexpected shape]'],
    AZURE_PASTED_REGION: ['set', '[withheld: unexpected shape]'],
    AZURE_SPACEY_REGION: ['set', '[withheld: unexpected shape]'],
    AZURE_DIGITS_REGION: ['set', '[withheld: unexpected shape]'],
    AZURE_LONG_DEPLOYMENT_TYPE: ['set', '[withheld: unexpected shape]'],
    INFERENCE_PROVIDER_ORDER: ['set', 'azure,anthropic,openai,cohere,openai-codex-subscription'],
    GENERATION_MODEL: ['set', 'azure/gpt-4o-mini'],
    EMBEDDING_PROVIDER: ['set', 'cohere'],
    CODEX_SUBSCRIPTION_TRANSPORT: ['set', 'app-server'],
    EMBEDDING_MODEL: ['set', '[withheld: unexpected shape]'],
    AZURE_OPENAI_ENDPOINT: ['set', 'core-ai-rg.cognitiveservices.azure.com'],
    AZURE_COHERE_ENDPOINT: ['set', 'core-ai-rg.services.ai.azure.com'],
    RERANK_ENDPOINT: ['set', '[withheld: no http(s) host]'],
    AZURE_SPEECH_ENDPOINT: ['set', '[withheld: no http(s) host]'],
    OPENAI_API_BASE: ['set', 'api.openai.com'],
    COHERE_URL: ['set', 'api.cohere.ai'],
    // Names only: the value is never shown, whatever it is.
    AZURE_OPENAI_API_KEY: ['set', '-'],
    ANTHROPIC_API_KEY: ['set', '-'],
    OPENAI_API_KEY: ['set', '-'],
    COHERE_API_KEY: ['set', '-'],
    STRIPE_API_KEY: ['set', '-'],
    TWILIO_AUTH_TOKEN: ['set', '-'],
    AZURE_OPENAI_ZDR_DIGEST: ['set', '-'],
    NOVU_SECRET_KEY: ['empty', '-'],
  }
  for (const [name, [state, shown]] of Object.entries(expected)) {
    assert.ok(found.has(name), `${name} is missing from the output`)
    assert.deepEqual(found.get(name), { state, shown }, name)
  }
})

maybe('variables that are not provider settings are never listed', () => {
  const found = rows(run(FIXTURE).out)
  for (const name of ['DATABASE_URL', 'JWT_SECRET', 'PATH', 'lowercase_var', 'NOT_A_LINE_WITH_EQUALS_CANARY_TEXT']) {
    assert.ok(!found.has(name), `${name} must not be listed`)
  }
})

maybe('output is sorted by name, so two runs diff cleanly', () => {
  const names = [...rows(run(FIXTURE).out).keys()]
  assert.deepEqual(names, [...names].sort())
})

maybe('a CRLF env file (a Windows .env) is read the same way', () => {
  const crlf = FIXTURE.replace(/\n/g, '\r\n')
  const result = run(crlf)
  assert.equal(result.status, 0)
  assert.ok(!result.all.includes('CANARY'))
  const found = rows(result.out.replace(/\r/g, ''))
  assert.deepEqual(found.get('AZURE_OPENAI_REGION'), { state: 'set', shown: 'swedencentral' })
  assert.deepEqual(found.get('AZURE_OPENAI_ENDPOINT'), { state: 'set', shown: 'core-ai-rg.cognitiveservices.azure.com' })
})

maybe('a secret in an allow-listed name is withheld when it does not look like the thing', () => {
  const secrets = [
    'AZURE_X_REGION=ghp_abcdefghijklmnopqrstuv',
    'AZURE_X_DEPLOYMENT_TYPE=Bearer eyJhbGciOiJIUzI1NiJ9',
    'AZURE_X_RESIDENCY=0123456789abcdef0123',
    'MODEL_PLANE_ALLOW_X=yes-please',
    'INFERENCE_PROVIDER_ORDER=0123456789abcdef0123456789abcdef0123456789',
    'GENERATION_MODEL=sk-proj-abcdefghijklmnopqrstuvwxyz',
  ].join('\n')
  const found = rows(run(secrets).out)
  for (const name of found.keys()) {
    assert.equal(found.get(name).shown, '[withheld: unexpected shape]', name)
  }
  assert.equal(found.size, 6)
})

maybe('a bad invocation prints usage and exits 2; an unreadable file is reported, not echoed', () => {
  const none = spawnSync('bash', [SCRIPT], { encoding: 'utf8' })
  assert.equal(none.status, 2)
  assert.match(none.stderr, /usage/)
  const unknown = spawnSync('bash', [SCRIPT, '--nope'], { encoding: 'utf8' })
  assert.equal(unknown.status, 2)
  const missing = spawnSync('bash', [SCRIPT, '--env-file', '/no/such/file.env'], { encoding: 'utf8' })
  assert.equal(missing.status, 1)
  assert.match(missing.stdout, /\[unreadable\]/)
})

maybe('an unknown container is reported as not inspectable and prints nothing from the environment', () => {
  const result = spawnSync('bash', [SCRIPT, '--container', 'definitely-not-a-running-container-name'], { encoding: 'utf8' })
  assert.equal(result.status, 1)
  assert.match(result.stdout, /## container definitely-not-a-running-container-name/)
  assert.match(result.stdout, /\[not inspectable|\[docker not available\]/)
})

maybe('the script file is LF-only, so a Linux shell can run it', () => {
  const bytes = fs.readFileSync(path.join(HERE, 'export-provider-posture.sh'))
  assert.ok(!bytes.includes(13), 'export-provider-posture.sh must not contain CR bytes')
  assert.ok(bytes.subarray(0, 2).toString() === '#!', 'it must start with a shebang')
})
