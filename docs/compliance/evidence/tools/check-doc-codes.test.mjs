// Tests for check-doc-codes.mjs. Run: node --test docs/compliance/evidence/tools/*.test.mjs
//
// The unit tests build a throwaway repository in the temp directory. The last test reads the
// REAL documents, the real registry and the real source, and fails (it is never skipped) when
// a document presents a code as existing that the source does not carry.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  CODE_LIKE,
  DEFAULT_ROOT,
  checkDocs,
  contractNumbers,
  extractCodes,
  isImplementationFile,
  loadRegistry,
  main,
  runtimeText,
  scanText,
} from './check-doc-codes.mjs'

function makeRepo(files, { git = false } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'doccodes-repo-'))
  for (const [relative, content] of Object.entries(files)) {
    const target = path.join(root, relative)
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, content)
  }
  if (git) execFileSync('git', ['init', '-q'], { cwd: root })
  return root
}

function codesOf(result) {
  return result.failures.map((failure) => failure.code)
}

const BASE = {
  built: { zdr_unavailable: ['apps/svc/src/a.rs'] },
  designed: { zdr_waiver: { task: 'Z-14', note: 'claim' } },
  terms: ['zdr_ephemeral'],
  scan: ['docs/a.md'],
}

test('extractCodes returns only whole backtick spans that look like a code, with their line', () => {
  const text = [
    'A `zdr_unavailable` refusal and `zdr_waiver` claim.',
    'Prose with zdr_bare_word outside backticks and `zdr: true` and `not_a_zdr_code`.',
    'Switch `ZDR_WAIVER_ENABLED` and header `x-retention-decision`, scope `retention:zdr-decide`.',
  ].join('\n')
  assert.deepEqual(
    extractCodes(text).map((c) => [c.code, c.line]),
    [
      ['zdr_unavailable', 1],
      ['zdr_waiver', 1],
      ['ZDR_WAIVER_ENABLED', 3],
      ['x-retention-decision', 3],
      ['retention:zdr-decide', 3],
    ],
  )
  assert.ok(CODE_LIKE.length >= 2)
})

test('a built code that the source carries passes; a missing file or a missing string fails built-missing', () => {
  const root = makeRepo({ 'apps/svc/src/a.rs': 'const K: &str = "zdr_unavailable";\n', 'docs/a.md': 'See `zdr_unavailable`.\n' })
  assert.deepEqual(codesOf(checkDocs({ root, registry: BASE, probeDesigned: false })), [])

  const gone = makeRepo({ 'docs/a.md': 'See `zdr_unavailable`.\n' })
  assert.deepEqual(codesOf(checkDocs({ root: gone, registry: BASE, probeDesigned: false })), ['built-missing'])

  const wrong = makeRepo({ 'apps/svc/src/a.rs': 'const K: &str = "other";\n', 'docs/a.md': 'See `zdr_unavailable`.\n' })
  const result = checkDocs({ root: wrong, registry: BASE, probeDesigned: false })
  assert.deepEqual(codesOf(result), ['built-missing'])
  assert.match(result.failures[0].message, /present .*zdr_unavailable.* as existing/)
})

test('a built code with no source file listed is itself a failure (nothing proves it)', () => {
  const root = makeRepo({ 'docs/a.md': 'x\n' })
  const registry = { ...BASE, built: { zdr_unavailable: [] } }
  assert.deepEqual(codesOf(checkDocs({ root, registry, probeDesigned: false })), ['built-missing'])
})

// A "built" code must be emitted by the running program. A comment that explains it, a test that
// pins it and a longer name that merely starts with it are all things a rename of the real emission
// leaves behind, so none of them may prove it.
const RUST_COMMENT_ONLY = [
  '// The refusal `zdr_unavailable` is sent here.',
  '/* and `zdr_unavailable` /* nested */ still a comment */',
  'fn status() -> u8 { 1 }',
  '',
].join('\n')

