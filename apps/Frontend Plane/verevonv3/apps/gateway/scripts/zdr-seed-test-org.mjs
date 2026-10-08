#!/usr/bin/env node
// Operator seed: switch Zero Data Retention on (or off) for ONE dedicated test
// organization, straight in org-core.
//
// Why this exists. The gateway refuses to turn org ZDR on while no AI provider is
// attested (409 zdr_no_attested_provider, apps/gateway/src/domains/orgs/settings.rs).
// That is right for every real organization: with nothing attested every
// interactive request would be refused. But the live proof of the ZDR work needs
// ONE test org that has the switch on while nothing is attested yet. org-core
// itself carries no such gate (it cannot read the model catalogue), so a direct
// PATCH is the operator-only way in. Runbook: docs/ZDR_OPERATOR_SEED_RUNBOOK.md.
//
// What it will not do:
//   * touch any org whose name does not start with "ZDR-TEST" and equal the name
//     you typed (--expect-name). It reads the org first and refuses on a mismatch;
//   * send anything unless --apply is given (the default prints the plan and exits);
//   * talk to anything but a loopback address: the credential never leaves the host;
//   * print the credential or a signature, in output or in an error.
//
// The credential. org-core grants `org:settings:write:self` to exactly one
// principal, verevon-gateway, and only :self scopes (which need a signed delegation
// naming a real member of the org) are valid for that principal. So the "operator
// credential" IS the gateway's org-core credential, supplied in ORG_CORE_SERVICE_TOKEN.
// This script signs the same v3 delegation the gateway signs
// (apps/gateway/src/upstream.rs control_service_delegation_headers), which org-core
// verifies in internal/http/service_auth.go (serviceDelegationSignature). A pinned
// vector in org-core's internal/http/zdr_seed_delegation_vector_test.go keeps the
// two from drifting.

import { createHash, createHmac, randomBytes } from 'node:crypto'
import { pathToFileURL } from 'node:url'

export const SERVICE_PRINCIPAL = 'verevon-gateway'
export const SERVICE_AUDIENCE = 'org-core'
export const TOKEN_ENV = 'ORG_CORE_SERVICE_TOKEN'
export const DEFAULT_BASE_URL = 'http://127.0.0.1:18080'
export const TEST_ORG_NAME_PREFIX = 'ZDR-TEST'

const IDENTIFIER = /^[A-Za-z0-9._:-]{1,128}$/
const USER_ROLES = new Set(['owner', 'admin'])

const base64url = (buffer) => buffer.toString('base64url')

/** org-core's serviceDelegationBodyDigest: base64url (no padding) of the body's SHA-256. */
export function bodyDigest(body) {
  return base64url(createHash('sha256').update(body).digest())
}

/**
 * org-core's serviceDelegationSignature: HMAC-SHA256, keyed with the credential,
 * over the eleven fields below joined by a newline, base64url without padding.
 */
export function delegationSignature(token, claims) {
  const canonical = [
    'v3',
    claims.principal,
    claims.audience,
    claims.timestamp,
    claims.nonce,
    claims.method,
    claims.uri,
    claims.userId,
    claims.orgId,
    claims.userRole,
    claims.bodySha256,
  ].join('\n')
  return base64url(createHmac('sha256', token).update(canonical).digest())
}

/** RFC 3339 to the second, as org-core parses it (no fractional part). */
export function delegationTimestamp(now) {
  return now.toISOString().replace(/\.\d{3}Z$/, 'Z')
}

/** The headers org-core's serviceAuthMiddleware needs for a `:self` scope. */
export function signedHeaders({ token, method, uri, body, userId, orgId, userRole, timestamp, nonce }) {
  const digest = bodyDigest(body ?? '')
  const signature = delegationSignature(token, {
    principal: SERVICE_PRINCIPAL,
    audience: SERVICE_AUDIENCE,
    timestamp,
    nonce,
    method,
    uri,
    userId,
    orgId,
    userRole,
    bodySha256: digest,
  })
  return {
    'X-Service-Id': SERVICE_PRINCIPAL,
    'X-Service-Token': token,
    'X-User-Id': userId,
    'X-Org-Id': orgId,
    'X-User-Role': userRole,
    'X-Delegation-Version': 'v3',
    'X-Delegation-Timestamp': timestamp,
    'X-Delegation-Nonce': nonce,
    'X-Delegation-Body-SHA256': digest,
    'X-Delegation-Signature': signature,
  }
}

