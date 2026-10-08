import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  assertLoopbackBaseUrl,
  bodyDigest,
  buildRequests,
  delegationSignature,
  isTestOrgName,
  parseArgs,
  run,
  signedHeaders,
  TOKEN_ENV,
  validateOptions,
} from './zdr-seed-test-org.mjs'

// The operator seed for a ZDR test org (docs/ZDR_OPERATOR_SEED_RUNBOOK.md).
//
// The gateway refuses to turn org ZDR on while no provider is attested; org-core
// has no such gate, so a direct, signed PATCH is the operator-only way to seed ONE
// test org. These tests pin (1) the signer to org-core's real verifier through fixed
// vectors that org-core's own suite asserts too, (2) every refusal the script makes
// before it sends anything, and (3) that org-core really stays ungated.

const here = dirname(fileURLToPath(import.meta.url))
const GATEWAY_ROOT = join(here, '..')
const FRONTEND_ROOT = join(GATEWAY_ROOT, '..', '..')

function repoRoot() {
  let dir = here
  for (;;) {
    if (existsSync(join(dir, 'apps', 'Control Plane', 'org-core'))) return dir
    const parent = dirname(dir)
    if (parent === dir) return undefined
    dir = parent
  }
}

/** A cross-tree file. Without REQUIRE_CONTRACT or CI a missing tree skips; with either it fails. */
function crossTreeFile(t, ...parts) {
  const root = repoRoot()
  const path = root ? join(root, ...parts) : undefined
  if (path && existsSync(path)) return readFileSync(path, 'utf8')
  if (process.env.REQUIRE_CONTRACT || process.env.CI) {
    assert.fail(`${parts.join('/')} is missing and REQUIRE_CONTRACT or CI is set`)
  }
  t.skip(`${parts.join('/')} is absent (set REQUIRE_CONTRACT=1 to make that a failure)`)
  return undefined
}

// Fixed vector, shared byte for byte with org-core's
// internal/http/zdr_seed_delegation_vector_test.go.
const VECTOR = {
  token: 'zdr-seed-vector-credential-0123456789abcdef',
  timestamp: '2026-10-06T12:00:00Z',
  orgId: 'org-zdr-test-1',
  userId: 'user-owner-1',
  userRole: 'owner',
  patchNonce: 'zdr-seed-nonce-0123456789ab-write',
  patchUri: '/api/v1/organizations/org-zdr-test-1/settings',
  patchBody: '{"zeroDataRetention":true}',
  patchDigest: 'YpBp4jNws7-HykHuKUZzz3oy0yWeM68CprEfp8oYMTo',
  patchSignature: 'GgWAflcO4GXuGV2wgfPaYpvzjTxNzxGRlrFNdwmawZs',
  readNonce: 'zdr-seed-nonce-0123456789ab-read',
  readUri: '/api/v1/organizations/org-zdr-test-1',
  readDigest: '47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU',
  readSignature: '2Ys0-1OJrSMw6tHjVBU-KfYnUgrcGC4rkCRb6ag-jW8',
}

test('the signer reproduces the vectors org-core verifies', () => {
  const patch = signedHeaders({
    token: VECTOR.token, method: 'PATCH', uri: VECTOR.patchUri, body: VECTOR.patchBody,
    userId: VECTOR.userId, orgId: VECTOR.orgId, userRole: VECTOR.userRole,
    timestamp: VECTOR.timestamp, nonce: VECTOR.patchNonce,
  })
  assert.equal(patch['X-Delegation-Body-SHA256'], VECTOR.patchDigest)
  assert.equal(patch['X-Delegation-Signature'], VECTOR.patchSignature)

  const read = signedHeaders({
    token: VECTOR.token, method: 'GET', uri: VECTOR.readUri, body: '',
    userId: VECTOR.userId, orgId: VECTOR.orgId, userRole: VECTOR.userRole,
    timestamp: VECTOR.timestamp, nonce: VECTOR.readNonce,
  })
  assert.equal(read['X-Delegation-Body-SHA256'], VECTOR.readDigest)
  assert.equal(read['X-Delegation-Signature'], VECTOR.readSignature)

  assert.equal(bodyDigest(VECTOR.patchBody), VECTOR.patchDigest)
  assert.equal(
    delegationSignature(VECTOR.token, {
      principal: 'verevon-gateway', audience: 'org-core', timestamp: VECTOR.timestamp,
      nonce: VECTOR.patchNonce, method: 'PATCH', uri: VECTOR.patchUri, userId: VECTOR.userId,
      orgId: VECTOR.orgId, userRole: VECTOR.userRole, bodySha256: VECTOR.patchDigest,
    }),
    VECTOR.patchSignature,
  )
})

