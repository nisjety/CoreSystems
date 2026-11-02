#!/usr/bin/env node
// Space E2E seed, gateway half (slice M0-T0). Driven by scripts/space-e2e-seed.sh.
//
// Seeded accounts must already exist and be email-verified (the shell half does
// that through apps/Control Plane/scripts/seed-verified-test-user.sh). This half
// talks to the same-origin gateway like a browser does:
//
//   org X  : A (owner), B, C, E   -- B/C/E are plain org members; Space roles
//                                    (editor, viewer, none) are granted by the
//                                    Space membership routes, not here
//   org Y  : D (owner)            -- a different org: cross-org negative cases
//   org Z  : F (owner)            -- named "ZDR-TEST ..." so the ZDR operator seed
//                                    (apps/Frontend Plane/verevonv3/apps/gateway/
//                                    scripts/zdr-seed-test-org.mjs) accepts it
//
// and writes one Playwright storage state per user (E2E_STORAGE_STATE_A..F) plus
// a secret-free summary (space-e2e-seed.json) into the output directory, which is
// git-ignored (tests/e2e/.auth/).
//
// Safety: loopback only; the shared password is read from a file, never from argv,
// and is never printed; nothing is sent without --apply.
//
// No AI model is selected, requested or entitled here. Space specs that need a
// model run must name a non-Verevon one explicitly (see tests/e2e/space-fixtures.ts).

import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

export const USERS = ['A', 'B', 'C', 'D', 'E', 'F']
export const ORG_X_NAME = 'Space E2E Org X'
export const ORG_Y_NAME = 'Space E2E Org Y'
export const ORG_Z_NAME = 'ZDR-TEST space e2e'
const IDENTIFIER = /^[A-Za-z0-9._:-]{1,128}$/

export const emailFor = (key) => `space-e2e-${key.toLowerCase()}@verevon.dev`

export class SeedError extends Error {}

export function assertLoopback(raw, label) {
  let parsed
  try {
    parsed = new URL(raw)
  } catch {
    throw new SeedError(`${label} is not a URL: ${raw}`)
  }
  const host = parsed.hostname.replace(/^\[|\]$/g, '')
  if (parsed.protocol !== 'http:' || !['127.0.0.1', 'localhost', '::1'].includes(host)) {
    throw new SeedError(`${label} must be http on loopback (127.0.0.1, localhost, ::1); got ${parsed.origin}`)
  }
  return parsed
}

/** Minimal cookie jar: name -> parsed Set-Cookie record, enough for a Playwright storage state. */
export class CookieJar {
  constructor(domain) {
    this.domain = domain
    this.cookies = new Map()
  }
  absorb(response) {
    const lines = typeof response.headers.getSetCookie === 'function' ? response.headers.getSetCookie() : []
    for (const line of lines) {
      const [pair, ...attrs] = line.split(';').map((part) => part.trim())
      const eq = pair.indexOf('=')
      if (eq < 1) continue
      const cookie = {
        name: pair.slice(0, eq),
        value: pair.slice(eq + 1),
        domain: this.domain,
        path: '/',
        expires: -1,
        httpOnly: false,
        secure: false,
        sameSite: 'Lax',
      }
      for (const attr of attrs) {
        const [rawKey, ...rest] = attr.split('=')
        const key = rawKey.toLowerCase()
        const val = rest.join('=')
        if (key === 'path' && val) cookie.path = val
        else if (key === 'httponly') cookie.httpOnly = true
        else if (key === 'secure') cookie.secure = true
        else if (key === 'samesite') cookie.sameSite = { strict: 'Strict', none: 'None' }[val.toLowerCase()] ?? 'Lax'
        else if (key === 'max-age' && /^-?\d+$/.test(val)) cookie.expires = Math.floor(Date.now() / 1000) + Number(val)
        else if (key === 'expires' && !Number.isNaN(Date.parse(val)) && cookie.expires === -1) {
          cookie.expires = Math.floor(Date.parse(val) / 1000)
        }
      }
      if (cookie.expires !== -1 && cookie.expires <= Math.floor(Date.now() / 1000)) this.cookies.delete(cookie.name)
      else this.cookies.set(cookie.name, cookie)
    }
  }
  header() {
    return [...this.cookies.values()].map((c) => `${c.name}=${c.value}`).join('; ')
  }
  storageState() {
    return { cookies: [...this.cookies.values()], origins: [] }
  }
}

