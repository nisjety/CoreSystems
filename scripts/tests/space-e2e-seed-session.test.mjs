// Unit tests for scripts/space-e2e-seed-session.mjs (slice M0-T0): the pure parts, the verified
// answer shapes, and the Space-role grant against a fake gateway (no network, no stack).
// The live proof (`bash scripts/space-e2e-seed.sh --apply` on the dev stack: six verified
// users, three orgs, the managed room with its roles, six storage states that sign in) needs
// the running stack and is a later-tier test, see the skipped case at the bottom.
// Run: node --test scripts/tests/space-e2e-seed-session.test.mjs
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'

import {
  CookieJar,
  ORG_X_NAME,
  ORG_Y_NAME,
  ORG_Z_NAME,
  ROOM_NAME,
  ROOM_ROLE_GRANTS,
  SeedError,
  USERS,
  assertLoopback,
  awaitRecipientReady,
  awaitRoomRegistration,
  createManagedRoom,
  emailFor,
  grantRoomRole,
  grantSpaceRoles,
  invitationIdOf,
  parseArgs,
  readRoomMembership,
  run,
  spaceRefOf,
} from '../space-e2e-seed-session.mjs'

describe('seed fixtures', () => {
  it('has exactly the six members A..F', () => {
    assert.deepEqual(USERS, ['A', 'B', 'C', 'D', 'E', 'F'])
  })

  it('derives a distinct, lower-case dev e-mail per member', () => {
    const emails = USERS.map(emailFor)
    assert.equal(new Set(emails).size, 6)
    assert.equal(emailFor('A'), 'space-e2e-a@verevon.dev')
    for (const e of emails) assert.match(e, /^space-e2e-[a-f]@verevon\.dev$/)
  })

  it('names org Z so the ZDR operator seed will accept it (ZDR-TEST prefix); X and Y are distinct', () => {
    assert.match(ORG_Z_NAME, /^ZDR-TEST/)
    assert.doesNotMatch(ORG_X_NAME, /ZDR/)
    assert.doesNotMatch(ORG_Y_NAME, /ZDR/)
    assert.notEqual(ORG_X_NAME, ORG_Y_NAME)
  })
})

describe('assertLoopback (the seed never talks to a non-local host)', () => {
  for (const ok of ['http://localhost:5173', 'http://127.0.0.1:4173', 'http://[::1]:5173', 'http://localhost']) {
    it(`accepts ${ok}`, () => {
      assert.equal(assertLoopback(ok, '--base-url').protocol, 'http:')
    })
  }
  for (const bad of [
    'https://localhost:5173',
    'http://example.com',
    'http://localhost.evil.test:5173',
    'http://127.0.0.2:5173',
    'http://10.0.0.5:5173',
    'http://app.verevon.dev',
    'ftp://localhost',
  ]) {
    it(`rejects ${bad}`, () => {
      assert.throws(() => assertLoopback(bad, '--base-url'), SeedError)
    })
  }
  it('rejects a non-URL and names the option', () => {
    assert.throws(() => assertLoopback('not a url', '--base-url'), /--base-url is not a URL/)
  })
})

describe('parseArgs', () => {
  const base = ['--base-url', 'http://localhost:5173', '--password-file', 'p.txt', '--out-dir', 'out']

  it('defaults to plan-only with the dev Postgres container and grants the Space roles', () => {
    const o = parseArgs(base)
    assert.equal(o.apply, false)
    assert.equal(o.ensureRoom, false)
    assert.equal(o.skipRoles, false)
    assert.equal(o.pgContainer, 'controlplane-postgres')
    assert.equal(o.baseUrl, 'http://localhost:5173')
  })

  it('reads the flags and valued options', () => {
    const o = parseArgs([...base, '--apply', '--ensure-room', '--skip-roles', '--pg-container', 'c', '--pg-user', 'u', '--pg-database', 'd'])
    assert.deepEqual([o.apply, o.ensureRoom, o.skipRoles, o.pgContainer, o.pgUser, o.pgDatabase], [true, true, true, 'c', 'u', 'd'])
  })

  it('rejects an unknown flag and names it', () => {
    assert.throws(() => parseArgs([...base, '--wat']), /unknown argument: --wat/)
  })

  it('requires --base-url, --password-file and --out-dir', () => {
    assert.throws(() => parseArgs(['--password-file', 'p', '--out-dir', 'o']), /missing --base-url/)
    assert.throws(() => parseArgs(['--base-url', 'http://localhost:1', '--out-dir', 'o']), /missing --password-file/)
    assert.throws(() => parseArgs(['--base-url', 'http://localhost:1', '--password-file', 'p']), /missing --out-dir/)
  })
})

