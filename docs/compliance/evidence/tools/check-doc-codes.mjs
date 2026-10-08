#!/usr/bin/env node
// check-doc-codes.mjs -- fail when a document names a refusal code, claim or switch that
// the source does not carry and the document does not say is unbuilt.
//
// The zero-retention (ZDR) documents describe two kinds of thing: behaviour that is in
// the code, and a design that is not built yet (the per-connection waiver, task Z-30 of
// the ZDR build). A reader cannot tell them apart by the sentence alone, and a design
// word that reads like a shipped code is exactly how a document ends up claiming more
// than the code does. This checker keeps the two apart with a registry
// (`doc-codes.json`, next to this file):
//
//   built      code -> the source files that must contain it. A code the documents present
//              as existing must be in at least one of those files, in code the program runs
//              (not a comment, not a Rust `#[cfg(test)]` item, not part of a longer name),
//              or the check fails (the OVERCLAIM direction, the one that matters).
//   designed   code -> { task, note }. A code the documents present as designed only. If a
//              source file now carries it, the check WARNS that the registry and the
//              documents are stale (the understatement direction; the final documents
//              task re-classifies it). A warning never fails the check.
//   terms      words that look like codes but are defined by a document, not by source.
//
// Rules, each reported with a stable `code`:
//   unregistered-code     a backticked token that looks like a ZDR code appears in a scanned
//                         document and is in none of the three lists above;
//   built-missing         a `built` code is not in a listed source file (or the file is gone);
//   unbuilt-not-labelled  a document names a `designed` code but never says "not built";
//   designed-claimed-built  a line names a `designed` code and says "built" without "not built"
//                         (a table row relabelled "Built" while the document says "not built"
//                         elsewhere passes the document-level rule above, not this one);
//   required-missing     a `requirements` row (mustContain) is absent from its document;
//   forbidden-present     a `requirements` row (mustNotContain) is still in its document;
//   rule-numbering        the numbered cross-plane contracts of the ownership matrix are not
//                         1..N without a gap or a repeat.
//
// Section scanning: a `scan` entry may bound a long document ({ file, from, to }): only the
// text from the first line that starts with `from` up to the line that starts with `to` is
// read. The decision ledger is one such file; its older entries name codes this check does
// not own.
//
// Exit status: 0 no failure (warnings may be printed), 1 at least one failure, 2 usage error.

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = path.dirname(fileURLToPath(import.meta.url))
export const DEFAULT_REGISTRY = path.join(HERE, 'doc-codes.json')
export const DEFAULT_ROOT = path.resolve(HERE, '..', '..', '..', '..')

/**
 * What counts as "looks like a ZDR code": a whole backtick span that is a snake_case word
 * starting `zdr_`, or one of the named identifiers of the waiver design. Anything else in
 * backticks is prose and is not this checker's business.
 */
export const CODE_LIKE = [
  /^zdr_[a-z0-9_]+$/,
  /^(?:retention_posture_unavailable|posture_mismatch|waiver_disabled|ZDR_WAIVER_ENABLED|x-retention-decision|retention:zdr-decide)$/,
]

const SOURCE_PATHSPECS = ['*.rs', '*.ts', '*.tsx', '*.go', '*.sql', '*.json', '*.sh', '*.yml']

export function readText(absolute) {
  return fs.readFileSync(absolute, 'utf8').replace(/\r\n/g, '\n')
}

// ---------------------------------------------------------------------------------------------
// What "the source carries a code" means.
//
// A code that a document presents as built must be EMITTED by the running program. A source file
// also mentions a code in a comment that explains it, in a test that pins it and inside a longer
// name that merely starts with it, and those are exactly what is left behind when the real
// emission is renamed. So the check reads the file as the running program does: comments blanked,
// Rust `#[cfg(test)]` items removed, and the code matched as a whole token. Only Rust, TypeScript
// and Go source is read this way; any other file (json, sh, yml, sql) is read as it is.
//
// This is a scanner, not a parser. It knows string, raw-string, character and comment syntax so a
// brace or a `//` inside a literal is not mistaken for code structure; it does not know regular
// expression literals or nested template substitutions. Where it errs it errs towards reporting
// `built-missing`, which a person then looks at, never towards passing.
// ---------------------------------------------------------------------------------------------

