import { expect, test, type Page } from '@playwright/test'

/**
 * Chat attachment gates (WS08 S1): a file the send path cannot deliver is
 * refused in the composer, by name, before anything is sent. Each case asserts
 * the two halves of that claim: a localized notice that names the file, and no
 * request to `/api/v1/chat/stream`.
 *
 * Needs no model: nothing here sends a turn. The ChatGPT-subscription case is
 * covered by `chat-client.test.ts` (the wire refuses to build) because it needs
 * a connected subscription; the temporary-chat case skips itself when the
 * deployment has no attested provider and the toggle is disabled.
 *
 * Selectors are classes and roles, never Norwegian copy, so the spec holds for
 * either locale; the file names are what the notices must contain.
 */

const CHAT = '/chat'
const COMPOSER = 'form textarea'
const FILE_INPUT = 'input[type="file"]'
const NOTICE = '.dashboard-composer-attachments__notice[role="alert"]'
const CHIP = '.dashboard-composer-attachment'
const STREAM_PATH = '/api/v1/chat/stream'

const CHAT_STORAGE_KEYS = [
  'verevon.chat.threadId',
  'verevon.chat.threadHistory.v1',
  'verevon.chat.threadTranscripts.v1',
  'verevon.chat.pendingLaunch',
]

// A valid 1x1 PNG, so a "good" image is a real one.
const PNG_1X1 = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
)

async function openFreshChat(page: Page) {
  await page.addInitScript(
    ([keys]) => {
      window.localStorage.setItem('verevon.locale', 'en')
      for (const key of keys as string[]) window.localStorage.removeItem(key)
    },
    [CHAT_STORAGE_KEYS] as const,
  )
  await page.goto(CHAT)
  await expect(page.locator(COMPOSER)).toBeVisible()
}

function watchStreamRequests(page: Page): string[] {
  const seen: string[] = []
  page.on('request', (request) => {
    if (request.url().includes(STREAM_PATH)) seen.push(request.url())
  })
  return seen
}

test.describe('chat attachment gates', () => {
  test('an unknown extension is refused with its name and never becomes a chip', async ({ page }) => {
    const streamRequests = watchStreamRequests(page)
    await openFreshChat(page)

    await page.locator(FILE_INPUT).setInputFiles({
      name: 'setup.exe',
      mimeType: 'application/x-msdownload',
      buffer: Buffer.from('MZ'),
    })

    await expect(page.locator(NOTICE)).toContainText('setup.exe')
    await expect(page.locator(CHIP)).toHaveCount(0)
    expect(streamRequests).toEqual([])
  })

  test('an oversize file is refused with its name', async ({ page }) => {
    const streamRequests = watchStreamRequests(page)
    await openFreshChat(page)

    await page.locator(FILE_INPUT).setInputFiles({
      name: 'annual-report.txt',
      mimeType: 'text/plain',
      buffer: Buffer.alloc(1_000_001, 'a'),
    })

    await expect(page.locator(NOTICE)).toContainText('annual-report.txt')
    await expect(page.locator(CHIP)).toHaveCount(0)
    expect(streamRequests).toEqual([])
  })

  test('a second image is refused by name, not silently ignored', async ({ page }) => {
    const streamRequests = watchStreamRequests(page)
    await openFreshChat(page)

    await page.locator(FILE_INPUT).setInputFiles({ name: 'first.png', mimeType: 'image/png', buffer: PNG_1X1 })
    await expect(page.locator(CHIP)).toHaveCount(1)
    await page.locator(FILE_INPUT).setInputFiles({ name: 'second.png', mimeType: 'image/png', buffer: PNG_1X1 })

    await expect(page.locator(NOTICE)).toContainText('second.png')
    await expect(page.locator(CHIP)).toHaveCount(1)
    expect(streamRequests).toEqual([])
  })

  test('an image is refused in Do mode', async ({ page }) => {
    const streamRequests = watchStreamRequests(page)
    await openFreshChat(page)

    const mode = page.getByRole('group', { name: /work mode|arbeidsmodus/i })
    // Do is offered only when the deployment allows it for the selected model.
    const doButton = mode.getByRole('button').nth(1)
    test.skip(!(await doButton.isEnabled()), 'Do is not available on this deployment')
    await doButton.click()
    await expect(doButton).toHaveAttribute('aria-pressed', 'true')

    await page.locator(FILE_INPUT).setInputFiles({ name: 'chart.png', mimeType: 'image/png', buffer: PNG_1X1 })

    await expect(page.locator(NOTICE)).toContainText('chart.png')
    await expect(page.locator(CHIP)).toHaveCount(0)
    expect(streamRequests).toEqual([])
  })

  test('an image is refused in a temporary chat', async ({ page }) => {
    const streamRequests = watchStreamRequests(page)
    await openFreshChat(page)

    const temporary = page.getByRole('button', { name: /temporary chat|midlertidig samtale/i }).first()
    test.skip(!(await temporary.isEnabled()), 'no zero-retention provider is attested on this deployment')
    await temporary.click()

    await page.locator(FILE_INPUT).setInputFiles({ name: 'chart.png', mimeType: 'image/png', buffer: PNG_1X1 })

    await expect(page.locator(NOTICE)).toContainText('chart.png')
    await expect(page.locator(CHIP)).toHaveCount(0)
    expect(streamRequests).toEqual([])
  })

  test('a document still attaches', async ({ page }) => {
    await openFreshChat(page)

    await page.locator(FILE_INPUT).setInputFiles({
      name: 'notes.md',
      mimeType: 'text/markdown',
      buffer: Buffer.from('# notes\n'),
    })

    await expect(page.locator(CHIP)).toHaveCount(1)
    await expect(page.locator(NOTICE)).toHaveCount(0)
  })
})
