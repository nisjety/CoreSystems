// Shrink-only gzipped budget for the production SPA build.
//
// Two sizes are budgeted, both gzipped because that is what a browser downloads:
// the entry chunk that dist/index.html loads, and the total of every JS file the
// build writes. A ceiling in bundle-budget.json is the most the build may weigh.
// Growth past a ceiling fails, and so does a shrink the ceiling does not follow.
// --update lowers ceilings to the measured size, and --base-ref refuses a change
// that raises a ceiling above the base branch's or unsets it, so a ceiling only
// ever comes down.
//
// A null ceiling is not recorded yet. The check prints the measured sizes, warns
// and passes until the first measurement is written with --update from a build
// on the Node version CI uses (22).
//
// Gzip output can differ by a few bytes between Node releases and hosts, so a
// size within TOLERANCE_BYTES of its ceiling passes in either direction.
//
// Usage, from the package root after `pnpm build`:
//   node scripts/check-bundle-budget.mjs [--base-ref <git-ref>]
//   node scripts/check-bundle-budget.mjs --update
// --dist and --budget point at other files; the tests use them.
import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import { gzipSync } from 'node:zlib'

const BUDGET_KEYS = ['entryGzipBytes', 'totalJsGzipBytes']
export const TOLERANCE_BYTES = 2048

const packageRoot = fileURLToPath(new URL('..', import.meta.url))

/** The src of the first module script tag, which is the entry chunk Vite writes. */
export function entryScriptSrc(html) {
  for (const [tag] of html.matchAll(/<script\b[^>]*>/gi)) {
    if (!/\btype="module"/i.test(tag)) continue
    const src = /\bsrc="([^"]+)"/i.exec(tag)
    if (src) return src[1]
  }
  return undefined
}

function jsFilesUnder(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(dir, entry.name)
    if (entry.isDirectory()) return jsFilesUnder(file)
    return entry.isFile() && entry.name.endsWith('.js') ? [file] : []
  })
}

function gzipBytes(file) {
  return gzipSync(readFileSync(file), { level: 9 }).length
}

/** Gzip sizes of the entry chunk and of every JS file under `dist`. */
export function measure(dist) {
  const indexFile = path.join(dist, 'index.html')
  if (!existsSync(indexFile)) throw new Error(`${indexFile} is missing: run the production build first`)
  const src = entryScriptSrc(readFileSync(indexFile, 'utf8'))
  if (src === undefined) throw new Error(`${indexFile} has no module script, so the entry chunk cannot be found`)
  const entry = path.join(dist, src)
  if (!existsSync(entry)) throw new Error(`${indexFile} loads ${src}, which is not in ${dist}`)
  const jsFiles = jsFilesUnder(dist)
  return {
    entry: path.relative(dist, entry).split(path.sep).join('/'),
    entryGzipBytes: gzipBytes(entry),
    jsFileCount: jsFiles.length,
    totalJsGzipBytes: jsFiles.reduce((sum, file) => sum + gzipBytes(file), 0),
  }
}

/** The committed budget. Each ceiling is a whole number of gzip bytes, or null until recorded. */
export function parseBudget(text) {
  const budget = JSON.parse(text)
  for (const key of BUDGET_KEYS) {
    const ceiling = budget[key]
    if (ceiling !== null && !(Number.isSafeInteger(ceiling) && ceiling >= 0)) {
      throw new Error(`bundle-budget.json ${key} must be a whole number of bytes, or null`)
    }
  }
  return budget
}

/** Problems with the measured sizes against the recorded ceilings, and the keys with no ceiling yet. */
export function checkCeilings(measured, budget) {
  const problems = []
  const unrecorded = []
  for (const key of BUDGET_KEYS) {
    const ceiling = budget[key]
    if (ceiling === null) {
      unrecorded.push(key)
      continue
    }
    const diff = measured[key] - ceiling
    if (diff > TOLERANCE_BYTES) {
      problems.push(`${key} is ${measured[key]} bytes, ${diff} over its ceiling of ${ceiling}`)
    } else if (diff < -TOLERANCE_BYTES) {
      problems.push(`${key} is ${measured[key]} bytes, ${-diff} under its ceiling of ${ceiling}: lower the ceiling with --update`)
    }
  }
  return { problems, unrecorded }
}

