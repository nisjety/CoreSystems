// Tests for check-citations.mjs. Run: node --test docs/compliance/evidence/tools/*.test.mjs
//
// Each test builds a throwaway repository in the temp directory, so nothing here
// depends on the state of the real working tree.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { checkDocument, main, parseCitation, splitRow } from './check-citations.mjs'

function makeRepo(files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cite-repo-'))
  for (const [relative, content] of Object.entries(files)) {
    const target = path.join(root, relative)
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, content)
  }
  return root
}

const HEADER = 'Engineering evidence, not legal advice.\n\n'

function check(root, body, options = {}) {
  return checkDocument(HEADER + body, { root, label: 'doc.md', ...options })
}

function codes(result) {
  return result.failures.map((failure) => failure.code)
}

const FIVE_LINES = 'one\ntwo\nthree\nfour\nfive\n'

test('a citation to an existing line, a range and a comma list passes (LF and CRLF files)', () => {
  const root = makeRepo({
    'src/lf.rs': FIVE_LINES,
    'src/crlf.rs': FIVE_LINES.replace(/\n/g, '\r\n'),
    'dir with space/file name.rs': FIVE_LINES,
  })
  const result = check(
    root,
    'See `src/lf.rs:1`, `src/lf.rs:2-5`, `src/crlf.rs:1,3,5` and `dir with space/file name.rs:4`.\n',
  )
  assert.deepEqual(result.failures, [])
  assert.equal(result.citations.length, 4)
})

test('the last line is in range, the line after it is not (a trailing newline adds no line)', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES, 'no-newline.rs': 'one\ntwo' })
  assert.deepEqual(codes(check(root, '`a.rs:5` `no-newline.rs:2`\n')), [])
  const past = check(root, '`a.rs:6`\n')
  assert.deepEqual(codes(past), ['line-out-of-range'])
  assert.match(past.failures[0].message, /line 6 is past the end of a\.rs \(5 lines\)/)
  const pastTwo = check(root, '`no-newline.rs:3`\n')
  assert.deepEqual(codes(pastTwo), ['line-out-of-range'])
})

test('a missing file fails with missing-file', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const result = check(root, 'Claim `src/not/there.rs:10`.\n')
  assert.deepEqual(codes(result), ['missing-file'])
  assert.equal(result.failures[0].line, 3)
  assert.match(result.failures[0].message, /does not exist/)
})

test('a directory is not a file', () => {
  const root = makeRepo({ 'dir.rs/inner.txt': 'x\n' })
  assert.deepEqual(codes(check(root, '`dir.rs:1`\n')), ['not-a-file'])
})

test('an out-of-range line fails, including a range end past the file and a range that runs backwards', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  assert.deepEqual(codes(check(root, '`a.rs:400`\n')), ['line-out-of-range'])
  const end = check(root, '`a.rs:3-9`\n')
  assert.deepEqual(codes(end), ['line-out-of-range'])
  assert.match(end.failures[0].message, /ends at 9/)
  const backwards = check(root, '`a.rs:5-2`\n')
  assert.deepEqual(codes(backwards), ['line-out-of-range'])
  assert.match(backwards.failures[0].message, /backwards/)
  assert.deepEqual(codes(check(root, '`a.rs:0`\n')), ['line-out-of-range'])
  assert.deepEqual(codes(check(root, '`a.rs:2,99`\n')), ['line-out-of-range'])
})

test('an anchor inside the cited lines passes, even across whitespace differences', () => {
  const root = makeRepo({ 'a.rs': 'fn alpha() {}\n    pub   enum   Residency {\n}\nfn gamma() {}\n' })
  assert.deepEqual(codes(check(root, '`a.rs:2 @"pub enum Residency"`\n')), [])
  assert.deepEqual(codes(check(root, '`a.rs:1-3 @"fn alpha"`\n')), [])
  assert.deepEqual(codes(check(root, '`a.rs:1,4 @"fn gamma"`\n')), [])
})

test('a moved anchor fails and says where the text is now', () => {
  // The citation was written when `fn target` sat on line 2. A line was added above it.
  const root = makeRepo({ 'a.rs': 'fn first() {}\n// new comment\nfn target() {}\nfn last() {}\n' })
  const result = check(root, '`a.rs:2 @"fn target"`\n')
  assert.deepEqual(codes(result), ['anchor-moved'])
  assert.match(result.failures[0].message, /now at line 3 of a\.rs/)
})

test('an anchor that is nowhere in the file fails with anchor-missing', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const result = check(root, '`a.rs:1-5 @"deleted function"`\n')
  assert.deepEqual(codes(result), ['anchor-missing'])
})

