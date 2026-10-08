#!/usr/bin/env node
// check-citations.mjs -- fail when an evidence document cites code that is not
// there.
//
// An evidence document makes claims about code. Each claim carries a citation in
// a backtick span:
//
//   `apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs:463-489`
//   `apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs:1156,1537`
//   `apps/Model Plane/rust/services/inference-core/src/provider/mod.rs:489 @"pub enum Residency"`
//
// Grammar:   `<repo-relative path>:<lines>[ @"<anchor>"]`
//            lines  = range ("," range)*        range = N | N-M      (1-based, inclusive)
//            anchor = a short string (80 chars at most) that must appear inside the
//                     cited lines. It is what catches DRIFT: a line number that is
//                     still in range but now points at other code.
//
// The checker fails when
//   * the cited path does not exist, or is not a file, or lies inside a copied
//     worktree, a build directory or node_modules (not evidence);
//   * a line is past the end of the file, or a range runs backwards;
//   * an anchor is not found in the cited lines (the message says where it moved);
//   * a backtick span looks like a citation (a path with a known extension, then a
//     colon) but does not fit the grammar -- nothing is ever silently ignored;
//   * a table with an "Evidence" column has a data row with no valid citation that is
//     not marked UNVERIFIED (so an unproven row cannot pass as a proven one);
//   * the document does not say "not legal advice" in its first 20 lines.
//
// Fenced code blocks are skipped, so a document can show the syntax. A cited file
// that contains NUL bytes is reported (an abrupt shutdown can leave a working-tree
// file zero-filled; a line number into it means nothing).
//
// Exit status: 0 all good, 1 at least one failure, 2 usage error.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = path.dirname(fileURLToPath(import.meta.url))

/** Extensions a citation path may have. A span with one of these plus a colon is a citation candidate. */
export const CITATION_EXTENSIONS = [
  'rs', 'go', 'ts', 'tsx', 'js', 'mjs', 'cjs', 'json', 'yml', 'yaml', 'toml', 'sql', 'sh',
  'proto', 'md', 'py', 'conf', 'env', 'txt', 'html', 'css', 'mod', 'xml',
  // trust-page-edits.diff is the record of the public wording the trust edits replaced, so a row can
  // cite the old text after the source no longer carries it.
  'diff',
]

// The optional leading "X:" lets a Windows drive-letter path be recognised as a
// citation (and then refused as absolute) instead of being silently ignored.
const CANDIDATE = new RegExp('^((?:[A-Za-z]:)?[^`:\\n]*\\.(?:' + CITATION_EXTENSIONS.join('|') + ')):(.*)$', 'i')
const RANGE = /^\d+(?:-\d+)?$/
const ANCHOR_SUFFIX = /^(.*?)\s+@"([^"]*)"\s*$/
const MAX_ANCHOR = 80
const HEADER_LINES = 20
const NOT_EVIDENCE_SEGMENTS = ['.claude', 'node_modules', 'target', '.git']

/**
 * Parse one backtick span. Returns null when the span is not a citation
 * candidate at all, or { ok: true, ... } / { ok: false, problem }.
 */
export function parseCitation(span) {
  const candidate = CANDIDATE.exec(span.trim())
  if (!candidate) return null
  const raw = span.trim()
  let rest = candidate[2]
  let anchor = null
  const withAnchor = ANCHOR_SUFFIX.exec(rest)
  if (withAnchor) {
    rest = withAnchor[1]
    anchor = withAnchor[2]
  }
  const pathPart = candidate[1].trim()
  if (/^\s/.test(candidate[2])) {
    return { ok: false, raw, path: pathPart, problem: 'no space is allowed after the colon (write path:12, not path: 12)' }
  }
  const ranges = []
  for (const piece of rest.trim().split(',')) {
    const text = piece.trim()
    if (!RANGE.test(text)) {
      return { ok: false, raw, path: pathPart, problem: `malformed line spec "${rest.trim()}" (want N, N-M or a comma list)` }
    }
    const [from, to] = text.split('-').map(Number)
    ranges.push({ from, to: to === undefined ? from : to })
  }
  if (anchor !== null && anchor.trim() === '') {
    return { ok: false, raw, path: pathPart, problem: 'empty anchor' }
  }
  if (anchor !== null && anchor.length > MAX_ANCHOR) {
    return { ok: false, raw, path: pathPart, problem: `anchor is ${anchor.length} characters; keep it under ${MAX_ANCHOR}`, code: 'anchor-too-long' }
  }
  return { ok: true, raw, path: pathPart, ranges, anchor }
}

