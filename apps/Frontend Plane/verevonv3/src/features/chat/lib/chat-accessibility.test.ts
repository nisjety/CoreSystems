/**
 * Plan items 14-17 (VEREVON_CHAT_DESIGN.md §7.4): the accessibility and
 * first-run behaviour the 2026-09-02 cross-check found missing.
 *
 * These are source guards rather than DOM tests, following the precedent in
 * `mid-run-input.test.ts`, `chat-route-ownership.test.ts`,
 * `workspace-rail-geometry.test.ts` and `work-content-identity.test.ts`:
 * `pnpm lint` cannot run in this repo (typescript-eslint does not support the
 * pinned TypeScript 7), and each of these behaviours was verified live once
 * (measurements recorded in the design doc) — what a guard buys is that the
 * specific construct which made it work cannot be silently undone.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

const CSS = 'src/styles/global.css'
const PAGE = 'src/features/chat/components/ChatPage.tsx'
const MESSAGES = 'src/features/chat/components/ChatMessages.tsx'

describe('item 15 — message action buttons meet a 44px target', () => {
  it('sizes the target from a token, not a literal', async () => {
    const css = await read(CSS)
    const rule = css.slice(css.indexOf('.verevon-chat-action-button,'))
    expect(rule.slice(0, 700)).toContain('width: var(--verevon-touch-target, 44px)')
    expect(rule.slice(0, 700)).toContain('height: var(--verevon-touch-target, 44px)')
  })

  it('compensates the token for the app-wide zoom so the target is really 44px', async () => {
    const css = await read(CSS)
    // `.core-product-shell` applies `zoom: 0.9` at >=768px, which would render a
    // 44px target at 39.6px. Measured 44.0px on screen after this override.
    expect(css).toContain('--verevon-touch-target: calc(44px / 0.9)')
  })

  it('keeps the visible button at 32px via a content-box clip', async () => {
    const css = await read(CSS)
    const rule = css.slice(css.indexOf('.verevon-chat-action-button,'), css.indexOf('.verevon-chat-action-button:hover'))
    expect(rule).toContain('background-clip: content-box')
    // The `background` SHORTHAND resets background-clip to border-box, which
    // would paint the hover fill across the whole 44px box. Both the base and
    // hover rules must use the longhand.
    expect(rule).not.toMatch(/^\s*background:/m)
  })

  it('does not let the hover rule reset the clip with a shorthand', async () => {
    const css = await read(CSS)
    const hover = css.slice(css.indexOf('.verevon-chat-action-button:hover'))
    const block = hover.slice(0, hover.indexOf('}'))
    expect(block).toContain('background-color:')
    expect(block).not.toMatch(/^\s*background:/m)
  })
})

describe('item 16 — streaming is announced to a screen reader', () => {
  it('renders a polite live region', async () => {
    const page = await read(PAGE)
    expect(page).toMatch(/role="status"\s+aria-live="polite"/)
    expect(page).toContain('{streamAnnouncement()}')
  })

  it('announces lifecycle, not every token', async () => {
    const page = await read(PAGE)
    // Piping the growing answer into the region re-reads the whole thing on
    // each token. Start, sparse heartbeat, completion — nothing per-token.
    expect(page).toContain("setStreamAnnouncement('Verevon svarer …')")
    expect(page).toContain('Svar fullført.')
    expect(page).toContain('10_000')
  })

  it('clears the heartbeat by returning a cleanup from the effect', async () => {
    const page = await read(PAGE)
    // Solid 2 runs a cleanup RETURNED from the effect fn; `onCleanup` inside
    // one is silently dropped, which would leak an interval per turn.
    expect(page).toContain('return () => clearInterval(heartbeat)')
  })
})

describe('item 14 — keyboard model', () => {
  it('closes the contextual panel on Escape and respects an inner dismissal', async () => {
    const page = await read(PAGE)
    const handler = page.slice(page.indexOf('const onKeyDown = (event: KeyboardEvent)'))
    expect(handler.slice(0, 400)).toContain("event.key !== 'Escape' || event.defaultPrevented")
    expect(handler.slice(0, 400)).toContain("setActiveTab('chat')")
  })

  it('puts focus somewhere real after the panel closes', async () => {
    const page = await read(PAGE)
    // The tab strip unmounts with the panel on a thread with no other evidence,
    // and focus then falls to <body> — measured. Hence the composer fallback.
    expect(page).toContain('strip?.isConnected')
    expect(page).toContain(".verevon-chat-page textarea')?.focus()")
  })

  it('returns focus to the composer only when it is stranded', async () => {
    const page = await read(PAGE)
    const block = page.slice(page.indexOf('let wasStreaming = false'))
    expect(block.slice(0, 900)).toContain('const stranded =')
    expect(block.slice(0, 900)).toContain('verevon-chat-stop-btn')
    // Reading the previous value from a second effect argument silently never
    // fired on this Solid 2 RC; the flag is tracked explicitly instead.
    expect(block.slice(0, 900)).toContain('wasStreaming && !streaming')
  })
})

describe('item 17 — proactive first message', () => {
  it('greets instead of asking a bare question', async () => {
    const messages = await read(MESSAGES)
    expect(messages).toContain('<h1>{greeting()}</h1>')
    expect(messages).not.toContain("i18n.tr('Hva vil du få gjort?'")
  })

  it('builds the greeting from real session context', async () => {
    const messages = await read(MESSAGES)
    expect(messages).toContain('props.userName?.trim().split(/\\s+/)[0]')
    expect(messages).toContain("i18n.tr('God morgen', 'Good morning')")
  })

  it('sources the suggested first move from actual thread history', async () => {
    const messages = await read(MESSAGES)
    // A suggestion the product invented is just another generic prompt, and
    // there are already three of those below it.
    expect(messages).toContain('readChatThreadHistory()')
    expect(messages).toContain('verevon-chat-empty__resume')
  })

  it('skips the offer rather than truncating a long thread title', async () => {
    const messages = await read(MESSAGES)
    expect(messages).toContain('recent.title.length <= 60 ? recent : null')
  })

  it('passes the user name through from the session', async () => {
    const page = await read(PAGE)
    expect(page).toContain('userName={session.user?.name}')
  })
})