test('an over-long or empty anchor is refused', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  assert.deepEqual(codes(check(root, `\`a.rs:1 @"${'x'.repeat(81)}"\`\n`)), ['anchor-too-long'])
  assert.deepEqual(codes(check(root, '`a.rs:1 @""`\n')), ['malformed'])
})

test('a malformed citation is reported, never silently ignored', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  for (const span of ['a.rs:1\u20133', 'a.rs:abc', 'a.rs:', 'a.rs:1-', 'a.rs:1;2', 'a.rs: 3']) {
    const result = check(root, `Bad \`${span}\`.\n`)
    assert.deepEqual(codes(result), ['malformed'], `span ${JSON.stringify(span)}`)
  }
})

test('code spans that are not file citations are left alone', () => {
  const root = makeRepo({})
  const result = check(root, 'Ports `5432:5432`, host `localhost:8080`, `http://example.com:443/x`, `mod.rs`, `fn foo()`.\n')
  assert.deepEqual(result.failures, [])
  assert.equal(result.citations.length, 0)
})

test('paths that are not the code of record are refused', () => {
  const root = makeRepo({ '.claude/worktrees/w/a.rs': FIVE_LINES, 'target/debug/b.rs': FIVE_LINES, 'a.rs': FIVE_LINES })
  for (const span of [
    '.claude/worktrees/w/a.rs:1',
    'target/debug/b.rs:1',
    '../outside.rs:1',
    '/etc/passwd.txt:1',
    'C:\\Users\\x\\a.rs:1',
    'src\\a.rs:1',
    'node_modules/pkg/index.js:1',
  ]) {
    const result = check(root, `\`${span}\`\n`)
    assert.deepEqual(codes(result), ['forbidden-path'], `span ${JSON.stringify(span)}`)
  }
})

test('a cited file with NUL bytes is reported', () => {
  const root = makeRepo({ 'zero.rs': Buffer.concat([Buffer.from('one\ntwo\n'), Buffer.alloc(16), Buffer.from('\nthree\n')]) })
  assert.deepEqual(codes(check(root, '`zero.rs:1`\n')), ['nul-bytes'])
})

test('fenced code blocks are skipped so the syntax can be shown', () => {
  const root = makeRepo({})
  const result = check(root, 'Example:\n\n```\n`nowhere/file.rs:99`\n```\n\n~~~\n`also/missing.rs:1`\n~~~\n')
  assert.deepEqual(result.failures, [])
  const outside = check(root, '```\n`inside.rs:1`\n```\n`outside.rs:1`\n')
  assert.deepEqual(codes(outside), ['missing-file'])
})

test('a table with an Evidence column needs a valid citation or UNVERIFIED in every data row', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const table = [
    '| Claim | Evidence |',
    '|---|---|',
    '| proven | `a.rs:2` |',
    '| not proven yet | UNVERIFIED, needs the live env |',
    '| nothing at all | just words |',
    '| broken citation | `gone.rs:1` |',
    '| escaped \\| pipe | `a.rs:1-2` |',
    '',
  ].join('\n')
  const result = check(root, table)
  assert.deepEqual(codes(result).sort(), ['evidence-missing', 'evidence-missing', 'missing-file'])
  assert.equal(result.unverifiedRows, 1)
  const lines = result.failures.filter((failure) => failure.code === 'evidence-missing').map((failure) => failure.line)
  assert.deepEqual(lines, [7, 8])
})

test('the rule applies only to tables with an Evidence column, and only to data rows', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const noEvidenceColumn = '| Claim | Note |\n|---|---|\n| x | no citation |\n'
  assert.deepEqual(check(root, noEvidenceColumn).failures, [])
  const second = '| A | Evidence |\n|---|---|\n| ok | `a.rs:1` |\n\ntext\n\n| B | Note |\n|---|---|\n| y | nothing |\n'
  assert.deepEqual(check(root, second).failures, [])
  assert.deepEqual(codes(check(root, noEvidenceColumn, { requireEvidenceRows: false })), [])
})

test('a pipe inside an anchor in a table cell is written \\| and is read back as |', () => {
  const root = makeRepo({ 'a.rs': 'let ok = left || right;\nother\n' })
  const table = '| Claim | Evidence |\n|---|---|\n| or | `a.rs:1 @"left \\|\\| right"` |\n'
  assert.deepEqual(check(root, table).failures, [])
  const wrong = '| Claim | Evidence |\n|---|---|\n| or | `a.rs:2 @"left \\|\\| right"` |\n'
  assert.deepEqual(codes(check(root, wrong)), ['anchor-moved', 'evidence-missing'])
})

