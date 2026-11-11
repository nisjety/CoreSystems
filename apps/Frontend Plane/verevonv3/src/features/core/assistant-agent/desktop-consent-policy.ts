import type { BrowserPolicyPort } from '@/features/settings/components/BrowserPolicySection'

/**
 * Settings > Browser (Q5-F1d, D-W5-4): whether the desktop browser may press a reject control on
 * cookie banners for this organization. The org policy key `browser.consent.reject` is off unless
 * the effective policy says `allowed`. Anything else is off: no organization, a refused or failed
 * read, a read that takes too long, and a key that is absent or set to another value. The answer is
 * kept per organization for a minute, so a run does not read the policy every time it starts.
 */

/** The catalogue ability (web-policy-catalogue.v1.json) that gates reject-only dismissal. */
export const CONSENT_REJECT_KEY = 'browser.consent.reject'

const cacheMs = 60_000
const readTimeoutMs = 5_000

/** One read of the effective view. It never rejects: every failure is `false`. */
async function readConsentRejectAllowed(port: BrowserPolicyPort, orgId: string): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    const load = await Promise.race([
      port.load(orgId),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error('The browser policy read took too long.')), readTimeoutMs)
      }),
    ])
    return load.available === true && load.effective.caps?.[CONSENT_REJECT_KEY] === 'allowed'
  } catch {
    return false
  } finally {
    clearTimeout(timer)
  }
}

/**
 * The answer the desktop agent asks before it starts a run: `true` only when the organization's
 * effective policy allows reject-only consent dismissal. Members may read the effective view, so the
 * answer does not depend on the viewer's role.
 */
export function createDesktopConsentPolicy(
  port: BrowserPolicyPort,
  orgId: () => string | null,
): () => Promise<boolean> {
  let cached: { orgId: string; until: number; allowed: Promise<boolean> } | null = null
  return async () => {
    const id = orgId()
    if (!id) return false
    const now = Date.now()
    if (!cached || cached.orgId !== id || cached.until <= now) {
      cached = { orgId: id, until: now + cacheMs, allowed: readConsentRejectAllowed(port, id) }
    }
    return cached.allowed
  }
}