export class SeedRefusal extends Error {
  constructor(message, exitCode = 2) {
    super(message)
    this.name = 'SeedRefusal'
    this.exitCode = exitCode
  }
}

export function isTestOrgName(name) {
  return typeof name === 'string' && name.trim().toUpperCase().startsWith(TEST_ORG_NAME_PREFIX)
}

function assertIdentifier(label, value) {
  if (typeof value !== 'string' || !IDENTIFIER.test(value)) {
    throw new SeedRefusal(`${label} is required and may contain only letters, digits and . _ : -`)
  }
}

/** The credential only ever goes to this host's own loopback. */
export function assertLoopbackBaseUrl(baseUrl) {
  let parsed
  try {
    parsed = new URL(baseUrl)
  } catch {
    throw new SeedRefusal(`--base-url is not a URL: ${baseUrl}`)
  }
  const host = parsed.hostname.replace(/^\[|\]$/g, '')
  if (parsed.protocol !== 'http:' || !['127.0.0.1', 'localhost', '::1'].includes(host)) {
    throw new SeedRefusal(
      `--base-url must be http on a loopback address (127.0.0.1, localhost, ::1): the credential must not leave this host. Got ${parsed.protocol}//${parsed.hostname}`,
    )
  }
  return parsed.origin
}

export function parseArgs(argv) {
  const options = { apply: false, zdr: 'true', userRole: 'owner', baseUrl: DEFAULT_BASE_URL }
  const valued = {
    '--org-id': 'orgId',
    '--user-id': 'userId',
    '--user-role': 'userRole',
    '--expect-name': 'expectName',
    '--zdr': 'zdr',
    '--base-url': 'baseUrl',
  }
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index]
    if (flag === '--apply') {
      options.apply = true
    } else if (flag in valued) {
      const value = argv[index + 1]
      if (value === undefined || value.startsWith('--')) throw new SeedRefusal(`${flag} needs a value`)
      options[valued[flag]] = value
      index += 1
    } else {
      throw new SeedRefusal(`unknown argument ${flag}`)
    }
  }
  return options
}

/** Validate the arguments into what the requests are built from. */
export function validateOptions(options) {
  assertIdentifier('--org-id', options.orgId)
  assertIdentifier('--user-id', options.userId)
  if (!USER_ROLES.has(options.userRole)) throw new SeedRefusal('--user-role must be owner or admin')
  if (options.zdr !== 'true' && options.zdr !== 'false') throw new SeedRefusal('--zdr must be true or false')
  if (!isTestOrgName(options.expectName)) {
    throw new SeedRefusal(
      `--expect-name is required and must be the exact name of a dedicated test org, which starts with "${TEST_ORG_NAME_PREFIX}". This script never touches any other org.`,
    )
  }
  return {
    orgId: options.orgId,
    userId: options.userId,
    userRole: options.userRole,
    zdr: options.zdr === 'true',
    expectName: options.expectName,
    baseUrl: assertLoopbackBaseUrl(options.baseUrl),
    apply: options.apply === true,
  }
}

/** The read and the write, signed. Pure: the clock, the nonce and the token are arguments. */
export function buildRequests({ baseUrl, orgId, userId, userRole, zdr, token, now, nonce }) {
  const timestamp = delegationTimestamp(now)
  const orgPath = `/api/v1/organizations/${encodeURIComponent(orgId)}`
  const patchBody = JSON.stringify({ zeroDataRetention: zdr })
  const common = { token, userId, orgId, userRole, timestamp }
  return {
    read: {
      method: 'GET',
      url: `${baseUrl}${orgPath}`,
      headers: signedHeaders({ ...common, method: 'GET', uri: orgPath, body: '', nonce: `${nonce}-read` }),
    },
    patch: {
      method: 'PATCH',
      url: `${baseUrl}${orgPath}/settings`,
      headers: {
        ...signedHeaders({ ...common, method: 'PATCH', uri: `${orgPath}/settings`, body: patchBody, nonce: `${nonce}-write` }),
        'Content-Type': 'application/json',
      },
      body: patchBody,
    },
  }
}

const unwrap = (payload) => (payload && typeof payload === 'object' && 'data' in payload ? payload.data : payload)

async function readJson(response) {
  try {
    return await response.json()
  } catch {
    return null
  }
}

