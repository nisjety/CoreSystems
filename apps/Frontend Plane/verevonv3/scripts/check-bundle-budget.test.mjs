import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { appendFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'

import {
  TOLERANCE_BYTES,
  checkCeilings,
  entryScriptSrc,
  measure,
  parseBudget,
  relaxations,
} from './check-bundle-budget.mjs'

const SCRIPT = fileURLToPath(new URL('./check-bundle-budget.mjs', import.meta.url))
const ENTRY_BYTES = 40_000
const LAZY_BYTES = 20_000

/** Printable bytes that gzip poorly, so a compressed size tracks the size of its input. */
function noise(length, seed) {
  const bytes = Buffer.alloc(length)
  let state = seed
  for (let i = 0; i < length; i += 1) {
    state = (Math.imul(state, 1103515245) + 12345) >>> 0
    bytes[i] = 32 + ((state >>> 16) % 95)
  }
  return bytes
}

function gzipLength(bytes) {
  return gzipSync(bytes, { level: 9 }).length
}

function tempDir(t) {
  const dir = mkdtempSync(path.join(tmpdir(), 'bundle-budget-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  return dir
}

/** A dist/ laid out like Vite's output: a classic script, a preload, the entry module, a lazy chunk and a stylesheet. */
function writeDist(root) {
  const dist = path.join(root, 'dist')
  mkdirSync(path.join(dist, 'assets'), { recursive: true })
  writeFileSync(
    path.join(dist, 'index.html'),
    [
      '<script src="/classic.js"></script>',
      '<link rel="modulepreload" crossorigin href="/assets/index-test.js">',
      '<script type="module" crossorigin src="/assets/index-test.js"></script>',
    ].join('\n'),
  )
  writeFileSync(path.join(dist, 'assets', 'index-test.js'), noise(ENTRY_BYTES, 1))
  writeFileSync(path.join(dist, 'assets', 'AuthPage-test.js'), noise(LAZY_BYTES, 2))
  writeFileSync(path.join(dist, 'assets', 'index-test.css'), 'body{margin:0}')
  return dist
}

function budgetFor(measured) {
  return { entryGzipBytes: measured.entryGzipBytes, totalJsGzipBytes: measured.totalJsGzipBytes }
}

function jsonOf(budget) {
  return `${JSON.stringify(budget, null, 2)}\n`
}

function readBudget(file) {
  return JSON.parse(readFileSync(file, 'utf8'))
}

function runCli(args, cwd) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { cwd, encoding: 'utf8' })
}

/** A git repo whose only tree holds `files`, so --base-ref can name it without a commit. */
function gitTree(repo, files) {
  mkdirSync(repo, { recursive: true })
  const git = (args, input) => execFileSync('git', args, { cwd: repo, input, encoding: 'utf8' }).trim()
  git(['init', '-q'])
  const entries = Object.entries(files).map(
    ([name, text]) => `100644 blob ${git(['hash-object', '-w', '--stdin'], text)}\t${name}\n`,
  )
  return git(['mktree'], entries.join(''))
}

test('entryScriptSrc takes the module script and skips classic scripts and preloads', () => {
  assert.equal(
    entryScriptSrc(
      '<script src="/a.js"></script><link rel="modulepreload" href="/assets/b.js">' +
        '<script type="module" crossorigin src="/assets/index-1.js"></script>',
    ),
    '/assets/index-1.js',
  )
  assert.equal(entryScriptSrc('<p>no script</p>'), undefined)
})

test('measure budgets the entry chunk and every JS file, and leaves CSS out', (t) => {
  const measured = measure(writeDist(tempDir(t)))
  const entryGzip = gzipLength(noise(ENTRY_BYTES, 1))
  assert.equal(measured.entry, 'assets/index-test.js')
  assert.equal(measured.entryGzipBytes, entryGzip)
  assert.equal(measured.jsFileCount, 2)
  assert.equal(measured.totalJsGzipBytes, entryGzip + gzipLength(noise(LAZY_BYTES, 2)))
})

test('measure refuses a dist without a build or without a module entry', (t) => {
  const root = tempDir(t)
  assert.throws(() => measure(path.join(root, 'dist')), /run the production build first/)
  const dist = writeDist(root)
  writeFileSync(path.join(dist, 'index.html'), '<script src="/classic.js"></script>')
  assert.throws(() => measure(dist), /has no module script/)
})

test('a 200 KB dependency in the entry is over the recorded ceiling', (t) => {
  const dist = writeDist(tempDir(t))
  const budget = budgetFor(measure(dist))
  appendFileSync(path.join(dist, 'assets', 'index-test.js'), noise(200 * 1024, 3))
  const { problems } = checkCeilings(measure(dist), budget)
  assert.ok(problems.some((problem) => problem.startsWith('entryGzipBytes is')), problems.join('\n'))
})

test('growth inside the tolerance passes and growth past it fails', (t) => {
  const measured = measure(writeDist(tempDir(t)))
  const { entryGzipBytes, totalJsGzipBytes } = measured
  assert.deepEqual(checkCeilings(measured, budgetFor(measured)).problems, [])
  const atEdge = { entryGzipBytes: entryGzipBytes - TOLERANCE_BYTES, totalJsGzipBytes }
  assert.deepEqual(checkCeilings(measured, atEdge).problems, [])
  const past = { entryGzipBytes: entryGzipBytes - TOLERANCE_BYTES - 1, totalJsGzipBytes }
  assert.match(checkCeilings(measured, past).problems.join('\n'), /entryGzipBytes is \d+ bytes, \d+ over its ceiling/)
})

test('a shrink past the tolerance must lower the ceiling', (t) => {
  const measured = measure(writeDist(tempDir(t)))
  const { entryGzipBytes, totalJsGzipBytes } = measured
  const within = { entryGzipBytes: entryGzipBytes + TOLERANCE_BYTES, totalJsGzipBytes }
  assert.deepEqual(checkCeilings(measured, within).problems, [])
  const loose = { entryGzipBytes: entryGzipBytes + TOLERANCE_BYTES + 1, totalJsGzipBytes }
  assert.match(checkCeilings(measured, loose).problems.join('\n'), /under its ceiling .* lower the ceiling with --update/)
})

test('a null ceiling is reported as unrecorded and enforces nothing', (t) => {
  const measured = measure(writeDist(tempDir(t)))
  assert.deepEqual(checkCeilings(measured, { entryGzipBytes: null, totalJsGzipBytes: null }), {
    problems: [],
    unrecorded: ['entryGzipBytes', 'totalJsGzipBytes'],
  })
})

test('parseBudget accepts whole bytes or null and rejects anything else', () => {
  assert.deepEqual(parseBudget('{"entryGzipBytes":1,"totalJsGzipBytes":null}'), {
    entryGzipBytes: 1,
    totalJsGzipBytes: null,
  })
  assert.throws(() => parseBudget('{"entryGzipBytes":1.5,"totalJsGzipBytes":null}'), /whole number of bytes/)
  assert.throws(() => parseBudget('{"entryGzipBytes":null}'), /totalJsGzipBytes must be/)
})

test('a change may lower a ceiling the base records, but not raise or unset it', () => {
  const base = { entryGzipBytes: 1000, totalJsGzipBytes: 5000 }
  assert.deepEqual(relaxations({ entryGzipBytes: 900, totalJsGzipBytes: 5000 }, base), [])
  assert.deepEqual(relaxations({ entryGzipBytes: 1001, totalJsGzipBytes: 5000 }, base), [
    'entryGzipBytes raises the base ceiling of 1000 to 1001',
  ])
  assert.deepEqual(relaxations({ entryGzipBytes: null, totalJsGzipBytes: 5000 }, base), [
    'entryGzipBytes was 1000 on the base and is now unrecorded',
  ])
  assert.deepEqual(relaxations({ entryGzipBytes: 1, totalJsGzipBytes: 1 }, { entryGzipBytes: null, totalJsGzipBytes: null }), [])
})

test('the CLI passes inside the budget, fails over it and warns while a ceiling is unrecorded', (t) => {
  const root = tempDir(t)
  const dist = writeDist(root)
  const measured = measure(dist)
  const budgetFile = path.join(root, 'bundle-budget.json')
  const args = ['--dist', dist, '--budget', budgetFile]

  writeFileSync(budgetFile, jsonOf({ entryGzipBytes: null, totalJsGzipBytes: null }))
  const unrecorded = runCli(args, root)
  assert.equal(unrecorded.status, 0)
  assert.match(unrecorded.stderr, /entryGzipBytes has no recorded ceiling/)

  writeFileSync(budgetFile, jsonOf(budgetFor(measured)))
  const passed = runCli(args, root)
  assert.equal(passed.status, 0)
  assert.match(passed.stdout, /bundle budget passed/)

  writeFileSync(budgetFile, jsonOf({ ...budgetFor(measured), entryGzipBytes: measured.entryGzipBytes - TOLERANCE_BYTES - 1 }))
  const over = runCli(args, root)
  assert.equal(over.status, 1)
  assert.match(over.stderr, /over its ceiling/)
})

test('--update records a first measurement, lowers a ceiling and refuses to raise one', (t) => {
  const root = tempDir(t)
  const dist = writeDist(root)
  const measured = measure(dist)
  const budgetFile = path.join(root, 'bundle-budget.json')
  const args = ['--update', '--dist', dist, '--budget', budgetFile]

  writeFileSync(budgetFile, jsonOf({ entryGzipBytes: null, totalJsGzipBytes: measured.totalJsGzipBytes + 10_000 }))
  assert.equal(runCli(args, root).status, 0)
  assert.deepEqual(readBudget(budgetFile), budgetFor(measured))

  writeFileSync(budgetFile, jsonOf({ ...budgetFor(measured), entryGzipBytes: measured.entryGzipBytes - TOLERANCE_BYTES - 1 }))
  const refused = runCli(args, root)
  assert.equal(refused.status, 1)
  assert.match(refused.stderr, /--update only lowers ceilings/)
  assert.equal(readBudget(budgetFile).entryGzipBytes, measured.entryGzipBytes - TOLERANCE_BYTES - 1)

  writeFileSync(budgetFile, jsonOf({ ...budgetFor(measured), entryGzipBytes: measured.entryGzipBytes - 100 }))
  assert.equal(runCli(args, root).status, 0)
  assert.equal(readBudget(budgetFile).entryGzipBytes, measured.entryGzipBytes - 100)
})

test('--base-ref refuses a ceiling above the base, accepts one below it and skips a base with no budget', (t) => {
  const root = tempDir(t)
  const dist = writeDist(root)
  const measured = measure(dist)
  const repo = path.join(root, 'repo')
  const budgetFile = path.join(repo, 'bundle-budget.json')
  mkdirSync(repo, { recursive: true })
  writeFileSync(budgetFile, jsonOf(budgetFor(measured)))
  const args = ['--dist', dist, '--budget', budgetFile]

  const tightBase = gitTree(repo, {
    'bundle-budget.json': jsonOf({ ...budgetFor(measured), entryGzipBytes: measured.entryGzipBytes - 100 }),
  })
  const raised = runCli([...args, '--base-ref', tightBase], root)
  assert.equal(raised.status, 1)
  assert.match(raised.stderr, /entryGzipBytes raises the base ceiling of \d+ to \d+/)

  const looseBase = gitTree(repo, {
    'bundle-budget.json': jsonOf({ ...budgetFor(measured), entryGzipBytes: measured.entryGzipBytes + 1000 }),
  })
  assert.equal(runCli([...args, '--base-ref', looseBase], root).status, 0)

  const predatesBudget = gitTree(repo, { 'README.md': 'no budget file on this base\n' })
  assert.equal(runCli([...args, '--base-ref', predatesBudget], root).status, 0)

  const unknown = runCli([...args, '--base-ref', '0123456789abcdef0123456789abcdef01234567'], root)
  assert.equal(unknown.status, 2)
  assert.match(unknown.stderr, /cannot resolve --base-ref/)
})
