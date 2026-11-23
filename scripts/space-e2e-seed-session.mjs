#!/usr/bin/env node
// Space E2E seed, gateway half (slice M0-T0). Driven by scripts/space-e2e-seed.sh.
//
// Seeded accounts must already exist and be email-verified (the shell half does
// that through apps/Control Plane/scripts/seed-verified-test-user.sh). This half
// talks to the same-origin gateway like a browser does:
//
//   org X  : A (owner), B, C, E   -- B/C/E are plain org members. A then creates the
//                                    managed room "Space E2E Room" and, through the Space
//                                    membership routes (WS04-S6), makes B its editor and C its
//                                    viewer; E holds no Space role. The organization room is not
//                                    used: its roster follows the organization and Control refuses
//                                    member writes there (room_roster_derived), so it cannot carry
//                                    the roles. A also owns "Space E2E Canvas Room", which has no
//                                    canvas yet (space_ref_no_canvas).
//   org Y  : D (owner)            -- a different org: cross-org negative cases
//   org Z  : F (owner)            -- named "ZDR-TEST ..." so the ZDR operator seed
//                                    (apps/Frontend Plane/verevonv3/apps/gateway/
//                                    scripts/zdr-seed-test-org.mjs) accepts it; F owns
//                                    "Space E2E ZDR Room" (space_ref_zdr)
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
  const userId = userIdOf(me.json)
  if (!userId) throw new SeedError(`/api/v1/me for user ${key} answered without user.id`)
  return { session, userId }
}

async function currentOrg(session) {
  const res = await session.call('GET', '/api/v1/session/current')
  if (res.status !== 200) throw new SeedError(`/api/v1/session/current returned ${res.status}`)
  return {
    orgId: res.json?.data?.org?.id ?? null,
    orgName: res.json?.data?.org?.name ?? null,
    onboardingStatus: res.json?.data?.onboardingStatus ?? null,
  }
}

const succeeded = (res) => res.status >= 200 && res.status < 300
const errorCode = (res) => (res.json?.error?.code ? ` (${res.json.error.code})` : '')

async function ensureOwnedOrg(session, key, name) {
  // The gateway answers 200 with the caller's existing owned organization (it dedupes) and 201 for a new one.
  // A 409 organization_conflict is another owner's organization with the same name, so it is not reused.
  const created = await session.call('POST', '/api/v1/onboarding/actions/create-organization', { name, plan: 'trial' })
  if (![200, 201].includes(created.status)) {
    throw new SeedError(`create-organization for user ${key} returned ${created.status}${errorCode(created)}`)
  }
  return confirmOwnedOrg(session, key, name)
}

/**
 * The caller's active organization must be the one named `name`. The gateway reports an empty org name when its
 * lookup fails, so an empty name is read again a few times and is never taken as a match.
 */
export async function confirmOwnedOrg(session, key, name, { attempts = 5, delayMs = 1000, sleep = pause } = {}) {
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    const { orgId, orgName } = await currentOrg(session)
    if (!orgId) throw new SeedError(`user ${key} has no active organization after create-organization`)
    if (orgName === name) return orgId
    if (orgName) {
      throw new SeedError(`user ${key} is in organization "${orgName}", not "${name}": reset the dev accounts before seeding`)
    }
    if (attempt < attempts) await sleep(delayMs)
  }
  throw new SeedError(
    `the gateway did not report the name of user ${key}'s organization after ${attempts} reads, so it cannot be confirmed as "${name}"`,
  )
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
    const invitationId = invitationIdOf(invite.json) ?? pendingInvitationId(pg, orgId, email)
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

// ---- Space roles (WS04-S6 membership routes) ------------------------------------------------

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const encoded = (value) => encodeURIComponent(value)

/** User id of a GET /api/v1/me answer: user-core answers `{ user: { id, ... } }` and the gateway relays it. */
export const userIdOf = (json) => pick(json, 'user.id', 'data.user.id', 'data.id', 'data.userId', 'id', 'userId')

/** Invitation id of a committed invite: auth-core answers `{ success, invitation: { id, email, role, expiresAt } }`. */
export const invitationIdOf = (json) => pick(json, 'invitation.id', 'data.invitation.id')

/** Space ref of a room or organization-room answer: `202 { data: { space: { space_ref, ... } } }`. */
export const spaceRefOf = (json) => pick(json, 'data.space.space_ref')

