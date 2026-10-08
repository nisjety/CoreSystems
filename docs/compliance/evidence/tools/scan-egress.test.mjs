// Tests for scan-egress.mjs. Run: node --test docs/compliance/evidence/tools/*.test.mjs
//
// Each test builds a throwaway tree in the temp directory, so nothing here depends on the
// state of the real working tree (other sessions edit it). The one property the plan asks
// for by name: a vendor in a source file is found, a vendor in a test file and a vendor in
// a document are not, and the output is identical from one run to the next.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  IGNORE_RULES,
  INTERNAL_ENV,
  VENDORS,
  blockCommentMask,
  classifyFile,
  componentOf,
  defaultStateInLine,
  envNamesInLine,
  findUrlsInLine,
  hostMatches,
  ignoreReasonFor,
  isCommentAt,
  main,
  normaliseHost,
  renderJson,
  renderSummary,
  renderText,
  rustTestLineMask,
  scan,
  scanManifest,
  scanText,
  strictProblems,
  vendorForHost,
  verifyDocument,
} from './scan-egress.mjs'

function makeTree(files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'scan-egress-'))
  for (const [relative, content] of Object.entries(files)) {
    const target = path.join(root, relative)
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, content)
  }
  return root
}

const CATALOGUE = [
  {
    id: 'acme-pay',
    name: 'Acme Pay',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.acmepay.test-vendor.com'],
    gateEnv: ['ACMEPAY_API_KEY'],
    note: 'fixture',
  },
  { id: 'acme-ai', name: 'Acme AI', class: 'subprocessor', role: 'processor', hosts: ['*.acmeai.example-vendor.net'], gateEnv: [] },
  { id: 'ghost-vendor', name: 'Ghost', class: 'infrastructure', hosts: ['ghost.vendor-nowhere.org'], gateEnv: [] },
]

function runScan(root, extra = {}) {
  return scan({ root, vendors: CATALOGUE, internalEnv: [], ...extra })
}

// ---------------------------------------------------------------------------------------
// the plan's named case: finds a vendor, ignores a test file, ignores a doc
// ---------------------------------------------------------------------------------------

test('a vendor in a source file is found; the same host in a test file, a document, node_modules or a Rust test block is not', () => {
  const root = makeTree({
    'apps/Billing Plane/pay/src/client.go': 'package pay\n\nconst base = "https://api.acmepay.test-vendor.com/v1"\n',
    'apps/Billing Plane/pay/src/client_test.go': 'package pay\n\nconst base = "https://api.openai.com/v1"\n',
    'apps/Billing Plane/pay/docs/design.md': 'We call https://api.anthropic.com for summaries.\n',
    'apps/Billing Plane/pay/README.md': 'See https://api.cohere.ai\n',
    'apps/Billing Plane/pay/node_modules/dep/index.js': 'fetch("https://registry.example-vendor.net/x")\n',
    'apps/Billing Plane/pay/tests/e2e/flow.spec.ts': 'goto("https://api.stripe.com")\n',
    'apps/Billing Plane/pay/src/lib.rs': [
      'pub const BASE: &str = "https://hello.acmeai.example-vendor.net/v1";',
      '',
      '#[cfg(test)]',
      'mod tests {',
      '    const FAKE: &str = "https://api.cohere.ai/rerank";',
      '}',
      '',
    ].join('\n'),
  })
  const result = runScan(root)
  const ids = result.vendors.map((vendor) => vendor.id)
  assert.deepEqual(ids, ['acme-ai', 'acme-pay'])
  assert.deepEqual(result.unclassified, [], 'the test file, the document and the cfg(test) block must not produce hosts')
  assert.deepEqual(result.vendorsNotFound, ['ghost-vendor'])
  const pay = result.vendors.find((vendor) => vendor.id === 'acme-pay')
  assert.equal(pay.evidenceStatus, 'code')
  assert.deepEqual(pay.hosts[0].evidence, ['apps/Billing Plane/pay/src/client.go:3'])
  assert.deepEqual(pay.components, ['Billing Plane/pay'])
  assert.equal(result.excluded.test, 2)
  assert.equal(result.excluded.doc, 2)
})

test('output is deterministic: two runs give identical JSON and text, and no absolute path or timestamp is in it', () => {
  const root = makeTree({
    'apps/X Plane/a/src/main.go': 'package a\nvar u = "https://api.acmepay.test-vendor.com/v1"\nvar v = "https://b.acmeai.example-vendor.net/x"\n',
    'apps/X Plane/a/src/other.ts': 'export const u = "https://zzz.unknown-host.org/x"\n',
  })
  const first = runScan(root)
  const second = runScan(root)
  assert.equal(renderJson(first), renderJson(second))
  assert.equal(renderText(first), renderText(second))
  const text = renderText(first) + renderJson(first)
  assert.ok(!text.includes(root), 'the temp directory path must not appear')
  assert.ok(!text.includes(root.split(path.sep).join('/')), 'the temp directory path must not appear (posix form)')
  assert.ok(!/\d{4}-\d{2}-\d{2}T\d{2}:/.test(text), 'no timestamp')
})

