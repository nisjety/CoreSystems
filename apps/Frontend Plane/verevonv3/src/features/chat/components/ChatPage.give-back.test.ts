/**
 * EP-1 give-back wiring: the live run panel's Give back reaches the chat controller's give-back caller.
 * Read as source text, the way the chat's other wiring tests are, because the check is that the prop is
 * passed; the caller's behaviour is pinned in use-chat-controller.give-back.test.ts.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

describe('ChatPage gives the live run panel the browser give-back', () => {
  it('takes the controller give-back and passes it to the live run panel', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')

    expect(page).toContain('giveBackBrowserRun,')
    expect(page).toContain('onBrowserGiveBack={(runId, note) => void giveBackBrowserRun(runId, note)}')
  })
})