test('the signed headers are exactly the set org-core requires, and nothing else', () => {
  const headers = signedHeaders({
    token: VECTOR.token, method: 'PATCH', uri: VECTOR.patchUri, body: VECTOR.patchBody,
    userId: VECTOR.userId, orgId: VECTOR.orgId, userRole: VECTOR.userRole,
    timestamp: VECTOR.timestamp, nonce: VECTOR.patchNonce,
  })
  assert.deepEqual(Object.keys(headers).sort(), [
    'X-Delegation-Body-SHA256', 'X-Delegation-Nonce', 'X-Delegation-Signature',
    'X-Delegation-Timestamp', 'X-Delegation-Version', 'X-Org-Id', 'X-Service-Id',
    'X-Service-Token', 'X-User-Id', 'X-User-Role',
  ])
  assert.equal(headers['X-Service-Id'], 'verevon-gateway')
  assert.equal(headers['X-Delegation-Version'], 'v3')
})

test('org-core asserts the very same literals (drift check)', (t) => {
  const source = crossTreeFile(
    t, 'apps', 'Control Plane', 'org-core', 'internal', 'http', 'zdr_seed_delegation_vector_test.go',
  )
  if (source === undefined) return
  for (const literal of [
    VECTOR.token, VECTOR.timestamp, VECTOR.orgId, VECTOR.userId, VECTOR.userRole,
    VECTOR.patchNonce, VECTOR.patchUri, VECTOR.patchBody, VECTOR.patchDigest, VECTOR.patchSignature,
    VECTOR.readNonce, VECTOR.readUri, VECTOR.readDigest, VECTOR.readSignature,
  ]) {
    assert.ok(source.includes(literal), `org-core's vector test no longer carries ${literal}`)
  }
})

test('org-core stays ungated: its settings handler never consults a provider catalogue', (t) => {
  const handlers = crossTreeFile(t, 'apps', 'Control Plane', 'org-core', 'internal', 'http', 'handlers.go')
  if (handlers === undefined) return
  const start = handlers.indexOf('func (s *Server) updateOrgSettings(')
  assert.ok(start >= 0, 'updateOrgSettings is gone from org-core')
  const end = handlers.indexOf('\nfunc ', start + 10)
  const body = handlers.slice(start, end === -1 ? undefined : end)
  for (const forbidden of ['/v1/models', 'attest', 'catalogue', 'catalog', 'zdr_no_attested_provider', 'model-gateway']) {
    assert.ok(
      !body.toLowerCase().includes(forbidden.toLowerCase()),
      `org-core's updateOrgSettings mentions ${forbidden}: the operator seed path would be closed`,
    )
  }
  // The attestation gate lives in the gateway only; this is what it answers.
  const gate = readFileSync(join(GATEWAY_ROOT, 'src', 'domains', 'orgs', 'settings.rs'), 'utf8')
  assert.ok(gate.includes('"zdr_no_attested_provider"'), 'the gateway gate is gone')
})

test('the runbook names the script, the test-org convention, the credential variable and the way back', () => {
  const runbook = readFileSync(join(FRONTEND_ROOT, 'docs', 'ZDR_OPERATOR_SEED_RUNBOOK.md'), 'utf8')
  for (const needle of ['zdr-seed-test-org.mjs', 'ZDR-TEST', TOKEN_ENV, '--zdr false', '--apply', 'zdr_no_attested_provider']) {
    assert.ok(runbook.includes(needle), `the runbook does not mention ${needle}`)
  }
})

// ---- the script's own refusals ------------------------------------------------

const GOOD = [
  '--org-id', VECTOR.orgId, '--user-id', VECTOR.userId, '--expect-name', 'ZDR-TEST Terra proof',
]

function recorder() {
  const lines = []
  return { lines, out: (line) => lines.push(line), text: () => lines.join('\n') }
}

function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

const ENV = { [TOKEN_ENV]: VECTOR.token }
const NOW = new Date('2026-10-06T12:00:00.000Z')
const NONCE = 'zdr-seed-nonce-0123456789ab'

function org(name, zdr = false) {
  return { id: VECTOR.orgId, name, plan: 'pro', metadata: { interactiveRetention: { zdr } } }
}