const LANGUAGE_BY_EXTENSION = new Map([
  ['.rs', 'rust'],
  ['.ts', 'ts'],
  ['.tsx', 'ts'],
  ['.js', 'ts'],
  ['.mjs', 'ts'],
  ['.cjs', 'ts'],
  ['.go', 'go'],
])

const isIdentifierChar = (character) => character !== undefined && /[A-Za-z0-9_]/.test(character)

/** Index just past the string, raw string or character literal that starts at `i`, or `i` when none does. */
function literalEnd(text, i, language) {
  const character = text[i]
  const length = text.length
  const scanQuoted = (from, quote) => {
    let j = from
    while (j < length) {
      if (text[j] === '\\') j += 2
      else if (text[j] === quote) return j + 1
      else j += 1
    }
    return length
  }
  if (language === 'rust') {
    if ((character === 'r' || (character === 'b' && text[i + 1] === 'r')) && !isIdentifierChar(text[i - 1])) {
      const raw = /^b?r(#*)"/.exec(text.slice(i, i + 80))
      if (raw) {
        const closing = `"${raw[1]}`
        const close = text.indexOf(closing, i + raw[0].length)
        return close === -1 ? length : close + closing.length
      }
    }
    if (character === '"') return scanQuoted(i + 1, '"')
    if (character === "'") {
      if (text[i + 1] === '\\') return scanQuoted(i + 2, "'")
      // `'x'` is a character literal; `'a` (no closing quote two on) is a lifetime or a label.
      if (text[i + 2] === "'" && text[i + 1] !== undefined) return i + 3
      return i
    }
    return i
  }
  if (language === 'go') {
    if (character === '"') return scanQuoted(i + 1, '"')
    if (character === '`') {
      const close = text.indexOf('`', i + 1)
      return close === -1 ? length : close + 1
    }
    if (character === "'") return scanQuoted(i + 1, "'")
    return i
  }
  if (language === 'ts') {
    if (character === '"' || character === "'" || character === '`') return scanQuoted(i + 1, character)
    return i
  }
  return i
}

/** `text` with every comment replaced by nothing (its line breaks are kept), literals untouched. */
function blankComments(text, language) {
  const out = []
  const length = text.length
  let i = 0
  while (i < length) {
    const character = text[i]
    if (character === '/' && text[i + 1] === '/') {
      while (i < length && text[i] !== '\n') i += 1
      continue
    }
    if (character === '/' && text[i + 1] === '*') {
      let depth = 1
      i += 2
      while (i < length && depth > 0) {
        if (language === 'rust' && text[i] === '/' && text[i + 1] === '*') {
          depth += 1
          i += 2
        } else if (text[i] === '*' && text[i + 1] === '/') {
          depth -= 1
          i += 2
        } else {
          if (text[i] === '\n') out.push('\n')
          i += 1
        }
      }
      continue
    }
    const end = literalEnd(text, i, language)
    if (end > i) {
      out.push(text.slice(i, end))
      i = end
      continue
    }
    out.push(character)
    i += 1
  }
  return out.join('')
}

/** Index just past the `[...]` attribute that opens at `open` (the `[`), literals respected. */
function attributeEnd(text, open) {
  let depth = 0
  let i = open
  while (i < text.length) {
    const end = literalEnd(text, i, 'rust')
    if (end > i) {
      i = end
      continue
    }
    if (text[i] === '[') depth += 1
    else if (text[i] === ']') {
      depth -= 1
      if (depth === 0) return i + 1
    }
    i += 1
  }
  return text.length
}

/** Rust `text` (comments already blanked) without the items gated by `#[cfg(test)]`. */
function removeCfgTestItems(text) {
  const marker = /#\[cfg\(test\)\]/g
  const kept = []
  let last = 0
  let match
  while ((match = marker.exec(text)) !== null) {
    let i = match.index + match[0].length
    for (;;) {
      while (i < text.length && /\s/.test(text[i])) i += 1
      if (text.startsWith('#[', i)) i = attributeEnd(text, i + 1)
      else break
    }
    // The item ends at the `}` that closes its first `{`, or at a `;` outside any bracket.
    let braces = 0
    let brackets = 0
    let end = text.length
    for (let j = i; j < text.length; ) {
      const literal = literalEnd(text, j, 'rust')
      if (literal > j) {
        j = literal
        continue
      }
      const character = text[j]
      if (character === '(' || character === '[') brackets += 1
      else if (character === ')' || character === ']') brackets -= 1
      else if (character === '{') braces += 1
      else if (character === '}') {
        braces -= 1
        if (braces === 0) {
          end = j + 1
          break
        }
      } else if (character === ';' && braces === 0 && brackets <= 0) {
        end = j + 1
        break
      }
      j += 1
    }
    kept.push(text.slice(last, match.index))
    last = end
    marker.lastIndex = end
  }
  kept.push(text.slice(last))
  return kept.join('')
}

/** The text of `file` as the running program has it: no comments, no Rust test items. */
export function runtimeText(text, file) {
  const language = LANGUAGE_BY_EXTENSION.get(path.extname(file).toLowerCase())
  if (!language) return text
  const code = blankComments(text.replace(/\r\n/g, '\n'), language)
  return language === 'rust' ? removeCfgTestItems(code) : code
}

const escapeRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** Whether `file`'s running code carries `code` as a whole token (`a_b` is not found in `a_b_c`). */
export function carriesCode(code, text, file) {
  return new RegExp(`(?<![A-Za-z0-9_])${escapeRegExp(code)}(?![A-Za-z0-9_])`).test(runtimeText(text, file))
}

/** Every backticked token in `text` that looks like a code, with the 1-based line it is on. */
export function extractCodes(text) {
  const found = []
  const lines = text.split('\n')
  lines.forEach((line, index) => {
    for (const match of line.matchAll(/`([^`\n]+)`/g)) {
      const token = match[1].trim()
      if (CODE_LIKE.some((pattern) => pattern.test(token))) found.push({ code: token, line: index + 1 })
    }
  })
  return found
}

/** The text a scan entry covers. A bounded entry returns only its section. */
export function scanText(entry, root) {
  const file = typeof entry === 'string' ? entry : entry.file
  const absolute = path.join(root, file)
  if (!fs.existsSync(absolute)) return { file, missing: true, text: '' }
  const text = readText(absolute)
  if (typeof entry === 'string' || !entry.from) return { file, text }
  const lines = text.split('\n')
  const start = lines.findIndex((line) => line.startsWith(entry.from))
  if (start === -1) return { file, text: '', sectionMissing: entry.from }
  let end = lines.length
  if (entry.to) {
    const stop = lines.findIndex((line, index) => index > start && line.startsWith(entry.to))
    if (stop !== -1) end = stop
  }
  return { file, text: lines.slice(start, end).join('\n') }
}

/**
 * A golden, a fixture or a test that names a code is the SPECIFICATION of a behaviour, not the
 * behaviour: the posture matrix of task Z-10 lists the designed refusal codes as expected
 * outputs before any plane emits them. Only implementation files count as "built".
 */
export function isImplementationFile(file) {
  if (/(^|\/)(tests?|fixtures?|testdata|contracts)\//i.test(file)) return false
  if (/\.(test|spec)\.[a-z]+$/i.test(file) || /_test\.go$/.test(file)) return false
  if (/\.golden\.[a-z]+$/i.test(file)) return false
  return true
}

function sourceFilesCarrying(code, root) {
  try {
    const out = execFileSync(
      'git',
      // A plain pathspec (not :(glob)) lets `*` cross directory separators, so `apps/*.rs` is every Rust file under apps.
      ['grep', '--untracked', '-l', '-I', '-F', '-e', code, '--', ...SOURCE_PATHSPECS.map((spec) => `apps/${spec}`)],
      { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 16 * 1024 * 1024 },
    )
    return out.split('\n').filter(Boolean).filter(isImplementationFile)
  } catch (error) {
    // git grep exits 1 for "no match" and a different status when it cannot run at all.
    if (error && error.status === 1) return []
    return null
  }
}

/** The numbered contracts of the ownership matrix, as { number, line }. */
export function contractNumbers(text) {
  const lines = text.split('\n')
  const start = lines.findIndex((line) => /^## 3\. /.test(line))
  if (start === -1) return null
  let end = lines.findIndex((line, index) => index > start && /^## /.test(line))
  if (end === -1) end = lines.length
  const numbers = []
  for (let index = start + 1; index < end; index += 1) {
    const match = /^(\d+)\. \*\*/.exec(lines[index])
    if (match) numbers.push({ number: Number(match[1]), line: index + 1 })
  }
  return numbers
}

export function checkDocs({ root = DEFAULT_ROOT, registry, probeDesigned = true } = {}) {
  const failures = []
  const warnings = []
  const fail = (code, message, where = null) => failures.push({ code, message, where })
  const built = registry.built ?? {}
  const designed = registry.designed ?? {}
  const terms = new Set(registry.terms ?? [])
  const known = new Set([...Object.keys(built), ...Object.keys(designed), ...terms])

  for (const [code, files] of Object.entries(built)) {
    if (!Array.isArray(files) || files.length === 0) {
      fail('built-missing', `built code ${code} lists no source file to prove it`)
      continue
    }
    const carrying = files.filter((file) => {
      const absolute = path.join(root, file)
      return fs.existsSync(absolute) && carriesCode(code, readText(absolute), file)
    })
    if (carrying.length === 0) {
      fail(
        'built-missing',
        `the documents present ${code} as existing, but none of its source files carries it outside comments and test code: ${files.join(', ')}`,
      )
    }
  }

  if (probeDesigned) {
    for (const [code, info] of Object.entries(designed)) {
      const carrying = sourceFilesCarrying(code, root)
      if (carrying === null) {
        warnings.push({ code: 'probe-unavailable', message: `could not search the source for designed code ${code} (git grep failed)` })
        break
      }
      if (carrying.length > 0) {
        warnings.push({
          code: 'designed-now-built',
          message: `${code} (task ${info.task ?? '?'}) is now in source (${carrying[0]}${carrying.length > 1 ? ` and ${carrying.length - 1} more` : ''}); move it to "built" and update the documents`,
        })
      }
    }
  }

  for (const entry of registry.scan ?? []) {
    const scanned = scanText(entry, root)
    if (scanned.missing) {
      fail('scan-missing', `scanned document does not exist: ${scanned.file}`)
      continue
    }
    if (scanned.sectionMissing) {
      fail('scan-missing', `${scanned.file}: the section starting "${scanned.sectionMissing}" was not found`)
      continue
    }
    const codes = extractCodes(scanned.text)
    let namesDesigned = false
    for (const { code, line } of codes) {
      if (!known.has(code)) {
        fail('unregistered-code', `${code} is named in ${scanned.file} (section line ${line}) but is in none of built, designed or terms of the registry`, scanned.file)
      }
      if (Object.hasOwn(designed, code)) namesDesigned = true
    }
    if (namesDesigned && !/not built/i.test(scanned.text)) {
      fail('unbuilt-not-labelled', `${scanned.file} names a designed code but never says "not built"`, scanned.file)
    }
    // The document-level rule above is satisfied by one "not built" anywhere. A table row or a
    // sentence that names a designed code and calls it built, in a document that says "not built"
    // elsewhere, would pass it, so the same claim is also checked line by line.
    scanned.text.split('\n').forEach((line, index) => {
      if (!/\bbuilt\b/i.test(line) || /\bnot (?:yet )?built\b|\bunbuilt\b/i.test(line)) return
      const named = [...line.matchAll(/`([^`\n]+)`/g)].map((match) => match[1].trim()).filter((token) => Object.hasOwn(designed, token))
      if (named.length > 0) {
        fail('designed-claimed-built', `${scanned.file} (section line ${index + 1}) names the designed code ${named[0]} on a line that says "built" and not "not built"`, scanned.file)
      }
    })
  }

  for (const row of registry.requirements ?? []) {
    const absolute = path.join(root, row.file)
    if (!fs.existsSync(absolute)) {
      fail('required-missing', `requirement file does not exist: ${row.file}`, row.file)
      continue
    }
    const text = readText(absolute)
    if (row.mustContain !== undefined && !text.includes(row.mustContain)) {
      fail('required-missing', `${row.file} must contain ${JSON.stringify(row.mustContain)} (${row.why})`, row.file)
    }
    if (row.mustNotContain !== undefined && text.includes(row.mustNotContain)) {
      fail('forbidden-present', `${row.file} still contains ${JSON.stringify(row.mustNotContain)} (${row.why})`, row.file)
    }
  }

  if (registry.contracts) {
    const { file, minimum } = registry.contracts
    const absolute = path.join(root, file)
    if (!fs.existsSync(absolute)) {
      fail('rule-numbering', `ownership matrix does not exist: ${file}`, file)
    } else {
      const numbers = contractNumbers(readText(absolute))
      if (numbers === null) {
        fail('rule-numbering', `${file} has no "## 3." section of numbered contracts`, file)
      } else {
        // Report the first break only: one gap or repeat shifts every later number.
        const broken = numbers.findIndex(({ number }, index) => number !== index + 1)
        if (broken !== -1) {
          fail('rule-numbering', `${file}:${numbers[broken].line}: contract ${numbers[broken].number} is out of sequence (expected ${broken + 1})`, file)
        }
        if (numbers.length < (minimum ?? 1)) {
          fail('rule-numbering', `${file} carries ${numbers.length} numbered contracts; at least ${minimum} are required`, file)
        }
      }
    }
  }

  return {
    failures,
    warnings,
    summary: {
      built: Object.keys(built).length,
      designed: Object.keys(designed).length,
      scanned: (registry.scan ?? []).length,
    },
  }
}

export function loadRegistry(file = DEFAULT_REGISTRY) {
  return JSON.parse(fs.readFileSync(file, 'utf8'))
}

export function main(argv = process.argv.slice(2), io = { out: console.log, err: console.error }) {
  let registryPath = DEFAULT_REGISTRY
  let root = DEFAULT_ROOT
  let probe = true
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--registry') registryPath = path.resolve(argv[++index] ?? '')
    else if (arg === '--root') root = path.resolve(argv[++index] ?? '')
    else if (arg === '--no-probe') probe = false
    else {
      io.err(`usage: check-doc-codes.mjs [--registry <file>] [--root <repo>] [--no-probe]\nunknown argument: ${arg}`)
      return 2
    }
  }
  const result = checkDocs({ root, registry: loadRegistry(registryPath), probeDesigned: probe })
  for (const warning of result.warnings) io.out(`warning [${warning.code}] ${warning.message}`)
  for (const failure of result.failures) io.out(`FAIL [${failure.code}] ${failure.message}`)
  io.out(
    `check-doc-codes: ${result.summary.scanned} scan entries, ${result.summary.built} built codes, ` +
      `${result.summary.designed} designed codes, ${result.warnings.length} warning(s), ${result.failures.length} failure(s)`,
  )
  return result.failures.length === 0 ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main())
}