export const ROOM_NAME = 'Space E2E Room'
/** A room in org X that has no canvas yet (E2E_SPACE_REF_NO_CANVAS, the space-canvas.spec.ts create path). */
export const CANVAS_ROOM_NAME = 'Space E2E Canvas Room'
/** A room in the ZDR org Z, owned by F (E2E_SPACE_REF_ZDR, the space-realtime and space-sidebar specs). */
export const ZDR_ROOM_NAME = 'Space E2E ZDR Room'
/** The Space roles the seed grants in ROOM_NAME. E is deliberately absent: E keeps no Space role. */
export const ROOM_ROLE_GRANTS = Object.freeze({ B: 'editor', C: 'viewer' })

/** A (the owner) creates a managed room. Its creator becomes the registration owner. */
export async function createManagedRoom(owner, name) {
  const res = await owner.session.call('POST', '/api/v1/spaces', { kind: 'room', name })
  if (!succeeded(res)) throw new SeedError(`creating room "${name}" returned ${res.status}${errorCode(res)}`)
  const ref = spaceRefOf(res.json)
  if (!ref) throw new SeedError(`creating room "${name}" answered without data.space.space_ref`)
  return ref
}

/**
 * Control registers a new room asynchronously (Convex `pending_registration`, then the registration worker).
 * Control refuses member writes until the room is active, and the owner's membership read is the proof.
 */
export async function awaitRoomRegistration(owner, spaceRef, { attempts = 90, delayMs = 2000, sleep = pause } = {}) {
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    const res = await owner.session.call('GET', `/api/v1/spaces/${encoded(spaceRef)}/membership`)
    if (res.status === 200 && pick(res.json, 'data.role') === 'owner') return
    if (attempt < attempts) await sleep(delayMs)
  }
  throw new SeedError(
    `room ${spaceRef} is not active in Control after ${attempts} checks; the Space registration worker must drain it, then re-run the seed`,
  )
}

/**
 * Makes `role` the subject's Space role. Adding carries the role; a subject who already holds another role
 * (a re-run) is changed with PATCH.
 */
export async function grantRoomRole(
  owner,
  spaceRef,
  subjectId,
  role,
  label = 'member',
  { attempts = 15, delayMs = 2000, sleep = pause } = {},
) {
  const membersPath = `/api/v1/spaces/${encoded(spaceRef)}/members`
  let added
  // Control's org-membership row for a new member is written asynchronously (OrganizationMemberAdded), so a
  // fresh member can read member_not_in_organization for a short while. Only that refusal is retried.
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    added = await owner.session.call('POST', membersPath, { subject_id: subjectId, role })
    if (succeeded(added) || added.json?.error?.code !== 'member_not_in_organization' || attempt === attempts) break
    await sleep(delayMs)
  }
  if (!succeeded(added)) throw new SeedError(`adding ${label} as ${role} returned ${added.status}${errorCode(added)}`)
  if (pick(added.json, 'data.role') === role) return
  const changed = await owner.session.call('PATCH', `${membersPath}/${encoded(subjectId)}`, { role })
  if (!succeeded(changed)) {
    throw new SeedError(`changing ${label} to ${role} returned ${changed.status}${errorCode(changed)}`)
  }
}

/**
 * The caller's own view of a room: the HTTP status, the role, whether the audience includes them, and Control's
 * roster mode ('managed' or 'org_derived') and registration state ('active' or 'archived'); null unless 200.
 */
export async function readRoomMembership(session, spaceRef) {
  const res = await session.call('GET', `/api/v1/spaces/${encoded(spaceRef)}/membership`)
  const ok = res.status === 200
  return {
    status: res.status,
    role: ok ? (pick(res.json, 'data.role') ?? null) : null,
    recipientReady: ok && res.json?.data?.recipient_ready === true,
    rosterMode: ok ? (pick(res.json, 'data.roster_mode') ?? null) : null,
    state: ok ? (pick(res.json, 'data.state') ?? null) : null,
  }
}

/**
 * A recorded room is reused only while the caller owns it, its roster is managed (an organization room refuses
 * member writes with room_roster_derived) and it is active (an archived room refuses them with room_archived).
 */
export const isReusableRoom = (membership) =>
  membership.status === 200 && membership.role === 'owner' && membership.rosterMode === 'managed' && membership.state === 'active'

