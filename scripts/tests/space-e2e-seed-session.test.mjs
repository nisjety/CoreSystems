// Unit tests for scripts/space-e2e-seed-session.mjs (slice M0-T0): the pure parts only.
// The live proof (`bash scripts/space-e2e-seed.sh --apply` on the dev stack: six verified
// users, three orgs, six storage states that sign in) needs the running stack and is a
// later-tier test, see the skipped case at the bottom.
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
  SeedError,
  USERS,
  assertLoopback,
  emailFor,
  parseArgs,
  run,
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

  it('defaults to plan-only with the dev Postgres container', () => {
    const o = parseArgs(base)
    assert.equal(o.apply, false)
    assert.equal(o.ensureRoom, false)
    assert.equal(o.pgContainer, 'controlplane-postgres')
    assert.equal(o.baseUrl, 'http://localhost:5173')
  })

  it('reads the flags and valued options', () => {
    const o = parseArgs([...base, '--apply', '--ensure-room', '--pg-container', 'c', '--pg-user', 'u', '--pg-database', 'd'])
    assert.deepEqual([o.apply, o.ensureRoom, o.pgContainer, o.pgUser, o.pgDatabase], [true, true, 'c', 'u', 'd'])
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
    assert.doesNotMatch(text, /password/i)
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

describe('live seed proof', () => {
  it('scripts/space-e2e-seed.sh --apply on the dev stack: six verified users, orgs X/Y/Z, six storage states that sign in', { skip: 'needs the live dev stack (auth-core, org-core, gateway): later tier, run by someone other than the author' }, () => {})
})