function normalizeWhitespace(text) {
  return text.replace(/\s+/g, ' ').trim()
}

/** Lines of a file as strings, without CR, plus a NUL flag. A final newline does not add a line. */
export function loadLines(absolutePath, cache) {
  if (cache.has(absolutePath)) return cache.get(absolutePath)
  const buffer = fs.readFileSync(absolutePath)
  const nul = buffer.includes(0)
  const text = buffer.toString('utf8')
  const lines = text.split('\n')
  if (lines.length > 0 && lines[lines.length - 1] === '') lines.pop()
  const entry = { lines: lines.map((line) => line.replace(/\r$/, '')), nul }
  cache.set(absolutePath, entry)
  return entry
}

function forbiddenPathProblem(citationPath) {
  if (citationPath.includes('\\')) return 'path uses a backslash; write a repo-relative path with forward slashes'
  if (/^[A-Za-z]:/.test(citationPath) || citationPath.startsWith('/')) return 'path is absolute; write it relative to the repository root'
  const segments = citationPath.split('/')
  if (segments.includes('..')) return 'path climbs out of the repository with ".."'
  const bad = segments.find((segment) => NOT_EVIDENCE_SEGMENTS.includes(segment))
  if (bad) return `path goes through "${bad}", which holds copies or build output, not the code of record`
  return null
}

/** Check one parsed citation. Returns an array of { code, message }. */
export function checkCitation(parsed, ctx) {
  const problems = []
  if (!parsed.ok) {
    problems.push({ code: parsed.code ?? 'malformed', message: parsed.problem })
    return problems
  }
  const forbidden = forbiddenPathProblem(parsed.path)
  if (forbidden) {
    problems.push({ code: 'forbidden-path', message: forbidden })
    return problems
  }
  const absolute = path.resolve(ctx.root, parsed.path)
  const relative = path.relative(ctx.root, absolute)
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    problems.push({ code: 'forbidden-path', message: 'path resolves outside the repository' })
    return problems
  }
  let stat
  try {
    stat = fs.statSync(absolute)
  } catch {
    problems.push({ code: 'missing-file', message: `file does not exist: ${parsed.path}` })
    return problems
  }
  if (!stat.isFile()) {
    problems.push({ code: 'not-a-file', message: `not a regular file: ${parsed.path}` })
    return problems
  }
  const { lines, nul } = loadLines(absolute, ctx.cache)
  if (nul) {
    problems.push({ code: 'nul-bytes', message: `file contains NUL bytes (corrupt or binary), so line numbers mean nothing: ${parsed.path}` })
    return problems
  }
  const total = lines.length
  for (const range of parsed.ranges) {
    if (range.from < 1) {
      problems.push({ code: 'line-out-of-range', message: `line numbers start at 1 (got ${range.from})` })
    } else if (range.to < range.from) {
      problems.push({ code: 'line-out-of-range', message: `range ${range.from}-${range.to} runs backwards` })
    } else if (range.from > total) {
      problems.push({ code: 'line-out-of-range', message: `line ${range.from} is past the end of ${parsed.path} (${total} lines)` })
    } else if (range.to > total) {
      problems.push({ code: 'line-out-of-range', message: `range ends at ${range.to}, past the end of ${parsed.path} (${total} lines)` })
    }
  }
  if (problems.length > 0) return problems

  if (parsed.anchor !== null) {
    const needle = normalizeWhitespace(parsed.anchor)
    const cited = parsed.ranges
      .map((range) => lines.slice(range.from - 1, range.to).join(' '))
      .map(normalizeWhitespace)
      .join(' ')
    if (!cited.includes(needle)) {
      let where = null
      for (let index = 0; index < total; index += 1) {
        if (normalizeWhitespace(lines[index]).includes(needle)) {
          where = index + 1
          break
        }
      }
      if (where !== null) {
        problems.push({
          code: 'anchor-moved',
          message: `anchor "${parsed.anchor}" is not in the cited lines; it is now at line ${where} of ${parsed.path}`,
        })
      } else {
        problems.push({
          code: 'anchor-missing',
          message: `anchor "${parsed.anchor}" does not appear anywhere in ${parsed.path}`,
        })
      }
    }
  }
  return problems
}

