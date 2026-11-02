import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

// WS07-S3: the client contract is a number pair that three files must agree on. The gateway
// publishes it (`client_contract.rs`), the SPA compares against its own copy
// (`src/shared/build/client-contract.ts`) and the build writes the SPA's number into
// `version.json` (`vite.config.ts`). A drift here would make a fresh bundle look stale (or
// a stale one look fresh), so this test reads all three and fails when they disagree.

const here = dirname(fileURLToPath(import.meta.url))
const FRONTEND_ROOT = join(here, '..', '..', '..')

const read = (...parts) => readFileSync(join(FRONTEND_ROOT, ...parts), 'utf8')

/** The integer in `<keyword> NAME: u32 = N;` or `<keyword> NAME = N`. */
function constant(source, name) {
  const match = new RegExp(`\\b${name}\\b(?:\\s*:\\s*[A-Za-z0-9_]+)?\\s*=\\s*(\\d+)\\b`).exec(source)
  assert.ok(match, `${name} is not defined as an integer literal`)
  return Number(match[1])
}

const rust = read('apps', 'gateway', 'src', 'client_contract.rs')
const spa = read('src', 'shared', 'build', 'client-contract.ts')
const vite = read('vite.config.ts')

test('CLIENT_CONTRACT in client_contract.rs equals client-contract.ts', () => {
  assert.equal(constant(rust, 'CLIENT_CONTRACT'), constant(spa, 'CLIENT_CONTRACT'))
})

test('CLIENT_MIN_CONTRACT in client_contract.rs equals client-contract.ts', () => {
  assert.equal(constant(rust, 'CLIENT_MIN_CONTRACT'), constant(spa, 'CLIENT_MIN_CONTRACT'))
})

test('clientContract in vite.config.ts (written to version.json) equals CLIENT_CONTRACT', () => {
  assert.equal(constant(vite, 'clientContract'), constant(rust, 'CLIENT_CONTRACT'))
  assert.equal(constant(vite, 'clientContract'), constant(spa, 'CLIENT_CONTRACT'))
})

test('vite.config.ts writes clientContract into version.json as client_contract', () => {
  assert.match(vite, /client_contract:\s*clientContract/)
})

test('the minimum never exceeds the contract in either mirror', () => {
  assert.ok(constant(rust, 'CLIENT_MIN_CONTRACT') <= constant(rust, 'CLIENT_CONTRACT'))
  assert.ok(constant(spa, 'CLIENT_MIN_CONTRACT') <= constant(spa, 'CLIENT_CONTRACT'))
})

test('the constant reader rejects a missing constant', () => {
  assert.throws(() => constant('const OTHER = 1', 'CLIENT_CONTRACT'), /not defined/)
})