test('the Evidence column is found wherever it sits and tolerates bold or case', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const table = '| Route | **evidence** | Region |\n|:--|:-:|--:|\n| r | `a.rs:3` | x |\n| s | nothing | y |\n'
  const result = check(root, table)
  assert.deepEqual(codes(result), ['evidence-missing'])
})

test('a document must say it is not legal advice near the top', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const without = checkDocument('# Title\n\nSome text `a.rs:1`.\n', { root, label: 'x.md' })
  assert.deepEqual(codes(without), ['header-missing'])
  const late = checkDocument(`${'filler\n'.repeat(25)}not legal advice\n`, { root, label: 'x.md' })
  assert.deepEqual(codes(late), ['header-missing'])
  const present = checkDocument('# Title\n\n> Engineering evidence, NOT LEGAL ADVICE.\n', { root, label: 'x.md' })
  assert.deepEqual(present.failures, [])
  const skipped = checkDocument('# Title\n', { root, label: 'x.md', requireHeader: false })
  assert.deepEqual(skipped.failures, [])
})

test('parseCitation understands the grammar and rejects the rest', () => {
  assert.deepEqual(parseCitation('a/b.rs:12').ranges, [{ from: 12, to: 12 }])
  assert.deepEqual(parseCitation('a/b.rs:12-20,30').ranges, [{ from: 12, to: 20 }, { from: 30, to: 30 }])
  assert.equal(parseCitation('a/b.rs:12 @"x y"').anchor, 'x y')
  assert.equal(parseCitation('no colon.rs'), null)
  assert.equal(parseCitation('plain text'), null)
  assert.equal(parseCitation('a/b.rs:x').ok, false)
  assert.equal(parseCitation('.\\x\\y.go:5').ok, true)
})

test('splitRow honours escaped pipes and optional outer pipes', () => {
  assert.deepEqual(splitRow('| a | b \\| c | d |'), ['a', 'b | c', 'd'])
  assert.deepEqual(splitRow('a | b'), ['a', 'b'])
})

test('main: exit codes, --show and --json', () => {
  const root = makeRepo({ 'a.rs': FIVE_LINES })
  const goodDoc = path.join(root, 'good.md')
  const badDoc = path.join(root, 'bad.md')
  fs.writeFileSync(goodDoc, `${HEADER}Cited: \`a.rs:2-3\`.\n`)
  fs.writeFileSync(badDoc, `${HEADER}Cited: \`gone.rs:2\`.\n`)
  const run = (argv) => {
    let out = ''
    let err = ''
    const status = main(argv, { out: (text) => { out += text }, err: (text) => { err += text } })
    return { status, out, err }
  }

  const good = run(['--root', root, goodDoc])
  assert.equal(good.status, 0)
  assert.match(good.out, /1 document\(s\), 1 citation\(s\), 0 UNVERIFIED row\(s\), 0 failure\(s\)/)

  const bad = run(['--root', root, badDoc])
  assert.equal(bad.status, 1)
  assert.match(bad.out, /bad\.md:3: \[missing-file\]/)

  const show = run(['--root', root, '--show', goodDoc])
  assert.equal(show.status, 0)
  assert.match(show.out, /--- good\.md:3 {2}a\.rs:2-3/)
  assert.match(show.out, / {5}2 {2}two/)
  assert.match(show.out, / {5}3 {2}three/)

  const json = run(['--root', root, '--json', badDoc])
  assert.equal(json.status, 1)
  assert.equal(JSON.parse(json.out).failures[0].code, 'missing-file')

  const missing = run(['--root', root, path.join(root, 'nope.md')])
  assert.equal(missing.status, 2)
  assert.match(missing.err, /no such file/)

  const dir = run(['--root', root, root])
  assert.equal(dir.status, 1)
  assert.match(dir.out, /2 document\(s\)/)

  assert.equal(run(['--bogus']).status, 2)
})

test('a citation into a .diff record is checked like any other (the old public wording lives there)', () => {
  const diff = '# header\n-\t\told wording that the edit removed\n+\t\tnew wording\n'
  const root = makeRepo({ 'docs/record.diff': diff })
  const good = check(root, 'Old text: `docs/record.diff:2 @"old wording that the edit removed"`.\n')
  assert.deepEqual(good.failures, [])
  assert.equal(good.citations.length, 1)
  const moved = check(root, 'Old text: `docs/record.diff:3 @"old wording that the edit removed"`.\n')
  assert.deepEqual(codes(moved), ['anchor-moved'])
  const past = check(root, 'Old text: `docs/record.diff:9`.\n')
  assert.deepEqual(codes(past), ['line-out-of-range'])
})
