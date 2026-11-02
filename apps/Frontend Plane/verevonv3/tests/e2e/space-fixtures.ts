import { existsSync } from 'node:fs'
import type { Browser, BrowserContext } from '@playwright/test'

/**
 * Shared Space e2e fixtures (slice M0-T0, plan D3 and section 6.0).
 *
 * Environment contract (written by `bash scripts/space-e2e-seed.sh --apply` to
 * tests/e2e/.auth/space-e2e.env.sh):
 *   E2E_SPACE_REF            an active, Control-registered Space owned by member A
 *   E2E_STORAGE_STATE_A..F   Playwright storage states: A owner, B editor, C viewer,
 *                            D other org, E same-org outsider, F ZDR org (org role
 *                            only: Space roles are granted by the Space routes)
 *   E2E_REQUIRE_SPACE=1      a skipped Space spec is a FAILURE (set at milestone gates)
 *   E2E_BASE_URL             a second Vite server, never :5173 (the main checkout's)
 *   E2E_SPACE_MODEL          the explicit non-Verevon model a spec may run an agent on
 *
 * Every Space spec is named `space-<feature>.spec.ts`, belongs to the `space`
 * Playwright project and uses `skipUnlessSpace` instead of `test.skip`, so that a
 * missing fixture is a loud failure at a gate and a quiet skip on a laptop.
 */

/** The part of Playwright's `test` these helpers use. */
export type SkipTarget = { skip: (condition: boolean, description?: string) => void }

export type SpaceMember = 'A' | 'B' | 'C' | 'D' | 'E' | 'F'
export const SPACE_MEMBERS: readonly SpaceMember[] = ['A', 'B', 'C', 'D', 'E', 'F']

/** True when a skipped Space spec must fail (milestone gate). */
export const requireSpace = (): boolean => process.env.E2E_REQUIRE_SPACE === '1'

export const spaceRef = (): string | undefined => process.env.E2E_SPACE_REF?.trim() || undefined

/** Path of a member's storage state when the env var is set and the file exists. */
export function storageStateFor(member: SpaceMember): string | undefined {
  const path = process.env[`E2E_STORAGE_STATE_${member}`]?.trim()
  return path && existsSync(path) ? path : undefined
}

/**
 * Converts a skip into a failure under `E2E_REQUIRE_SPACE=1`. Use this in place of
 * `test.skip(condition, reason)` in every Space spec; `unavailable` is the condition
 * under which the spec cannot run.
 */
export function skipUnlessSpace(test: SkipTarget, unavailable: boolean, reason: string): void {
  if (!unavailable) return
  if (requireSpace()) {
    throw new Error(`E2E_REQUIRE_SPACE=1: Space spec cannot skip. Missing fixture: ${reason}`)
  }
  test.skip(true, reason)
}

/** Skip-or-fail unless `E2E_SPACE_REF` is set; returns the ref. */
export function requireSpaceRef(test: SkipTarget): string {
  const ref = spaceRef()
  skipUnlessSpace(test, !ref, 'E2E_SPACE_REF must name an active fixture Space')
  return ref ?? ''
}

/** Skip-or-fail unless every named member has a storage state; returns their paths. */
export function requireMembers(
  test: SkipTarget,
  ...members: SpaceMember[]
): Record<SpaceMember, string> {
  const states: Partial<Record<SpaceMember, string>> = {}
  const missing: string[] = []
  for (const member of members) {
    const path = storageStateFor(member)
    if (path) states[member] = path
    else missing.push(`E2E_STORAGE_STATE_${member}`)
  }
  skipUnlessSpace(test, missing.length > 0, `${missing.join(', ')} must point at a seeded storage state (scripts/space-e2e-seed.sh)`)
  return states as Record<SpaceMember, string>
}

/** A browser context signed in as one seeded member. The caller closes it. */
export async function memberContext(browser: Browser, baseURL: string | undefined, member: SpaceMember): Promise<BrowserContext> {
  const storageState = storageStateFor(member)
  if (!storageState) throw new Error(`E2E_STORAGE_STATE_${member} is not set or the file is missing`)
  return browser.newContext({ baseURL, storageState })
}

/**
 * Model ids that route through Verevon's own router. Space specs never run an agent on
 * these: they test Space behaviour, not the router, and must not spend router budget.
 * Mirrors the router-owned ids (`verevon`, `verevon-*` such as budget, balance, genius,
 * auto, plus the aliases `auto` and `default`).
 */
export const VEREVON_ROUTED_MODEL = /^\s*(?:verevon(?:[-_/.:][\w.:/-]*)?|auto|default)\s*$/i

export function isVerevonRoutedModel(model: string): boolean {
  return VEREVON_ROUTED_MODEL.test(model)
}

/**
 * The explicit model a Space spec may run an agent on: `E2E_SPACE_MODEL`, which must
 * not be a Verevon-routed id. Skip-or-fail when unset; throws when it is a Verevon id.
 */
export function spaceTestModel(test: SkipTarget): string {
  const model = process.env.E2E_SPACE_MODEL?.trim()
  skipUnlessSpace(test, !model, 'E2E_SPACE_MODEL must name an explicit non-Verevon model for agent-run specs')
  if (model && isVerevonRoutedModel(model)) {
    throw new Error(`E2E_SPACE_MODEL=${model} is a Verevon-routed model; Space specs must use an explicit non-Verevon model`)
  }
  return model ?? ''
}