/** Split a markdown table row into cells. Supports \| inside a cell. */
export function splitRow(line) {
  let text = line.trim()
  if (text.startsWith('|')) text = text.slice(1)
  if (text.endsWith('|') && !text.endsWith('\\|')) text = text.slice(0, -1)
  const cells = []
  let current = ''
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index]
    if (char === '\\' && text[index + 1] === '|') {
      current += '|'
      index += 1
    } else if (char === '|') {
      cells.push(current)
      current = ''
    } else {
      current += char
    }
  }
  cells.push(current)
  return cells.map((cell) => cell.trim())
}

const SEPARATOR_ROW = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/

function spans(line) {
  const found = []
  const pattern = /`([^`\n]+)`/g
  let match
  while ((match = pattern.exec(line)) !== null) found.push(match[1])
  return found
}

/**
 * Check one document. `label` is how failures name it. Returns
 * { citations: [{line, raw, parsed}], failures: [{line, code, message}], unverifiedRows }.
 */
export function checkDocument(markdown, options) {
  const root = options.root
  const label = options.label ?? '(document)'
  const ctx = { root, cache: options.cache ?? new Map() }
  const failures = []
  const citations = []
  let unverifiedRows = 0
  const fail = (line, code, message) => failures.push({ line, code, message, label })

  const sourceLines = markdown.split('\n').map((line) => line.replace(/\r$/, ''))

  if (options.requireHeader !== false) {
    const head = sourceLines.slice(0, HEADER_LINES).join('\n').toLowerCase()
    if (!head.includes('not legal advice')) {
      fail(1, 'header-missing', `the first ${HEADER_LINES} lines must say this is engineering evidence and "not legal advice"`)
    }
  }

  let inFence = false
  let fenceMarker = ''
  let table = null // { evidenceColumn }
  for (let index = 0; index < sourceLines.length; index += 1) {
    const lineNumber = index + 1
    const line = sourceLines[index]
    const fence = /^\s*(```|~~~)/.exec(line)
    if (fence) {
      if (!inFence) {
        inFence = true
        fenceMarker = fence[1]
      } else if (fence[1] === fenceMarker) {
        inFence = false
      }
      table = null
      continue
    }
    if (inFence) continue

    const isTableLine = line.trim().startsWith('|')
    if (!isTableLine) table = null

    // Citations in this line. In a table row GitHub-flavoured markdown writes a
    // pipe inside a code span as \|, so undo that before reading the span.
    const lineCitations = []
    const spanSource = isTableLine ? line.split('\\|').join('|') : line
    for (const span of spans(spanSource)) {
      const parsed = parseCitation(span)
      if (!parsed) continue
      const problems = checkCitation(parsed, ctx)
      citations.push({ line: lineNumber, raw: parsed.raw, parsed, problems })
      lineCitations.push({ parsed, problems })
      for (const problem of problems) {
        fail(lineNumber, problem.code, `\`${parsed.raw}\`: ${problem.message}`)
      }
    }

    // Evidence-column rule.
    if (options.requireEvidenceRows !== false && isTableLine) {
      if (table === null) {
        const next = sourceLines[index + 1]
        if (next !== undefined && SEPARATOR_ROW.test(next) && next.includes('-')) {
          const headers = splitRow(line)
          const column = headers.findIndex((header) => /^evidence\b/i.test(header.replace(/[*_`]/g, '').trim()))
          table = { evidenceColumn: column, headerLine: lineNumber }
          // The header row and separator carry no claim.
          index += 1
          continue
        }
      } else if (table.evidenceColumn >= 0) {
        const cells = splitRow(line)
        const cell = cells[table.evidenceColumn] ?? ''
        const cellCitations = spans(cell).map(parseCitation).filter(Boolean)
        const valid = cellCitations.filter((citation) => citation.ok && checkCitation(citation, ctx).length === 0)
        const marked = /\bUNVERIFIED\b/.test(cell)
        if (marked) unverifiedRows += 1
        if (valid.length === 0 && !marked) {
          fail(lineNumber, 'evidence-missing', 'table row has no valid citation in its Evidence cell and is not marked UNVERIFIED')
        }
      }
    }
  }
  return { citations, failures, unverifiedRows }
}

function collectMarkdown(target, found) {
  const stat = fs.statSync(target)
  if (stat.isDirectory()) {
    for (const name of fs.readdirSync(target).sort()) {
      if (name === 'node_modules' || name.startsWith('.')) continue
      collectMarkdown(path.join(target, name), found)
    }
  } else if (target.toLowerCase().endsWith('.md')) {
    found.push(target)
  }
}

export function findRepoRoot(start) {
  let dir = path.resolve(start)
  for (;;) {
    if (fs.existsSync(path.join(dir, '.git'))) return dir
    const parent = path.dirname(dir)
    if (parent === dir) return null
    dir = parent
  }
}

function excerpt(parsed, ctx) {
  const absolute = path.resolve(ctx.root, parsed.path)
  const { lines } = loadLines(absolute, ctx.cache)
  const out = []
  for (const range of parsed.ranges) {
    const to = Math.min(range.to, range.from + 39)
    for (let n = range.from; n <= to; n += 1) out.push(`${String(n).padStart(6)}  ${lines[n - 1]}`)
    if (to < range.to) out.push(`        ... (${range.to - to} more lines)`)
  }
  return out.join('\n')
}

function usage() {
  process.stderr.write(
    [
      'usage: node check-citations.mjs [--root DIR] [--show] [--json] [--no-header-rule] [--no-evidence-rule] [PATH...]',
      '  PATH   a markdown file or a directory of them (default: the evidence directory)',
      '  --show print every cited excerpt, for a second-reader pass',
      '',
    ].join('\n'),
  )
}

export function main(argv, io = { out: (text) => process.stdout.write(text), err: (text) => process.stderr.write(text) }) {
  const args = argv.slice()
  const targets = []
  let root = null
  let show = false
  let json = false
  let requireHeader = true
  let requireEvidenceRows = true
  while (args.length > 0) {
    const arg = args.shift()
    if (arg === '--root') {
      root = args.shift()
      if (!root) { usage(); return 2 }
    } else if (arg === '--show') show = true
    else if (arg === '--json') json = true
    else if (arg === '--no-header-rule') requireHeader = false
    else if (arg === '--no-evidence-rule') requireEvidenceRows = false
    else if (arg === '-h' || arg === '--help') { usage(); return 0 }
    else if (arg.startsWith('--')) { usage(); return 2 }
    else targets.push(arg)
  }
  const resolvedRoot = path.resolve(root ?? findRepoRoot(HERE) ?? path.join(HERE, '..', '..', '..', '..'))
  if (targets.length === 0) targets.push(path.join(HERE, '..'))

  const files = []
  for (const target of targets) {
    if (!fs.existsSync(target)) {
      io.err(`check-citations: no such file or directory: ${target}\n`)
      return 2
    }
    collectMarkdown(target, files)
  }
  if (files.length === 0) {
    io.err('check-citations: no markdown files found\n')
    return 2
  }

  const cache = new Map()
  const all = []
  let citationCount = 0
  let unverified = 0
  const report = []
  for (const file of files) {
    const label = path.relative(resolvedRoot, file).split(path.sep).join('/')
    const result = checkDocument(fs.readFileSync(file, 'utf8'), { root: resolvedRoot, label, cache, requireHeader, requireEvidenceRows })
    citationCount += result.citations.length
    unverified += result.unverifiedRows
    all.push(...result.failures)
    report.push({ file, label, result })
  }

  if (json) {
    io.out(`${JSON.stringify({ files: files.length, citations: citationCount, unverifiedRows: unverified, failures: all }, null, 2)}\n`)
  } else {
    for (const failure of all) io.out(`${failure.label}:${failure.line}: [${failure.code}] ${failure.message}\n`)
    io.out(`check-citations: ${files.length} document(s), ${citationCount} citation(s), ${unverified} UNVERIFIED row(s), ${all.length} failure(s)\n`)
  }
  if (show && !json) {
    const ctx = { root: resolvedRoot, cache }
    for (const { label, result } of report) {
      for (const citation of result.citations) {
        if (citation.problems.length > 0) continue
        io.out(`\n--- ${label}:${citation.line}  ${citation.raw}\n${excerpt(citation.parsed, ctx)}\n`)
      }
    }
  }
  return all.length === 0 ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2))
}