/**
 * Reuses `existingRef` while it is still a reusable room of `owner`, else creates a managed room named `name`.
 * `onCreated(ref)` runs right after a create, so the caller records the ref before anything else can fail.
 * `keep(ref)` may refuse an otherwise reusable room (one that has a canvas by now, say).
 */
export async function ensureManagedRoom(owner, { existingRef = null, name, onCreated = () => {}, keep = async () => true, log = () => {} }) {
  if (existingRef && isReusableRoom(await readRoomMembership(owner.session, existingRef)) && (await keep(existingRef))) {
    return { ref: existingRef, created: false }
  }
  const ref = await createManagedRoom(owner, name)
  await onCreated(ref)
  log(`room "${name}" created; waiting for Control to register it`)
  return { ref, created: true }
}

/** False only when the canvas read answers 200 with a canvas: a room that has one is no create-path fixture. */
export async function roomHasNoCanvas(session, spaceRef) {
  const res = await session.call('GET', `/api/v1/spaces/${encoded(spaceRef)}/canvas`)
  if (res.status !== 200) return true
  return res.json?.data?.canvas == null
}

/**
 * A fixture room that only has to exist and be active (no roles): reuse or create it, then wait for Control.
 * Not fatal: a failure is logged and the ref comes back null, so the specs that need it skip (or fail at a gate).
 */
export async function ensureFixtureRoom(owner, { existingRef = null, name, keep, onCreated, sleep = pause, log = () => {} }) {
  try {
    const { ref } = await ensureManagedRoom(owner, { existingRef, name, keep, onCreated, log })
    await awaitRoomRegistration(owner, ref, { sleep })
    return ref
  } catch (error) {
    log(`room "${name}" is not ready: ${error.message}`)
    return null
  }
}

/** Bounded wait for the member's audience. Not fatal: a late audience is healed by the reconcile cron. */
export async function awaitRecipientReady(session, spaceRef, { attempts = 15, delayMs = 2000, sleep = pause } = {}) {
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    if ((await readRoomMembership(session, spaceRef)).recipientReady) return true
    if (attempt < attempts) await sleep(delayMs)
  }
  return false
}

/**
 * Creates (or reuses, while Control still shows A as its owner) ROOM_NAME and grants ROOM_ROLE_GRANTS through
 * the membership routes. The organization room is never used: its roster follows the organization.
 */
export async function grantSpaceRoles({
  owner,
  members,
  existingRef = null,
  roomName = ROOM_NAME,
  onCreated = () => {},
  sleep = pause,
  log = () => {},
}) {
  for (const key of Object.keys(ROOM_ROLE_GRANTS)) {
    // Checked before any write, so a missing id never leaves a new room behind.
    if (!members[key]?.userId) throw new SeedError(`user ${key} has no user id, so the membership route cannot name them`)
  }
  const { ref } = await ensureManagedRoom(owner, { existingRef, name: roomName, onCreated, log })
  await awaitRoomRegistration(owner, ref, { sleep })
  const roles = {}
  for (const [key, role] of Object.entries(ROOM_ROLE_GRANTS)) {
    const member = members[key]
    await grantRoomRole(owner, ref, member.userId, role, `user ${key}`, { sleep })
    const seen = await readRoomMembership(member.session, ref)
    if (seen.role !== role) throw new SeedError(`user ${key} reads role "${seen.role}" in the room, expected "${role}"`)
    const ready = await awaitRecipientReady(member.session, ref, { sleep })
    if (!ready) log(`user ${key} is not in the room audience yet; the reconcile cron will catch up`)
    roles[key] = { role, recipient_ready: ready }
  }
  if (members.E) {
    const outsider = await readRoomMembership(members.E.session, ref)
    if (outsider.status === 200) throw new SeedError('user E can read the room but must hold no Space role')
  }
  roles.E = { role: 'none' }
  return { space_ref: ref, roles }
}

