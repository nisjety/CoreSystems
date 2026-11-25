/**
 * "Fork from here" opens the copy the page is handed (F-f05-21).
 *
 * ForkAction hands the new thread and the server's session to `onForked`. The page routes both into
 * `openForkedChatThread`, which puts the copy in the local history cache and selects it. The helper's outcome is
 * pinned in chat-thread-history.test.ts by reading the cache and the active thread back. That check replaces a spy
 * on `selectChatThread`, because a spy cannot see the helper's own call to it.
 *
 * The full page has no cheap render harness (see work-content-identity.test.ts), so the page's wiring is pinned as
 * source text: the message row passes the callback to the helper, and the page imports the helper it calls.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

/** The MessageBlock element as the page writes it: from its opening tag to the `/>` that closes it at the same indent. */
function messageBlockElement(page: string): string {
  const start = page.indexOf('<MessageBlock')
  expect(start, 'the page renders MessageBlock').toBeGreaterThan(-1)
  const close = /\r?\n {6}\/>/.exec(page.slice(start))
  expect(close, 'MessageBlock closes at the indent it opens at').not.toBeNull()
  return page.slice(start, start + (close?.index ?? 0))
}

describe('chat page wiring for a forked thread', () => {
  it('passes the new thread and its session from the message row to openForkedChatThread', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(messageBlockElement(page)).toMatch(
      /onForked=\{\(threadId, session\) => openForkedChatThread\(threadId, session\)\}/,
    )
  })

  it('imports openForkedChatThread from the chat thread history module', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    const imported = /import\s*\{([^}]*)\}\s*from\s*'\.\.\/lib\/chat-thread-history'/.exec(page)?.[1] ?? ''
    expect(imported.split(',').map((name) => name.trim())).toContain('openForkedChatThread')
  })
})
