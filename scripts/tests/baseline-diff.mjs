#!/usr/bin/env node
// Set difference of failing test ids between a stored baseline and a current run
// (slice M0-T0, plan 6.6). Never trusts a process exit code alone: a new failing
// id is a regression, an id that vanished is "fixed", and a command present in
// the baseline but missing from the current run is reported as "not run".
//
// Usage: node scripts/tests/baseline-diff.mjs <baseline-dir> <current-dir>
// Exit 0 when there is no new failing id, 1 when there is at least one, 2 on bad input.
// Both directories are the output of scripts/tests/baseline-capture.mjs.

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

function load(dir) {
  const records = new Map()
  for (const name of readdirSync(dir)) {
    if (!name.endsWith('.json') || name === 'manifest.json') continue
    const record = JSON.parse(readFileSync(join(dir, name), 'utf8'))
    records.set(record.id ?? name.replace(/\.json$/, ''), new Set(record.failing_ids ?? []))
  }
  return records
}

export function diff(baseline, current) {
  const report = { regressions: [], fixed: [], notRun: [], newCommands: [] }
  for (const [id, ids] of baseline) {
    const now = current.get(id)
    if (!now) { report.notRun.push(id); continue }
    for (const failing of now) if (!ids.has(failing)) report.regressions.push(`${id}: ${failing}`)
    for (const failing of ids) if (!now.has(failing)) report.fixed.push(`${id}: ${failing}`)
  }
  for (const id of current.keys()) if (!baseline.has(id)) report.newCommands.push(id)
  return report
}

function main(argv) {
  const [baselineDir, currentDir] = argv
  if (!baselineDir || !currentDir || !existsSync(baselineDir) || !existsSync(currentDir)) {
    console.error('usage: baseline-diff.mjs <baseline-dir> <current-dir>')
    process.exit(2)
  }
  const report = diff(load(baselineDir), load(currentDir))
  const section = (title, rows) => { if (rows.length) console.log(`${title} (${rows.length})\n${rows.map((r) => `  ${r}`).join('\n')}`) }
  section('NEW FAILURES', report.regressions)
  section('fixed since baseline', report.fixed)
  section('not run (a missing check is not a pass)', report.notRun)
  section('commands without a baseline', report.newCommands)
  if (!report.regressions.length) console.log('no new failing ids')
  process.exit(report.regressions.length ? 1 : 0)
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) main(process.argv.slice(2))