/** How this budget raises or unsets a ceiling that the base branch records. */
export function relaxations(budget, base) {
  return BUDGET_KEYS.flatMap((key) => {
    if (base[key] === null) return []
    if (budget[key] === null) return [`${key} was ${base[key]} on the base and is now unrecorded`]
    if (budget[key] > base[key]) return [`${key} raises the base ceiling of ${base[key]} to ${budget[key]}`]
    return []
  })
}

/** The budget committed at `ref`, or null when that commit predates the budget file. */
function baseBudget(ref, budgetFile) {
  const cwd = path.dirname(budgetFile)
  try {
    execFileSync('git', ['rev-parse', '--verify', '--quiet', `${ref}^{tree}`], { cwd, stdio: 'ignore' })
  } catch {
    throw new Error(`cannot resolve --base-ref ${ref}: fetch the base branch first`)
  }
  let text
  try {
    text = execFileSync('git', ['show', `${ref}:./${path.basename(budgetFile)}`], {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
  } catch {
    return null
  }
  return parseBudget(text)
}

function report(problems) {
  for (const problem of problems) console.error(`bundle budget: ${problem}`)
}

function check(budget, measured, baseRef, budgetFile) {
  const { problems, unrecorded } = checkCeilings(measured, budget)
  if (baseRef !== undefined) {
    const base = baseBudget(baseRef, budgetFile)
    if (base !== null) {
      problems.push(...relaxations(budget, base).map((problem) => `${problem} (base ${baseRef})`))
    }
  }
  const prefix = process.env.GITHUB_ACTIONS === 'true' ? '::warning::' : 'warning: '
  for (const key of unrecorded) {
    console.warn(`${prefix}${key} has no recorded ceiling, so it is not enforced yet: record the first measurement with --update`)
  }
  if (problems.length > 0) {
    report(problems)
    return 1
  }
  console.log(`bundle budget ${unrecorded.length === 0 ? 'passed' : 'passed with unrecorded ceilings'}`)
  return 0
}

function update(budgetFile, budget, measured) {
  const next = { ...budget }
  const problems = []
  for (const key of BUDGET_KEYS) {
    const ceiling = budget[key]
    if (ceiling === null || measured[key] < ceiling) {
      next[key] = measured[key]
    } else if (measured[key] - ceiling > TOLERANCE_BYTES) {
      problems.push(`${key} is ${measured[key]} bytes, over its ceiling of ${ceiling}: --update only lowers ceilings`)
    }
  }
  if (problems.length > 0) {
    report(problems)
    return 1
  }
  const changed = BUDGET_KEYS.filter((key) => next[key] !== budget[key])
  if (changed.length === 0) {
    console.log('bundle budget unchanged')
    return 0
  }
  writeFileSync(budgetFile, `${JSON.stringify(next, null, 2)}\n`)
  for (const key of changed) console.log(`${key}: ${budget[key]} -> ${next[key]}`)
  return 0
}

function run(argv) {
  const { values } = parseArgs({
    args: argv,
    options: {
      'base-ref': { type: 'string' },
      budget: { type: 'string' },
      dist: { type: 'string' },
      update: { type: 'boolean' },
    },
    strict: true,
  })
  const budgetFile = path.resolve(values.budget ?? path.join(packageRoot, 'bundle-budget.json'))
  const budget = parseBudget(readFileSync(budgetFile, 'utf8'))
  const measured = measure(path.resolve(values.dist ?? path.join(packageRoot, 'dist')))
  console.log(
    `${measured.entry}: ${measured.entryGzipBytes} gzip bytes; ${measured.jsFileCount} JS files: ${measured.totalJsGzipBytes} gzip bytes`,
  )
  if (values.update) return update(budgetFile, budget, measured)
  return check(budget, measured, values['base-ref'], budgetFile)
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = run(process.argv.slice(2))
  } catch (error) {
    console.error(`check-bundle-budget: ${error.message}`)
    process.exitCode = 2
  }
}
