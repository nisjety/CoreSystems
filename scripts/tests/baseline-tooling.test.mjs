// Unit tests for the Space baseline tooling (slices M0-T0 and M0-CI):
//   scripts/tests/baseline-capture.mjs   parsers and command list
//   scripts/tests/baseline-diff.mjs      set difference of failing ids
//   scripts/gates/space-baseline-compare.sh   the CI ratchet, driven by a fake `pnpm`
// Run: node --test scripts/tests/baseline-tooling.test.mjs
// No suites are executed, no network, no model calls: the compare-script tests copy the
// scripts into a temporary checkout and replace `pnpm` with a shim that prints canned output.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { commands, parse } from './baseline-capture.mjs'
import { diff } from './baseline-diff.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = resolve(HERE, '..', '..')

const scratch = mkdtempSync(join(tmpdir(), 'baseline-tooling-test-'))
after(() => rmSync(scratch, { recursive: true, force: true }))

function writeJson(name, value) {
  const file = join(scratch, name)
  writeFileSync(file, JSON.stringify(value))
  return file
}

describe('baseline-capture parsers', () => {
  it('jest-json: failed assertions become <file>::<fullName>, suites that failed to run get a marker, passes are ignored', () => {
    const cwd = join(scratch, 'pkg')
    const file = writeJson('jest.json', {
      testResults: [
        {
          name: join(cwd, 'src', 'a.test.ts'),
          status: 'failed',
          assertionResults: [
            { status: 'passed', fullName: 'a works' },
            { status: 'failed', fullName: 'a breaks' },
            { status: 'failed', title: 'only a title' },
          ],
        },
        { name: join(cwd, 'src', 'b.test.ts'), status: 'failed', assertionResults: [] },
        { name: join(cwd, 'src', 'c.test.ts'), status: 'passed', assertionResults: [{ status: 'passed', fullName: 'c' }] },
      ],
    })
    assert.deepEqual(parse('jest-json', { file, cwd }), [
      'src/a.test.ts::a breaks',
      'src/a.test.ts::only a title',
      'src/b.test.ts::(suite failed to run)',
    ])
  })

  it('jest-json: an empty or missing testResults array yields no ids', () => {
    assert.deepEqual(parse('jest-json', { file: writeJson('empty.json', {}), cwd: scratch }), [])
  })

  it('eslint-json: only severity 2 messages count, ids are file|rule|message with no line numbers', () => {
    const cwd = join(scratch, 'pkg')
    const file = writeJson('eslint.json', [
      {
        filePath: join(cwd, 'src', 'x.ts'),
        messages: [
          { severity: 2, ruleId: 'no-undef', message: "'foo' is not defined", line: 3 },
          { severity: 1, ruleId: 'no-console', message: 'warn only' },
          { severity: 2, ruleId: null, message: 'Parsing error' },
        ],
      },
      { filePath: join(cwd, 'src', 'y.ts'), messages: [] },
    ])
    assert.deepEqual(parse('eslint-json', { file, cwd }), [
      "src/x.ts|no-undef|'foo' is not defined",
      'src/x.ts|parse|Parsing error',
    ])
  })

  it('tsc: parses error lines, normalises backslashes and drops line/column so the id survives edits', () => {
    const text = [
      'src\\a.ts(10,5): error TS2322: Type string is not assignable to number.',
      'src/a.ts(99,1): error TS2322: Type string is not assignable to number.',
      '  src/b.ts(1,1): error TS7006: Parameter x implicitly has an any type.',
      'src/c.ts(1,1): warning TS9999: not an error',
      'Found 3 errors.',
    ].join('\r\n')
    assert.deepEqual(parse('tsc', { text }), [
      'src/a.ts|TS2322|Type string is not assignable to number.',
      'src/b.ts|TS7006|Parameter x implicitly has an any type.',
    ])
  })

  it('tap: collects `not ok` titles, strips directives, ignores `ok` and summary lines', () => {
    const text = [
      'TAP version 13',
      'ok 1 - passes',
      'not ok 2 - fails hard',
      '  not ok 3 - nested failure',
      'not ok 4 - skipped by directive # SKIP not ready',
      'ok 5 - fine',
      '# tests 5',
      '# fail 3',
    ].join('\n')
    assert.deepEqual(parse('tap', { text }), ['fails hard', 'nested failure', 'skipped by directive'])
  })

  it('cargo: ties each FAILED test to its Running binary and reports compile errors', () => {
    const text = [
      '     Running unittests src/lib.rs (target/debug/deps/gateway-abc123.exe)',
      'test a::ok ... ok',
      'test a::broken ... FAILED',
      '     Running tests/api.rs (target/debug/deps/api-def456.exe)',
      'test api::bad ... FAILED',
            'error: could not compile `model-gateway` (lib) due to 2 previous errors',
      'test result: FAILED. 1 passed; 3 failed',
    ].join('\n')
    const ids = parse('cargo', { text })
    assert.deepEqual(ids, ['compile::model-gateway', 'tests/api.rs::api::bad', 'unittests src/lib.rs::a::broken'])
    assert.equal(ids.some((id) => id.endsWith('::a::ok')), false)
  })

  it('go-json: fail events become package::Test, and a package-level fail is dropped when its tests already failed', () => {
    const lines = [
      { Action: 'run', Package: 'ex/a', Test: 'TestOk' },
      { Action: 'pass', Package: 'ex/a', Test: 'TestOk' },
      { Action: 'fail', Package: 'ex/a', Test: 'TestBad' },
      { Action: 'fail', Package: 'ex/a' },
      { Action: 'fail', Package: 'ex/b' }, // build failure: no test ids, so the package id stays
      { Action: 'output', Package: 'ex/c', Output: 'x' },
    ].map((e) => JSON.stringify(e))
    lines.splice(2, 0, 'not json at all')
    assert.deepEqual(parse('go-json', { text: lines.join('\n') }), ['ex/a::TestBad', 'ex/b::(package)'])
  })

  it('every parser returns a sorted, de-duplicated array', () => {
    const text = ['not ok 1 - z', 'not ok 2 - a', 'not ok 3 - z'].join('\n')
    assert.deepEqual(parse('tap', { text }), ['a', 'z'])
  })

  it('an unknown parser name throws', () => {
    assert.throws(() => parse('nope', { text: '' }), /unknown parser/)
  })
})

