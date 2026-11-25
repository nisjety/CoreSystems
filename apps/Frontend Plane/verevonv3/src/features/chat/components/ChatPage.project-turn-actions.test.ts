/**
 * WS18-S4 (F-f06-16): the page wires a project thread's withheld turn actions and shows the one-line hint under the
 * header. The full page has no cheap render harness (see ChatPage.fork-wiring.test.ts), so the wiring is pinned as
 * source text: the controller's accessor is read, the message row receives it, and the hint sits under the header
 * and can be dismissed for its thread.
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

describe('chat page wiring for the turn actions of a project thread', () => {
  it('reads the controller accessor it passes down', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(page).toMatch(/^ {4}turnActionsAllowed,$/m)
  })

  it('keys the hint on the known project term, so an ordinary thread that is still loading shows no project copy', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(page).toMatch(/^ {4}projectThreadOpen,$/m)
    expect(page).toMatch(/const turnActionsHintOpen = \(\) => projectThreadOpen\(\) && turnHintDismissedFor\(\) !== state\.threadId/)
  })

  it('passes the accessor to every message row, so the answer row can hide its actions', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(messageBlockElement(page)).toMatch(/turnActionsAllowed=\{turnActionsAllowed\(\)\}/)
  })

  it('shows the one-line hint under the thread header, in both languages, and dismisses it for that thread only', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    const header = page.indexOf('<ChatHeader')
    const hint = page.indexOf('<Show when={turnActionsHintOpen()}>')
    const density = page.indexOf('<Show when={transcriptDensityEnabled}>')

    expect(header, 'the header renders').toBeGreaterThan(-1)
    expect(hint, 'the hint renders').toBeGreaterThan(header)
    expect(density, 'the hint sits before the transcript density control').toBeGreaterThan(hint)
    expect(page).toContain("'Gjenta, grene og vurdere er ikke tilgjengelig i prosjektsamtaler ennå.'")
    expect(page).toContain("'Regenerate, branch and rating are not available in project chats yet.'")
    expect(page).toContain('onClick={() => setTurnHintDismissedFor(state.threadId)}')
  })
})
