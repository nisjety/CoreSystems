/**
 * Q14 cohort report: per-workload p50 / p95 / max of every phase duration,
 * from the `<label>-phases` attachments the acceptance helper writes.
 *
 *   node tests/e2e/product-phase-report.ts <file-or-directory>... [--json]
 *
 * Inputs may be the attachment files themselves, Playwright JSON reports
 * (attachments inline as base64 or by path), or directories holding either;
 * directories are walked recursively. The same run found twice (a report and
 * its attachment file) is counted once.
 *
 * The rules come from Q14, and the report enforces them rather than leaving
 * them to the reader:
 *  - failed, stopped and incomplete runs stay in the cohort, with their
 *    outcome counts printed beside the numbers;
 *  - no p95 is printed for fewer than 20 samples — it is withheld, with the
 *    sample count, because a "p95" of a handful of runs is just its maximum.
 *
 * Every duration is on the browser clock (see `product-phase-timing.ts`).
 * Plain Node runs this file directly (type stripping); it needs no build.
 */
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  isPhaseReport,
  MIN_P95_SAMPLES,
  PHASE_DURATIONS,
  summarizeCohort,
  type PhaseReport,
  type WorkloadSummary,
} from './product-phase-timing.ts'

const MAX_INPUT_BYTES = 64 * 1024 * 1024

function listFiles(path: string): string[] {
  const stat = statSync(path)
  if (stat.isFile()) return [path]
  if (!stat.isDirectory()) return []
  return readdirSync(path).flatMap((entry) => listFiles(join(path, entry)))
}

function parseJsonFile(path: string): unknown {
  if (statSync(path).size > MAX_INPUT_BYTES) return undefined
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return undefined
  }
}

/** Every `*-phases` attachment inside a Playwright JSON report, wherever it sits. */
function attachmentsIn(report: unknown, reportPath: string): PhaseReport[] {
  const found: PhaseReport[] = []
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit)
      return
    }
    if (!value || typeof value !== 'object') return
    const record = value as Record<string, unknown>
    if (Array.isArray(record.attachments)) {
      for (const attachment of record.attachments as Array<Record<string, unknown>>) {
        if (typeof attachment?.name !== 'string' || !attachment.name.endsWith('-phases')) continue
        let text: string | undefined
        if (typeof attachment.body === 'string') text = Buffer.from(attachment.body, 'base64').toString('utf8')
        else if (typeof attachment.path === 'string') {
          const path = resolve(dirname(reportPath), attachment.path)
          text = existsSync(path) ? readFileSync(path, 'utf8') : undefined
        }
        try {
          const parsed: unknown = text ? JSON.parse(text) : undefined
          if (isPhaseReport(parsed)) found.push(parsed)
        } catch {
          // A truncated attachment is not a sample.
        }
      }
    }
    for (const entry of Object.values(record)) if (entry && typeof entry === 'object') visit(entry)
  }
  visit(report)
  return found
}

export function collectPhaseReports(paths: readonly string[]): PhaseReport[] {
  const seen = new Set<string>()
  const reports: PhaseReport[] = []
  for (const file of paths.flatMap((path) => listFiles(resolve(path)))) {
    if (!/\.json$/i.test(file) && !/-phases/i.test(file)) continue
    const parsed = parseJsonFile(file)
    const candidates = isPhaseReport(parsed) ? [parsed] : attachmentsIn(parsed, file)
    for (const report of candidates) {
      const key = createHash('sha256').update(JSON.stringify(report)).digest('hex')
      if (seen.has(key)) continue
      seen.add(key)
      reports.push(report)
    }
  }
  return reports
}

const ms = (value: number | null) => (value == null ? '—' : `${(value / 1000).toFixed(2)} s`)

export function formatCohort(summaries: readonly WorkloadSummary[]): string {
  if (summaries.length === 0) return 'No phase attachments found.'
  const lines: string[] = []
  for (const summary of summaries) {
    const { completed, failed, stopped, incomplete } = summary.outcomes
    lines.push(`${summary.workload}`)
    lines.push(`  runs ${summary.runs} (completed ${completed}, failed ${failed}, stopped ${stopped}, incomplete ${incomplete})`)
    for (const name of PHASE_DURATIONS) {
      const metric = summary.metrics[name]
      if (metric.n === 0) continue
      const p95 = metric.p95 == null ? `withheld (${metric.p95Withheld ?? `fewer than ${MIN_P95_SAMPLES} samples`})` : ms(metric.p95)
      lines.push(`  ${name.padEnd(12)} n=${String(metric.n).padEnd(4)} p50 ${ms(metric.p50).padEnd(9)} p95 ${p95.padEnd(9)} max ${ms(metric.max)}`)
    }
  }
  return lines.join('\n')
}

const isMain = Boolean(process.argv[1]) && resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)
if (isMain) {
  const args = process.argv.slice(2)
  const json = args.includes('--json')
  const paths = args.filter((arg) => arg !== '--json')
  if (paths.length === 0) {
    console.error('usage: node tests/e2e/product-phase-report.ts <file-or-directory>... [--json]')
    // `exitCode`, not `exit()`: an immediate exit can abort Node on Windows
    // while the piped stderr write is still closing.
    process.exitCode = 2
  } else {
    const summaries = summarizeCohort(collectPhaseReports(paths))
    console.log(json ? JSON.stringify(summaries, null, 2) : formatCohort(summaries))
  }
}