const RUST_TEST_ONLY = [
  "fn pick<'a>(x: &'a str) -> &'a str { x }",
  '#[cfg(test)]',
  'mod tests {',
  '    fn braces() { let open = "{"; let close = \'}\'; let raw = r#"}"#; assert_eq!(open, "{"); }',
  '    fn pins() { assert_eq!(code(), "zdr_unavailable"); }',
  '}',
  '#[cfg(test)]',
  'fn helper() -> &\'static str { "zdr_unavailable" }',
  '#[cfg(test)]',
  'use some::thing as zdr_unavailable;',
  '',
].join('\n')

// Real code after an early test module, as in model-gateway's sse.rs: a brace inside a string or a
// character inside the module must not make the scanner skip the code that follows it.
const RUST_CODE_AFTER_TEST_MODULE = [
  '#[cfg(test)]',
  'mod early {',
  '    fn t() { let s = "}}"; let c = \'{\'; let e = \'\\\'\'; assert!(s.len() == 2 && c != e); }',
  '}',
  'fn emit() -> &\'static str { "zdr_unavailable" }',
  '#[cfg(not(test))]',
  'const NOT_TEST: &str = "kept";',
  '',
].join('\n')

test('runtimeText drops comments and #[cfg(test)] items from Rust, and keeps strings and the code after a test module', () => {
  const comment = runtimeText(RUST_COMMENT_ONLY, 'a.rs')
  assert.doesNotMatch(comment, /zdr_unavailable/)
  assert.match(comment, /fn status\(\) -> u8 \{ 1 \}/)

  const onlyTests = runtimeText(RUST_TEST_ONLY, 'a.rs')
  assert.doesNotMatch(onlyTests, /zdr_unavailable|mod tests|helper/)
  assert.match(onlyTests, /fn pick<'a>/)

  const after = runtimeText(RUST_CODE_AFTER_TEST_MODULE, 'a.rs')
  assert.match(after, /"zdr_unavailable"/)
  assert.match(after, /NOT_TEST: &str = "kept"/, '#[cfg(not(test))] is runtime code')
  assert.doesNotMatch(after, /mod early/)

  // A URL in a string is not a comment, in any language.
  assert.match(runtimeText('let u = "http://x/zdr_unavailable";\n', 'a.rs'), /zdr_unavailable/)
  assert.match(runtimeText("const u = 'http://x'; const c = 'zdr_unavailable'; // not this\n", 'a.ts'), /'zdr_unavailable'/)
  assert.doesNotMatch(runtimeText("// zdr_unavailable\n/* zdr_unavailable */\n/**\n * zdr_unavailable\n */\nconst x = 1\n", 'a.ts'), /zdr_unavailable/)
  assert.match(runtimeText('var s = `zdr_unavailable // kept`\n', 'a.ts'), /zdr_unavailable/)
  assert.doesNotMatch(runtimeText('// zdr_unavailable\n/* zdr_unavailable */\nfunc f() {}\n', 'a.go'), /zdr_unavailable/)
  assert.match(runtimeText('var c = "zdr_unavailable" // x\nvar r = `zdr_unavailable`\n', 'a.go'), /"zdr_unavailable"/)
  // Files that are not source in these languages are read as they are.
  assert.match(runtimeText('{"code":"zdr_unavailable"} // kept\n', 'a.json'), /zdr_unavailable/)
  // Line endings do not matter and line breaks survive blanking.
  assert.equal(runtimeText('a\r\n// c\r\nb\r\n', 'a.rs').split('\n').length, 4)
})

test('a built code that only a comment, a test or a longer name carries is built-missing (a renamed emission cannot hide behind them)', () => {
  const failing = (file, content) => {
    const registry = { ...BASE, built: { zdr_unavailable: [file] } }
    const root = makeRepo({ [file]: content, 'docs/a.md': 'See `zdr_unavailable`.\n' })
    return codesOf(checkDocs({ root, registry, probeDesigned: false }))
  }
  assert.deepEqual(failing('apps/svc/src/a.rs', RUST_COMMENT_ONLY), ['built-missing'])
  assert.deepEqual(failing('apps/svc/src/a.rs', RUST_TEST_ONLY), ['built-missing'])
  assert.deepEqual(failing('apps/svc/src/a.rs', RUST_CODE_AFTER_TEST_MODULE), [])
  assert.deepEqual(failing('apps/svc/src/a.rs', 'fn zdr_unavailable_status() {}\nconst K: &str = "zdr_unavailable_other";\n'), ['built-missing'])
  assert.deepEqual(failing('apps/svc/src/a.ts', "// zdr_unavailable\n/** zdr_unavailable */\nconst x = 'other'\n"), ['built-missing'])
  assert.deepEqual(failing('apps/svc/src/a.ts', "const e = { error: 'zdr_unavailable' }\n"), [])
  assert.deepEqual(failing('apps/svc/src/a.go', '// zdr_unavailable\nconst k = "other"\n'), ['built-missing'])
  assert.deepEqual(failing('apps/svc/src/a.go', 'const k = "zdr_unavailable"\n'), [])
  // The message says what the check read, so a reader knows why a file that mentions the code failed.
  const registry = { ...BASE, built: { zdr_unavailable: ['apps/svc/src/a.rs'] } }
  const root = makeRepo({ 'apps/svc/src/a.rs': RUST_COMMENT_ONLY, 'docs/a.md': 'x\n' })
  assert.match(checkDocs({ root, registry, probeDesigned: false }).failures[0].message, /outside comments and test code/)
})

test('a code-like token in a scanned document that is in no list is unregistered-code', () => {
  const root = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'New `zdr_made_up` code. See `zdr_unavailable`.\n' })
  const result = checkDocs({ root, registry: BASE, probeDesigned: false })
  assert.deepEqual(codesOf(result), ['unregistered-code'])
  assert.match(result.failures[0].message, /zdr_made_up/)
})