test('the summary form does not move when unrelated code moves, but changes when a vendor appears', () => {
  const body = 'package a\nvar u = "https://api.acmepay.test-vendor.com/v1"\n'
  const before = makeTree({ 'apps/P/a/src/a.go': body })
  const shifted = makeTree({ 'apps/P/a/src/a.go': `// header\n// header\n${body}\n\n` })
  const summaryBefore = renderSummary(runScan(before))
  assert.equal(summaryBefore, renderSummary(runScan(shifted)), 'a shifted line number must not change the summary')
  assert.notEqual(renderJson(runScan(before)), renderJson(runScan(shifted)), 'the full report does carry line numbers')
  assert.ok(!/:\d+/.test(summaryBefore.replace(/class=|role=|evidence=|wiring=/g, '')), 'no file:line in the summary')
  const grown = makeTree({ 'apps/P/a/src/a.go': `${body}var v = "https://x.acmeai.example-vendor.net/y"\n` })
  assert.notEqual(summaryBefore, renderSummary(runScan(grown)))
  assert.match(renderSummary(runScan(grown)), /vendor acme-ai /)
})

test('file order does not matter: the same files created in another order give the same result', () => {
  const files = {
    'apps/P/a/src/one.go': 'package a\nvar u = "https://api.acmepay.test-vendor.com/v1"\n',
    'apps/P/a/src/two.go': 'package a\nvar u = "https://api.acmepay.test-vendor.com/v2"\n',
    'apps/P/b/src/three.ts': 'export const u = "https://x.acmeai.example-vendor.net/v1"\n',
  }
  const forward = makeTree(files)
  const reversed = makeTree(Object.fromEntries(Object.entries(files).reverse()))
  assert.equal(renderJson(runScan(forward)), renderJson(runScan(reversed)))
})

test('git mode lists tracked and untracked-not-ignored files and skips ignored ones; the result equals the walk', (t) => {
  const git = spawnSync('git', ['--version'], { encoding: 'utf8' })
  if (git.status !== 0) {
    t.skip('git is not on PATH')
    return
  }
  const root = makeTree({
    '.gitignore': 'apps/P/a/src/ignored.go\n',
    'apps/P/a/src/tracked.go': 'package a\nvar u = "https://api.acmepay.test-vendor.com/v1"\n',
    'apps/P/a/src/ignored.go': 'package a\nvar u = "https://b.acmeai.example-vendor.net/v1"\n',
  })
  const run = (args) => spawnSync('git', args, { cwd: root, encoding: 'utf8' })
  assert.equal(run(['init', '-q']).status, 0)
  assert.equal(run(['add', '.gitignore', 'apps/P/a/src/tracked.go']).status, 0)
  fs.writeFileSync(path.join(root, 'apps/P/a/src/untracked.ts'), 'export const u = "https://c.acmeai.example-vendor.net/v1"\n')
  const viaGit = runScan(root)
  assert.equal(viaGit.fileSelection, 'git')
  assert.deepEqual(viaGit.vendors.map((vendor) => vendor.id), ['acme-ai', 'acme-pay'])
  const ai = viaGit.vendors.find((vendor) => vendor.id === 'acme-ai')
  assert.equal(ai.hosts.length, 1, 'only the untracked file counts, not the ignored one')
  assert.equal(ai.hosts[0].host, 'c.acmeai.example-vendor.net')
  const viaWalk = runScan(root, { git: false })
  assert.equal(viaWalk.fileSelection, 'walk')
  assert.equal(viaWalk.vendors.find((vendor) => vendor.id === 'acme-ai').hosts.length, 2, 'the walk does not read .gitignore')
})

// ---------------------------------------------------------------------------------------
// classification of files
// ---------------------------------------------------------------------------------------

test('classifyFile keeps runtime source and config and gives a reason for everything else', () => {
  const reason = (file) => classifyFile(file).reason
  assert.equal(classifyFile('apps/Model Plane/rust/services/inference-core/src/provider/openai.rs').scan, true)
  assert.equal(classifyFile('apps/Model Plane/deploy/docker-compose.yml').kind, 'compose')
  assert.equal(classifyFile('docker-compose.yml').kind, 'compose-legacy')
  assert.equal(classifyFile('apps/Ingestion Plane/.env.example').kind, 'env')
  assert.equal(classifyFile('.env.example').kind, 'env-legacy')
  assert.equal(classifyFile('apps/Control Plane/auth-core/package.json').kind, 'manifest')
  assert.equal(classifyFile('apps/Ingestion Plane/imports-core/requirements.txt').kind, 'manifest')
  assert.equal(reason('apps/a/src/client_test.go'), 'test')
  assert.equal(reason('apps/a/src/client.test.ts'), 'test')
  assert.equal(reason('apps/a/src/client.spec.tsx'), 'test')
  assert.equal(reason('apps/a/tests/foo.rs'), 'test')
  assert.equal(reason('apps/a/src/stream_e2e.rs'), 'test')
  assert.equal(reason('apps/a/docs/design.md'), 'doc')
  assert.equal(reason('README.md'), 'doc')
  assert.equal(reason('apps/a/plans/x.json'), 'doc')
  assert.equal(reason('apps/a/node_modules/x/y.js'), 'skipped-directory')
  assert.equal(reason('apps/a/.claude/worktrees/w/src/a.rs'), 'skipped-directory')
  assert.equal(reason('apps/a/target/debug/x.rs'), 'skipped-directory')
  assert.equal(reason('apps/a/package-lock.json'), 'lockfile')
  assert.equal(reason('apps/a/src/gen/model.ts'), 'skipped-directory')
  assert.equal(reason('apps/a/src/x.pb.go'), 'generated')
  assert.equal(reason('apps/a/sdk/python/client.py'), 'generated')
  assert.equal(reason('apps/a/scripts/deploy.sh'), 'tooling')
  assert.equal(reason('apps/a/Dockerfile'), 'tooling')
  assert.equal(reason('apps/a/.github/workflows/ci.yml'), 'tooling')
  assert.equal(reason('apps/a/migrations/0001.sql'), 'not-source')
  assert.equal(reason('apps/a/logo.png'), 'not-source')
})