describe('baseline-capture command list', () => {
  it('defines every watch-list command with a known parser, a cwd and a timeout', () => {
    const known = new Set(['jest-json', 'eslint-json', 'tsc', 'tap', 'cargo', 'go-json'])
    const list = commands(scratch)
    const ids = list.map((c) => c.id)
    assert.equal(new Set(ids).size, ids.length, 'ids are unique')
    for (const id of ['auth-core-jest', 'spa-vitest', 'spa-node-tests', 'spa-typecheck', 'spa-lint', 'gateway-cargo', 'model-plane-cargo', 'convex-test', 'convex-typecheck', 'convex-lint']) {
      assert.ok(ids.includes(id), `missing command ${id}`)
    }
    for (const c of list) {
      assert.ok(known.has(c.parser), `${c.id}: parser ${c.parser}`)
      assert.ok(c.cwd && c.minutes > 0, `${c.id}: cwd/minutes`)
      assert.equal(typeof c.cmd('<out>'), 'string')
    }
  })

  it('never invokes a model, router or live network endpoint', () => {
    for (const c of commands(scratch)) assert.doesNotMatch(c.cmd('<out>'), /verevon|curl|http:|https:/i, c.id)
  })

  it('discovers Go services only when a go.mod exists', () => {
    const root = mkdtempSync(join(scratch, 'go-root-'))
    mkdirSync(join(root, 'apps', 'Control Plane', 'svc-a'), { recursive: true })
    writeFileSync(join(root, 'apps', 'Control Plane', 'svc-a', 'go.mod'), 'module x\n')
    mkdirSync(join(root, 'apps', 'Control Plane', 'no-mod'), { recursive: true })
    mkdirSync(join(root, 'apps', 'Model Plane', 'go', 'services', 'svc-b'), { recursive: true })
    writeFileSync(join(root, 'apps', 'Model Plane', 'go', 'services', 'svc-b', 'go.mod'), 'module y\n')
    const goIds = commands(root).filter((c) => c.parser === 'go-json').map((c) => c.id).sort()
    assert.deepEqual(goIds, ['go-cp-svc-a', 'go-mp-svc-b'])
  })
})

