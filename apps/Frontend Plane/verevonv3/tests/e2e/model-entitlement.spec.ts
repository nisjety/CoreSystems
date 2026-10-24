import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * Model entitlement in the chat composer (WS10 S6). The organisation's model
 * policy rides beside the catalogue as `entitlement`; the composer reconciles to
 * it. These cases stub `/api/v1/models` and the stream, so they prove what the SPA
 * does with the server's answer, not that a policy exists: the live flow (an owner
 * edits a role policy, a member's catalogue shrinks) is WS10-S7's.
 *
 * Selectors are classes and roles; copy is asserted only where the Norwegian text
 * is the claim (the refusal), with the locale set explicitly.
 */

const CHAT = '/chat'
const COMPOSER = 'form textarea'
const MODEL_BUTTON = 'button[aria-label="Choose AI model"], button[aria-label="Velg AI-modell"]'
const MODEL_MENU = '.dashboard-composer-model-menu'
const NOTICE = '.dashboard-composer-stale-model-notice'
const MODELS_PATH = '**/api/v1/models'
const STREAM_PATH = '**/api/v1/chat/stream'

const CHAT_STORAGE_KEYS = [
  'verevon.chat.threadId',
  'verevon.chat.threadHistory.v1',
  'verevon.chat.threadTranscripts.v1',
  'verevon.chat.pendingLaunch',
]

const model = (id: string, effortLevels: string[] = ['low', 'medium', 'high']) => ({
  id,
  name: id,
  provider: 'openai',
  modality: 'chat',
  effort_levels: effortLevels,
})

const RESTRICTED = {
  state: 'role',
  enforced: true,
  revision: 5,
  restricted: true,
  hidden_count: 1,
  max_effort: 'medium',
}

function serveCatalogue(page: Page, body: unknown) {
  return page.route(MODELS_PATH, (route: Route) => route.fulfill({ json: body }))
}

async function openChat(page: Page, locale: 'en' | 'no' = 'en') {
  await page.addInitScript(
    ([keys, lang]) => {
      window.localStorage.setItem('verevon.locale', lang as string)
      for (const key of keys as string[]) window.localStorage.removeItem(key)
    },
    [CHAT_STORAGE_KEYS, locale] as const,
  )
  await page.goto(CHAT)
  await expect(page.locator(COMPOSER)).toBeVisible()
}

async function pickModel(page: Page, name: string) {
  await page.locator(MODEL_BUTTON).first().click()
  await page.locator(MODEL_MENU).getByText(name, { exact: true }).click()
}

test.describe('model entitlement in the composer', () => {
  test('a pinned model the policy no longer lists resets with a notice', async ({ page }) => {
    await serveCatalogue(page, { models: [model('model-a'), model('model-b')] })
    await openChat(page)
    await pickModel(page, 'model-b')
    await expect(page.locator(MODEL_BUTTON).first()).toContainText('model-b')

    // The next catalogue load no longer lists model-b for this member.
    await page.unroute(MODELS_PATH)
    await serveCatalogue(page, { models: [model('model-a')], entitlement: RESTRICTED })
    await page.reload()
    await expect(page.locator(COMPOSER)).toBeVisible()

    await expect(page.locator(NOTICE).first()).toContainText('model-b')
    await expect(page.locator(MODEL_BUTTON).first()).not.toContainText('model-b')
  })

  test('the effort select stops at the policy ceiling', async ({ page }) => {
    await serveCatalogue(page, { models: [model('model-a')], entitlement: RESTRICTED })
    await openChat(page)
    await pickModel(page, 'model-a')

    const select = page.getByRole('combobox', { name: /response effort|svarinnsats/i })
    await expect(select.locator('option[value="quick"]')).toHaveCount(1)
    await expect(select.locator('option[value="auto"]')).toHaveCount(1)
    await expect(select.locator('option[value="deep"]')).toHaveCount(0)
    await expect(select.locator('option[value="max"]')).toHaveCount(0)
  })

  test('a denied pinned send shows the Norwegian refusal, not a generic failure', async ({ page }) => {
    await serveCatalogue(page, { models: [model('model-a')] })
    await page.route(STREAM_PATH, (route: Route) =>
      route.fulfill({
        status: 403,
        json: {
          error: {
            code: 'model_not_entitled',
            message: 'denied',
            retryable: false,
            action: 'choose_another_model',
            details: { reason: 'not_in_allow_list', model: 'model-a' },
          },
        },
      }),
    )
    await openChat(page, 'no')
    await pickModel(page, 'model-a')

    await page.locator(COMPOSER).fill('Hei')
    await page.keyboard.press('Enter')

    await expect(page.getByText(/ikke tilgjengelig for deg|ikke på listen/).first()).toBeVisible()
  })
})