test('componentOf names the plane and the service', () => {
  assert.equal(componentOf('apps/Model Plane/rust/services/inference-core/src/provider/openai.rs'), 'Model Plane/inference-core')
  assert.equal(componentOf('apps/Model Plane/deploy/docker-compose.yml'), 'Model Plane/deploy')
  assert.equal(componentOf('apps/Ingestion Plane/integration-corev2/internal/config/config.go'), 'Ingestion Plane/integration-corev2')
  assert.equal(componentOf('apps/Ingestion Plane/docker-compose.yml'), 'Ingestion Plane')
  assert.equal(componentOf('apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs'), 'Frontend Plane/verevonv3/gateway')
  assert.equal(componentOf('apps/Frontend Plane/verevonv3/src/features/x.ts'), 'Frontend Plane/verevonv3/src')
  assert.equal(componentOf('apps/Frontend Plane/verevonv3/index.html'), 'Frontend Plane/verevonv3')
  assert.equal(componentOf('docker-compose.yml'), '(root)')
})

// ---------------------------------------------------------------------------------------
// hosts
// ---------------------------------------------------------------------------------------

test('normaliseHost handles placeholders, ports, userinfo, stray braces and dynamic hosts', () => {
  assert.equal(normaliseHost('API.Example.COM/v1/x?y=1'), 'api.example.com')
  assert.equal(normaliseHost('api.example.com:8443/v1'), 'api.example.com')
  assert.equal(normaliseHost('user:pw@api.example.com/v1'), 'api.example.com')
  assert.equal(normaliseHost('{region}.tts.speech.microsoft.com/cognitiveservices'), '*.tts.speech.microsoft.com')
  assert.equal(normaliseHost('${REGION}.tts.speech.microsoft.com'), '*.tts.speech.microsoft.com')
  assert.equal(normaliseHost('%s.api.example.com'), '*.api.example.com')
  assert.equal(normaliseHost('api.linkedin.com}'), 'api.linkedin.com', 'the closing brace of ${VAR:-https://host} is not part of the host')
  assert.equal(normaliseHost('host.docker.internal:3011}'), 'host.docker.internal')
  assert.equal(normaliseHost('{shop}'), null)
  assert.equal(normaliseHost('${BASE_URL}/path'), null)
  assert.equal(normaliseHost('[::1]:8080/x'), '[::1]')
})

test('findUrlsInLine finds every scheme and stops at quotes and brackets', () => {
  const urls = findUrlsInLine('a("https://one.example-vendor.net/x"); b(\'wss://two.example-vendor.net\'); c(`http://three.example-vendor.net:80/p`)')
  assert.deepEqual(urls.map((url) => `${url.scheme}:${url.host}`), ['https:one.example-vendor.net', 'wss:two.example-vendor.net', 'http:three.example-vendor.net'])
})

test('hostMatches: exact, wildcard subdomains only, and placeholder labels', () => {
  assert.equal(hostMatches('api.openai.com', 'api.openai.com'), true)
  assert.equal(hostMatches('api.openai.com', 'x.api.openai.com'), false)
  assert.equal(hostMatches('*.openai.azure.com', 'my-res.openai.azure.com'), true)
  assert.equal(hostMatches('*.openai.azure.com', 'openai.azure.com'), false)
  assert.equal(hostMatches('*.tts.speech.microsoft.com', '*.tts.speech.microsoft.com'), true)
  assert.equal(hostMatches('*.myshopify.com', 'evil-myshopify.com'), false)
})

test('vendorForHost prefers an exact pattern over a wildcard', () => {
  const vendors = [
    { id: 'wild', hosts: ['*.example-vendor.net'] },
    { id: 'exact', hosts: ['api.example-vendor.net'] },
  ]
  assert.equal(vendorForHost('api.example-vendor.net', vendors).id, 'exact')
  assert.equal(vendorForHost('other.example-vendor.net', vendors).id, 'wild')
  assert.equal(vendorForHost('unrelated.org', vendors), null)
})

test('ignore rules: loopback, in-cluster names, placeholders, schemas and first-party domains carry a reason', () => {
  for (const host of ['localhost', '127.0.0.1', '0.0.0.0', 'host.docker.internal', '192.168.1.4', '10.0.0.7', '172.20.1.1', '[::1]']) {
    assert.equal(ignoreReasonFor(host).id, 'loopback-or-private', host)
  }
  for (const host of ['redis', 'auth-core', 'svc.cluster.local', 'db.internal', 'thing.localhost']) {
    assert.equal(ignoreReasonFor(host).id, 'in-cluster-name', host)
  }
  for (const host of ['example.com', 'foo.example.org', 'a.com', 'my-vendor.example', 'contoso.sharepoint.com', 'your-endpoint.openai.azure.com', 'x.test']) {
    assert.equal(ignoreReasonFor(host).id, 'reserved-placeholder', host)
  }
  for (const host of ['www.w3.org', 'json-schema.org', 'schemas.xmlsoap.org']) {
    assert.equal(ignoreReasonFor(host).id, 'schema-or-namespace', host)
  }
  for (const host of ['verevon.ai', 'docs.verevon.ai', 'tools.coresystem.com']) {
    assert.equal(ignoreReasonFor(host).id, 'first-party-domain', host)
  }
  assert.equal(ignoreReasonFor('api.openai.com'), null)
  assert.equal(ignoreReasonFor('notverevon.ai'), null, 'a suffix match needs the dot')
  for (const rule of IGNORE_RULES) assert.ok(rule.reason.length > 20, `${rule.id} needs a real reason`)
})

