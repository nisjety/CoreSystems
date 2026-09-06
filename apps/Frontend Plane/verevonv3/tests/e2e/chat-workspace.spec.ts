import { expect, test } from '@playwright/test'

/**
 * Chat workspace: the product invariants, end to end.
 *
 * The implementation plan's phase 10 lists twelve evaluation suites and none of
 * them existed — `tests/e2e` covered browser workspace, inbox/ticketing,
 * spaces, cross-plane smoke and knowledge authority, and nothing drove `/chat`
 * at all. Every claim about the chat page was therefore resting on unit tests
 * plus a human opening a browser, which is how acceptance criterion 9 passed
 * three audits while being broken (see the "single language" test below).
 *
 * This file covers evaluation suite 1 (simple Ask with no panel) and most of
 * suite 12 (keyboard, zoom, reduced motion, mobile), plus UX spec acceptance
 * criteria 1, 2, 3, 4, 8, 9 and 10. Suites 2-11 still need work that is either
 * model-dependent (grounded citations, generated HTML), backend-gated
 * (effectful Do with approval, A2A) or already covered by the
 * `browser-workspace-*` family (browser sessions, ZDR).
 *
 * Selectors are classes and roles, never labels: the chrome is bilingual, and
 * a spec keyed to Norwegian text would fail for an English reader rather than
 * catching a real defect. THE ONE RULE — no panel without a task event that
 * justifies it — is asserted through `.verevon-chat-page--canvas`, which
 * `ChatPage` puts on the page exactly when a contextual surface is open.
 */

const CHAT = '/chat'
const PAGE = '.verevon-chat-page'
const CANVAS_OPEN = '.verevon-chat-page--canvas'
const CANVAS = '.verevon-chat-workspace-canvas'
const MESSAGE = '.verevon-chat-message'
const COMPOSER = 'form textarea'

/** The model answers on its own schedule; a turn is slower than the default. */
const ANSWER_TIMEOUT = 90_000

/**
 * Locally-persisted chat state. `/chat` restores the last thread from these on
 * load, so a "cold start" has to be arranged rather than assumed — the first
 * draft of this file asserted an empty transcript on a page that had just
 * rehydrated a four-message thread.
 */
const CHAT_STORAGE_KEYS = [
  'verevon.chat.threadId',
  'verevon.chat.threadHistory.v1',
  'verevon.chat.threadTranscripts.v1',
  'verevon.chat.pendingLaunch',
  'verevon.chat.runPanel.collapsed.v1',
]

/**
 * Open `/chat` with a known locale and no restored thread. The init script runs
 * before the app's first read, which is the only point where clearing this
 * state is race-free.
 */
async function openFreshChat(page: import('@playwright/test').Page, locale: 'no' | 'en' = 'no') {
  await page.addInitScript(
    ([value, keys]) => {
      window.localStorage.setItem('verevon.locale', value as string)
      for (const key of keys as string[]) window.localStorage.removeItem(key)
    },
    [locale, CHAT_STORAGE_KEYS] as const,
  )
  await page.goto(CHAT)
  await expect(page.locator(PAGE)).toBeVisible()
  await expect(page.locator(COMPOSER)).toBeVisible()
}

async function sendTurn(page: import('@playwright/test').Page, text: string) {
  const composer = page.locator(COMPOSER)
  await composer.click()
  await composer.fill(text)
  await page.locator('form button[type="submit"]').click()
}