test('a term (a word a document defines) passes without a source file', () => {
  const root = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'Class `zdr_ephemeral`.\n' })
  assert.deepEqual(codesOf(checkDocs({ root, registry: BASE, probeDesigned: false })), [])
})

test('a document that names a designed code must say "not built"', () => {
  const unlabelled = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'The `zdr_waiver` claim carries it.\n' })
  assert.deepEqual(codesOf(checkDocs({ root: unlabelled, registry: BASE, probeDesigned: false })), ['unbuilt-not-labelled'])

  const labelled = makeRepo({
    'apps/svc/src/a.rs': 'zdr_unavailable',
    'docs/a.md': 'The `zdr_waiver` claim carries it (designed, Not Built).\n',
  })
  assert.deepEqual(codesOf(checkDocs({ root: labelled, registry: BASE, probeDesigned: false })), [])
})

test('a line that names a designed code and says "built" is designed-claimed-built even when the document says "not built" elsewhere', () => {
  const doc = (state) => ['The waiver is not built.', '', `| \`zdr_waiver\` | the claim | ${state} |`, ''].join('\n')
  const rooted = (state) => makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': doc(state) })
  // The document-level rule is satisfied by the first sentence, so only the line rule can catch the relabelled row.
  const bad = checkDocs({ root: rooted('Built'), registry: BASE, probeDesigned: false })
  assert.deepEqual(codesOf(bad), ['designed-claimed-built'])
  assert.match(bad.failures[0].message, /zdr_waiver.*"built" and not "not built"/)
  assert.deepEqual(codesOf(checkDocs({ root: rooted('Designed, not built'), registry: BASE, probeDesigned: false })), [])
  assert.deepEqual(codesOf(checkDocs({ root: rooted('Not yet built'), registry: BASE, probeDesigned: false })), [])
  // A line that says "built" but names no designed code is not this rule's business.
  const other = makeRepo({
    'apps/svc/src/a.rs': 'zdr_unavailable',
    'docs/a.md': 'The waiver is not built.\nThe `zdr_unavailable` refusal is built.\n',
  })
  assert.deepEqual(codesOf(checkDocs({ root: other, registry: BASE, probeDesigned: false })), [])
})