// ---------------------------------------------------------------------------------------
// reading lines
// ---------------------------------------------------------------------------------------

test('scanText splits code hits from comment hits and reads CRLF files', () => {
  const text = [
    'package a',
    '// docs: https://comment-only.example-vendor.net/guide',
    'var u = "https://code.example-vendor.net/x" // see https://trailing.example-vendor.net/y',
    '/*',
    ' * https://block.example-vendor.net/z',
    ' */',
    'var w = "https://after-block.example-vendor.net/q"',
    '',
  ].join('\r\n')
  const { hits } = scanText('src/a.go', text)
  const by = Object.fromEntries(hits.map((hit) => [hit.host, hit.where]))
  assert.deepEqual(by, {
    'comment-only.example-vendor.net': 'comment',
    'code.example-vendor.net': 'code',
    'trailing.example-vendor.net': 'comment',
    'block.example-vendor.net': 'comment',
    'after-block.example-vendor.net': 'code',
  })
  assert.equal(hits.find((hit) => hit.host === 'code.example-vendor.net').line, 3)
})

test('a URL used as an identifier (a JWT claim name, a namespace) is not a request target', () => {
  const text = 'const openAIAuthClaim = "https://api.openai.com/auth"\nconst base = "https://api.openai.com/v1"\n'
  const { hits } = scanText('src/credentials.go', text)
  assert.equal(hits[0].where, 'comment')
  assert.equal(hits[1].where, 'code')
})

test('Python docstrings and hash comments are comments; a glob in a backticked comment does not open a block', () => {
  const py = ['"""', 'Use https://docs-only.example-vendor.net/guide', '"""', 'URL = "https://real.example-vendor.net/api"', '# https://hash.example-vendor.net', ''].join('\n')
  const hits = Object.fromEntries(scanText('app/main.py', py).hits.map((hit) => [hit.host, hit.where]))
  assert.deepEqual(hits, { 'docs-only.example-vendor.net': 'comment', 'real.example-vendor.net': 'code', 'hash.example-vendor.net': 'comment' })

  const go = ['package a', '// `/internal/oauth/*` is the surface', 'var u = "https://still-code.example-vendor.net"', ''].join('\n')
  assert.deepEqual(blockCommentMask(go.split('\n'), 'go'), [false, false, false, false])
  assert.equal(scanText('src/a.go', go).hits[0].where, 'code')
})

test('isCommentAt', () => {
  assert.equal(isCommentAt('  // note', 4, 'rs'), true)
  assert.equal(isCommentAt('let a = "x"; // https://u', 20, 'rs'), true)
  assert.equal(isCommentAt('let a = "https://u";', 9, 'rs'), false)
  assert.equal(isCommentAt('# comment', 2, 'yml'), true)
  assert.equal(isCommentAt('key: https://u # why', 5, 'yml'), false)
  assert.equal(isCommentAt('key: x # https://u', 12, 'yml'), true)
})

test('rustTestLineMask skips a cfg(test) module, a cfg(test) use and a cfg(test) function, and nothing else', () => {
  const lines = [
    'const A: &str = "https://real-one.example-vendor.net";', // 0
    '#[cfg(test)]', // 1
    'mod tests {', // 2
    '    const B: &str = "https://in-test.example-vendor.net";', // 3
    '}', // 4
    'const C: &str = "https://real-two.example-vendor.net";', // 5
    '#[cfg(test)]', // 6
    'use crate::fixture::URL;', // 7
    'const D: &str = "https://real-three.example-vendor.net";', // 8
    '#[cfg(test)]', // 9
    '#[allow(dead_code)]', // 10
    'fn helper() -> &\'static str {', // 11
    '    "https://in-helper.example-vendor.net"', // 12
    '}', // 13
    'const E: &str = "https://real-four.example-vendor.net";', // 14
  ]
  const mask = rustTestLineMask(lines)
  assert.deepEqual(
    mask.map((value, index) => (value ? index : -1)).filter((index) => index >= 0),
    [1, 2, 3, 4, 6, 7, 9, 10, 11, 12, 13],
  )
  const hosts = scanText('src/lib.rs', lines.join('\n')).hits.map((hit) => hit.host)
  assert.deepEqual(hosts, ['real-one.example-vendor.net', 'real-two.example-vendor.net', 'real-three.example-vendor.net', 'real-four.example-vendor.net'])
})

// ---------------------------------------------------------------------------------------
// environment variables
// ---------------------------------------------------------------------------------------

test('envNamesInLine recognises how each language reads a variable', () => {
  const names = (line) => [...envNamesInLine(line).keys()].sort()
  assert.deepEqual(names('let k = std::env::var("ACME_API_KEY").ok();'), ['ACME_API_KEY'])
  assert.deepEqual(names('key := os.Getenv("ACME_API_KEY")'), ['ACME_API_KEY'])
  assert.deepEqual(names('Key: envOr("ACME_API_KEY", "")'), ['ACME_API_KEY'])
  assert.deepEqual(names('Key: getString("ACME_API_KEY", "")'), ['ACME_API_KEY'])
  assert.deepEqual(names('const k = process.env.ACME_API_KEY'), ['ACME_API_KEY'])
  assert.deepEqual(names("const k = process.env['ACME_API_KEY']"), ['ACME_API_KEY'])
  assert.deepEqual(names('k = os.environ["ACME_API_KEY"]'), ['ACME_API_KEY'])
  assert.deepEqual(names("k = os.getenv('ACME_API_KEY')"), ['ACME_API_KEY'])
  assert.deepEqual(names('#[arg(env = "ACME_API_KEY")]'), ['ACME_API_KEY'])
  assert.deepEqual(names('Key string `env:"ACME_API_KEY"`'), ['ACME_API_KEY'])
  assert.deepEqual(names('      ACME_API_KEY: ${ACME_API_KEY:-}'), ['ACME_API_KEY'])
  assert.deepEqual(names('      X: ${OUTER:-${INNER:-}}'), ['INNER', 'OUTER'], 'a nested default still names both variables')
  assert.deepEqual(names('const message = "no variables here"'), [])
})