/** The summary an earlier run wrote, or {}: a missing or unreadable summary just means no reuse. */
export function previousSummary(file) {
  try {
    const parsed = JSON.parse(readFileSync(file, 'utf8'))
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

export function parseArgs(argv) {
  const options = {
    apply: false,
    ensureRoom: false,
    skipRoles: false,
    pgContainer: 'controlplane-postgres',
    pgUser: 'coresystem',
    pgDatabase: 'auth_service',
  }
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
    else if (flag === '--skip-roles') options.skipRoles = true
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
  if (options.ensureRoom) plan.push('A ensures the organization room (recorded as organization_room_ref)')
  if (!options.skipRoles) {
    plan.push(`managed room "${ROOM_NAME}" owned by A: B editor, C viewer, E no Space role (Space membership routes)`)
    plan.push(`room "${CANVAS_ROOM_NAME}" owned by A with no canvas yet (space_ref_no_canvas)`)
    plan.push(`room "${ZDR_ROOM_NAME}" owned by F in org Z (space_ref_zdr)`)
  }
  if (!options.apply) {
    console.log(`plan only (pass --apply to send):\n  ${plan.join('\n  ')}`)
    return null
  }
  const password = readFileSync(options.passwordFile, 'utf8').trim()
  if (!password) throw new SeedError('the password file is empty')
  mkdirSync(options.outDir, { recursive: true })
  const pg = { container: options.pgContainer, user: options.pgUser, database: options.pgDatabase }

  // space_ref is the managed room that carries the Space roles (E2E_SPACE_REF); organization_room_ref is the
  // organization's own room, which holds no roles the seed can grant. space_ref_no_canvas and space_ref_zdr are
  // the canvas-create and ZDR fixture rooms. Room refs start from the previous run, and the summary is written as
  // soon as a room is created, so a failed run never leaves a room the next run cannot find again.
  const summaryFile = join(options.outDir, 'space-e2e-seed.json')
  const previous = previousSummary(summaryFile)
  const carried = (key) => (typeof previous[key] === 'string' && previous[key] ? previous[key] : null)
  const summary = {
    gateway_origin: origin,
    users: {},
    orgs: {},
    states: {},
    space_ref: carried('space_ref'),
    space_ref_no_canvas: carried('space_ref_no_canvas'),
    space_ref_zdr: carried('space_ref_zdr'),
    fixture_rooms_ready: null,
    organization_room_ref: null,
    room: null,
    roles: null,
  }
  const saveSummary = () => writeFileSync(summaryFile, JSON.stringify(summary, null, 2))
  const recordRoom = (key) => (ref) => {
    summary[key] = ref
    saveSummary()
  }
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
  const members = {}
  for (const key of ['B', 'C', 'E']) {
    const member = await joinOrg(origin, domain, key, password, owners.A, summary.orgs.X.id, pg)
    members[key] = member
    summary.users[key] = { email: emailFor(key), user_id: member.userId ?? null, org: 'X', role: 'member' }
    summary.states[key] = writeState(options.outDir, key, member)
  }
  if (options.ensureRoom) {
    const room = await owners.A.session.call('POST', '/api/v1/spaces/organization-room', {})
    if (!succeeded(room)) throw new SeedError(`organization-room returned ${room.status}${errorCode(room)}`)
    summary.organization_room_ref = spaceRefOf(room.json) ?? null
    if (!summary.organization_room_ref) throw new SeedError('organization-room answered without data.space.space_ref')
  }
  if (!options.skipRoles) {
    const log = (line) => console.log(`    ${line}`)
    const granted = await grantSpaceRoles({
      owner: owners.A,
      members,
      existingRef: summary.space_ref,
      onCreated: recordRoom('space_ref'),
      log,
    })
    summary.space_ref = granted.space_ref
    summary.room = { name: ROOM_NAME, owner: 'A' }
    summary.roles = granted.roles
    // A fixture room that is not ready keeps its recorded ref (the next run reuses it once Control registers
    // it), and fixture_rooms_ready says whether the shell may export it.
    summary.fixture_rooms_ready = { no_canvas: false, zdr: false }
    saveSummary()
    const noCanvas = await ensureFixtureRoom(owners.A, {
      existingRef: summary.space_ref_no_canvas,
      name: CANVAS_ROOM_NAME,
      keep: (ref) => roomHasNoCanvas(owners.A.session, ref),
      onCreated: recordRoom('space_ref_no_canvas'),
      log,
    })
    if (noCanvas) Object.assign(summary, { space_ref_no_canvas: noCanvas })
    summary.fixture_rooms_ready.no_canvas = Boolean(noCanvas)
    saveSummary()
    const zdr = await ensureFixtureRoom(owners.F, {
      existingRef: summary.space_ref_zdr,
      name: ZDR_ROOM_NAME,
      onCreated: recordRoom('space_ref_zdr'),
      log,
    })
    if (zdr) Object.assign(summary, { space_ref_zdr: zdr })
    summary.fixture_rooms_ready.zdr = Boolean(zdr)
  }
  saveSummary()
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