test('a bounded scan reads only its section and fails when the section is gone', () => {
  const doc = ['# Ledger', '## 2026-01-01 — Old', 'Old `zdr_old_code` here.', '## 2026-02-02 — Mine', 'Mine `zdr_unavailable`.', '## 2026-03-03 — Next', 'Next `zdr_other`.'].join('\n')
  const root = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/ledger.md': doc })
  const scan = [{ file: 'docs/ledger.md', from: '## 2026-02-02 — Mine', to: '## 2026-03-03' }]
  assert.deepEqual(codesOf(checkDocs({ root, registry: { ...BASE, scan }, probeDesigned: false })), [])
  assert.match(scanText(scan[0], root).text, /Mine/)
  assert.doesNotMatch(scanText(scan[0], root).text, /Old|Next/)

  const gone = [{ file: 'docs/ledger.md', from: '## 2099 — Missing', to: '## 2026-03-03' }]
  assert.deepEqual(codesOf(checkDocs({ root, registry: { ...BASE, scan: gone }, probeDesigned: false })), ['scan-missing'])
  assert.deepEqual(codesOf(checkDocs({ root, registry: { ...BASE, scan: ['docs/none.md'] }, probeDesigned: false })), ['scan-missing'])
})

test('requirements: mustContain and mustNotContain, each with its reason', () => {
  const root = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'Old text stays. See `zdr_unavailable`.\n' })
  const requirements = [
    { file: 'docs/a.md', mustContain: 'New text', why: 'rewritten' },
    { file: 'docs/a.md', mustNotContain: 'Old text', why: 'stale' },
    { file: 'docs/missing.md', mustContain: 'x', why: 'exists' },
  ]
  const result = checkDocs({ root, registry: { ...BASE, requirements }, probeDesigned: false })
  assert.deepEqual(codesOf(result), ['required-missing', 'forbidden-present', 'required-missing'])
  assert.match(result.failures[0].message, /rewritten/)
  assert.match(result.failures[1].message, /stale/)
})

test('contractNumbers reads the numbered rules of section 3 only, and the check finds a gap, a repeat and too few', () => {
  const matrix = (numbers) =>
    ['# M', '## 2. Other', '99. **not a contract**', '## 3. Non-negotiable', ...numbers.map((n) => `${n}. **Rule ${n}.** text`), '## 4. Notes', '1. **later**'].join('\n')
  assert.deepEqual(contractNumbers(matrix([1, 2, 3])).map((c) => c.number), [1, 2, 3])

  const root = (numbers) => makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'x', 'apps/m.md': matrix(numbers) })
  const registry = { ...BASE, scan: ['docs/a.md'], contracts: { file: 'apps/m.md', minimum: 3 } }
  assert.deepEqual(codesOf(checkDocs({ root: root([1, 2, 3]), registry, probeDesigned: false })), [])
  assert.deepEqual(codesOf(checkDocs({ root: root([1, 2, 4]), registry, probeDesigned: false })), ['rule-numbering'])
  assert.deepEqual(codesOf(checkDocs({ root: root([1, 2, 2, 3]), registry, probeDesigned: false })), ['rule-numbering'])
  assert.deepEqual(codesOf(checkDocs({ root: root([1, 2]), registry, probeDesigned: false })), ['rule-numbering'])
  const noSection = makeRepo({ 'docs/a.md': 'x', 'apps/svc/src/a.rs': 'zdr_unavailable', 'apps/m.md': '# nothing\n' })
  assert.deepEqual(codesOf(checkDocs({ root: noSection, registry, probeDesigned: false })), ['rule-numbering'])
})