test('without --apply it prints the plan and sends nothing, not even a read', async () => {
  const sent = []
  const { lines, out, text } = recorder()
  const code = await run(GOOD, { env: ENV, fetchImpl: async (...args) => { sent.push(args); return jsonResponse({}) }, now: NOW, nonce: NONCE, out })
  assert.equal(code, 0)
  assert.deepEqual(sent, [])
  assert.match(text(), /PLAN only, nothing is sent/)
  assert.match(text(), /switched ON for that org only/)
  assert.ok(lines.length > 0)
  assert.ok(!text().includes(VECTOR.token))
})

test('it refuses a name that is not a ZDR-TEST org, before any request', async () => {
  for (const name of ['Aquatiq AS', 'zdr test', 'Production ZDR-TEST', '']) {
    const sent = []
    const { out, text } = recorder()
    const code = await run(
      ['--org-id', VECTOR.orgId, '--user-id', VECTOR.userId, '--expect-name', name, '--apply'],
      { env: ENV, fetchImpl: async (...args) => { sent.push(args); return jsonResponse({}) }, now: NOW, nonce: NONCE, out },
    )
    assert.equal(code, 2, `name ${JSON.stringify(name)}`)
    assert.deepEqual(sent, [])
    assert.match(text(), /ZDR-TEST/)
  }
  assert.equal(isTestOrgName('zdr-test lower case is fine'), true)
  assert.equal(isTestOrgName('  ZDR-TEST padded'), true)
})

test('it reads the org first and refuses when the stored name is not the typed one', async () => {
  const calls = []
  const { out, text } = recorder()
  const fetchImpl = async (url, init) => {
    calls.push({ url, method: init.method })
    return jsonResponse(org('Aquatiq AS'))
  }
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 2)
  assert.deepEqual(calls, [{ url: `http://127.0.0.1:18080${VECTOR.readUri}`, method: 'GET' }])
  assert.match(text(), /Nothing was changed/)
})

test('it refuses an org that merely starts the same: the typed name must be the whole name', async () => {
  const calls = []
  const { out } = recorder()
  const fetchImpl = async (url, init) => { calls.push(init.method); return jsonResponse(org('ZDR-TEST Terra proof (the real customer)')) }
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 2)
  assert.deepEqual(calls, ['GET'])
})

test('it refuses an answer for a different org id', async () => {
  const calls = []
  const { out } = recorder()
  const fetchImpl = async (url, init) => { calls.push(init.method); return jsonResponse({ ...org('ZDR-TEST Terra proof'), id: 'org-other' }) }
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 2)
  assert.deepEqual(calls, ['GET'])
})

test('it only ever talks to loopback, so the credential cannot leave the host', () => {
  assert.equal(assertLoopbackBaseUrl('http://127.0.0.1:18080'), 'http://127.0.0.1:18080')
  assert.equal(assertLoopbackBaseUrl('http://localhost:18080'), 'http://localhost:18080')
  for (const bad of ['https://127.0.0.1:18080', 'http://org-core:8080', 'http://example.com', 'http://127.0.0.1.evil.example', 'not a url']) {
    assert.throws(() => assertLoopbackBaseUrl(bad), /base-url/, bad)
  }
})

test('it rejects malformed ids, roles, flags and unknown arguments', () => {
  const ok = parseArgs(GOOD)
  assert.doesNotThrow(() => validateOptions(ok))
  assert.throws(() => validateOptions({ ...ok, orgId: 'org/../other' }), /--org-id/)
  assert.throws(() => validateOptions({ ...ok, userId: '' }), /--user-id/)
  assert.throws(() => validateOptions({ ...ok, userRole: 'member' }), /--user-role/)
  assert.throws(() => validateOptions({ ...ok, zdr: 'yes' }), /--zdr/)
  assert.throws(() => parseArgs([...GOOD, '--force']), /unknown argument/)
  assert.throws(() => parseArgs(['--org-id']), /needs a value/)
})

test('it needs the credential, and says which variable, without printing anything secret', async () => {
  const { out, text } = recorder()
  const code = await run([...GOOD, '--apply'], { env: {}, fetchImpl: async () => jsonResponse({}), now: NOW, nonce: NONCE, out })
  assert.equal(code, 2)
  assert.match(text(), new RegExp(TOKEN_ENV))
})