test('defaultStateInLine says empty or non-empty and never returns the value', () => {
  assert.equal(defaultStateInLine('  K: ${K:-}', 'K'), 'empty-default')
  assert.equal(defaultStateInLine('  K: ${K:-https://secret-looking.example-vendor.net}', 'K'), 'non-empty-default')
  assert.equal(defaultStateInLine('  K: ${K}', 'K'), 'no-default')
  assert.equal(defaultStateInLine('  K: ${K:?K is required}', 'K'), 'required')
  assert.equal(defaultStateInLine('  RERANK: ${RERANK:-${COHERE_URL:-}}', 'RERANK'), 'empty-default', 'a default that is an empty variable is empty')
  assert.equal(defaultStateInLine('  RERANK: ${RERANK:-${COHERE_URL:-}}', 'COHERE_URL'), 'empty-default')
  assert.equal(defaultStateInLine('K=', 'K'), 'empty-default')
  assert.equal(defaultStateInLine('K=abc123', 'K'), 'non-empty-default')
  assert.equal(defaultStateInLine('      - KEY_X=value', 'K'), null)
  for (const state of ['empty-default', 'non-empty-default', 'no-default', 'required']) assert.ok(!/abc123|secret-looking/.test(state))
})

test('gating variables are verified in source and looked up in compose and env files; legacy root files are kept apart', () => {
  const root = makeTree({
    'apps/Billing Plane/pay/src/config.go': 'package pay\nfunc Load() { _ = os.Getenv("ACMEPAY_API_KEY"); _ = "https://api.acmepay.test-vendor.com" }\n',
    'apps/Billing Plane/docker-compose.yml': 'services:\n  pay:\n    environment:\n      ACMEPAY_API_KEY: ${ACMEPAY_API_KEY:-}\n',
    'docker-compose.yml': 'services:\n  legacy:\n    environment:\n      - ACMEPAY_API_KEY=${ACMEPAY_API_KEY:-hunter2hunter2}\n',
  })
  const vendor = runScan(root).vendors.find((item) => item.id === 'acme-pay')
  const gate = vendor.gates[0]
  assert.equal(gate.read, 'literal')
  assert.equal(gate.firstRead, 'apps/Billing Plane/pay/src/config.go:2')
  assert.equal(gate.wiredIn, 2)
  assert.equal(gate.firstWiringKind, 'compose')
  assert.equal(gate.defaultState, 'empty-default', 'the canonical file decides; the legacy default is not mixed in')
  assert.equal(vendor.configWiring, 'compose')
  assert.ok(!renderJson(runScan(root)).includes('hunter2'), 'a default value must never reach the output')
})

test('configWiring: none, legacy-compose-only, env-example-only, ungated, config-flag, per-user-connection', () => {
  const root = makeTree({
    'apps/P/a/src/a.go': 'var u = "https://one.vend-one.org"\nvar v = "https://two.vend-two.org"\nvar w = "https://three.vend-three.org"\nvar x = "https://four.vend-four.org"\nvar y = "https://five.vend-five.org"\n',
    'apps/P/a/src/env.go': 'var a = os.Getenv("ONE_API_KEY"); var b = os.Getenv("TWO_API_KEY"); var c = os.Getenv("THREE_API_KEY")\n',
    'docker-compose.yml': 'services:\n  s:\n    environment:\n      TWO_API_KEY: ${TWO_API_KEY:-}\n',
    'apps/P/a/.env.example': 'THREE_API_KEY=\n',
  })
  const vendors = [
    { id: 'one', name: 'One', class: 'subprocessor', hosts: ['one.vend-one.org'], gateEnv: ['ONE_API_KEY'] },
    { id: 'two', name: 'Two', class: 'subprocessor', hosts: ['two.vend-two.org'], gateEnv: ['TWO_API_KEY'] },
    { id: 'three', name: 'Three', class: 'subprocessor', hosts: ['three.vend-three.org'], gateEnv: ['THREE_API_KEY'] },
    { id: 'four', name: 'Four', class: 'subprocessor', hosts: ['four.vend-four.org'], gateEnv: [] },
    { id: 'five', name: 'Five', class: 'user-authorised', hosts: ['five.vend-five.org'], gateEnv: [], gatedBy: 'a flag', wiring: 'per-user-connection' },
  ]
  const result = scan({ root, vendors, internalEnv: [] })
  const wiring = Object.fromEntries(result.vendors.map((vendor) => [vendor.id, vendor.configWiring]))
  assert.deepEqual(wiring, { one: 'none', two: 'legacy-compose-only', three: 'env-example-only', four: 'ungated', five: 'per-user-connection' })
})

// ---------------------------------------------------------------------------------------
// SDKs, signals, comment-only, unclassified
// ---------------------------------------------------------------------------------------