export class Session {
  constructor(origin, domain) {
    this.origin = origin
    this.jar = new CookieJar(domain)
  }
  async call(method, path, body) {
    const headers = { origin: this.origin, accept: 'application/json' }
    if (body !== undefined) headers['content-type'] = 'application/json'
    const cookie = this.jar.header()
    if (cookie) headers.cookie = cookie
    const response = await fetch(`${this.origin}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      redirect: 'manual',
    })
    this.jar.absorb(response)
    const text = await response.text()
    let json = null
    try {
      json = text ? JSON.parse(text) : null
    } catch {
      json = null
    }
    return { status: response.status, json }
  }
}

const pick = (value, ...paths) => {
  for (const path of paths) {
    let cursor = value
    for (const key of path.split('.')) cursor = cursor?.[key]
    if (typeof cursor === 'string' && cursor) return cursor
  }
  return undefined
}

async function waitForGateway(session) {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    try {
      if ((await session.call('GET', '/health')).status === 200) return
    } catch {
      // the listener can open before the proxy answers
    }
    await new Promise((resolve) => setTimeout(resolve, 1000))
  }
  throw new SeedError('the same-origin gateway did not answer /health with 200')
}

async function signIn(origin, domain, key, password) {
  const session = new Session(origin, domain)
  await waitForGateway(session)
  const res = await session.call('POST', '/api/v1/auth/sign-in', { email: emailFor(key), password })
  if (res.status < 200 || res.status >= 300) throw new SeedError(`sign-in failed for user ${key}: ${res.status}`)
  const me = await session.call('GET', '/api/v1/me')
  if (me.status !== 200) throw new SeedError(`/api/v1/me for user ${key} returned ${me.status}`)
  const userId = pick(me.json, 'data.id', 'data.userId', 'data.user.id', 'id', 'userId')
  return { session, userId }
}

async function currentOrg(session) {
  const res = await session.call('GET', '/api/v1/session/current')
  if (res.status !== 200) throw new SeedError(`/api/v1/session/current returned ${res.status}`)
  return {
    orgId: res.json?.data?.org?.id ?? null,
    onboardingStatus: res.json?.data?.onboardingStatus ?? null,
  }
}

async function ensureOwnedOrg(session, key, name) {
  const created = await session.call('POST', '/api/v1/onboarding/actions/create-organization', { name, plan: 'trial' })
  if (![200, 201].includes(created.status)) {
    const duplicate = created.status === 400 && created.json?.error?.message === 'Organization already exists'
    if (!duplicate) throw new SeedError(`create-organization for user ${key} returned ${created.status}`)
  }
  const { orgId } = await currentOrg(session)
  if (!orgId) throw new SeedError(`user ${key} has no active organization after create-organization`)
  return orgId
}

async function completeOnboarding(session, key, orgId) {
  const { onboardingStatus } = await currentOrg(session)
  if (onboardingStatus === 'COMPLETED') return
  const done = await session.call('POST', '/api/v1/onboarding/complete', { orgId, plan: 'trial', source: 'space-e2e-seed' })
  if (done.status !== 200) throw new SeedError(`onboarding/complete for user ${key} returned ${done.status}`)
}

function pendingInvitationId(pg, orgId, email) {
  if (!IDENTIFIER.test(orgId)) throw new SeedError('organization id has unexpected characters')
  const sql = `SELECT id FROM invitation WHERE organization_id = '${orgId}' AND lower(email) = '${email.toLowerCase()}' AND status = 'pending' LIMIT 1`
  const out = execFileSync('docker', ['exec', pg.container, 'psql', '-U', pg.user, '-d', pg.database, '-tA', '-c', sql], {
    encoding: 'utf8',
  }).trim()
  return out || undefined
}

async function joinOrg(origin, domain, key, password, owner, orgId, pg) {
  const email = emailFor(key)
  const invite = await owner.session.call('POST', `/api/v1/orgs/${encodeURIComponent(orgId)}/members/invite`, {
    email,
    role: 'member',
  })
  if (![200, 201].includes(invite.status)) throw new SeedError(`invite of user ${key} returned ${invite.status}`)
  const member = await signIn(origin, domain, key, password)
  const before = await currentOrg(member.session)
  if (before.orgId !== orgId) {
    const invitationId =
      pick(invite.json, 'data.id', 'data.invitation.id', 'data.invitationId', 'id') ?? pendingInvitationId(pg, orgId, email)
    // Re-run after a partial seed: no pending invitation and already a member is fine.
    if (invitationId) {
      const accepted = await member.session.call('POST', `/api/v1/orgs/invitations/${encodeURIComponent(invitationId)}/accept`)
      if (accepted.status < 200 || accepted.status >= 300) throw new SeedError(`accept for user ${key} returned ${accepted.status}`)
    }
    const switched = await member.session.call('POST', '/api/v1/orgs/switch-active', { organizationId: orgId })
    if (switched.status < 200 || switched.status >= 300) throw new SeedError(`switch-active for user ${key} returned ${switched.status}`)
  }
  const after = await currentOrg(member.session)
  if (after.orgId !== orgId) throw new SeedError(`user ${key} is not in the intended org after joining`)
  await completeOnboarding(member.session, key, orgId)
  return member
}

function writeState(outDir, key, member) {
  const file = join(outDir, `space-e2e-state-${key}.json`)
  writeFileSync(file, JSON.stringify(member.session.jar.storageState(), null, 2), { mode: 0o600 })
  return file
}

export function parseArgs(argv) {
  const options = { apply: false, ensureRoom: false, pgContainer: 'controlplane-postgres', pgUser: 'coresystem', pgDatabase: 'auth_service' }
  const valued = {
    '--base-url': 'baseUrl',
    '--password-file': 'passwordFile',
    '--out-dir': 'outDir',
    '--pg-container': 'pgContainer',
    '--pg-user': 'pgUser',
    '--pg-database': 'pgDatabase',
  }
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i]
    if (flag === '--apply') options.apply = true
    else if (flag === '--ensure-room') options.ensureRoom = true
    else if (valued[flag]) {
      options[valued[flag]] = argv[i + 1]
      i += 1
    } else throw new SeedError(`unknown argument: ${flag}`)
  }
  for (const required of ['baseUrl', 'passwordFile', 'outDir']) {
    if (!options[required]) throw new SeedError(`missing --${required.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`)
  }
  return options
}

export async function run(options) {
  const parsed = assertLoopback(options.baseUrl, '--base-url')
  const origin = parsed.origin
  const domain = parsed.hostname.replace(/^\[|\]$/g, '')
  const plan = [
    `gateway ${origin}`,
    `org X "${ORG_X_NAME}": A owner; B, C, E invited as members and accepted`,
    `org Y "${ORG_Y_NAME}": D owner`,
    `org Z "${ORG_Z_NAME}": F owner (ZDR switch is a separate operator step)`,
    `storage states and space-e2e-seed.json -> ${options.outDir}`,
  ]
  if (options.ensureRoom) plan.push('A ensures the organization room and its space_ref is recorded')
  if (!options.apply) {
    console.log(`plan only (pass --apply to send):\n  ${plan.join('\n  ')}`)
    return null
  }
  const password = readFileSync(options.passwordFile, 'utf8').trim()
  if (!password) throw new SeedError('the password file is empty')
  mkdirSync(options.outDir, { recursive: true })
  const pg = { container: options.pgContainer, user: options.pgUser, database: options.pgDatabase }

  const summary = { gateway_origin: origin, users: {}, orgs: {}, states: {}, space_ref: null }
  const owners = {}
  for (const [key, orgKey, name] of [['A', 'X', ORG_X_NAME], ['D', 'Y', ORG_Y_NAME], ['F', 'Z', ORG_Z_NAME]]) {
    const owner = await signIn(origin, domain, key, password)
    const orgId = await ensureOwnedOrg(owner.session, key, name)
    await completeOnboarding(owner.session, key, orgId)
    owners[key] = owner
    summary.orgs[orgKey] = { id: orgId, name }
    summary.users[key] = { email: emailFor(key), user_id: owner.userId ?? null, org: orgKey, role: 'owner' }
    summary.states[key] = writeState(options.outDir, key, owner)
  }
  for (const key of ['B', 'C', 'E']) {
    const member = await joinOrg(origin, domain, key, password, owners.A, summary.orgs.X.id, pg)
    summary.users[key] = { email: emailFor(key), user_id: member.userId ?? null, org: 'X', role: 'member' }
    summary.states[key] = writeState(options.outDir, key, member)
  }
  if (options.ensureRoom) {
    const room = await owners.A.session.call('POST', '/api/v1/spaces/organization-room', {})
    // UNVERIFIED response shape: tolerate the three spellings the gateway has used.
    const ref = pick(room.json, 'data.space_ref', 'data.spaceRef', 'data.space.space_ref')
    if (ref) summary.space_ref = ref
    else console.warn(`organization-room returned ${room.status} without a space_ref: set E2E_SPACE_REF by hand`)
  }
  writeFileSync(join(options.outDir, 'space-e2e-seed.json'), JSON.stringify(summary, null, 2))
  return summary
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  let outDir = ''
  try {
    const options = parseArgs(process.argv.slice(2))
    outDir = options.outDir
    const summary = await run(options)
    if (summary) console.log(`seeded users ${USERS.join(', ')}; orgs ${Object.keys(summary.orgs).join(', ')}; summary in ${join(outDir, 'space-e2e-seed.json')}`)
  } catch (error) {
    console.error(`space-e2e-seed-session: ${error.message}`)
    process.exit(error instanceof SeedError ? 2 : 1)
  }
}
