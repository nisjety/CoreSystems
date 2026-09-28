import { mkdirSync, writeFileSync } from 'node:fs'
import { isAbsolute, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

// This file lives at tests/e2e/gate-evidence.ts; two levels up is the app root.
const repoRoot = resolve(fileURLToPath(new URL('.', import.meta.url)), '../..')

function within(parent: string, child: string): boolean {
  const relation = relative(parent, child)
  return !relation || (!relation.startsWith('..' + sep) && !isAbsolute(relation))
}

/**
 * Every gate-evidence writer (durability/recovery/experience specs) shares
 * one contract: a coordinator-provided, absolute directory OUTSIDE this
 * repository — never a fallback path inside it — that a before/after run
 * (e.g. bracketing a container restart) can diff. Failing loud when the env
 * var is missing or points inside the repo is deliberate: a silent skip or a
 * silent in-repo fallback would defeat that diff, and would risk writing
 * private/product evidence into version control.
 */
export function writeGateEvidence(envVar: string, name: string, payload: unknown): string {
  const raw = process.env[envVar]
  if (!raw || !isAbsolute(raw)) {
    throw new Error(`${envVar} must be set to an absolute directory outside the repository to collect gate evidence`)
  }
  const dir = resolve(raw)
  if (within(repoRoot, dir) || within(dir, repoRoot)) {
    throw new Error(`${envVar} must stay outside the repository (got ${dir})`)
  }
  mkdirSync(dir, { recursive: true })
  const path = resolve(dir, `${name}.json`)
  writeFileSync(path, JSON.stringify(payload, null, 2))
  return path
}
