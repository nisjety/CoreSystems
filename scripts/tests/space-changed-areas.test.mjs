// Tests for scripts/gates/space-changed-areas.sh (slice M0-CI): which areas does a change touch?
// space-gate.yml runs one job per touched area, so a wrong answer here either skips a job that
// must run (a silent hole in the gate) or runs one that should not. The script's own
// --self-test also runs in CI (gate-self-test job); these cases drive the real CLI.
// Run: node --test scripts/tests/space-changed-areas.test.mjs
// The workflow-level proofs (GitHub accepts space-gate.yml and convex-core.yml; a seeded failing
// test turns space-gate red; pnpm 11.3.0 `install --frozen-lockfile` works) need a real pull
// request: see the skipped cases at the bottom.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..')
const SCRIPT = join(REPO, 'scripts', 'gates', 'space-changed-areas.sh')
const BASH = ['C:/Program Files/Git/bin/bash.exe', '/bin/bash', '/usr/bin/bash'].find((p) => existsSync(p))
const scratch = mkdtempSync(join(tmpdir(), 'changed-areas-test-'))
after(() => rmSync(scratch, { recursive: true, force: true }))

function parseOutput(stdout) {
  const out = {}
  for (const line of stdout.split(/\r?\n/)) {
    const i = line.indexOf('=')
    if (i > 0) out[line.slice(0, i)] = line.slice(i + 1)
  }
  return out
}

/** Runs the script on a list of changed paths (the --files mode). */
function areasFor(...paths) {
  const file = join(scratch, `files-${Math.random().toString(36).slice(2)}.txt`)
  writeFileSync(file, `${paths.join('\n')}\n`)
  const res = spawnSync(BASH, [SCRIPT, '--files', file], { encoding: 'utf8', env: { ...process.env, GITHUB_OUTPUT: '' } })
  assert.equal(res.status, 0, res.stdout + res.stderr)
  return parseOutput(res.stdout)
}

