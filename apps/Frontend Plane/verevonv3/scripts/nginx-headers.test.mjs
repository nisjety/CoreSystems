import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

// WS07-S6: `nginx-security-headers.conf` carries the ENFORCED Content-Security-Policy and a
// Report-Only candidate. The candidate is the enforced policy with `https:` removed from
// connect-src and img-src, plus a report-uri to the gateway sink. These tests pin that
// relationship so editing one header without the other fails here.

const here = dirname(fileURLToPath(import.meta.url))
const conf = readFileSync(join(here, '..', 'nginx-security-headers.conf'), 'utf8')

/** The quoted value of `add_header <name> "<value>" always;` (comment lines are skipped). */
function header(name) {
  const lines = conf
    .split(/\r?\n/)
    .filter((line) => !line.trimStart().startsWith('#'))
    .filter((line) => new RegExp(`^add_header ${name} "`).test(line))
  assert.equal(lines.length, 1, `expected exactly one ${name} header, found ${lines.length}`)
  const match = /^add_header \S+ "(.*)" always;\s*$/.exec(lines[0])
  assert.ok(match, `${name} is not in the form add_header ${name} "..." always;`)
  return match[1]
}

/** `{ directive: 'sources...' }` in order, from a policy string. */
function directives(policy) {
  const out = new Map()
  for (const part of policy.split(';')) {
    const text = part.trim()
    if (text === '') continue
    const [name, ...sources] = text.split(/\s+/)
    assert.ok(!out.has(name), `duplicate directive ${name}`)
    out.set(name, sources.join(' '))
  }
  return out
}

const enforced = header('Content-Security-Policy')
const reportOnly = header('Content-Security-Policy-Report-Only')
const enforcedDirectives = directives(enforced)
const reportDirectives = directives(reportOnly)

test('the snippet carries one enforced and one report-only CSP header', () => {
  assert.ok(enforced.length > 0)
  assert.ok(reportOnly.length > 0)
  assert.notEqual(enforced, reportOnly)
})

test('the enforced header is unchanged: https: stays in connect-src and img-src', () => {
  assert.equal(enforcedDirectives.get('connect-src'), "'self' https:")
  assert.equal(enforcedDirectives.get('img-src'), "'self' data: https: blob:")
  assert.equal(enforcedDirectives.get('default-src'), "'self'")
  assert.equal(enforcedDirectives.get('frame-ancestors'), "'none'")
  assert.equal(enforcedDirectives.get('object-src'), "'none'")
  assert.ok(!enforcedDirectives.has('report-uri'), 'the enforced header must not report')
  assert.ok(!enforcedDirectives.has('report-to'))
})

test("the candidate narrows connect-src to 'self' and drops https: from img-src", () => {
  assert.equal(reportDirectives.get('connect-src'), "'self'")
  assert.equal(reportDirectives.get('img-src'), "'self' data: blob:")
  assert.ok(!/https:(?!\/\/)/.test(reportDirectives.get('connect-src')))
  assert.ok(!/https:(?!\/\/)/.test(reportDirectives.get('img-src')))
})

test('the candidate ends with report-uri to the gateway csp-report sink', () => {
  const names = [...reportDirectives.keys()]
  assert.equal(names.at(-1), 'report-uri')
  assert.equal(reportDirectives.get('report-uri'), '/api/v1/client-diagnostics/csp-report')
  assert.ok(reportOnly.trimEnd().endsWith('report-uri /api/v1/client-diagnostics/csp-report'))
})

test('every other directive is identical to the enforced header', () => {
  const differing = new Set(['connect-src', 'img-src', 'report-uri'])
  const rest = (map) => [...map].filter(([name]) => !differing.has(name))
  assert.deepEqual(rest(reportDirectives), rest(enforcedDirectives))
  // Same directive set apart from the report-uri the candidate adds.
  assert.deepEqual(
    [...reportDirectives.keys()].filter((name) => name !== 'report-uri'),
    [...enforcedDirectives.keys()],
  )
})

test('the report-only header is emitted on error responses too', () => {
  const line = conf
    .split(/\r?\n/)
    .find((candidate) => candidate.startsWith('add_header Content-Security-Policy-Report-Only '))
  assert.ok(line?.endsWith(' always;'))
})
