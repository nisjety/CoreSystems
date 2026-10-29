import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * Connected apps in the composer (WS23 S6, code half). Stubbed the way model-entitlement.spec.ts
 * stubs the catalogue: the catalogue advertises connected-app reads on a chat model, the connection
 * list names two accounts, and the send is captured. These cases prove what the SPA shows and what it
 * puts on the wire. A real mailbox answering a question is WS23-S7's live proof, not this file's.
 *
 * Selectors are roles and the composer's own aria-labels; the chip is named by its account label.
 */

const CHAT = '/chat'
const COMPOSER = 'form textarea'
const CONTEXT_BUTTON = 'button[aria-label^="Kontekst for dette svaret"], button[aria-label^="Context for this answer"]'
const MODELS_PATH = '**/api/v1/models'
const CONNECTIONS_PATH = '**/api/v1/integrations/connections'
const STREAM_PATH = '**/api/v1/chat/stream'

const CHAT_STORAGE_KEYS = [
  'verevon.chat.threadId',
  'verevon.chat.threadHistory.v1',
  'verevon.chat.threadTranscripts.v1',
  'verevon.chat.pendingLaunch',
]

const catalogue = {
  models: [{ id: 'model-a', name: 'model-a', provider: 'openai', modality: 'chat', features: ['connector_scope'] }],
}

const connections = {
  connections: [
    { id: 'conn-ms-1', providerKey: 'microsoft', status: 'active', displayName: 'Ada (Outlook)' },
    { id: 'conn-g-1', providerKey: 'google', status: 'active', displayName: 'Ada Gmail' },
  ],
}

async function openChat(page: Page) {
  await page.route(MODELS_PATH, (route: Route) => route.fulfill({ json: catalogue }))
  await page.route(CONNECTIONS_PATH, (route: Route) => route.fulfill({ json: connections }))
  await page.addInitScript((keys: string[]) => {
    window.localStorage.setItem('verevon.locale', 'no')
    for (const key of keys) window.localStorage.removeItem(key)
  }, CHAT_STORAGE_KEYS)
  await page.goto(CHAT)
  await expect(page.locator(COMPOSER)).toBeVisible()
}

/** Sends one message and returns the JSON body the composer put on the stream request. */
async function sendAndCapture(page: Page): Promise<Record<string, unknown>> {
  // The stream is refused on purpose: only the request is under test, and the refusal keeps the page from waiting.
  await page.route(STREAM_PATH, (route: Route) =>
    route.fulfill({ status: 503, json: { error: { code: 'unavailable', message: 'stub', retryable: false, action: 'none' } } }),
  )
  const sent = page.waitForRequest((request) => request.url().includes('/api/v1/chat/stream') && request.method() === 'POST')
  await page.locator(COMPOSER).fill('Hva sendte Ada meg?')
  await page.keyboard.press('Enter')
  return (await sent).postDataJSON() as Record<string, unknown>
}

test.describe('connected apps in the composer', () => {
  test('offers the connected accounts beside the source filter and sends only the one picked', async ({ page }) => {
    await openChat(page)
    await page.locator(CONTEXT_BUTTON).first().click()

    const chip = page.getByRole('button', { name: /Ada \(Outlook\)/ })
    await expect(chip).toHaveAttribute('aria-pressed', 'false')
    await chip.click()
    await expect(chip).toHaveAttribute('aria-pressed', 'true')

    const body = await sendAndCapture(page)
    expect(body.connector_scope).toEqual(['conn-ms-1'])
  })

  test('leaves connector_scope off the wire when no account is picked', async ({ page }) => {
    await openChat(page)
    await page.locator(CONTEXT_BUTTON).first().click()
    await expect(page.getByRole('button', { name: /Ada Gmail/ })).toBeVisible()

    const body = await sendAndCapture(page)
    expect(body).not.toHaveProperty('connector_scope')
  })
})