describe('run', () => {
  it('without --apply prints the plan and sends nothing (returns null, reads no password file)', async () => {
    const lines = []
    const original = console.log
    console.log = (...a) => lines.push(a.join(' '))
    try {
      const result = await run({ apply: false, ensureRoom: true, baseUrl: 'http://localhost:5173', passwordFile: 'does-not-exist.txt', outDir: 'unused-out-dir' })
      assert.equal(result, null)
    } finally {
      console.log = original
    }
    const text = lines.join('\n')
    assert.match(text, /plan only/)
    assert.match(text, /org X/)
    assert.match(text, /org Y/)
    assert.match(text, /org Z/)
    assert.match(text, /ensures the organization room/)
    assert.match(text, /managed room "Space E2E Room" owned by A: B editor, C viewer, E no Space role/)
    assert.doesNotMatch(text, /password/i)
  })

  it('--skip-roles drops the managed-room line from the plan', async () => {
    const lines = []
    const original = console.log
    console.log = (...a) => lines.push(a.join(' '))
    try {
      await run({ apply: false, skipRoles: true, baseUrl: 'http://localhost:5173', passwordFile: 'x', outDir: 'unused' })
    } finally {
      console.log = original
    }
    assert.doesNotMatch(lines.join('\n'), /managed room/)
  })

  it('refuses a non-loopback base URL before doing anything', async () => {
    await assert.rejects(run({ apply: true, baseUrl: 'http://example.com', passwordFile: 'x', outDir: 'x' }), SeedError)
  })

  it('--apply with an empty password file is refused', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'seed-test-'))
    try {
      const file = join(dir, 'pw')
      writeFileSync(file, '  \n')
      await assert.rejects(run({ apply: true, baseUrl: 'http://localhost:5173', passwordFile: file, outDir: join(dir, 'out') }), /password file is empty/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

describe('CookieJar (becomes the Playwright storage state)', () => {
  const response = (...lines) => ({ headers: { getSetCookie: () => lines } })

  it('absorbs Set-Cookie attributes into a storage-state cookie', () => {
    const jar = new CookieJar('localhost')
    jar.absorb(response('sid=abc=def; Path=/api; HttpOnly; Secure; SameSite=Strict; Max-Age=3600'))
    const [c] = jar.storageState().cookies
    assert.equal(c.name, 'sid')
    assert.equal(c.value, 'abc=def')
    assert.equal(c.domain, 'localhost')
    assert.equal(c.path, '/api')
    assert.equal(c.httpOnly, true)
    assert.equal(c.secure, true)
    assert.equal(c.sameSite, 'Strict')
    assert.ok(c.expires > Date.now() / 1000)
    assert.deepEqual(jar.storageState().origins, [])
  })

  it('builds a Cookie header, lets a later cookie replace an earlier one and drops expired ones', () => {
    const jar = new CookieJar('localhost')
    jar.absorb(response('a=1', 'b=2'))
    jar.absorb(response('a=3'))
    assert.equal(jar.header(), 'a=3; b=2')
    jar.absorb(response('b=; Max-Age=0'))
    assert.equal(jar.header(), 'a=3')
  })

  it('ignores a malformed cookie line and a response without getSetCookie', () => {
    const jar = new CookieJar('localhost')
    jar.absorb(response('=novalue', 'noequals'))
    jar.absorb({ headers: {} })
    assert.equal(jar.header(), '')
  })
})

// ---- verified answer shapes and the Space-role grant, against a fake gateway (no network) -------

/** A session stand-in: `handler(method, path, body)` answers each call, and every call is recorded. */
function fakeSession(handler) {
  const calls = []
  return {
    calls,
    async call(method, path, body) {
      calls.push({ method, path, body })
      return handler(method, path, body)
    },
  }
}

const noSleep = async () => {}
const ok = (json, status = 200) => ({ status, json })
const refuse = (status, code) => ({ status, json: { error: { code, message: code } } })
const MEMBERSHIP = /^\/api\/v1\/spaces\/([^/]+)\/membership$/
const MEMBERS = /^\/api\/v1\/spaces\/([^/]+)\/members(\/[^/]+)?$/

describe('verified answer shapes', () => {
  it("reads the invitation id from auth-core's invite answer", () => {
    assert.equal(invitationIdOf({ success: true, invitation: { id: 'inv_1', email: 'b@x.dev', role: 'member', expiresAt: 'z' } }), 'inv_1')
  })

  it('has no invitation id for a duplicate invite (the gateway answers invitation_created false)', () => {
    assert.equal(invitationIdOf({ data: { email: 'b@x.dev', role: 'member', invitation_created: false } }), undefined)
  })

  it('reads the Space ref from data.space.space_ref, the spelling the room answers use', () => {
    assert.equal(spaceRefOf({ data: { space: { space_ref: 'room-1', kind: 'room' } } }), 'room-1')
    assert.equal(spaceRefOf({ data: { space_ref: 'room-1' } }), undefined)
  })
})

describe('managed room and Space roles (WS04-S6 membership routes)', () => {
  it('creates a named room with the owner and returns its ref', async () => {
    const owner = { session: fakeSession(() => ok({ data: { space: { space_ref: 'room-1' } } }, 202)) }
    assert.equal(await createManagedRoom(owner, ROOM_NAME), 'room-1')
    assert.deepEqual(owner.session.calls, [{ method: 'POST', path: '/api/v1/spaces', body: { kind: 'room', name: ROOM_NAME } }])
  })

  it('names the refusal code when the room cannot be created', async () => {
    const owner = { session: fakeSession(() => refuse(409, 'organization_required')) }
    await assert.rejects(createManagedRoom(owner, ROOM_NAME), (e) => e instanceof SeedError && /organization_required/.test(e.message))
  })

  it('waits for Control to register the room, then the owner reads active', async () => {
    let reads = 0
    const owner = { session: fakeSession(() => (reads++ < 2 ? refuse(404, 'room_not_found') : ok({ data: { role: 'owner' } }))) }
    const sleeps = []
    await awaitRoomRegistration(owner, 'room-1', { attempts: 5, delayMs: 7, sleep: async (ms) => sleeps.push(ms) })
    assert.equal(owner.session.calls.length, 3)
    assert.deepEqual(sleeps, [7, 7])
  })

  it('gives up with a clear message when the room never becomes active', async () => {
    const owner = { session: fakeSession(() => refuse(404, 'room_not_found')) }
    await assert.rejects(
      awaitRoomRegistration(owner, 'room-1', { attempts: 3, delayMs: 1, sleep: noSleep }),
      (e) => e instanceof SeedError && /not active in Control after 3 checks/.test(e.message),
    )
  })

  it('grants a role by adding with it, and sends no PATCH when the answer already shows that role', async () => {
    const owner = { session: fakeSession(() => ok({ data: { role: 'editor', changed: true } })) }
    await grantRoomRole(owner, 'room-1', 'user-b', 'editor', 'user B')
    assert.deepEqual(owner.session.calls, [{ method: 'POST', path: '/api/v1/spaces/room-1/members', body: { subject_id: 'user-b', role: 'editor' } }])
  })

  it('changes a role the subject already holds with PATCH', async () => {
    const owner = {
      session: fakeSession((method) => (method === 'POST' ? ok({ data: { role: 'viewer', changed: false } }) : ok({ data: { role: 'editor' } }))),
    }
    await grantRoomRole(owner, 'room-1', 'user-b', 'editor', 'user B')
    assert.deepEqual(owner.session.calls, [
      { method: 'POST', path: '/api/v1/spaces/room-1/members', body: { subject_id: 'user-b', role: 'editor' } },
      { method: 'PATCH', path: '/api/v1/spaces/room-1/members/user-b', body: { role: 'editor' } },
    ])
  })

  it('names the typed refusal of the membership route (an organization room refuses member writes)', async () => {
    const owner = { session: fakeSession(() => refuse(409, 'room_roster_derived')) }
    await assert.rejects(grantRoomRole(owner, 'room-1', 'user-b', 'editor', 'user B'), (e) => e instanceof SeedError && /room_roster_derived/.test(e.message))
  })

  it("reads the caller's own role and whether the audience includes them", async () => {
    assert.deepEqual(await readRoomMembership(fakeSession(() => ok({ data: { role: 'viewer', recipient_ready: true } })), 'room-1'), {
      status: 200,
      role: 'viewer',
      recipientReady: true,
    })
    assert.deepEqual(await readRoomMembership(fakeSession(() => refuse(404, 'room_not_found')), 'room-1'), {
      status: 404,
      role: null,
      recipientReady: false,
    })
  })

  it('reports an audience that is still not ready after a bounded wait, without failing', async () => {
    const session = fakeSession(() => ok({ data: { role: 'editor', recipient_ready: false } }))
    assert.equal(await awaitRecipientReady(session, 'room-1', { attempts: 3, delayMs: 1, sleep: noSleep }), false)
    assert.equal(session.calls.length, 3)
  })

  describe('grantSpaceRoles', () => {
    /** Owner A's gateway answers the room routes; B and C read the role they were granted; E reads nothing. */
    function world({ bRole = 'editor', cRole = 'viewer', ready = true, eReads = false, registered = true, oldRoomOwned = false } = {}) {
      const owner = fakeSession((method, path, body) => {
        if (method === 'POST' && path === '/api/v1/spaces') return ok({ data: { space: { space_ref: 'room-1' } } }, 202)
        const read = MEMBERSHIP.exec(path)
        if (method === 'GET' && read) {
          const ref = read[1]
          if ((ref === 'room-1' && registered) || (ref === 'old-room' && oldRoomOwned)) return ok({ data: { role: 'owner' } })
          return refuse(404, 'room_not_found')
        }
        if (method === 'POST' && MEMBERS.test(path)) return ok({ data: { role: body.role, changed: true, member_count: 3 } })
        if (method === 'PATCH' && MEMBERS.test(path)) return ok({ data: { role: body.role, changed: true } })
        return refuse(500, `unexpected ${method} ${path}`)
      })
      const memberSession = (role) =>
        fakeSession((method, path) => (method === 'GET' && MEMBERSHIP.test(path) ? ok({ data: { role, recipient_ready: ready } }) : refuse(404, 'room_not_found')))
      const members = {
        B: { userId: 'user-b', session: memberSession(bRole) },
        C: { userId: 'user-c', session: memberSession(cRole) },
        E: { userId: 'user-e', session: eReads ? memberSession('viewer') : fakeSession(() => refuse(404, 'room_not_found')) },
      }
      return { owner: { session: owner }, members }
    }

    it('creates the room, adds B as editor and C as viewer, and checks each one through the membership read', async () => {
      const w = world()
      const logs = []
      const result = await grantSpaceRoles({ owner: w.owner, members: w.members, sleep: noSleep, log: (line) => logs.push(line) })
      assert.equal(result.space_ref, 'room-1')
      assert.deepEqual(result.roles, {
        B: { role: 'editor', recipient_ready: true },
        C: { role: 'viewer', recipient_ready: true },
        E: { role: 'none' },
      })
      const writes = w.owner.session.calls.filter((c) => c.method !== 'GET')
      assert.deepEqual(writes, [
        { method: 'POST', path: '/api/v1/spaces', body: { kind: 'room', name: ROOM_NAME } },
        { method: 'POST', path: '/api/v1/spaces/room-1/members', body: { subject_id: 'user-b', role: ROOM_ROLE_GRANTS.B } },
        { method: 'POST', path: '/api/v1/spaces/room-1/members', body: { subject_id: 'user-c', role: ROOM_ROLE_GRANTS.C } },
      ])
      assert.equal(w.owner.session.calls.some((c) => c.path.includes('organization-room')), false)
      assert.equal(logs.length, 1)
    })

    it('reuses the room an earlier run recorded while A still owns it', async () => {
      const w = world({ oldRoomOwned: true })
      const result = await grantSpaceRoles({ owner: w.owner, members: w.members, existingRef: 'old-room', sleep: noSleep })
      assert.equal(result.space_ref, 'old-room')
      assert.equal(w.owner.session.calls.some((c) => c.method === 'POST' && c.path === '/api/v1/spaces'), false)
    })

    it('creates a new room when the recorded one is no longer owned by A', async () => {
      const w = world({ oldRoomOwned: false })
      const result = await grantSpaceRoles({ owner: w.owner, members: w.members, existingRef: 'old-room', sleep: noSleep })
      assert.equal(result.space_ref, 'room-1')
      assert.equal(w.owner.session.calls.some((c) => c.method === 'POST' && c.path === '/api/v1/spaces'), true)
    })

    it('refuses when a member reads a different role than the one granted', async () => {
      const w = world({ cRole: 'editor' })
      await assert.rejects(
        grantSpaceRoles({ owner: w.owner, members: w.members, sleep: noSleep }),
        (e) => e instanceof SeedError && /user C reads role "editor" in the room, expected "viewer"/.test(e.message),
      )
    })

    it('refuses when E can read the room, since E must hold no Space role', async () => {
      const w = world({ eReads: true })
      await assert.rejects(
        grantSpaceRoles({ owner: w.owner, members: w.members, sleep: noSleep }),
        (e) => e instanceof SeedError && /user E can read the room/.test(e.message),
      )
    })

    it('waits for registration before the first membership write', async () => {
      const w = world({ registered: false })
      await assert.rejects(
        grantSpaceRoles({ owner: w.owner, members: w.members, sleep: noSleep }),
        (e) => e instanceof SeedError && /not active in Control/.test(e.message),
      )
      assert.equal(w.owner.session.calls.some((c) => c.method === 'POST' && c.path.endsWith('/members')), false)
    })

    it('reports a late audience in the log and the summary, without failing the seed', async () => {
      const w = world({ ready: false })
      const logs = []
      const result = await grantSpaceRoles({ owner: w.owner, members: w.members, sleep: noSleep, log: (line) => logs.push(line) })
      assert.deepEqual(result.roles.B, { role: 'editor', recipient_ready: false })
      assert.match(logs.join('\n'), /user B is not in the room audience yet/)
    })
  })
})

describe('live seed proof', () => {
  it('scripts/space-e2e-seed.sh --apply on the dev stack: six verified users, orgs X/Y/Z, six storage states that sign in', { skip: 'needs the live dev stack (auth-core, org-core, gateway): later tier, run by someone other than the author' }, () => {})
})
