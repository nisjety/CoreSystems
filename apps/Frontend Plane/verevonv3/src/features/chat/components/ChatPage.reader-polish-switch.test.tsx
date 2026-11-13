// @vitest-environment jsdom

/**
 * Verification plan 3.4: VITE_CHAT_READER_POLISH (F-t3-098) at the page level. The lib suite (reader-polish.test.ts)
 * drives createReaderAppearance with injected deps. This file pins what the page itself uses: the switch read once at
 * load from the build env, the live deps (the appearance settings read and the organisation's flag read), the
 * transcript's per-turn anchor, and the page's wiring of the attributes and the reader tools. Off, the page writes no
 * attribute, renders no reader tools, reads neither the appearance settings nor the organisation's flags, and a turn
 * carries no data-turn-id. Each "off" case has an "on" control so the off pass cannot come from a fixture that never
 * renders the feature.
 */

import { render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

const spies = vi.hoisted(() => ({
  getAppearanceSettings: vi.fn(async () => ({ fontSize: 'large' })),
  orgFlag: vi.fn((_name: string, fallback: boolean) => fallback),
}))

vi.mock('@/shared/api/navbar-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/navbar-client')>()
  return { ...actual, getAppearanceSettings: spies.getAppearanceSettings }
})
vi.mock('@/shared/api/org-flags-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/org-flags-client')>()
  return { ...actual, orgFlag: spies.orgFlag }
})

const NOT_TRUE = [undefined, '', 'false', '1', 'TRUE', 'True', ' true', 'yes'] as const

async function freshReaderPolish(value: string | undefined) {
  vi.resetModules()
  vi.unstubAllEnvs()
  if (value !== undefined) vi.stubEnv('VITE_CHAT_READER_POLISH', value)
  else vi.stubEnv('VITE_CHAT_READER_POLISH', undefined as unknown as string)
  return import('../lib/reader-polish')
}

async function source(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

afterEach(() => {
  vi.unstubAllEnvs()
  vi.clearAllMocks()
  window.localStorage.clear()
})

describe('VITE_CHAT_READER_POLISH: the page-level switch', () => {
  it.each(NOT_TRUE)('off for %j: no attribute, no settings read, no organisation read', async (value) => {
    // A dyslexia choice cached on this device must not leak through with the switch off.
    window.localStorage.setItem('verevon.reader.dyslexia.v1', '1')
    const polish = await freshReaderPolish(value)
    expect(polish.readerPolishEnabled).toBe(false)
    expect(polish.readerPolishOn()).toBe(false)

    const appearance = polish.createReaderAppearance()
    await new Promise((resolve) => setTimeout(resolve, 5))
    flush()
    expect(appearance.attributes('compact')).toEqual({})
    expect(appearance.attributes(undefined)).toEqual({})
    expect(appearance.dyslexiaFont()).toBe(false)
    expect(spies.getAppearanceSettings).not.toHaveBeenCalled()
    expect(spies.orgFlag).not.toHaveBeenCalled()
  })

  it('on for the literal "true" (control): the live deps read the settings and write the stored size', async () => {
    const polish = await freshReaderPolish('true')
    expect(polish.readerPolishEnabled).toBe(true)
    expect(polish.readerPolishOn()).toBe(true)

    const appearance = polish.createReaderAppearance()
    await new Promise((resolve) => setTimeout(resolve, 5))
    flush()
    expect(spies.getAppearanceSettings).toHaveBeenCalledTimes(1)
    expect(appearance.attributes(undefined)['data-reader-font-size']).toBe('large')
    expect(spies.orgFlag).toHaveBeenCalledWith('VITE_CHAT_READER_POLISH', true)
  })

  it('on, but the organisation turned it off: no attribute', async () => {
    spies.orgFlag.mockImplementation(() => false)
    const polish = await freshReaderPolish('true')
    expect(polish.readerPolishOn()).toBe(false)
    const appearance = polish.createReaderAppearance()
    await new Promise((resolve) => setTimeout(resolve, 5))
    flush()
    expect(appearance.attributes('comfortable')).toEqual({})
    spies.orgFlag.mockImplementation((_name: string, fallback: boolean) => fallback)
  })
})

describe('VITE_CHAT_READER_POLISH: the transcript anchor on a turn', () => {
  const turn = {
    id: 'turn-user-1',
    role: 'user' as const,
    content: 'Hei',
    createdAt: new Date('2026-11-12T10:00:00Z').toISOString(),
    streaming: false,
    tools: [],
    attachments: [],
  }

  async function renderUserTurn(value: string | undefined) {
    await freshReaderPolish(value)
    const { UserMessage } = await import('./ChatMessages')
    const view = render(() => (
      <UserMessage
        copied={false}
        message={turn as never}
        onBranch={() => undefined}
        onCopy={() => undefined}
        onEdit={() => undefined}
      />
    ))
    flush()
    return view.container.querySelector('article.verevon-chat-message--user')
  }

  it.each([undefined, 'false', 'TRUE'])('off for %j: the user turn carries no data-turn-id', async (value) => {
    const article = await renderUserTurn(value)
    expect(article).not.toBeNull()
    expect(article!.hasAttribute('data-turn-id')).toBe(false)
  })

  it('on for the literal "true" (control): the user turn carries its id', async () => {
    const article = await renderUserTurn('true')
    expect(article?.getAttribute('data-turn-id')).toBe('turn-user-1')
  })
})

describe('VITE_CHAT_READER_POLISH: ChatPage wiring', () => {
  // The page has no cheap unit harness (see ChatPage.voice-first-turn.test.tsx). Pin that every reader-polish surface
  // on the page goes through the switch: the tools mount under readerPolishOn(), the outline is empty without it, and
  // the workspace attributes come only from createReaderAppearance (empty with the switch off, proven above).
  it('mounts the reader tools and builds the outline only under readerPolishOn()', async () => {
    const page = await source('src/features/chat/components/ChatPage.tsx')
    expect(page).toMatch(/<Show when=\{readerPolishOn\(\)\}>\s*<ReaderTools/)
    expect(page).toContain('createMemo(() => (readerPolishOn() ? chapterEntries(state.turns) : []))')
    expect(page).toContain('{...readerAppearance.attributes(')
    expect(page.match(/<ReaderTools\b/g)).toHaveLength(1)
    expect(page.match(/data-reader-/g) ?? []).toHaveLength(0)
  })
})
