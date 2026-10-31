// Lints the product files this change adds under src/ with the UI copy and
// raw-colour selectors, applied to every product file rather than only the chat
// and agents scope. A new file has no baseline entry, so its leaks are fixed
// before it merges. The ESLint class does not read the baseline; only the CLI
// does, so no suppressions apply here.
//
// Usage: node scripts/ui-gates-new-files.mjs <base-ref>
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { ESLint } from 'eslint'
import { BASE_RESTRICTED_SYNTAX, UI_COLOR_SYNTAX, UI_COPY_SYNTAX } from '../eslint.ui-gates.config.js'

const root = fileURLToPath(new URL('..', import.meta.url))
const base = process.argv[2]
if (!base) {
  console.error('usage: node scripts/ui-gates-new-files.mjs <base-ref>')
  process.exit(2)
}

// --relative makes the paths relative to this package, which is the cwd ESLint reads from.
// --no-renames reports a file moved into src/ as added, so the move cannot skip the scan.
let listing
try {
  listing = execFileSync(
    'git',
    ['diff', '--name-only', '--no-renames', '--diff-filter=A', '--relative', `${base}...HEAD`, '--', 'src'],
    { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  )
} catch {
  console.error(`Cannot list the files this change adds against ${base}. Fetch the base branch and pass a commit it can resolve.`)
  process.exit(2)
}
const added = listing
  .split('\n')
  .filter((file) => /\.(ts|tsx)$/.test(file) && !/\.(test|spec)\.(ts|tsx)$/.test(file))

if (added.length === 0) {
  console.log('No product files under src/ are added by this change.')
  process.exit(0)
}

const eslint = new ESLint({
  cwd: root,
  overrideConfigFile: fileURLToPath(new URL('../eslint.ui-gates.config.js', import.meta.url)),
  overrideConfig: {
    files: ['src/**/*.{ts,tsx}'],
    rules: {
      'no-restricted-syntax': ['error', ...BASE_RESTRICTED_SYNTAX, ...UI_COPY_SYNTAX, ...UI_COLOR_SYNTAX],
    },
  },
})

const results = await eslint.lintFiles(added)
let errors = 0
for (const result of results) {
  for (const message of result.messages) {
    if (message.severity !== 2) continue
    errors += 1
    const where = path.relative(root, result.filePath).split(path.sep).join('/')
    console.error(`${where}:${message.line}:${message.column} ${message.message} (${message.ruleId ?? 'parse'})`)
  }
}

if (errors > 0) {
  console.error(`${errors} UI copy or raw-colour finding(s) in ${added.length} file(s) this change adds.`)
  process.exit(1)
}
console.log(`UI copy and raw-colour gate passed for ${added.length} file(s) this change adds.`)
