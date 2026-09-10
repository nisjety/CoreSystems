// Phase 4 A8 — proves the no-fabricated-state lint actually fires.
//
// Runs the REAL project ESLint flat config (auto-discovered eslint.config.js)
// against a reintroduced A1-style fabricated-security array, and asserts the
// guard reports it. A clean, resource-shaped equivalent must NOT be flagged.
//
// `new ESLint()` resolves eslint.config.js and relative file paths from the
// vitest cwd (the verevonv3 package root) — no node:path/url needed, so this
// stays inside the SPA's browser-typed tsconfig.
import { ESLint } from 'eslint'
import { beforeAll, describe, expect, it } from 'vitest'

let eslint: ESLint

beforeAll(async () => {
  eslint = new ESLint()
  // Warm the ESLint flat-config + TS parser once. Cold-start is ~8s alone but
  // has been measured at ~69s when the whole suite runs in parallel and this
  // worker competes for CPU — so the hook budget is sized for the full run,
  // not the isolated one. Warming here keeps each assertion fast and stable.
  await eslint.lintText('export const warmup = 1\n', { filePath: 'src/__a8_warmup__.ts' })
}, 180_000)

async function syntaxGuardMessages(code: string, relativeFilePath: string): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath: relativeFilePath })
  return (result?.messages ?? [])
    .filter((m) => m.ruleId === 'no-restricted-syntax')
    .map((m) => m.message)
}

describe('A8 no-fabricated-state lint guard', () => {
  it('fires on a reintroduced A1-style hardcoded security posture', async () => {
    const fabricated = [
      'const securityToggles = [',
      "  { title: 'Require MFA for admins', enabled: true },",
      "  { title: 'Restrict sign-in to verified domains', enabled: true },",
      ']',
      'export const toggles = securityToggles',
      '',
    ].join('\n')

    const messages = await syntaxGuardMessages(fabricated, 'src/__a8_fixture_invalid__.ts')
    expect(messages.length).toBeGreaterThan(0)
    expect(messages.some((m) => /security\/compliance posture/i.test(m))).toBe(true)
  }, 30_000)

  it('does not fire on a resource-shaped security read (no hardcoded enabled:true)', async () => {
    const honest = [
      "import type { ResourceResult } from '@/shared/read-data'",
      'type Toggle = { title: string; enabled: boolean }',
      'export function render(result: ResourceResult<Toggle[]>) {',
      '  return result.data',
      '}',
      '',
    ].join('\n')

    const messages = await syntaxGuardMessages(honest, 'src/__a8_fixture_valid__.ts')
    expect(messages).toEqual([])
  }, 30_000)
})