describe('space-changed-areas.sh', { skip: BASH ? false : 'no usable bash on this host' }, () => {
  it('--self-test passes', () => {
    const res = spawnSync(BASH, [SCRIPT, '--self-test'], { encoding: 'utf8' })
    assert.equal(res.status, 0, res.stdout + res.stderr)
    assert.match(res.stdout, /self-test passed/)
  })

  it('emits every key the workflow reads, as booleans or JSON arrays', () => {
    const a = areasFor('README.md')
    for (const key of ['cp_go_services', 'mp_go_services', 'principals', 'auth_core', 'mp_rust', 'proto', 'gateway', 'spa', 'notification_core', 'full_run']) {
      assert.ok(key in a, `missing ${key}`)
    }
    assert.equal(a.cp_go_services, '[]')
    assert.equal(a.mp_go_services, '[]')
    for (const key of ['principals', 'auth_core', 'mp_rust', 'proto', 'gateway', 'spa', 'notification_core', 'full_run']) {
      assert.equal(a[key], 'false', `an unrelated file must not select ${key}`)
    }
  })

  it('an untouched area stays false (its job is skipped, which the required check treats as green)', () => {
    const a = areasFor('apps/Control Plane/user-core/internal/http/spaces.go')
    assert.equal(a.cp_go_services, '["user-core"]')
    assert.equal(a.spa, 'false')
    assert.equal(a.gateway, 'false')
    assert.equal(a.mp_rust, 'false')
    assert.equal(a.full_run, 'false')
  })

  it('selects several Control Plane Go services as a JSON array in a stable order', () => {
    const a = areasFor('apps/Control Plane/org-core/x.go', 'apps/Control Plane/user-core/y.go')
    assert.equal(a.cp_go_services, '["user-core","org-core"]')
  })

  it('does not match a service by prefix (user-core-extra is not user-core)', () => {
    assert.equal(areasFor('apps/Control Plane/user-core-extra/x.go').cp_go_services, '[]')
  })

  it('a regenerated Go binding selects every Model Plane Go service', () => {
    const a = areasFor('apps/Model Plane/go/gen/model_plane/v1/session.pb.go')
    assert.equal(JSON.parse(a.mp_go_services).length, 8)
  })

  it('a single Model Plane Go service selects only itself', () => {
    assert.equal(areasFor('apps/Model Plane/go/services/cost-core/main.go').mp_go_services, '["cost-core"]')
  })

  it('proto changes select the proto and Rust jobs', () => {
    const a = areasFor('apps/Model Plane/proto/model_plane/v1/session.proto')
    assert.equal(a.proto, 'true')
    assert.equal(a.mp_rust, 'true')
  })

  it('SPA src selects spa but not gateway; a gateway-only change selects gateway but not spa', () => {
    const spa = areasFor('apps/Frontend Plane/verevonv3/src/features/spaces/a.ts')
    assert.equal(spa.spa, 'true')
    assert.equal(spa.gateway, 'false')
    const gw = areasFor('apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs')
    assert.equal(gw.gateway, 'true')
    assert.equal(gw.spa, 'false')
  })

  it('a mixed SPA and gateway change selects both', () => {
    const a = areasFor('apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs', 'apps/Frontend Plane/verevonv3/package.json')
    assert.equal(a.gateway, 'true')
    assert.equal(a.spa, 'true')
  })

  it('principal registry and auth-core changes select their jobs', () => {
    assert.equal(areasFor('apps/Control Plane/config/plane-service-principals.json').principals, 'true')
    assert.equal(areasFor('apps/Control Plane/config/plane-service-principals.json').auth_core, 'true')
    assert.equal(areasFor('apps/Control Plane/auth-core/src/x.ts').auth_core, 'true')
    assert.equal(areasFor('apps/Application Plane/notification-core/src/x.ts').notification_core, 'true')
  })

  it('Convex changes are handled by convex-core.yml, not by space-gate areas', () => {
    const a = areasFor('apps/Application Plane/convex-core/convex/schema.ts')
    for (const key of ['spa', 'gateway', 'mp_rust', 'proto', 'full_run']) assert.equal(a[key], 'false', key)
  })

  it('an unknown base selects EVERYTHING (a gate that cannot tell what changed must not decide nothing did)', () => {
    const res = spawnSync(BASH, [SCRIPT], { encoding: 'utf8', cwd: REPO, env: { ...process.env, BASE_SHA: '0000000000000000000000000000000000000000', HEAD_SHA: 'HEAD', GITHUB_OUTPUT: '' } })
    assert.equal(res.status, 0, res.stdout + res.stderr)
    const a = parseOutput(res.stdout)
    assert.equal(a.full_run, 'true')
    for (const key of ['principals', 'auth_core', 'mp_rust', 'proto', 'gateway', 'spa', 'notification_core']) assert.equal(a[key], 'true', key)
    assert.equal(JSON.parse(a.cp_go_services).length, 3)
    assert.equal(JSON.parse(a.mp_go_services).length, 8)
  })

  it('an empty BASE_SHA also selects everything', () => {
    const res = spawnSync(BASH, [SCRIPT], { encoding: 'utf8', cwd: REPO, env: { ...process.env, BASE_SHA: '', GITHUB_OUTPUT: '' } })
    assert.equal(parseOutput(res.stdout).full_run, 'true')
  })

  it('appends to $GITHUB_OUTPUT when it is set', () => {
    const outFile = join(scratch, 'github-output.txt')
    writeFileSync(outFile, '')
    const list = join(scratch, 'one-file.txt')
    writeFileSync(list, 'apps/Control Plane/audit-core/x.go\n')
    const res = spawnSync(BASH, [SCRIPT, '--files', list], { encoding: 'utf8', env: { ...process.env, GITHUB_OUTPUT: outFile } })
    assert.equal(res.status, 0, res.stderr)
    assert.match(readFileSync(outFile, 'utf8'), /^cp_go_services=\["audit-core"\]$/m)
  })
})

describe('workflow-level proofs (need a real pull request on GitHub)', () => {
  it('space-gate.yml: GitHub accepts the workflow; space-gate is red on a seeded failing test in an area job and green on an untouched area', { skip: 'needs a real PR on GitHub: later tier' }, () => {})
  it('convex-core.yml: pnpm install --frozen-lockfile works with pnpm 11.3.0; convex-test, typecheck and lint run and compare', { skip: 'needs a real PR on GitHub: later tier' }, () => {})
})
