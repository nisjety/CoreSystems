import { expect, test, type Browser, type Page } from '@playwright/test'

/**
 * Org-admin Space policy console (WS03-S6), live through the gateway into user-core.
 *
 * Needs a running stack and a signed-in org owner or admin, so it is gated: set
 * E2E_ADMIN_STORAGE_STATE to a Playwright storage-state file of that admin. Without it the
 * cases skip; they never fall back to the default account.
 *
 * The read cases change nothing. The write case flips one entitlement and restores it, so it
 * runs only when E2E_SPACE_POLICY_ORG_OK=1 says the signed-in org is a DEDICATED test org whose
 * Space policy may be edited. It restores the original value at the end, also when it fails.
 *
 * A stack with the gateway's admin API off (404 space_policy_admin_disabled) shows the
 * not-switched-on note and no console; those cases skip with that reason.
 */

const ADMIN_STATE = process.env.E2E_ADMIN_STORAGE_STATE
const WRITES_OK = process.env.E2E_SPACE_POLICY_ORG_OK === '1'

const KNOWLEDGE_LABEL = 'Bruke kunnskap i rom'

async function openConsole(browser: Browser): Promise<{ page: Page; close: () => Promise<void> }> {
  const context = await browser.newContext({ storageState: ADMIN_STATE! })
  const page = await context.newPage()
  await page.goto('/settings/space-policy')
  return { page, close: () => context.close() }
}

type ConsoleState = 'console' | 'not_provisioned' | 'admin_disabled'

async function settledState(page: Page): Promise<ConsoleState> {
  const consoleSwitch = page.getByRole('switch', { name: KNOWLEDGE_LABEL })
  const notProvisioned = page.getByTestId('space-policy-not-provisioned')
  const disabled = page.getByText('Rom-policy er ikke slått på for denne installasjonen.')
  await expect(consoleSwitch.or(notProvisioned).or(disabled)).toBeVisible()
  if (await disabled.isVisible()) return 'admin_disabled'
  if (await notProvisioned.isVisible()) return 'not_provisioned'
  return 'console'
}

test.describe('space policy admin console', () => {
  test.skip(!ADMIN_STATE, 'needs E2E_ADMIN_STORAGE_STATE (an org owner or admin session)')

  test('an admin sees the console, the presets, the impact line and the history', async ({ browser }) => {
    const { page, close } = await openConsole(browser)
    try {
      await expect(page.getByRole('heading', { name: 'Rom-policy' }).first()).toBeVisible()
      const state = await settledState(page)
      test.skip(state === 'admin_disabled', 'the gateway admin API is switched off on this stack')
      if (state === 'not_provisioned') {
        // Deny by default: nothing can be switched on until Verevon operations set the attestation.
        await expect(page.getByRole('switch')).toHaveCount(0)
        return
      }
      await expect(page.getByTestId('space-policy-presets')).toBeVisible()
      await expect(page.getByTestId('space-policy-impact')).toContainText('aktive rom')
      await expect(page.getByRole('switch')).toHaveCount(8)
      await expect(page.getByTestId('space-policy-history').or(page.getByTestId('space-policy-history-empty'))).toBeVisible()
    } finally {
      await close()
    }
  })

  test('the sidebar lists Rom-policy for an admin only while the console is available', async ({ browser }) => {
    const { page, close } = await openConsole(browser)
    try {
      const state = await settledState(page)
      const link = page.getByRole('link', { name: 'Rom-policy' })
      if (state === 'admin_disabled') await expect(link).toHaveCount(0)
      else await expect(link.first()).toBeVisible()
    } finally {
      await close()
    }
  })

  test.describe('writes', () => {
    test.skip(!WRITES_OK, 'writes need E2E_SPACE_POLICY_ORG_OK=1 on a dedicated test org')

    test('turning an entitlement off asks to confirm, saves, and is restored at the end', async ({ browser }) => {
      const { page, close } = await openConsole(browser)
      const knowledge = page.getByRole('switch', { name: KNOWLEDGE_LABEL })
      let original: boolean | null = null
      try {
        const state = await settledState(page)
        test.skip(state !== 'console', `the console is not writable here (${state})`)
        test.skip(await page.getByTestId('space-policy-read-only').isVisible(), 'admin writes are switched off on this stack')

        original = await knowledge.isChecked()
        await knowledge.click()
        const save = page.getByRole('button', { name: 'Lagre endringer' })
        await save.click()
        if (original) {
          // Turning something off arms the button first and says what is lost.
          await expect(page.getByText(/Slår av:/)).toBeVisible()
          await page.getByRole('button', { name: 'Bekreft at du slår av rettigheter' }).click()
        }
        await expect(page.getByText(/Lagret\.|Ingen endring/)).toBeVisible()
        await expect(knowledge).toBeChecked({ checked: !original })
        await expect(page.getByTestId('space-policy-history')).toBeVisible()
      } finally {
        // Restore, so the dedicated org ends where it started.
        if (original !== null && (await knowledge.isChecked().catch(() => original)) !== original) {
          await knowledge.click()
          const restore = page.getByRole('button', { name: 'Lagre endringer' })
          await restore.click()
          const confirm = page.getByRole('button', { name: 'Bekreft at du slår av rettigheter' })
          if (await confirm.isVisible().catch(() => false)) await confirm.click()
          await expect(page.getByText(/Lagret\.|Ingen endring/)).toBeVisible()
        }
        await close()
      }
    })
  })
})