describe('baseline-diff', () => {
  const set = (...ids) => new Set(ids)

  it('a new failing id is a regression', () => {
    const report = diff(new Map([['cmd', set('a')]]), new Map([['cmd', set('a', 'b')]]))
    assert.deepEqual(report.regressions, ['cmd: b'])
    assert.deepEqual(report.fixed, [])
  })

  it('a vanished failing id is reported as fixed, not as a regression', () => {
    const report = diff(new Map([['cmd', set('a', 'b')]]), new Map([['cmd', set('a')]]))
    assert.deepEqual(report.regressions, [])
    assert.deepEqual(report.fixed, ['cmd: b'])
  })

  it('a command in the baseline but absent from the current run is "not run", never a pass', () => {
    const report = diff(new Map([['cmd', set()], ['other', set('x')]]), new Map([['other', set('x')]]))
    assert.deepEqual(report.notRun, ['cmd'])
    assert.deepEqual(report.regressions, [])
  })

  it('a command with no baseline is listed separately and its failures are not regressions', () => {
    const report = diff(new Map(), new Map([['fresh', set('boom')]]))
    assert.deepEqual(report.newCommands, ['fresh'])
    assert.deepEqual(report.regressions, [])
  })

  it('an unchanged failing set is clean', () => {
    const report = diff(new Map([['cmd', set('a', 'b')]]), new Map([['cmd', set('b', 'a')]]))
    assert.deepEqual(report, { regressions: [], fixed: [], notRun: [], newCommands: [] })
  })

  it('CLI: exit 0 for the same set, 1 for a new id, 2 for bad input', () => {
    const dir = (name, record) => {
      const d = join(scratch, name)
      mkdirSync(d, { recursive: true })
      writeFileSync(join(d, 'demo.json'), JSON.stringify(record))
      writeFileSync(join(d, 'manifest.json'), '{}') // manifest.json must be ignored
      return d
    }
    const base = dir('cli-base', { id: 'demo', failing_ids: ['old'] })
    const same = dir('cli-same', { id: 'demo', failing_ids: ['old'] })
    const worse = dir('cli-worse', { id: 'demo', failing_ids: ['old', 'new'] })
    const script = join(HERE, 'baseline-diff.mjs')
    const run = (...args) => spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' })
    assert.equal(run(base, same).status, 0)
    const bad = run(base, worse)
    assert.equal(bad.status, 1)
    assert.match(bad.stdout, /NEW FAILURES \(1\)[\s\S]*demo: new/)
    assert.equal(run(base, join(scratch, 'does-not-exist')).status, 2)
    assert.equal(run().status, 2)
  })
})

// ---- scripts/gates/space-baseline-compare.sh -------------------------------------------

function findBash() {
  const candidates = [process.env.BASH, 'C:/Program Files/Git/bin/bash.exe', '/bin/bash', '/usr/bin/bash'].filter(Boolean)
  return candidates.find((p) => existsSync(p)) ?? null
}
const BASH = findBash()