test('CRLF documents are read the same as LF documents', () => {
  const root = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'Line `zdr_unavailable`.\r\nLine `zdr_waiver` is not built.\r\n' })
  assert.deepEqual(codesOf(checkDocs({ root, registry: BASE, probeDesigned: false })), [])
})

test('a designed code that a source file now carries is a WARNING, never a failure', () => {
  const root = makeRepo(
    { 'apps/svc/src/a.rs': 'zdr_unavailable', 'apps/svc/src/b.rs': 'const W: &str = "zdr_waiver";', 'docs/a.md': 'The `zdr_waiver` claim is not built.\n' },
    { git: true },
  )
  const result = checkDocs({ root, registry: BASE, probeDesigned: true })
  assert.deepEqual(result.failures, [])
  assert.deepEqual(result.warnings.map((w) => w.code), ['designed-now-built'])
  assert.match(result.warnings[0].message, /zdr_waiver.*Z-14.*now in source/)
})

test('a golden, a fixture or a test that names a designed code is a specification, not an implementation', () => {
  assert.equal(isImplementationFile('apps/Model Plane/rust/services/model-gateway/src/normalize.rs'), true)
  assert.equal(isImplementationFile('apps/Control Plane/auth-core/src/auth/plane-token.controller.ts'), true)
  for (const spec of [
    'apps/Model Plane/api/contracts/zdr-posture-matrix.json',
    'apps/Control Plane/auth-core/src/auth/fixtures/x.json',
    'apps/Ingestion Plane/integration-corev2/internal/api/testdata/y.json',
    'apps/Model Plane/rust/services/model-gateway/tests/z.rs',
    'apps/Control Plane/auth-core/src/auth/convex-token.service.spec.ts',
    'apps/Frontend Plane/verevonv3/src/shared/i18n/errors.test.ts',
    'apps/Ingestion Plane/integration-corev2/internal/api/server_test.go',
    'apps/Model Plane/x/a.golden.json',
  ]) {
    assert.equal(isImplementationFile(spec), false, spec)
  }

  // And through the probe: a designed code that only a fixture carries gives no warning.
  const root = makeRepo(
    {
      'apps/svc/src/a.rs': 'zdr_unavailable',
      'apps/api/contracts/matrix.json': '{"expect":"zdr_waiver"}',
      'docs/a.md': 'The `zdr_waiver` claim is not built.\n',
    },
    { git: true },
  )
  const result = checkDocs({ root, registry: BASE, probeDesigned: true })
  assert.deepEqual(result.failures, [])
  assert.deepEqual(result.warnings, [])
})

test('main: exits 0 on a clean registry, 1 on a failure, 2 on a usage error', () => {
  const root = makeRepo({ 'apps/svc/src/a.rs': 'zdr_unavailable', 'docs/a.md': 'See `zdr_unavailable`.\n' })
  const registryPath = path.join(root, 'registry.json')
  fs.writeFileSync(registryPath, JSON.stringify(BASE))
  const lines = []
  const io = { out: (line) => lines.push(line), err: (line) => lines.push(line) }
  assert.equal(main(['--registry', registryPath, '--root', root, '--no-probe'], io), 0)
  assert.match(lines.at(-1), /0 failure\(s\)/)

  fs.writeFileSync(path.join(root, 'docs/a.md'), 'See `zdr_unregistered`.\n')
  assert.equal(main(['--registry', registryPath, '--root', root, '--no-probe'], io), 1)
  assert.ok(lines.some((line) => line.startsWith('FAIL [unregistered-code]')))

  assert.equal(main(['--bogus'], io), 2)
})

test('THE REAL TREE: every code the ZDR documents present as existing is in source, none is unclassified, the rules are numbered', () => {
  const registry = loadRegistry()
  const result = checkDocs({ root: DEFAULT_ROOT, registry, probeDesigned: false })
  assert.deepEqual(result.failures, [])
  assert.ok(result.summary.built >= 8, 'the registry must keep proving the built codes')
  assert.ok(result.summary.designed >= 8, 'the registry must keep naming the designed codes')
})