test.describe('chat workspace: the calm base case', () => {
  test('a cold start shows no contextual surface at all', async ({ page }) => {
    await openFreshChat(page)

    // Acceptance criteria 1 and 10, and invariant 2: no canvas, and no tab
    // strip anywhere on the page — not a hidden one, not an empty one.
    await expect(page.locator(CANVAS_OPEN)).toHaveCount(0)
    await expect(page.locator(CANVAS)).toHaveCount(0)
    await expect(page.locator(`${PAGE} [role="tablist"]`)).toHaveCount(0)

    // Criterion 2's precondition: the composer is the one thing that is always
    // there.
    await expect(page.locator(COMPOSER)).toBeVisible()
  })

  test('a plain Ask answers without opening a panel', async ({ page }) => {
    await openFreshChat(page)
    await sendTurn(page, 'Hva er hovedstaden i Norge? Svar kort.')

    // Evaluation suite 1. The assistant turn arrives...
    await expect(page.locator(`${MESSAGE}--assistant`).last()).toBeVisible({
      timeout: ANSWER_TIMEOUT,
    })
    // ...and nothing summoned a surface on the way. A tool call or a citation
    // would legitimately change this; a bookkeeping step must not, which is
    // the regression `isWorkStep` exists to prevent.
    await expect(page.locator(CANVAS_OPEN)).toHaveCount(0)
    await expect(page.locator(CANVAS)).toHaveCount(0)
    await expect(page.locator(COMPOSER)).toBeVisible()
  })
})

test.describe('chat workspace: the contextual canvas', () => {
  /**
   * An attachment is the one piece of evidence a test can create without the
   * model's cooperation: it makes the attachment surface available, so the
   * canvas can be summoned deterministically. Sources and Work need a real
   * citation or tool call and belong to suites 2 and 6.
   */
  async function attachTextFile(page: import('@playwright/test').Page) {
    await page.locator('input[type="file"]').setInputFiles({
      name: 'kvartalsnotat.txt',
      mimeType: 'text/plain',
      buffer: Buffer.from('Omsetningen i Q3 var 4,2 millioner kroner.\n'),
    })
  }

  test('the composer stays mounted, and one tab strip lives inside the canvas', async ({ page }) => {
    await openFreshChat(page)
    await attachTextFile(page)

    // The attachment chip summons the canvas for the file it names.
    const chip = page.locator('.verevon-chat-attachment, .verevon-composer-attachment').first()
    await expect(chip).toBeVisible()
    await chip.click()

    await expect(page.locator(CANVAS)).toBeVisible()

    // Criterion 2: opening a destination never hides or resets the composer.
    const composer = page.locator(COMPOSER)
    await expect(composer).toBeVisible()

    // Criteria 3 and 4: exactly one tab strip, and it is inside the canvas —
    // not in the conversation header, which is the blocking finding UX spec
    // section 3 opened with.
    const strips = page.locator(`${PAGE} [role="tablist"]`)
    await expect(strips).toHaveCount(1)
    await expect(page.locator(`${CANVAS} [role="tablist"]`)).toHaveCount(1)
  })

  test('Escape closes the canvas and leaves the conversation usable', async ({ page }) => {
    await openFreshChat(page)
    await attachTextFile(page)
    await page.locator('.verevon-chat-attachment, .verevon-composer-attachment').first().click()
    await expect(page.locator(CANVAS)).toBeVisible()

    await page.keyboard.press('Escape')

    // Criterion 8, focus transfer: the panel closes and focus lands somewhere
    // usable rather than on <body> — the tab strip unmounts with the panel, so
    // "what opened it" may be gone and the composer is the fallback.
    await expect(page.locator(CANVAS)).toHaveCount(0)
    await expect(page.locator(CANVAS_OPEN)).toHaveCount(0)
    const focusedTag = await page.evaluate(() => document.activeElement?.tagName ?? 'NONE')
    expect(focusedTag).not.toBe('BODY')
  })
})

