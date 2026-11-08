#!/usr/bin/env node
// Day-0 baseline capture (slice M0-T0, plan 6.0 and 6.6): runs the regression
// watch-list commands and stores the FAILING test ids as one JSON per command in
// docs/plans/evidence/baseline-<date>/. New failures later are the set
// difference (scripts/tests/baseline-diff.mjs). Exit codes are never trusted:
// a command that exits non-zero with no parsed id is recorded as an
// `<command>::exit-<code>` failure so it still shows up in the diff.
//
// Usage (from the repo root):
//   node scripts/tests/baseline-capture.mjs --list
//   node scripts/tests/baseline-capture.mjs [--only id,id] [--root <checkout>] [--out <dir>] [--date YYYY-MM-DD]
//
// --root points at the checkout to measure. The baseline is meant for the BASE
// commit: run it from a clean `git worktree add <dir> <base-sha>` and pass that
// directory as --root, not from a tree that already carries Space edits.
//
// This script runs the project's own test commands; it adds no tests and no
// model calls. Nothing here reads .env files or prints environment values.

import { spawnSync, execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const FV = 'apps/Frontend Plane/verevonv3'
const MP_RUST = 'apps/Model Plane/rust'
const CONVEX = 'apps/Application Plane/convex-core'
const AUTH = 'apps/Control Plane/auth-core'
// Shared cargo target dirs of the local build host; CI (Linux) overrides them so no `C:/dev/...` directory is created in the checkout.
const TARGETS = { mp: process.env.SPACE_TARGET_MP ?? 'C:/dev/space-target-mp', gw: process.env.SPACE_TARGET_GW ?? 'C:/dev/space-target-gw' }

/** Each command: id, cwd (repo relative), command line, parser, env, timeout in minutes. */
export function commands(root) {
  const list = [
    { id: 'auth-core-jest', cwd: AUTH, parser: 'jest-json', env: { REQUIRE_CONTRACT: '1' }, minutes: 30,
      cmd: (o) => `pnpm exec jest --json --outputFile="${o}"` },
    { id: 'spa-vitest', cwd: FV, parser: 'jest-json', minutes: 30,
      cmd: (o) => `pnpm exec vitest run --reporter=json --outputFile="${o}"` },
    { id: 'spa-node-tests', cwd: FV, parser: 'tap', minutes: 10,
      cmd: () => 'node --test --test-reporter=tap apps/gateway/scripts/security-coverage-gate.test.mjs apps/gateway/scripts/action-surface-contract.test.mjs apps/gateway/scripts/product-truth-contract.test.mjs apps/gateway/scripts/zdr-seed-test-org.test.mjs' },
    { id: 'spa-typecheck', cwd: FV, parser: 'tsc', minutes: 20, cmd: () => 'pnpm exec tsc -b' },
    { id: 'spa-lint', cwd: FV, parser: 'eslint-json', minutes: 30,
      cmd: (o) => `pnpm exec eslint . -f json -o "${o}"` },
    { id: 'gateway-cargo', cwd: FV, parser: 'cargo', minutes: 60, env: { CARGO_TARGET_DIR: TARGETS.gw, CARGO_INCREMENTAL: '0' },
      cmd: () => 'cargo test --manifest-path apps/gateway/Cargo.toml --all-targets --no-fail-fast' },
    { id: 'model-plane-cargo', cwd: MP_RUST, parser: 'cargo', minutes: 90, env: { CARGO_TARGET_DIR: TARGETS.mp, CARGO_INCREMENTAL: '0' },
      cmd: () => 'cargo test --workspace --no-fail-fast' },
    { id: 'convex-test', cwd: CONVEX, parser: 'tap', minutes: 15,
      cmd: () => 'pnpm exec tsx --test --test-reporter=tap "test/*.test.cjs"' },
    { id: 'convex-typecheck', cwd: CONVEX, parser: 'tsc', minutes: 10, cmd: () => 'pnpm exec tsc --noEmit' },
    { id: 'convex-lint', cwd: CONVEX, parser: 'eslint-json', minutes: 10,
      cmd: (o) => `pnpm exec eslint "convex/**/*.ts" -f json -o "${o}"` },
  ]
  // Go: every Control Plane service and Model Plane Go service with a go.mod.
  const goRoots = ['apps/Control Plane', 'apps/Model Plane/go/services']
  for (const base of goRoots) {
    const abs = join(root, base)
    if (!existsSync(abs)) continue
    for (const entry of readdirSync(abs, { withFileTypes: true })) {
      if (entry.isDirectory() && existsSync(join(abs, entry.name, 'go.mod'))) {
        list.push({ id: `go-${base.includes('Model') ? 'mp' : 'cp'}-${entry.name}`, cwd: `${base}/${entry.name}`, parser: 'go-json', minutes: 20,
          cmd: () => 'go test -json ./...' })
      }
    }
  }
  return list
}

// ---- parsers: each returns a sorted, de-duplicated array of failing ids ---------

const uniq = (ids) => [...new Set(ids)].sort()

function parseJestJson(file, cwd) {
  const data = JSON.parse(readFileSync(file, 'utf8'))
  const ids = []
  for (const suite of data.testResults ?? []) {
    const name = relative(cwd, suite.name ?? suite.testFilePath ?? '').replaceAll('\\', '/')
    const failed = (suite.assertionResults ?? []).filter((t) => t.status === 'failed')
    for (const t of failed) ids.push(`${name}::${t.fullName ?? t.title}`)
    if (suite.status === 'failed' && failed.length === 0) ids.push(`${name}::(suite failed to run)`)
  }
  return uniq(ids)
}

function parseEslintJson(file, cwd) {
  const data = JSON.parse(readFileSync(file, 'utf8'))
  const ids = []
  for (const f of data) {
    const name = relative(cwd, f.filePath).replaceAll('\\', '/')
    for (const m of f.messages ?? []) if (m.severity === 2) ids.push(`${name}|${m.ruleId ?? 'parse'}|${m.message}`)
  }
  return uniq(ids)
}

function parseTsc(text) {
  const ids = []
  for (const line of text.split(/\r?\n/)) {
    const m = /^(.+?)\((\d+),(\d+)\): error (TS\d+): (.*)$/.exec(line.trim())
    if (m) ids.push(`${m[1].replaceAll('\\', '/')}|${m[4]}|${m[5]}`) // line numbers drift, so they are not part of the id
  }
  return uniq(ids)
}

function parseTap(text) {
  const ids = []
  for (const line of text.split(/\r?\n/)) {
    const m = /^\s*not ok \d+ - (.+?)(?:\s+#.*)?$/.exec(line)
    if (m && !/^# /.test(m[1])) ids.push(m[1])
  }
  return uniq(ids)
}

function parseCargo(text) {
  const ids = []
  let binary = 'unknown'
  for (const line of text.split(/\r?\n/)) {
    const run = /^\s*(?:Running|Doc-tests)\s+(.+?)(?:\s+\(.*\))?\s*$/.exec(line)
    if (run) binary = run[1].replaceAll('\\', '/')
    const failed = /^test (\S+) \.\.\. FAILED$/.exec(line)
    if (failed) ids.push(`${binary}::${failed[1]}`)
    const compile = /^error: could not compile `([^`]+)`/.exec(line)
    if (compile) ids.push(`compile::${compile[1]}`)
  }
  return uniq(ids)
}

function parseGoJson(text) {
  const ids = []
  for (const line of text.split(/\r?\n/)) {
    if (!line.startsWith('{')) continue
    let ev
    try { ev = JSON.parse(line) } catch { continue }
    if (ev.Action !== 'fail') continue
    ids.push(ev.Test ? `${ev.Package}::${ev.Test}` : `${ev.Package}::(package)`)
  }
  // A package fail whose tests also failed is noise: keep package ids only when no test id exists for it.
  const withTests = new Set(ids.filter((i) => !i.endsWith('::(package)')).map((i) => i.split('::')[0]))
  return uniq(ids.filter((i) => !i.endsWith('::(package)') || !withTests.has(i.split('::')[0])))
}

export function parse(kind, { file, text, cwd }) {
  switch (kind) {
    case 'jest-json': return parseJestJson(file, cwd)
    case 'eslint-json': return parseEslintJson(file, cwd)
    case 'tsc': return parseTsc(text)
    case 'tap': return parseTap(text)
    case 'cargo': return parseCargo(text)
    case 'go-json': return parseGoJson(text)
    default: throw new Error(`unknown parser ${kind}`)
  }
}

// ---- runner ---------------------------------------------------------------------

function version(cmd, args) {
  try { return execFileSync(cmd, args, { encoding: 'utf8', shell: process.platform === 'win32' }).trim().split('\n')[0] } catch { return 'unavailable' }
}

function runOne(spec, root, scratch) {
  const cwd = resolve(root, spec.cwd)
  if (!existsSync(cwd)) return { skipped: `directory missing: ${spec.cwd}` }
  const outFile = join(scratch, `${spec.id}.out.json`)
  const started = Date.now()
  const res = spawnSync(spec.cmd(outFile), {
    cwd, shell: true, encoding: 'utf8', maxBuffer: 1024 * 1024 * 512, timeout: spec.minutes * 60_000,
    env: { ...process.env, CI: '1', FORCE_COLOR: '0', NO_COLOR: '1', ...(spec.env ?? {}) },
  })
  const text = `${res.stdout ?? ''}\n${res.stderr ?? ''}`
  let ids = []
  let note = ''
  try {
    ids = parse(spec.parser, { file: outFile, text, cwd })
  } catch (error) {
    note = `parse error: ${error.message}`
  }
  const timedOut = res.error?.code === 'ETIMEDOUT'
  const exit = res.status ?? (timedOut ? 'timeout' : 'signal')
  if ((exit !== 0 && ids.length === 0) || note) ids = uniq([...ids, `${spec.id}::exit-${exit}`])
  return { exit, ids, note, duration_ms: Date.now() - started }
}

export function main(argv) {
  const args = { list: false, only: null, root: process.cwd(), out: null, date: new Date().toISOString().slice(0, 10) }
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i]
    if (a === '--list') args.list = true
    else if (a === '--only') args.only = argv[++i].split(',')
    else if (a === '--root') args.root = resolve(argv[++i])
    else if (a === '--out') args.out = resolve(argv[++i])
    else if (a === '--date') args.date = argv[++i]
    else { console.error(`unknown argument: ${a}`); process.exit(1) }
  }
  const all = commands(args.root)
  if (args.list) { for (const c of all) console.log(`${c.id}\t${c.cwd}\t${c.cmd('<out>')}`); return }
  const selected = args.only ? all.filter((c) => args.only.includes(c.id)) : all
  const out = args.out ?? join(args.root, 'docs/plans/evidence', `baseline-${args.date}`)
  mkdirSync(out, { recursive: true })
  const scratch = mkdtempSync(join(tmpdir(), 'space-baseline-'))
  let sha = 'unknown'
  let dirty = 'unknown'
  try {
    sha = execFileSync('git', ['-C', args.root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
    dirty = execFileSync('git', ['-C', args.root, 'status', '--porcelain', '--untracked-files=no'], { encoding: 'utf8' }).trim() ? 'tracked-modifications-present' : 'clean'
  } catch { /* not a git checkout */ }
  const manifest = {
    captured_at: new Date().toISOString(), git_sha: sha, tree: dirty, host: process.platform,
    node: process.version, pnpm: version('pnpm', ['--version']), cargo: version('cargo', ['--version']), go: version('go', ['version']),
    commands: [],
  }
  for (const spec of selected) {
    console.log(`== ${spec.id} (${spec.cwd})`)
    const result = runOne(spec, args.root, scratch)
    if (result.skipped) {
      console.log(`   skipped: ${result.skipped}`)
      manifest.commands.push({ id: spec.id, skipped: result.skipped })
      continue
    }
    const record = {
      id: spec.id, cwd: spec.cwd, command: spec.cmd('<out>'), git_sha: sha, exit_code: result.exit,
      duration_ms: result.duration_ms, parse_note: result.note, failing_count: result.ids.length, failing_ids: result.ids,
    }
    writeFileSync(join(out, `${spec.id}.json`), `${JSON.stringify(record, null, 2)}\n`)
    manifest.commands.push({ id: spec.id, exit_code: result.exit, failing_count: result.ids.length })
    console.log(`   exit ${result.exit}, ${result.ids.length} failing ids`)
  }
  writeFileSync(join(out, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  rmSync(scratch, { recursive: true, force: true })
  console.log(`baseline written to ${out}`)
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) main(process.argv.slice(2))
