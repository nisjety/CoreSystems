import { expect, test } from '@playwright/test'
import { composer, openFresh, transcript } from './product-acceptance-helpers'
import { latestQualifyingThreadPerScenario } from './product-qualification-threads'
import { writeGateEvidence } from './gate-evidence'

/**
 * Experience gate, chat half (see evidence/PRODUCT_RECORDING_READINESS_2026-09-24.md
 * §7 and RECORDING-WORKFLOW.md step 3): mobile AND desktop viewports, a
 * keyboard-only flow, and reduced motion, checked on the actual four
 * journeys — not just one synthetic prompt (`product-experience.spec.ts`'s
 * existing "Q08 mobile markdown, keyboard and reduced motion" test covers a
 * single made-up prompt at mobile width only).
 *
 * This reopens a qualifying thread per scenario (zero new model calls,
 * mirroring the durability spec) rather than driving a new turn, so it adds
 * no live model calls and no cost to the recording streak.
 *
 * Captions/media tracks are the marketing product-section player's
 * contract, not chat's: these four journeys never render `<video>`/`<audio>`
 * elements, so no caption/track assertion applies here by design.
 */
const VIEWPORTS = [
  { label: 'desktop', width: 1440, height: 1000 },
  { label: 'mobile', width: 390, height: 844 },
] as const

/** The four product journeys the Q16 qualification series runs (RUN-LOG.md, PRODUCT_RECORDING_READINESS_2026-09-24.md §7). */
const EXPECTED_SCENARIO_COUNT = 4

const threads = latestQualifyingThreadPerScenario()

test.describe('Experience: mobile + desktop, keyboard and reduced motion on the four journeys', () => {
  test.skip(threads.length === 0, 'No qualifying capture threads found under Q16_CAPTURE_ROOT (or the default private folder); nothing to reopen.')

  // Silently running on however many scenarios happen to have a qualifying
  // thread would let this gate report a clean pass while a scenario is
  // missing entirely — assert the full set instead of just "some data exists".
  test('the qualification data itself is complete: all four journeys are represented', () => {
    const scenarios = new Set(threads.map((t) => t.scenario))
    expect(scenarios.size, `expected ${EXPECTED_SCENARIO_COUNT} journeys, found: ${[...scenarios].sort().join(', ') || '(none)'}`).toBe(EXPECTED_SCENARIO_COUNT)
  })

  for (const viewport of VIEWPORTS) {
    for (const thread of threads) {
      test(`${viewport.label} / ${thread.scenario}: no overflow, keyboard reaches controls, reduced motion renders cleanly`, async ({ page }, info) => {
        const errors: string[] = []
        page.on('pageerror', (error) => errors.push(error.message))
        page.on('console', (entry) => { if (entry.type() === 'error') errors.push(entry.text()) })

        await page.setViewportSize({ width: viewport.width, height: viewport.height })
        await page.emulateMedia({ reducedMotion: 'reduce' })
        await openFresh(page, `/chat?thread_id=${encodeURIComponent(thread.threadId)}`)
        await expect.poll(async () => (await transcript(page))?.threadId, { timeout: 20_000 }).toBe(thread.threadId)
        await expect(page.locator('.verevon-chat-message--assistant').last()).toBeVisible()

        // No horizontal page overflow at this viewport.
        const noPageOverflow = await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)
        expect(noPageOverflow, `${viewport.label} page must not overflow horizontally`).toBe(true)

        // The rendered result panel itself must not clip past its own edge —
        // a hidden page overflow can mask an inner grid column clipping the
        // document (the same check product-readiness.spec.ts uses).
        const artifactView = page.locator('.verevon-chat-artifact-view')
        let noPanelOverflow = true
        if (await artifactView.count()) {
          noPanelOverflow = await artifactView.first().evaluate((panel) => {
            const right = panel.getBoundingClientRect().right
            return [...panel.querySelectorAll('.verevon-chat-artifact-view__head, .verevon-chat-artifact-view__body, .verevon-chat-markdown > p')]
              .every((element) => element.getBoundingClientRect().right <= right + 1)
          })
        }
        expect(noPanelOverflow, `${viewport.label} result panel content must stay within its own bounds`).toBe(true)

        // Keyboard-only flow: from the composer, Tab must move focus onto a
        // real control (never stall on <body>), repeatedly.
        await composer(page).focus()
        await expect(composer(page)).toBeFocused()
        const reachedTags: string[] = []
        for (let i = 0; i < 5; i++) {
          await page.keyboard.press('Tab')
          reachedTags.push(await page.evaluate(() => document.activeElement?.tagName ?? 'NONE'))
        }
        expect(reachedTags.every((tag) => tag !== 'BODY' && tag !== 'NONE'), `${viewport.label} keyboard Tab must always land on a focusable control, got ${JSON.stringify(reachedTags)}`).toBe(true)

        expect(page.locator('vite-error-overlay')).toHaveCount(0)
        expect(errors, `${viewport.label} reopening under reduced motion must raise no console/page errors`).toEqual([])

        await page.screenshot({ path: info.outputPath(`${viewport.label}-${thread.scenario}.png`), fullPage: true })
        const summary = {
          scenario: thread.scenario, threadId: thread.threadId, viewport: viewport.label,
          noPageOverflow, noPanelOverflow, keyboardReachedTags: reachedTags, consoleErrors: errors,
          checkedAt: new Date().toISOString(),
        }
        await info.attach('experience-summary', { body: JSON.stringify(summary), contentType: 'application/json' })
        writeGateEvidence('GATE_EVIDENCE_DIR', `experience-${viewport.label}-${thread.scenario}`, summary)
      })
    }
  }
})