test('apply: reads, then writes exactly one signed PATCH, and never prints the credential or a signature', async () => {
  const calls = []
  const { out, text } = recorder()
  const fetchImpl = async (url, init) => {
    calls.push({ url, method: init.method, headers: init.headers, body: init.body })
    if (init.method === 'GET') return jsonResponse(org('ZDR-TEST Terra proof', false))
    return jsonResponse(org('ZDR-TEST Terra proof', true))
  }
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })

  assert.equal(code, 0, text())
  assert.deepEqual(calls.map((call) => [call.method, call.url]), [
    ['GET', `http://127.0.0.1:18080${VECTOR.readUri}`],
    ['PATCH', `http://127.0.0.1:18080${VECTOR.patchUri}`],
  ])
  const patch = calls[1]
  assert.equal(patch.body, VECTOR.patchBody)
  assert.equal(patch.headers['Content-Type'], 'application/json')
  assert.equal(patch.headers['X-Service-Id'], 'verevon-gateway')
  assert.equal(patch.headers['X-User-Id'], VECTOR.userId)
  assert.equal(patch.headers['X-Org-Id'], VECTOR.orgId)
  assert.equal(patch.headers['X-Delegation-Timestamp'], VECTOR.timestamp)
  // Same clock, nonce, identity and credential as the pinned vector, so the whole
  // request the script sends is the one org-core's suite verifies.
  assert.equal(patch.headers['X-Delegation-Signature'], VECTOR.patchSignature)
  assert.equal(calls[0].headers['X-Delegation-Signature'], VECTOR.readSignature)
  // The two requests are distinct: org-core consumes each nonce once.
  assert.notEqual(calls[0].headers['X-Delegation-Nonce'], patch.headers['X-Delegation-Nonce'])
  assert.match(text(), /ZDR is OFF/)
  assert.match(text(), /ZDR is now ON/)
  for (const secret of [VECTOR.token, patch.headers['X-Delegation-Signature'], calls[0].headers['X-Delegation-Signature']]) {
    assert.ok(!text().includes(secret), 'the output carries a secret')
  }
})

test('apply with --zdr false sends the off body: the way back after the proof', async () => {
  const calls = []
  const { out, text } = recorder()
  const fetchImpl = async (url, init) => {
    calls.push({ method: init.method, body: init.body })
    return jsonResponse(org('ZDR-TEST Terra proof', init.method === 'GET'))
  }
  const code = await run([...GOOD, '--zdr', 'false', '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 0, text())
  assert.equal(calls[1].body, '{"zeroDataRetention":false}')
  assert.match(text(), /ZDR is now OFF/)
})

test('org-core refusing the write (a plan without ZDR) is reported and is a failure', async () => {
  const { out, text } = recorder()
  const fetchImpl = async (url, init) => init.method === 'GET'
    ? jsonResponse(org('ZDR-TEST Terra proof'))
    : jsonResponse({ error: 'plan_upgrade_required', message: 'Zero Data Retention is available on a higher plan.' }, 402)
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 1)
  assert.match(text(), /HTTP 402/)
  assert.match(text(), /plan_upgrade_required/)
})

test('a transport error carries no header and so no credential', async () => {
  const { out, text } = recorder()
  const fetchImpl = async (url, init) => { throw Object.assign(new TypeError(`fetch failed ${JSON.stringify(init.headers)}`), { cause: init.headers }) }
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 1)
  assert.ok(!text().includes(VECTOR.token))
  assert.ok(!text().includes('X-Service-Token'))
})

test('both requests refuse redirects, so the credential cannot be forwarded to another host', async () => {
  // fetch follows redirects by default and resends custom headers such as X-Service-Token to the
  // new origin; only authorization and cookie are dropped. Loopback-only means nothing if a
  // redirect can take the request elsewhere.
  const inits = []
  const { out } = recorder()
  const fetchImpl = async (url, init) => {
    inits.push(init)
    return jsonResponse(org('ZDR-TEST Terra proof', init.method === 'PATCH'))
  }
  const code = await run([...GOOD, '--apply'], { env: ENV, fetchImpl, now: NOW, nonce: NONCE, out })
  assert.equal(code, 0)
  assert.deepEqual(inits.map((init) => init.redirect), ['error', 'error'])
})

test('buildRequests signs the read and the write with different nonces under the same clock', () => {
  const requests = buildRequests({
    baseUrl: 'http://127.0.0.1:18080', orgId: VECTOR.orgId, userId: VECTOR.userId, userRole: 'owner',
    zdr: true, token: VECTOR.token, now: NOW, nonce: 'zdr-seed-nonce-0123456789ab',
  })
  assert.equal(requests.read.headers['X-Delegation-Nonce'], 'zdr-seed-nonce-0123456789ab-read')
  assert.equal(requests.patch.headers['X-Delegation-Nonce'], 'zdr-seed-nonce-0123456789ab-write')
  assert.equal(requests.patch.headers['X-Delegation-Timestamp'], VECTOR.timestamp)
})