test('scanManifest reads runtime dependencies and skips dev ones', () => {
  const pkg = ['{', '  "name": "x",', '  "dependencies": {', '    "resend": "^6.0.0",', '    "zod": "^3"', '  },', '  "devDependencies": {', '    "vitest": "^2"', '  }', '}', ''].join('\n')
  assert.deepEqual(scanManifest('app/package.json', pkg).map((dependency) => dependency.name), ['resend', 'zod'])
  const cargo = ['[package]', 'name = "x"', '[dependencies]', 'aws-sdk-s3 = "1"', 'reqwest = { version = "0.12" }', '[dev-dependencies]', 'wiremock = "0.6"', ''].join('\n')
  assert.deepEqual(scanManifest('crate/Cargo.toml', cargo).map((dependency) => dependency.name), ['aws-sdk-s3', 'reqwest'])
  const gomod = ['module x', '', 'require (', '\tgithub.com/novuhq/novu-go/v3 v3.1.0', '\tgithub.com/gin-gonic/gin v1.9.0 // indirect', ')', ''].join('\n')
  assert.deepEqual(scanManifest('svc/go.mod', gomod).map((dependency) => dependency.name), ['github.com/novuhq/novu-go/v3', 'github.com/gin-gonic/gin'])
  const req = ['# comment', 'notion-client==2.2.1', 'hubspot-api-client>=9', ''].join('\n')
  assert.deepEqual(scanManifest('svc/requirements.txt', req).map((dependency) => dependency.name), ['notion-client', 'hubspot-api-client'])
})

test('a vendor whose host is hidden by an SDK is found through its declared dependency', () => {
  const root = makeTree({
    'apps/C/auth/package.json': '{\n  "dependencies": {\n    "resend": "^6"\n  }\n}\n',
    'apps/C/auth/src/mail.ts': 'const key = process.env.RESEND_API_KEY\n',
  })
  const result = scan({
    root,
    vendors: [{ id: 'resend', name: 'Resend', class: 'subprocessor', hosts: ['api.resend.com'], sdks: [{ ecosystem: 'npm', name: 'resend' }], gateEnv: ['RESEND_API_KEY'] }],
    internalEnv: [],
  })
  const vendor = result.vendors[0]
  assert.equal(vendor.evidenceStatus, 'env-only')
  assert.deepEqual(vendor.sdks.map((sdk) => sdk.origin), ['apps/C/auth/package.json:3'])
  assert.deepEqual(vendor.components, ['C/auth'])
})

test('a host that appears only in a comment is a comment-only host, never a strict failure; a code host nobody classified is', () => {
  const root = makeTree({
    'apps/P/a/src/a.go': 'package a\n// see https://docs-only.example-vendor.net/x\nvar u = "https://mystery.unknown-vendor.org/api"\n',
  })
  const result = scan({ root, vendors: [], internalEnv: [] })
  assert.deepEqual(result.commentOnlyHosts.map((item) => item.host), ['docs-only.example-vendor.net'])
  assert.equal(result.unclassified.length, 1)
  assert.equal(result.unclassified[0].host, 'mystery.unknown-vendor.org')
  assert.equal(result.unclassified[0].evidence[0].origin, 'apps/P/a/src/a.go:3')
  const problems = strictProblems(result)
  assert.equal(problems.length, 1)
  assert.match(problems[0], /unclassified host mystery\.unknown-vendor\.org/)
})

test('comment-only vendor evidence is reported as comment-only and gets no config wiring', () => {
  const root = makeTree({ 'apps/P/a/src/a.go': 'package a\n// the vendor lives at https://hosted.vend-six.org/docs\n' })
  const result = scan({ root, vendors: [{ id: 'six', name: 'Six', class: 'subprocessor', hosts: ['hosted.vend-six.org'], gateEnv: [] }], internalEnv: [] })
  assert.equal(result.vendors[0].evidenceStatus, 'comment-only')
  assert.equal(result.vendors[0].configWiring, 'n/a')
})

test('a signal in a named file finds a vendor that has no host and no variable', () => {
  const root = makeTree({
    'apps/I/svc/internal/mailtask/postmark.go': 'package mailtask\ntype postmarkInboundPayload struct{}\n',
    'apps/I/svc/internal/mailtask/postmark_test.go': 'package mailtask\ntype postmarkInboundPayload struct{}\n',
  })
  const result = scan({
    root,
    vendors: [{ id: 'postmark', name: 'Postmark', class: 'subprocessor', hosts: [], gateEnv: [], signals: [{ label: 'receiver', path: 'internal/mailtask/[^/]+\\.go$', pattern: 'postmark' }] }],
    internalEnv: [],
  })
  assert.equal(result.vendors[0].evidenceStatus, 'signal-only')
  assert.deepEqual(result.vendors[0].signals[0].evidence, ['apps/I/svc/internal/mailtask/postmark.go:2'])
})

test('credential-like variable names no vendor claims fail strict until a vendor or a reviewed reason covers them', () => {
  const root = makeTree({
    'apps/P/a/src/a.ts': 'const a = process.env.NEWVENDOR_API_KEY\nconst b = process.env.SERVICE_X_SERVICE_TOKEN\nconst c = process.env.ACMEPAY_API_KEY\n',
  })
  const vendors = [{ id: 'acme-pay', name: 'Acme Pay', class: 'subprocessor', hosts: [], gateEnv: ['ACMEPAY_API_KEY'] }]
  const open = scan({ root, vendors, internalEnv: INTERNAL_ENV })
  assert.deepEqual(open.unmappedCredentialNames.map((item) => item.name), ['NEWVENDOR_API_KEY'])
  assert.deepEqual(open.reviewedInternalNames.map((item) => item.name), ['SERVICE_X_SERVICE_TOKEN'])
  assert.ok(strictProblems(open).some((problem) => problem.includes('NEWVENDOR_API_KEY')))
  const covered = scan({ root, vendors: [...vendors, { id: 'newvendor', name: 'New', class: 'subprocessor', hosts: [], gateEnv: ['NEWVENDOR_API_KEY'] }], internalEnv: INTERNAL_ENV })
  assert.deepEqual(covered.unmappedCredentialNames, [])
})

