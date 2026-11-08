import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * The Customize hub and the agent-mode picker (WS25-S11). The org agent policy and the hook rules are stubbed
 * at the gateway boundary, so these cases prove what the SPA does with the server's answer: the hub's gate, an
 * admin's write, and the picker's limit under the org cap. The enforcement of that cap is WS25-S9's, and the
 * live flow against a stack with ORG_AGENT_POLICY_API on is a proof the lead runs.
 *
 * Selectors are roles and labels; copy is asserted with the locale set explicitly.
 */

const CUSTOMIZE = '/agents/customize'
const CHAT = '/chat'
const POLICY_PATH = '**/api/v1/org/agent-policy'
const EFFECTIVE_PATH = '**/api/v1/org/agent-policy/effective'
const HOOKS_PATH = '**/api/v1/org/hooks'
const MODELS_PATH = '**/api/v1/models'

const STORED = {
  org_id: 'org_1',
  revision: 7,
  digest: 'd1',
  settings: {
    mode_cap: 'auto',
    rung_cap: 'danger_full_access',
    lockdown: false,
    'ext.mcp_servers': { mode: 'any', ids: [] },
    mcp_tool_approval: 'trust_annotations',
  },
}

const effectiveWithCap = (cap: string) => ({
  org_id: 'org_1',
  revision: 7,
  digest: 'd1',
  settings: [
    { key: 'mode_cap', kind: 'enum', status: 'enforced', value: cap, source: cap === 'auto' ? 'platform_default' : 'org_policy', fail_closed_value: 'lockdown' },
  ],
})

async function openPage(page: Page, path: string) {
  await page.addInitScript(() => {
    window.localStorage.setItem('verevon.locale', 'en')
  })
  await page.goto(path)
}

test.describe('Customize hub and agent-mode picker', () => {
  test('the hub says it is not switched on while the deployment has the policy API off', async ({ page }) => {
    await page.route(POLICY_PATH, (route: Route) => route.fulfill({ status: 404, json: { error: { code: 'not_found', message: 'off' } } }))

    await openPage(page, CUSTOMIZE)

    await expect(page.getByText('Customize is not switched on for this deployment yet.')).toBeVisible()
  })

  test('an admin sets a read-only cap, saving the whole document at the revision it read', async ({ page }) => {
    let saved: unknown
    await page.route(POLICY_PATH, async (route: Route) => {
      if (route.request().method() === 'PUT') {
        saved = route.request().postDataJSON()
        return route.fulfill({ json: { ...STORED, revision: 8 } })
      }
      return route.fulfill({ json: STORED })
    })
    await page.route(EFFECTIVE_PATH, (route: Route) => route.fulfill({ json: effectiveWithCap('auto') }))
    await page.route(HOOKS_PATH, (route: Route) => route.fulfill({ json: { org_id: 'org_1', revision: 7, rules: [] } }))

    await openPage(page, CUSTOMIZE)
    await page.getByRole('combobox', { name: /Highest agent mode/ }).selectOption('read_only')
    await page.getByRole('button', { name: 'Save agent policy' }).click()

    await expect.poll(() => saved).toMatchObject({ revision: 7, settings: { mode_cap: 'read_only' } })
  })

  test('the composer offers only the modes at or below the org cap', async ({ page }) => {
    await page.route(EFFECTIVE_PATH, (route: Route) => route.fulfill({ json: effectiveWithCap('read_only') }))
    await page.route(POLICY_PATH, (route: Route) => route.fulfill({ json: { ...STORED, settings: { ...STORED.settings, mode_cap: 'read_only' } } }))
    await page.route(MODELS_PATH, (route: Route) => route.fulfill({ json: { models: [{ id: 'model-a', name: 'model-a', provider: 'openai', modality: 'chat' }] } }))

    await openPage(page, CHAT)

    const picker = page.getByRole('combobox', { name: 'Agent mode' })
    await expect(picker).toBeVisible()
    const values = await picker.locator('option').evaluateAll((options) => options.map((option) => (option as HTMLOptionElement).value))
    expect(values).toEqual(['lockdown', 'read_only'])
  })
})
