import { expect, test, type Browser } from '@playwright/test'

/**
 * Role-gated audit reads (WS05-S1), live through the gateway into audit-core.
 *
 * An org owner or admin reads the whole org trail (`meta.scope` org); any other
 * member reads only their own rows (`meta.scope` self), and a foreign `user_id`
 * is a typed 403 `audit_scope_denied`. Needs a running stack and two signed-in
 * sessions of the SAME org, so it is gated: set E2E_ADMIN_STORAGE_STATE and
 * E2E_MEMBER_STORAGE_STATE to Playwright storage-state files (one per role).
 * Without them the cases skip, they never fall back to the default account.
 */

const ADMIN_STATE = process.env.E2E_ADMIN_STORAGE_STATE
const MEMBER_STATE = process.env.E2E_MEMBER_STORAGE_STATE
const FOREIGN_USER_ID = 'user_that_is_not_the_caller'

async function readAudit(browser: Browser, storageState: string, query: string) {
  const context = await browser.newContext({ storageState })
  try {
    const response = await context.request.get(`/api/v1/audit${query}`)
    const body = (await response.json().catch(() => null)) as {
      data?: unknown[]
      meta?: { scope?: string }
      error?: { code?: string; details?: { scope?: string } }
    } | null
    return { status: response.status(), body }
  } finally {
    await context.close()
  }
}

test.describe('audit role gate', () => {
  test.skip(!ADMIN_STATE || !MEMBER_STATE, 'needs E2E_ADMIN_STORAGE_STATE and E2E_MEMBER_STORAGE_STATE (two roles in one org)')

  test('an admin reads the org trail and may filter to any user', async ({ browser }) => {
    const org = await readAudit(browser, ADMIN_STATE!, '?limit=5')
    expect(org.status).toBe(200)
    expect(org.body?.meta?.scope).toBe('org')

    const foreign = await readAudit(browser, ADMIN_STATE!, `?limit=5&user_id=${FOREIGN_USER_ID}`)
    expect(foreign.status).toBe(200)
    expect(foreign.body?.meta?.scope).toBe('org')
  })

  test('a member reads only their own rows', async ({ browser }) => {
    const own = await readAudit(browser, MEMBER_STATE!, '?limit=5')
    expect(own.status).toBe(200)
    expect(own.body?.meta?.scope).toBe('self')
  })

  test('a member asking for another user is refused with a typed 403', async ({ browser }) => {
    const refused = await readAudit(browser, MEMBER_STATE!, `?limit=5&user_id=${FOREIGN_USER_ID}`)
    expect(refused.status).toBe(403)
    expect(refused.body?.error?.code).toBe('audit_scope_denied')
    expect(refused.body?.error?.details?.scope).toBe('self')
  })

  test('a member gets the content-free org-wide tool_action view and nothing else org-wide', async ({ browser }) => {
    const redacted = await readAudit(browser, MEMBER_STATE!, '?event=tool_action&limit=20')
    expect(redacted.status).toBe(200)
    expect(redacted.body?.meta?.scope).toBe('org_redacted')
    for (const row of (redacted.body?.data ?? []) as Record<string, unknown>[]) {
      expect(row.event).toBe('tool_action')
      for (const leaked of ['user_id', 'subject', 'resource_id', 'actor_role', 'request_id', 'org_id']) {
        expect(row).not.toHaveProperty(leaked)
      }
    }
  })
})