function describeFailure(label, response, payload) {
  const reported = payload && typeof payload === 'object' ? payload.error ?? payload.message : undefined
  const text = typeof reported === 'string' ? reported : reported?.code ?? reported?.message
  return `${label} failed: HTTP ${response.status}${text ? ` (${text})` : ''}`
}

/**
 * Run the seed. `env`, `fetchImpl`, `now`, `nonce` and `out` are arguments so a test can
 * drive every branch without a network or a real credential.
 * Returns the process exit code: 0 done or planned, 1 org-core refused, 2 refused here.
 */
export async function run(argv, { env = process.env, fetchImpl = fetch, now = new Date(), nonce = randomBytes(18).toString('base64url'), out = console.log } = {}) {
  try {
    const options = validateOptions(parseArgs(argv))
    const switchWord = options.zdr ? 'ON' : 'OFF'

    if (!options.apply) {
      out(`PLAN only, nothing is sent. Add --apply to run it.`)
      out(`  org           ${options.orgId} (must be named exactly: ${options.expectName})`)
      out(`  acting as     ${options.userId} (${options.userRole}), a member of that org`)
      out(`  1. GET   ${options.baseUrl}/api/v1/organizations/${options.orgId}`)
      out(`     refuses unless the org's name equals the name above and starts with ${TEST_ORG_NAME_PREFIX}`)
      out(`  2. PATCH ${options.baseUrl}/api/v1/organizations/${options.orgId}/settings {"zeroDataRetention":${options.zdr}}`)
      out(`  Zero Data Retention would be switched ${switchWord} for that org only.`)
      out(`  The credential is read from ${TOKEN_ENV} and is never printed.`)
      return 0
    }

    const token = env[TOKEN_ENV]
    if (typeof token !== 'string' || token.length < 32) {
      throw new SeedRefusal(`${TOKEN_ENV} must hold the verevon-gateway org-core credential (at least 32 characters)`)
    }
    const requests = buildRequests({ ...options, token, now, nonce })

    // `redirect: 'error'` on both requests: fetch would otherwise follow a redirect to another
    // host and resend X-Service-Token there (only authorization/cookie are dropped
    // cross-origin), which would defeat the loopback-only rule above.
    const readResponse = await fetchImpl(requests.read.url, { method: 'GET', headers: requests.read.headers, redirect: 'error' })
    const readPayload = await readJson(readResponse)
    if (!readResponse.ok) {
      out(describeFailure('reading the org', readResponse, readPayload))
      return 1
    }
    const org = unwrap(readPayload) ?? {}
    if (org.id !== undefined && org.id !== options.orgId) {
      throw new SeedRefusal(`org-core answered for a different org (${String(org.id)}); nothing was changed`)
    }
    if (org.name !== options.expectName || !isTestOrgName(org.name)) {
      throw new SeedRefusal(
        `refused: org ${options.orgId} is named ${JSON.stringify(org.name)}, not ${JSON.stringify(options.expectName)} (or not a ${TEST_ORG_NAME_PREFIX} org). Nothing was changed.`,
      )
    }
    const current = org.metadata?.interactiveRetention?.zdr === true ? 'ON' : 'OFF'
    out(`org ${options.orgId} ${JSON.stringify(org.name)}, plan ${org.plan ?? 'unknown'}, ZDR is ${current}`)

    const patchResponse = await fetchImpl(requests.patch.url, {
      method: 'PATCH',
      headers: requests.patch.headers,
      body: requests.patch.body,
      redirect: 'error',
    })
    const patchPayload = await readJson(patchResponse)
    if (!patchResponse.ok) {
      out(describeFailure('switching ZDR', patchResponse, patchPayload))
      return 1
    }
    const after = unwrap(patchPayload)?.metadata?.interactiveRetention?.zdr
    out(`ZDR is now ${after === true ? 'ON' : after === false ? 'OFF' : 'not reported'} for org ${options.orgId}`)
    return after === options.zdr ? 0 : 1
  } catch (error) {
    if (error instanceof SeedRefusal) {
      out(error.message)
      return error.exitCode
    }
    // Never let a transport error carry a header (it would hold the credential).
    out(`failed: ${error instanceof Error ? error.name : 'unknown error'} (no detail printed on purpose)`)
    return 1
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  run(process.argv.slice(2)).then((code) => process.exit(code))
}