test('strict also fails on a stale catalogue entry and on a curated variable that exists nowhere', () => {
  const root = makeTree({ 'apps/P/a/src/a.go': 'package a\nvar u = "https://api.acmepay.test-vendor.com"\n' })
  const result = scan({ root, vendors: CATALOGUE, internalEnv: [] })
  const problems = strictProblems(result)
  assert.ok(problems.some((problem) => /catalogue vendor "ghost-vendor"/.test(problem)))
  assert.ok(problems.some((problem) => /curated variable ACMEPAY_API_KEY/.test(problem)))
})

test('Rust config crates: PREFIX__FIELD_NAME is verified through the struct field', () => {
  const root = makeTree({
    'apps/I/edge/src/config.rs': 'pub struct EdgeConfig {\n    pub brave_search_key: Option<String>,\n}\n',
  })
  const result = scan({
    root,
    vendors: [{ id: 'brave', name: 'Brave', class: 'subprocessor', hosts: [], gateEnv: ['QUARRY_EDGE__BRAVE_SEARCH_KEY'] }],
    internalEnv: [],
  })
  const gate = result.vendors[0].gates[0]
  assert.equal(gate.read, 'rust-config-field')
  assert.equal(gate.firstRead, 'apps/I/edge/src/config.rs:2')
  assert.equal(result.vendors[0].evidenceStatus, 'env-only')
})

// ---------------------------------------------------------------------------------------
// the register check
// ---------------------------------------------------------------------------------------

test('verifyDocument: every found vendor needs a row; EXCLUDED needs a reason; unknown ids fail', () => {
  const root = makeTree({
    'apps/B/p/src/a.go': 'package p\nvar u = "https://api.acmepay.test-vendor.com"\nvar v = "https://x.acmeai.example-vendor.net"\n',
  })
  const result = runScan(root)
  const good = [
    '| Id | Vendor | Disposition |',
    '|---|---|---|',
    '| `acme-pay` | Acme Pay | LISTED |',
    '| `acme-ai` | Acme AI | EXCLUDED: customer-directed connector, not a sub-processor |',
    '| `ghost-vendor` | Ghost | LISTED |',
    '',
  ].join('\r\n')
  assert.deepEqual(verifyDocument(result, good), [])

  const missing = good.replace('| `acme-pay` | Acme Pay | LISTED |\r\n', '')
  assert.match(verifyDocument(result, missing).join('\n'), /vendor "acme-pay".*no row/)

  const noReason = good.replace('EXCLUDED: customer-directed connector, not a sub-processor', 'EXCLUDED')
  assert.match(verifyDocument(result, noReason).join('\n'), /"acme-ai" is marked EXCLUDED without a reason/)

  const shortReason = good.replace('EXCLUDED: customer-directed connector, not a sub-processor', 'EXCLUDED: n/a')
  assert.match(verifyDocument(result, shortReason).join('\n'), /without a reason/)

  const unknown = `${good}| \`made-up\` | Nobody | LISTED |\n`
  assert.match(verifyDocument(result, unknown).join('\n'), /"made-up" is not a catalogue vendor id/)

  const noDisposition = good.replace('| `acme-pay` | Acme Pay | LISTED |', '| `acme-pay` | Acme Pay | maybe |')
  assert.match(verifyDocument(result, noDisposition).join('\n'), /neither LISTED nor EXCLUDED/)

  const otherTables = [
    good,
    '| Class | Meaning |',
    '|---|---|',
    '| `subprocessor` | not a vendor id |',
    '',
    '| Vendor | Reading |',
    '|---|---|',
    '| `acme-pay` | a second table that mentions the vendor without a disposition |',
    '',
  ].join('\n')
  assert.deepEqual(verifyDocument(result, otherTables), [], 'only the table headed "Id" is the register')
})

// ---------------------------------------------------------------------------------------
// the built-in catalogue
// ---------------------------------------------------------------------------------------