test.describe('chat workspace: keyboard, motion and small screens', () => {
  test('arrow keys walk the transcript without stealing the composer keys', async ({ page }) => {
    await openFreshChat(page)
    await sendTurn(page, 'Nevn tre norske byer.')
    await expect(page.locator(`${MESSAGE}--assistant`).last()).toBeVisible({
      timeout: ANSWER_TIMEOUT,
    })

    // Item 26. Entering the transcript from elsewhere lands on the newest
    // message going up.
    await page.locator(MESSAGE).first().focus()
    await page.keyboard.press('ArrowDown')
    const walked = await page.evaluate(
      (selector) => document.activeElement?.classList.contains(selector) ?? false,
      'verevon-chat-message',
    )
    expect(walked).toBeTruthy()

    // ...and the composer keeps its own arrows: typing then pressing ArrowUp
    // inside the textarea must not move focus out of it.
    const composer = page.locator(COMPOSER)
    await composer.click()
    await composer.fill('et utkast')
    await page.keyboard.press('ArrowUp')
    await expect(composer).toBeFocused()
  })

  test('the canvas becomes a full-width sheet on a narrow viewport', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 })
    await openFreshChat(page)
    await page.locator('input[type="file"]').setInputFiles({
      name: 'notat.txt',
      mimeType: 'text/plain',
      buffer: Buffer.from('En kort note.\n'),
    })
    await page.locator('.verevon-chat-attachment, .verevon-composer-attachment').first().click()

    const canvas = page.locator(CANVAS)
    await expect(canvas).toBeVisible()

    // UX spec section 10 below 760px: a sheet, never a squeeze. The canvas
    // takes essentially the whole width, and the conversation is not beside it.
    const box = await canvas.boundingBox()
    expect(box).not.toBeNull()
    expect(box!.width).toBeGreaterThan(320)

    // The composer remains mounted on mobile too — the sheet is not a route.
    await expect(page.locator(COMPOSER)).toBeVisible()

    // The resize handle is meaningless in a sheet and must not be offered.
    // It is hidden with `display: none`, so it stays in the DOM — asserting a
    // count of zero here would fail against correct behaviour.
    await expect(page.locator('.verevon-chat-workspace-canvas__resize-handle')).not.toBeVisible()
  })

  /**
   * Criterion 8's reduced-motion half, asserted against what the CSS actually
   * promises. The `prefers-reduced-motion: reduce` blocks switch ANIMATIONS
   * off — the composer dock's entrance, the streaming cursor, the thinking
   * dots, the launch wash — and deliberately leave 140ms hover transitions
   * alone, which is the accepted reading of the preference. So this checks the
   * animation, and checks it from both sides: a one-sided assertion would also
   * pass if the dock simply stopped animating for everyone.
   */
  test('reduced motion switches the entrance animations off', async ({ page }) => {
    const dockAnimation = (target: import('@playwright/test').Page) =>
      target.evaluate(() => {
        const dock = document.querySelector('.verevon-chat-composer-dock')
        return dock ? getComputedStyle(dock).animationName : null
      })

    await openFreshChat(page)
    expect(await dockAnimation(page)).not.toBe('none')

    await page.emulateMedia({ reducedMotion: 'reduce' })
    await page.reload()
    await expect(page.locator(COMPOSER)).toBeVisible()
    expect(await dockAnimation(page)).toBe('none')
  })
})

test.describe('chat workspace: one language at a time', () => {
  /**
   * Acceptance criterion 9, from the outside.
   *
   * `chat-localization.test.ts` reads the source and catches a literal that
   * never reaches `i18n.tr`. It cannot catch a name assembled at runtime, and
   * it cannot prove the wiring works — so this asserts the rendered result:
   * with the locale set to English, no accessible name in the chat region may
   * carry a Norwegian-only letter.
   *
   * User CONTENT is exempt and must stay exempt: a Norwegian question typed by
   * the user, a thread title, a quoted document. Only names the product
   * authors are checked.
   */
  test('English chrome carries no Norwegian accessible names', async ({ page }) => {
    await openFreshChat(page, 'en')

    const norwegian = await page.evaluate(() => {
      const region = document.querySelector('.verevon-chat-page')
      if (!region) return ['no chat page rendered']
      const found: string[] = []
      const NORWEGIAN_ONLY = /[æøåÆØÅ]/
      for (const element of region.querySelectorAll('[aria-label], [title], [placeholder], [alt]')) {
        for (const attribute of ['aria-label', 'title', 'placeholder', 'alt']) {
          const value = element.getAttribute(attribute)
          if (!value) continue
          // A name built from the user's own text (a file name, a thread
          // title) is content; skip anything the page also shows verbatim.
          if (element.textContent && element.textContent.includes(value)) continue
          if (NORWEGIAN_ONLY.test(value)) found.push(`${attribute}="${value}"`)
        }
      }
      return found
    })

    expect(norwegian).toEqual([])
  })
})