describe('space-baseline-compare.sh', { skip: BASH ? false : 'no usable bash on this host' }, () => {
  let root
  let shimDir

  before(() => {
    root = mkdtempSync(join(scratch, 'checkout-'))
    mkdirSync(join(root, 'scripts', 'gates'), { recursive: true })
    mkdirSync(join(root, 'scripts', 'tests'), { recursive: true })
    for (const f of ['scripts/gates/space-baseline-compare.sh', 'scripts/tests/baseline-capture.mjs', 'scripts/tests/baseline-diff.mjs']) {
      copyFileSync(join(REPO, f), join(root, f))
    }
    mkdirSync(join(root, 'apps', 'Application Plane', 'convex-core'), { recursive: true })
    // Fake pnpm: prints the canned output from $SHIM_OUTPUT_FILE and exits 2 (like `tsc` with errors).
    shimDir = join(root, 'shim')
    mkdirSync(shimDir)
    writeFileSync(join(shimDir, 'pnpm.cmd'), '@echo off\r\nif exist "%SHIM_OUTPUT_FILE%" type "%SHIM_OUTPUT_FILE%"\r\nexit /b %SHIM_EXIT%\r\n')
    writeFileSync(join(shimDir, 'pnpm'), '#!/bin/sh\n[ -f "$SHIM_OUTPUT_FILE" ] && cat "$SHIM_OUTPUT_FILE"\nexit "${SHIM_EXIT:-0}"\n')
    chmodSync(join(shimDir, 'pnpm'), 0o755)
  })

  function baseline(failingIds, date = '2026-01-01') {
    const dir = join(root, 'docs', 'plans', 'evidence', `baseline-${date}`)
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'convex-typecheck.json'), JSON.stringify({ id: 'convex-typecheck', failing_ids: failingIds }))
  }
  function clearBaselines() {
    rmSync(join(root, 'docs'), { recursive: true, force: true })
  }

  function runCompare(args, tscOutput, exit = 2) {
    const out = join(root, 'shim-output.txt')
    writeFileSync(out, tscOutput ?? '')
    const pathKey = Object.keys(process.env).find((k) => k.toLowerCase() === 'path') ?? 'PATH'
    const env = { ...process.env, [pathKey]: `${shimDir}${delimiter}${process.env[pathKey] ?? ''}`, SHIM_OUTPUT_FILE: out, SHIM_EXIT: String(exit) }
    return spawnSync(BASH, [join(root, 'scripts', 'gates', 'space-baseline-compare.sh'), ...args], { encoding: 'utf8', env, timeout: 120_000 })
  }

  const OLD = 'src/old.ts(1,1): error TS2322: pre-existing problem'
  const NEW = 'src/new.ts(7,3): error TS2345: brand new problem'

  it('--self-test passes (unchanged set green, new failing id red)', () => {
    const res = spawnSync(BASH, [join(root, 'scripts', 'gates', 'space-baseline-compare.sh'), '--self-test'], { encoding: 'utf8', timeout: 60_000 })
    assert.equal(res.status, 0, res.stdout + res.stderr)
    assert.match(res.stdout, /self-test passed/)
  })

  it('no argument is a usage error (exit 2)', () => {
    const res = runCompare([])
    assert.equal(res.status, 2)
    assert.match(res.stderr, /usage/)
  })

  it('with a committed baseline, an unchanged failing set passes', () => {
    clearBaselines()
    baseline(['src/old.ts|TS2322|pre-existing problem'])
    const res = runCompare(['convex-typecheck'], OLD)
    assert.equal(res.status, 0, res.stdout + res.stderr)
    assert.match(res.stdout, /no new failing ids/)
  })

  it('with a committed baseline, a new failing id fails the gate and is named', () => {
    clearBaselines()
    baseline(['src/old.ts|TS2322|pre-existing problem'])
    const res = runCompare(['convex-typecheck'], `${OLD}\n${NEW}`)
    assert.equal(res.status, 1, res.stdout + res.stderr)
    assert.match(res.stdout, /NEW FAILURES/)
    assert.match(res.stdout, /src\/new\.ts\|TS2345\|brand new problem/)
  })

  it('a vanished failing id still passes and is reported as fixed', () => {
    clearBaselines()
    baseline(['src/old.ts|TS2322|pre-existing problem'])
    const res = runCompare(['convex-typecheck'], '', 0)
    assert.equal(res.status, 0, res.stdout + res.stderr)
    assert.match(res.stdout, /fixed since baseline/)
  })

  it('uses the newest baseline directory that holds a record for the id', () => {
    clearBaselines()
    baseline(['src/new.ts|TS2345|brand new problem'], '2025-12-01') // older: knows the "new" failure
    baseline(['src/old.ts|TS2322|pre-existing problem'], '2026-02-01') // newer: does not
    const res = runCompare(['convex-typecheck'], `${OLD}\n${NEW}`)
    assert.equal(res.status, 1, 'the newest baseline must win')
  })

  it('no stored baseline and no failing ids passes', () => {
    clearBaselines()
    const res = runCompare(['convex-typecheck'], '', 0)
    assert.equal(res.status, 0, res.stdout + res.stderr)
    assert.match(res.stdout, /no stored baseline and no failing ids: pass/)
  })

  it('no stored baseline but failing ids fails with an explanation (cannot prove they are pre-existing)', () => {
    clearBaselines()
    const res = runCompare(['convex-typecheck'], OLD)
    assert.equal(res.status, 1, res.stdout + res.stderr)
    assert.match(res.stdout + res.stderr, /no stored baseline/)
  })

  it('a non-zero exit with no parsable id is recorded as <id>::exit-<code>, not trusted as a pass', () => {
    clearBaselines()
    const res = runCompare(['convex-typecheck'], 'something unparsable', 3)
    assert.equal(res.status, 1, res.stdout + res.stderr)
  })

  it('an unknown command id produces no record and fails', () => {
    clearBaselines()
    const res = runCompare(['no-such-command'], '')
    assert.equal(res.status, 1, res.stdout + res.stderr)
    assert.match(res.stdout + res.stderr, /produced no record/)
  })
})