test('the built-in catalogue is well formed: unique ids, known classes, no guessed region or DPA, no duplicate hosts', () => {
  const classes = new Set(['subprocessor', 'user-authorised', 'infrastructure', 'public-source'])
  const roles = new Set(['processor', 'independent-recipient', 'customer-directed', 'no-content', 'unknown'])
  const ids = new Set()
  const patterns = new Map()
  for (const vendor of VENDORS) {
    assert.match(vendor.id, /^[a-z0-9][a-z0-9-]*$/, `id ${vendor.id}`)
    assert.ok(!ids.has(vendor.id), `duplicate id ${vendor.id}`)
    ids.add(vendor.id)
    assert.ok(classes.has(vendor.class), `${vendor.id}: class ${vendor.class}`)
    assert.ok(roles.has(vendor.role), `${vendor.id}: role ${vendor.role}`)
    assert.ok(vendor.name && vendor.name.length >= 3, `${vendor.id}: name`)
    assert.ok(vendor.region === undefined && vendor.dpa === undefined, `${vendor.id}: region and DPA must stay TBC in the tool, never be guessed`)
    for (const pattern of vendor.hosts) {
      assert.match(pattern, /^(\*\.)?[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/, `${vendor.id}: host pattern ${pattern}`)
      assert.ok(!patterns.has(pattern), `host pattern ${pattern} is in both ${patterns.get(pattern)} and ${vendor.id}`)
      patterns.set(pattern, vendor.id)
    }
    for (const name of vendor.gateEnv ?? []) assert.match(name, /^[A-Z][A-Z0-9]*(?:_{1,2}[A-Z0-9]+)+$/, `${vendor.id}: variable ${name}`)
    for (const signal of vendor.signals ?? []) {
      assert.doesNotThrow(() => new RegExp(signal.path), `${vendor.id}: signal path`)
      assert.doesNotThrow(() => new RegExp(signal.pattern, 'i'), `${vendor.id}: signal pattern`)
    }
    if (vendor.class === 'subprocessor') assert.ok(vendor.note || vendor.id.length > 0)
  }
  for (const rule of INTERNAL_ENV) assert.ok(rule.reason.length > 15)
  const claimedBy = new Map()
  for (const vendor of VENDORS) {
    for (const name of vendor.gateEnv ?? []) {
      assert.ok(!claimedBy.has(name) || claimedBy.get(name) === vendor.id, `${name} is claimed by both ${claimedBy.get(name)} and ${vendor.id}`)
      claimedBy.set(name, vendor.id)
    }
  }
})

test('the built-in catalogue classifies the vendors the plan names', () => {
  const byHost = (host) => vendorForHost(host, VENDORS)?.id
  assert.equal(byHost('api.openai.com'), 'openai-api')
  assert.equal(byHost('api.anthropic.com'), 'anthropic-api')
  assert.equal(byHost('chatgpt.com'), 'openai-chatgpt-subscription')
  assert.equal(byHost('api.cognitive.microsofttranslator.com'), 'microsoft-azure-ai')
  assert.equal(byHost('*.tts.speech.microsoft.com'), 'microsoft-azure-ai')
  assert.equal(byHost('core-ai-rg.cognitiveservices.azure.com'), 'microsoft-azure-ai')
  assert.equal(byHost('api.cohere.ai'), 'cohere-api')
  assert.equal(byHost('api.search.brave.com'), 'brave-search')
  assert.equal(byHost('google.serper.dev'), 'serper-search')
  assert.equal(byHost('eu.api.novu.co'), 'novu')
  assert.equal(byHost('api.stripe.com'), 'stripe-billing')
  assert.equal(byHost('test.api.dibspayment.eu'), 'nexi-checkout')
  assert.equal(byHost('graph.microsoft.com'), 'microsoft-365-entra-graph')
  assert.equal(byHost('gmail.googleapis.com'), 'google-accounts-apis')
  assert.equal(byHost('postmarkapp.com'), 'postmark')
})

// ---------------------------------------------------------------------------------------
// the command line
// ---------------------------------------------------------------------------------------

function capture() {
  const out = []
  const err = []
  return { io: { out: (text) => out.push(text), err: (text) => err.push(text) }, out, err }
}

test('main: --json prints the result, --strict exits 1 on an unclassified host, a usage error exits 2', () => {
  const root = makeTree({ 'apps/P/a/src/a.go': 'package a\nvar u = "https://mystery.unknown-vendor.org/api"\n' })
  const catalogue = path.join(root, 'catalogue.json')
  fs.writeFileSync(catalogue, JSON.stringify(CATALOGUE))

  const plain = capture()
  assert.equal(main(['--root', root, '--catalogue', catalogue, '--json'], plain.io), 0)
  const parsed = JSON.parse(plain.out.join(''))
  assert.equal(parsed.tool, 'scan-egress')
  assert.equal(parsed.unclassified[0].host, 'mystery.unknown-vendor.org')

  const strict = capture()
  assert.equal(main(['--root', root, '--catalogue', catalogue, '--strict'], strict.io), 1)
  assert.match(strict.err.join(''), /strict: unclassified host mystery\.unknown-vendor\.org/)

  const summary = capture()
  assert.equal(main(['--root', root, '--catalogue', catalogue, '--summary'], summary.io), 0)
  assert.match(summary.out.join(''), /^scan-egress summary/)
  assert.match(summary.out.join(''), /unclassified mystery\.unknown-vendor\.org/)
  assert.equal(main(['--root', root, '--catalogue', catalogue, '--json', '--summary'], capture().io), 2, 'one output form at a time')

  const bad = capture()
  assert.equal(main(['--no-such-flag'], bad.io), 2)
  assert.equal(main(['--root'], capture().io), 2)
  assert.equal(main(['--catalogue', path.join(root, 'missing.json'), '--root', root], capture().io), 2)
})

test('main: --verify-doc passes for a complete register and fails for an incomplete one', () => {
  const root = makeTree({ 'apps/B/p/src/a.go': 'package p\nvar u = "https://api.acmepay.test-vendor.com"\n' })
  const catalogue = path.join(root, 'catalogue.json')
  fs.writeFileSync(catalogue, JSON.stringify(CATALOGUE.slice(0, 1)))
  const complete = path.join(root, 'complete.md')
  const header = '| Id | Vendor | Disposition |\n|---|---|---|\n'
  fs.writeFileSync(complete, `${header}| \`acme-pay\` | Acme Pay | LISTED |\n`)
  const incomplete = path.join(root, 'incomplete.md')
  fs.writeFileSync(incomplete, `${header}| \`somebody-else\` | Other | LISTED |\n`)

  assert.equal(main(['--root', root, '--catalogue', catalogue, '--verify-doc', complete], capture().io), 0)
  const failing = capture()
  assert.equal(main(['--root', root, '--catalogue', catalogue, '--verify-doc', incomplete], failing.io), 1)
  assert.match(failing.err.join(''), /acme-pay.*no row in the register/)
  assert.equal(main(['--root', root, '--catalogue', catalogue, '--verify-doc', path.join(root, 'nope.md')], capture().io), 2)
})
