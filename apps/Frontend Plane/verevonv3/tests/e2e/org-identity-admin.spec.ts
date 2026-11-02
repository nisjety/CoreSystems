import { expect, test, type Browser } from '@playwright/test'

/**
 * Organization identity administration (WS05-S6), live through the gateway into org-core.
 *
 * An owner or admin claims a company domain, receives a DNS TXT challenge, and org-core verifies
 * it against a resolver. A member is refused with a typed 403. Needs a running stack with
 * ORG_IDENTITY_ADMIN_API and ORG_DOMAIN_VERIFICATION_ENABLED on, org-core's ORG_DOMAIN_DNS_RESOLVERS
 * pointing at a STUB resolver, and two signed-in sessions of the SAME org:
 *
 *   E2E_ADMIN_STORAGE_STATE / E2E_MEMBER_STORAGE_STATE  Playwright storage-state files, one per role
 *   E2E_ORG_ID                                          the org both sessions are in
 *   E2E_DNS_STUB_PUBLISH_URL                            optional: POST {name, value} publishes a TXT
 *                                                       record on the stub resolver. Without it the
 *                                                       verified leg is skipped.
 *
 * Without the first three the cases skip; they never fall back to the default account.
 */

const ADMIN_STATE = process.env.E2E_ADMIN_STORAGE_STATE
const MEMBER_STATE = process.env.E2E_MEMBER_STORAGE_STATE
const ORG_ID = process.env.E2E_ORG_ID
const PUBLISH_URL = process.env.E2E_DNS_STUB_PUBLISH_URL

type Envelope = {
  data?: Record<string, unknown> & { domains?: Array<Record<string, unknown>>; challenge?: { name?: string; value?: string } }
  error?: { code?: string; details?: Record<string, unknown> }
}

async function call(browser: Browser, storageState: string, method: 'GET' | 'POST' | 'DELETE', path: string, data?: unknown) {
  const context = await browser.newContext({ storageState })
  try {
    const response = await context.request.fetch(`/api/v1/orgs/${ORG_ID}/domains${path}`, { method, data })
    const body = (await response.json().catch(() => null)) as Envelope | null
    return { status: response.status(), body }
  } finally {
    await context.close()
  }
}

// A unique company-shaped name per run so a re-run never meets an earlier claim.
const DOMAIN = `e2e-${Date.now().toString(36)}.example.org`

test.describe('org identity administration', () => {
  test.skip(!ADMIN_STATE || !MEMBER_STATE || !ORG_ID, 'needs E2E_ADMIN_STORAGE_STATE, E2E_MEMBER_STORAGE_STATE and E2E_ORG_ID')
  test.describe.configure({ mode: 'serial' })

  test('a member is refused every domain route with a typed 403', async ({ browser }) => {
    for (const [method, path, data] of [
      ['GET', '', undefined],
      ['POST', '', { domain: DOMAIN }],
      ['POST', `/${DOMAIN}/verify`, undefined],
      ['DELETE', `/${DOMAIN}`, undefined],
    ] as const) {
      const refused = await call(browser, MEMBER_STATE!, method, path, data)
      expect(refused.status, `${method} ${path}`).toBe(403)
      expect(refused.body?.error?.code).toBe('identity_admin_required')
    }
  })

  test('a free-mail domain is refused with the reason', async ({ browser }) => {
    const refused = await call(browser, ADMIN_STATE!, 'POST', '', { domain: 'gmail.com' })
    expect(refused.status).toBe(400)
    expect(refused.body?.error?.code).toBe('invalid_domain')
    expect(refused.body?.error?.details?.reason).toBe('free_mail')
  })

  test('an admin gets a DNS challenge, fails the check, then verifies once the record is published', async ({ browser }) => {
    const added = await call(browser, ADMIN_STATE!, 'POST', '', { domain: DOMAIN })
    expect([200, 201]).toContain(added.status)
    const challenge = added.body?.data?.challenge
    expect(challenge?.name).toBe(`_verevon-challenge.${DOMAIN}`)
    expect(challenge?.value).toMatch(/^verevon-site-verification=.{16,}/)

    // Nothing published yet: the check answers pending with a closed reason, never verified.
    const early = await call(browser, ADMIN_STATE!, 'POST', `/${DOMAIN}/verify`)
    expect(early.status).toBe(200)
    expect(early.body?.data?.status).toBe('pending')
    expect(['txt_not_found', 'txt_mismatch', 'dns_unavailable']).toContain(early.body?.data?.reason)

    test.skip(!PUBLISH_URL, 'needs E2E_DNS_STUB_PUBLISH_URL to publish the record on the stub resolver')
    const publish = await fetch(PUBLISH_URL!, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name: challenge!.name, value: challenge!.value }),
    })
    expect(publish.ok).toBe(true)

    const verified = await call(browser, ADMIN_STATE!, 'POST', `/${DOMAIN}/verify`)
    expect(verified.status).toBe(200)
    expect(verified.body?.data?.status).toBe('verified')

    const listed = await call(browser, ADMIN_STATE!, 'GET', '')
    const row = listed.body?.data?.domains?.find((candidate) => candidate.domain === DOMAIN)
    expect(row?.status).toBe('verified')
    expect(row).not.toHaveProperty('challenge')
  })

  test('the claim can be removed and is then gone from the list', async ({ browser }) => {
    const removed = await call(browser, ADMIN_STATE!, 'DELETE', `/${DOMAIN}`)
    expect(removed.status).toBe(200)
    const listed = await call(browser, ADMIN_STATE!, 'GET', '')
    expect(listed.body?.data?.domains?.some((candidate) => candidate.domain === DOMAIN)).toBeFalsy()
  })
})
